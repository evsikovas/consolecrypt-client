//! Workstation Git data import. Never checks out or executes repository files.
use crate::{
    AutomationError, GIT_SOURCE_FILE, GitProjectSource, ProjectRoot, Result, validate_project_path,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

const MAX_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ENTRIES: usize = 10000;
fn invalid() -> AutomationError {
    AutomationError::Invalid("workstation Git import failed or exceeded its limits")
}
/// Fast local prerequisite check before creating a durable import record.
pub async fn git_workstation_available() -> bool {
    let mut command = Command::new("git");
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x08000000);
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => status.success(),
        _ => {
            let _ = child.kill().await;
            false
        }
    }
}
struct Git {
    workspace: PathBuf,
    repository: PathBuf,
    cancel: CancellationToken,
    deadline: tokio::time::Instant,
}
#[allow(unsafe_code)] // Fixed owned-child OS custody only; no arbitrary process or command API.
mod process_group;
use process_group::ProcessGroup;

fn storage_budget(root: &Path) -> Result<()> {
    let mut pending = vec![root.to_owned()];
    let mut bytes = 0u64;
    let mut count = 0usize;
    while let Some(directory) = pending.pop() {
        let children = match std::fs::read_dir(directory) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for entry in children {
            let entry = entry?;
            let metadata = match entry.path().symlink_metadata() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            count += 1;
            if count > 20000 || metadata.file_type().is_symlink() {
                return Err(invalid());
            }
            if metadata.is_dir() {
                pending.push(entry.path())
            } else if metadata.is_file() {
                bytes = bytes.checked_add(metadata.len()).ok_or_else(invalid)?;
                if bytes > MAX_BYTES + 16 * 1024 * 1024 {
                    return Err(invalid());
                }
            } else {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
impl Git {
    async fn run(&self, args: &[&str], limit: usize) -> Result<Vec<u8>> {
        if self.cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.workspace).env_clear();
        for key in ["PATH", "SystemRoot", "SYSTEMROOT", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        cmd.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", self.workspace.join("empty.config"))
            .env("GIT_CONFIG_GLOBAL", self.workspace.join("empty.config"))
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("LC_ALL", "C")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args([
                "-c",
                "credential.helper=",
                "-c",
                "protocol.allow=never",
                "-c",
                "protocol.https.allow=always",
                "-c",
                "http.sslVerify=true",
                "-c",
                "http.followRedirects=false",
                "-c",
                "fetch.fsckObjects=true",
                "-c",
                "transfer.fsckObjects=true",
                "-c",
                "submodule.recurse=false",
            ])
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                self.workspace.join("empty-hooks").display()
            ))
            .arg("--git-dir")
            .arg(&self.repository)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.as_std_mut().process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.as_std_mut().creation_flags(0x08000000);
        }
        let mut child = cmd.spawn()?;
        #[cfg(any(unix, windows))]
        let _group = ProcessGroup::capture(&child)?;
        let mut output = child.stdout.take().ok_or_else(invalid)?;
        let mut errors = child.stderr.take().ok_or_else(invalid)?;
        let operation = async {
            let (status, bytes, _) = tokio::try_join!(
                async {
                    let result = child.wait().await.map_err(AutomationError::Io);
                    #[cfg(unix)]
                    _group
                        .exited
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    result
                },
                bounded(&mut output, limit),
                bounded(&mut errors, 64 * 1024)
            )?;
            if !status.success() {
                return Err(invalid());
            }
            Ok(bytes)
        };
        let monitor = async {
            loop {
                storage_budget(&self.workspace)?;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            #[allow(unreachable_code)]
            Ok::<(), AutomationError>(())
        };
        let result = tokio::select! {biased;
            _=self.cancel.cancelled()=>Err(AutomationError::Closed),
            _=tokio::time::sleep_until(self.deadline)=>Err(invalid()),
            value=monitor=>value.and(Err(invalid())),
            value=operation=>value,
        };
        if result.is_err() {
            #[cfg(unix)]
            _group.stop();
            let _ = child.kill().await;
            let _ = child.wait().await;
            #[cfg(unix)]
            _group
                .exited
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        result
    }
}
async fn bounded(reader: &mut (impl AsyncRead + Unpin), limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

#[derive(Debug)]
struct Blob {
    path: String,
    object: String,
    mode: u32,
    size: u64,
}
fn tree(raw: &[u8]) -> Result<Vec<Blob>> {
    let mut entries = Vec::new();
    let mut names = BTreeSet::new();
    let mut bytes = 0u64;
    for row in raw.split(|b| *b == 0).filter(|row| !row.is_empty()) {
        let (head, path) = row.split_at(row.iter().position(|b| *b == b'\t').ok_or_else(invalid)?);
        let path = std::str::from_utf8(&path[1..])
            .map_err(|_| invalid())?
            .to_owned();
        validate_project_path(&path)?;
        if path == GIT_SOURCE_FILE
            || path
                .split('/')
                .any(|part| part.eq_ignore_ascii_case(".git"))
            || !names.insert(path.clone())
        {
            return Err(invalid());
        }
        let parts = std::str::from_utf8(head)
            .map_err(|_| invalid())?
            .split_whitespace()
            .collect::<Vec<_>>();
        if parts.len() != 4
            || parts[1] != "blob"
            || !["100644", "100755"].contains(&parts[0])
            || ![40, 64].contains(&parts[2].len())
            || !parts[2]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(invalid());
        }
        let size = parts[3].parse::<u64>().map_err(|_| invalid())?;
        bytes = bytes.checked_add(size).ok_or_else(invalid)?;
        if bytes > MAX_BYTES || entries.len() >= MAX_ENTRIES {
            return Err(invalid());
        }
        entries.push(Blob {
            path,
            object: parts[2].into(),
            mode: if parts[0] == "100755" { 0o700 } else { 0o600 },
            size,
        });
    }
    if entries.is_empty() {
        return Err(invalid());
    }
    Ok(entries)
}

/// Both directories are new caller-owned staging directories. The destination
/// becomes publishable only after ProjectRoot's normal revision/baseline checks.
/// A missing local Git executable fails without falling back to an SSH Runner.
pub async fn capture_git_workstation(
    destination: &Path,
    workspace: &Path,
    url: String,
    reference: String,
    ca_pem: Option<String>,
    cancel: CancellationToken,
) -> Result<GitProjectSource> {
    let source = GitProjectSource {
        version: 1,
        kind: "git".into(),
        url,
        reference,
        commit: "0".repeat(40),
        ca_sha256: ca_pem
            .as_ref()
            .map(|v| hex::encode(Sha256::digest(v.as_bytes()))),
        executable_paths: vec![],
    };
    source.validate()?;
    if !ProjectRoot::open(destination)?
        .revision()?
        .entries
        .is_empty()
    {
        return Err(invalid());
    }
    let git = Git {
        workspace: workspace.to_owned(),
        repository: workspace.join("repository.git"),
        cancel,
        deadline: tokio::time::Instant::now() + Duration::from_secs(180),
    };
    std::fs::write(workspace.join("empty.config"), b"")?;
    std::fs::create_dir(workspace.join("empty-hooks"))?;
    if let Some(ca) = ca_pem {
        std::fs::write(workspace.join("public-ca.pem"), ca)?;
    }
    git.run(&["init", "--bare", "--template="], 4096).await?;
    let ca_config = workspace.join("public-ca.pem");
    let ca_arg = format!("http.sslCAInfo={}", ca_config.display());
    let mut fetch = vec![
        "fetch",
        "--depth=1",
        "--no-tags",
        "--no-recurse-submodules",
        "--",
        source.url.as_str(),
        source.reference.as_str(),
    ];
    if ca_config.exists() {
        fetch.splice(0..0, ["-c", ca_arg.as_str()]);
    }
    git.run(&fetch, 4096).await?;
    materialize(&git, destination, source).await
}

async fn materialize(
    git: &Git,
    destination: &Path,
    mut source: GitProjectSource,
) -> Result<GitProjectSource> {
    let commit = git
        .run(&["rev-parse", "--verify", "FETCH_HEAD^{commit}"], 128)
        .await?;
    source.commit = std::str::from_utf8(&commit)
        .map_err(|_| invalid())?
        .trim()
        .into();
    source.validate()?;
    let entries = tree(
        &git.run(
            &["ls-tree", "-r", "-l", "-z", "--full-tree", &source.commit],
            4 * 1024 * 1024,
        )
        .await?,
    )?;
    let root = ProjectRoot::open(destination)?;
    let mut directories = BTreeSet::new();
    for entry in &entries {
        let mut parent = String::new();
        let parts = entry.path.split('/').collect::<Vec<_>>();
        for part in &parts[..parts.len() - 1] {
            if !parent.is_empty() {
                parent.push('/')
            }
            parent.push_str(part);
            if directories.insert(parent.clone()) {
                root.import_directory(&parent)?;
            }
        }
        let data = git
            .run(&["cat-file", "blob", &entry.object], entry.size as usize)
            .await?;
        if data.len() as u64 != entry.size {
            return Err(invalid());
        }
        let mut file = root.import_file(&entry.path, entry.mode)?;
        file.write_all(&data)?;
        root.sync_import_file(&file)?;
        if entry.mode == 0o700 {
            source.executable_paths.push(entry.path.clone());
        }
    }
    source.validate()?;
    let mut provenance = root.import_file(GIT_SOURCE_FILE, 0o600)?;
    provenance.write_all(&serde_json::to_vec(&source).map_err(|_| invalid())?)?;
    root.sync_import_file(&provenance)?;
    drop(provenance);
    root.revision()?;
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(mode: &str, path: &str, size: u64) -> Vec<u8> {
        format!("{mode} blob {} {size}\t{path}\0", "a".repeat(40)).into_bytes()
    }
    #[test]
    fn accepts_literal_regular_blob_paths_and_modes() {
        let parsed = tree(&row("100755", "roles/demo/main.yml", 12)).unwrap();
        assert_eq!(parsed[0].path, "roles/demo/main.yml");
        assert_eq!(parsed[0].mode, 0o700);
    }
    #[test]
    fn refuses_links_submodules_escape_reserved_and_duplicate_paths() {
        for (mode, path) in [
            ("120000", "link"),
            ("160000", "module"),
            ("100644", "../escape"),
            ("100644", GIT_SOURCE_FILE),
            ("100644", ".git/config"),
        ] {
            assert!(tree(&row(mode, path, 1)).is_err());
        }
        let mut duplicate = row("100644", "playbook.yml", 1);
        duplicate.extend(row("100644", "playbook.yml", 1));
        assert!(tree(&duplicate).is_err());
    }
    #[test]
    fn refuses_snapshot_byte_and_entry_budgets() {
        assert!(tree(&row("100644", "large", MAX_BYTES + 1)).is_err());
        let mut many = Vec::new();
        for n in 0..=MAX_ENTRIES {
            many.extend(row("100644", &format!("file{n}"), 0));
        }
        assert!(tree(&many).is_err());
    }
    #[tokio::test]
    async fn actual_local_git_materializes_only_data_with_provenance_and_binary_modes() {
        let workspace = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("empty.config"), b"").unwrap();
        std::fs::create_dir(workspace.path().join("empty-hooks")).unwrap();
        let git = Git {
            workspace: workspace.path().into(),
            repository: workspace.path().join("repository.git"),
            cancel: CancellationToken::new(),
            deadline: tokio::time::Instant::now() + Duration::from_secs(30),
        };
        git.run(&["init", "--bare", "--template="], 4096)
            .await
            .unwrap();
        let binary = [0, 255, 13, 10, 0, 1];
        for (path, mode, data) in [
            ("nested/data.bin", "100644", binary.as_slice()),
            (
                "tool.sh",
                "100755",
                b"#!/bin/sh\nprintf must-never-execute\n".as_slice(),
            ),
            (
                ".gitattributes",
                "100644",
                b"* filter=must-never-execute\n".as_slice(),
            ),
        ] {
            let literal = workspace.path().join("literal-input");
            std::fs::write(&literal, data).unwrap();
            let object = git
                .run(
                    &[
                        "hash-object",
                        "-w",
                        "--no-filters",
                        "--",
                        literal.to_str().unwrap(),
                    ],
                    128,
                )
                .await
                .unwrap();
            let object = std::str::from_utf8(&object).unwrap().trim();
            git.run(
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("{mode},{object},{path}"),
                ],
                4096,
            )
            .await
            .unwrap();
        }
        let tree = git.run(&["write-tree"], 128).await.unwrap();
        let tree = std::str::from_utf8(&tree).unwrap().trim();
        let commit = git
            .run(
                &[
                    "-c",
                    "user.name=Synthetic fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit-tree",
                    tree,
                    "-m",
                    "Synthetic data only",
                ],
                128,
            )
            .await
            .unwrap();
        std::fs::write(git.repository.join("FETCH_HEAD"), &commit).unwrap();
        let source = GitProjectSource {
            version: 1,
            kind: "git".into(),
            url: "https://example.invalid/fixture.git".into(),
            reference: "fixture".into(),
            commit: "0".repeat(40),
            ca_sha256: None,
            executable_paths: vec![],
        };
        let source = materialize(&git, destination.path(), source).await.unwrap();
        assert_eq!(
            std::fs::read(destination.path().join("nested/data.bin")).unwrap(),
            binary
        );
        assert_eq!(source.executable_paths, vec!["tool.sh"]);
        let root = ProjectRoot::open(destination.path()).unwrap();
        let (actual, _) = root.git_source().unwrap().unwrap();
        assert_eq!(actual.commit, source.commit);
        assert!(!destination.path().join(".git").exists());
        assert_eq!(root.revision().unwrap().entries.len(), 5);
    }
}
