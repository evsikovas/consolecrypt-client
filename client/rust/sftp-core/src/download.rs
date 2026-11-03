//! Directory-scoped, atomic local downloads. Remote names never reopen an
//! ambient directory, and replacing a destination never modifies its inode.

use cap_std::fs::{Dir, OpenOptions};
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path};

#[derive(Debug)]
pub struct DownloadRoot {
    dir: Dir,
}

impl DownloadRoot {
    /// Establish authority over the directory explicitly selected by the user.
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            dir: Dir::open_ambient_dir(path, cap_std::ambient_authority())?,
        })
    }

    pub fn create_dir_all(&self, relative: &Path) -> io::Result<()> {
        validate(relative)?;
        self.dir.create_dir_all(relative)
    }

    pub(crate) fn stage(&self, relative: &Path) -> io::Result<(StagedDownload, std::fs::File)> {
        validate(relative)?;
        let parent = relative
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        // Resolves symlinks relative to the capability and refuses escapes.
        // Retain this handle through commit, including if a directory is renamed.
        let dir = self.dir.open_dir(parent)?;
        let target = relative.file_name().ok_or_else(invalid)?.to_owned();
        let temporary: OsString = format!(".cc-download-{}.part", uuid::Uuid::new_v4()).into();
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = dir.open_with(&temporary, &options)?.into_std();
        Ok((
            StagedDownload {
                dir,
                target,
                temporary,
                retained: false,
            },
            file,
        ))
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "download path must stay inside the selected directory",
    )
}

fn validate(path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) struct StagedDownload {
    dir: Dir,
    target: OsString,
    temporary: OsString,
    retained: bool,
}

impl StagedDownload {
    pub fn mark_downloaded(&self) -> io::Result<()> {
        cc_platform_core::mark_downloaded_in(&self.dir, Path::new(&self.temporary))
    }

    pub fn commit(&mut self, overwrite: bool) -> io::Result<()> {
        if overwrite {
            self.dir.rename(&self.temporary, &self.dir, &self.target)?;
        } else {
            // Atomic create-without-replace: a destination appearing during
            // transfer cannot defeat overwrite=false, including a dangling link.
            self.dir
                .hard_link(&self.temporary, &self.dir, &self.target)?;
            self.dir.remove_file(&self.temporary)?;
        }
        self.retained = true;
        Ok(())
    }

    pub fn keep_partial(&mut self) {
        // Never replace an existing destination with an interrupted transfer.
        if self.commit(false).is_err() {
            self.retained = true;
        }
    }
}

impl Drop for StagedDownload {
    fn drop(&mut self) {
        if !self.retained {
            let _ = self.dir.remove_file(&self.temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn replacing_hardlink_preserves_the_original_inode() {
        let selected = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let protected = outside.path().join("protected");
        std::fs::write(&protected, b"keep").unwrap();
        std::fs::hard_link(&protected, selected.path().join("download")).unwrap();
        let root = DownloadRoot::open(selected.path()).unwrap();
        let (mut staged, mut file) = root.stage(Path::new("download")).unwrap();
        file.write_all(b"new").unwrap();
        drop(file);
        staged.commit(true).unwrap();
        assert_eq!(std::fs::read(&protected).unwrap(), b"keep");
        assert_eq!(
            std::fs::read(selected.path().join("download")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn no_overwrite_is_atomic_when_destination_appears_during_transfer() {
        let selected = tempfile::tempdir().unwrap();
        let root = DownloadRoot::open(selected.path()).unwrap();
        let (mut staged, mut file) = root.stage(Path::new("download")).unwrap();
        file.write_all(b"new").unwrap();
        drop(file);
        std::fs::write(selected.path().join("download"), b"winner").unwrap();
        assert_eq!(
            staged.commit(false).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        drop(staged);
        assert_eq!(
            std::fs::read(selected.path().join("download")).unwrap(),
            b"winner"
        );
        assert_eq!(std::fs::read_dir(selected.path()).unwrap().count(), 1);
    }

    #[test]
    fn dropping_interrupted_download_keeps_destination_and_cleans_staging() {
        let selected = tempfile::tempdir().unwrap();
        std::fs::write(selected.path().join("download"), b"keep").unwrap();
        let root = DownloadRoot::open(selected.path()).unwrap();
        let (staged, mut file) = root.stage(Path::new("download")).unwrap();
        file.write_all(b"partial").unwrap();
        drop(file);
        drop(staged);
        assert_eq!(
            std::fs::read(selected.path().join("download")).unwrap(),
            b"keep"
        );
        assert_eq!(std::fs::read_dir(selected.path()).unwrap().count(), 1);
        assert!(root.stage(Path::new("../escape")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn directory_links_cannot_escape_root_and_parent_swap_does_not_redirect_commit() {
        use std::os::unix::fs::symlink;
        let selected = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), selected.path().join("escape")).unwrap();
        let root = DownloadRoot::open(selected.path()).unwrap();
        assert!(root.create_dir_all(Path::new("escape/child")).is_err());
        assert!(root.stage(Path::new("escape/file")).is_err());
        root.create_dir_all(Path::new("safe")).unwrap();
        let (mut staged, mut file) = root.stage(Path::new("safe/file")).unwrap();
        file.write_all(b"new").unwrap();
        drop(file);
        std::fs::rename(selected.path().join("safe"), selected.path().join("moved")).unwrap();
        symlink(outside.path(), selected.path().join("safe")).unwrap();
        staged.mark_downloaded().unwrap();
        staged.commit(true).unwrap();
        assert_eq!(
            std::fs::read(selected.path().join("moved/file")).unwrap(),
            b"new"
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
