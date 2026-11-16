//! Copy an exact verified revision into a fresh caller-owned staging tree.
//! No links, external paths, filters, executable code or ambient Git config.
use crate::{AutomationError, ProjectEntry, ProjectRevision, ProjectRoot, Result};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use tokio_util::sync::CancellationToken;

impl ProjectRoot {
    /// Run only in a bounded filesystem worker. The staging directory is not
    /// published until this completes; its owner removes partial failures.
    pub fn freeze_to(
        &self,
        expected: &ProjectRevision,
        destination: &ProjectRoot,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if expected.version != 1
            || expected.entries.len() > 20000
            || expected.bytes > 100 * 1024 * 1024
            || !destination.revision()?.entries.is_empty()
        {
            return Err(AutomationError::Invalid(
                "invalid baseline staging tree or revision",
            ));
        }
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        if self.revision()? != *expected {
            return Err(AutomationError::FileChanged);
        }
        let mut buffer = vec![0u8; 64 * 1024];
        for entry in &expected.entries {
            if cancel.is_cancelled() {
                return Err(AutomationError::Closed);
            }
            match entry {
                ProjectEntry::Directory { path } => destination.import_directory(path)?,
                ProjectEntry::File {
                    path,
                    mode,
                    bytes,
                    sha256,
                } => {
                    let mut input = self.open_file(path)?;
                    if input.metadata()?.len() != *bytes {
                        return Err(AutomationError::FileChanged);
                    }
                    let mut output = destination.import_file(path, *mode)?;
                    let mut remaining = *bytes;
                    let mut hash = Sha256::new();
                    while remaining > 0 {
                        if cancel.is_cancelled() {
                            return Err(AutomationError::Closed);
                        }
                        let limit = remaining.min(buffer.len() as u64) as usize;
                        let count = input.read(&mut buffer[..limit])?;
                        if count == 0 {
                            return Err(AutomationError::FileChanged);
                        }
                        hash.update(&buffer[..count]);
                        output.write_all(&buffer[..count])?;
                        remaining -= count as u64;
                    }
                    if input.read(&mut buffer[..1])? != 0 || hex::encode(hash.finalize()) != *sha256
                    {
                        return Err(AutomationError::FileChanged);
                    }
                    destination.sync_import_file(&output)?;
                }
            }
        }
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        if self.revision()? != *expected || destination.revision()? != *expected {
            return Err(AutomationError::FileChanged);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_baseline_preserves_binary_unicode_empty_directories_and_portable_modes() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join("папка/empty")).unwrap();
        std::fs::write(
            source.path().join("папка/data.bin"),
            [0, 255, 13, 10, 0, 123],
        )
        .unwrap();
        std::fs::write(
            source.path().join("run.sh"),
            b"# fixture; never executed\r\n",
        )
        .unwrap();
        let metadata = crate::GitProjectSource {
            version: 1,
            kind: "git".into(),
            url: "https://example.test/repo.git".into(),
            reference: "main".into(),
            commit: "a".repeat(40),
            ca_sha256: None,
            executable_paths: vec!["run.sh".into()],
        };
        std::fs::write(
            source.path().join(crate::GIT_SOURCE_FILE),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                source.path().join("run.sh"),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        let root = ProjectRoot::open(source.path()).unwrap();
        let frozen = ProjectRoot::open(destination.path()).unwrap();
        let revision = root.revision().unwrap();
        root.freeze_to(&revision, &frozen, &CancellationToken::new())
            .unwrap();
        assert_eq!(frozen.revision().unwrap(), revision);
        assert_eq!(
            std::fs::read(destination.path().join("папка/data.bin")).unwrap(),
            vec![0, 255, 13, 10, 0, 123]
        );
        assert!(
            root.freeze_to(&revision, &frozen, &CancellationToken::new())
                .is_err(),
            "never replace an existing baseline"
        );
    }
    #[test]
    fn cancelled_or_stale_sources_never_become_a_complete_baseline() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("playbook.yml"), "---\n").unwrap();
        let root = ProjectRoot::open(source.path()).unwrap();
        let frozen = ProjectRoot::open(destination.path()).unwrap();
        let revision = root.revision().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(root.freeze_to(&revision, &frozen, &cancel).is_err());
        assert!(frozen.revision().unwrap().entries.is_empty());
        std::fs::write(source.path().join("playbook.yml"), "# changed\n").unwrap();
        assert!(root
            .freeze_to(&revision, &frozen, &CancellationToken::new())
            .is_err());
        assert!(frozen.revision().unwrap().entries.is_empty());
    }
}
