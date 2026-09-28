//! In-memory remote + recording opener for edit engine tests.

use crate::edit::{AppRef, ChooseOutcome, EditOpener, EditRemote, OpenError, OpenWith, ProgressFn};
use crate::{EntryKind, RemoteEntry, SftpError, TransferProgress};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct FakeFile {
    pub data: Vec<u8>,
    pub perms: u32,
    pub mtime: i64,
    pub uid: u32,
    pub gid: u32,
    /// Symlink target.
    pub link: Option<String>,
}

#[derive(Debug)]
pub struct FakeState {
    pub files: BTreeMap<String, FakeFile>,
    pub clock: i64,
    pub posix_rename: bool,
    /// The next N writes fail with a transient error.
    pub fail_writes: u32,
    /// Creating new files is denied (directory not writable).
    pub deny_create: bool,
    /// Owner of newly created files.
    pub new_file_uid: u32,
    pub ops: Vec<String>,
}

#[derive(Debug)]
pub struct FakeRemote {
    pub st: Mutex<FakeState>,
}

fn not_found(p: &str) -> SftpError {
    SftpError::NotFound(p.to_string())
}

impl FakeRemote {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            st: Mutex::new(FakeState {
                files: BTreeMap::new(),
                clock: 0,
                posix_rename: true,
                fail_writes: 0,
                deny_create: false,
                new_file_uid: 1000,
                ops: Vec::new(),
            }),
        })
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut FakeState) -> T) -> T {
        f(&mut self.st.lock().unwrap())
    }

    pub fn put(&self, path: &str, data: &[u8], perms: u32) {
        self.with(|s| {
            s.clock += 1;
            let mtime = s.clock;
            s.files.insert(
                path.into(),
                FakeFile {
                    data: data.to_vec(),
                    perms,
                    mtime,
                    uid: 1000,
                    gid: 1000,
                    link: None,
                },
            );
        });
    }

    pub fn symlink(&self, path: &str, target: &str) {
        self.with(|s| {
            s.files.insert(
                path.into(),
                FakeFile {
                    data: Vec::new(),
                    perms: 0o777,
                    mtime: 0,
                    uid: 1000,
                    gid: 1000,
                    link: Some(target.into()),
                },
            );
        });
    }

    /// Someone else edits the remote file.
    pub fn edit_by_other(&self, path: &str, data: &[u8]) {
        self.with(|s| {
            s.clock += 1;
            let mtime = s.clock;
            let f = s.files.get_mut(path).expect("exists");
            f.data = data.to_vec();
            f.mtime = mtime;
        });
    }

    pub fn delete(&self, path: &str) {
        self.with(|s| s.files.remove(path));
    }

    pub fn data(&self, path: &str) -> Option<Vec<u8>> {
        self.with(|s| s.files.get(path).map(|f| f.data.clone()))
    }

    pub fn file(&self, path: &str) -> Option<FakeFile> {
        self.with(|s| s.files.get(path).cloned())
    }

    pub fn ops(&self) -> Vec<String> {
        self.with(|s| s.ops.clone())
    }

    /// Number of recorded operations starting with `prefix`.
    pub fn count(&self, prefix: &str) -> usize {
        self.with(|s| s.ops.iter().filter(|o| o.starts_with(prefix)).count())
    }

    pub fn paths(&self) -> Vec<String> {
        self.with(|s| s.files.keys().cloned().collect())
    }

    fn resolve(s: &FakeState, path: &str) -> Result<String, SftpError> {
        let mut p = path.to_string();
        for _ in 0..8 {
            match s.files.get(&p) {
                Some(FakeFile { link: Some(t), .. }) => p = t.clone(),
                Some(_) => return Ok(p),
                None => return Err(not_found(path)),
            }
        }
        Err(not_found(path))
    }

    fn entry(path: &str, f: &FakeFile, follow_kind: bool) -> RemoteEntry {
        RemoteEntry {
            name: crate::base_name(path),
            path: path.to_string(),
            kind: if f.link.is_some() && !follow_kind {
                EntryKind::Symlink
            } else {
                EntryKind::File
            },
            size: f.data.len() as u64,
            permissions: Some(f.perms),
            uid: Some(f.uid),
            gid: Some(f.gid),
            user: None,
            group: None,
            modified: chrono::DateTime::from_timestamp(1_700_000_000 + f.mtime, 0),
        }
    }

    fn write_check(s: &mut FakeState) -> Result<(), SftpError> {
        if s.fail_writes > 0 {
            s.fail_writes -= 1;
            return Err(SftpError::Remote {
                path: "<fake>".into(),
                message: "injected failure".into(),
            });
        }
        Ok(())
    }
}

fn report(progress: ProgressFn<'_>, len: usize) {
    let total = Some(len as u64);
    progress(TransferProgress {
        transferred: 0,
        total,
    });
    progress(TransferProgress {
        transferred: len as u64,
        total,
    });
}

#[async_trait]
impl EditRemote for FakeRemote {
    async fn stat(&self, path: &str) -> Result<RemoteEntry, SftpError> {
        self.with(|s| {
            let real = Self::resolve(s, path)?;
            Ok(Self::entry(path, &s.files[&real], true))
        })
    }

    async fn lstat(&self, path: &str) -> Result<RemoteEntry, SftpError> {
        self.with(|s| {
            s.files
                .get(path)
                .map(|f| Self::entry(path, f, false))
                .ok_or_else(|| not_found(path))
        })
    }

    async fn canonicalize(&self, path: &str) -> Result<String, SftpError> {
        self.with(|s| Self::resolve(s, path))
    }

    async fn read_file(
        &self,
        path: &str,
        max_len: u64,
        progress: ProgressFn<'_>,
    ) -> Result<Vec<u8>, SftpError> {
        let data = self.with(|s| {
            s.ops.push(format!("read {path}"));
            let real = Self::resolve(s, path)?;
            Ok::<_, SftpError>(s.files[&real].data.clone())
        })?;
        if data.len() as u64 > max_len {
            return Err(SftpError::Local {
                path: path.into(),
                source: std::io::Error::other("file exceeds the edit size limit"),
            });
        }
        report(progress, data.len());
        Ok(data)
    }

    async fn write_new_file(
        &self,
        path: &str,
        data: &[u8],
        progress: ProgressFn<'_>,
    ) -> Result<(), SftpError> {
        self.with(|s| {
            s.ops.push(format!("create {path}"));
            if s.deny_create {
                return Err(SftpError::PermissionDenied(path.into()));
            }
            Self::write_check(s)?;
            if s.files.contains_key(path) {
                return Err(SftpError::AlreadyExists(path.into()));
            }
            s.clock += 1;
            let f = FakeFile {
                data: data.to_vec(),
                perms: 0o600,
                mtime: s.clock,
                uid: s.new_file_uid,
                gid: 1000,
                link: None,
            };
            s.files.insert(path.into(), f);
            Ok(())
        })?;
        report(progress, data.len());
        Ok(())
    }

    async fn overwrite_file(
        &self,
        path: &str,
        data: &[u8],
        progress: ProgressFn<'_>,
    ) -> Result<(), SftpError> {
        self.with(|s| {
            s.ops.push(format!("overwrite {path}"));
            Self::write_check(s)?;
            s.clock += 1;
            let mtime = s.clock;
            let f = s.files.entry(path.into()).or_insert(FakeFile {
                data: Vec::new(),
                perms: 0o644,
                mtime,
                uid: 1000,
                gid: 1000,
                link: None,
            });
            f.data = data.to_vec();
            f.mtime = mtime;
            Ok::<(), SftpError>(())
        })?;
        report(progress, data.len());
        Ok(())
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<(), SftpError> {
        self.with(|s| {
            s.ops.push(format!("chmod {path} {mode:o}"));
            let f = s.files.get_mut(path).ok_or_else(|| not_found(path))?;
            f.perms = mode;
            Ok(())
        })
    }

    async fn remove_file(&self, path: &str) -> Result<(), SftpError> {
        self.with(|s| {
            s.ops.push(format!("remove {path}"));
            s.files
                .remove(path)
                .map(|_| ())
                .ok_or_else(|| not_found(path))
        })
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        self.with(|s| {
            s.ops.push(format!("rename {from} {to}"));
            if s.files.contains_key(to) {
                // OpenSSH semantics: RENAME never overwrites.
                return Err(SftpError::Remote {
                    path: from.into(),
                    message: "Failure".into(),
                });
            }
            let f = s.files.remove(from).ok_or_else(|| not_found(from))?;
            s.files.insert(to.into(), f);
            Ok(())
        })
    }

    fn supports_posix_rename(&self) -> bool {
        self.with(|s| s.posix_rename)
    }

    async fn posix_rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        self.with(|s| {
            s.ops.push(format!("posix_rename {from} {to}"));
            let f = s.files.remove(from).ok_or_else(|| not_found(from))?;
            s.files.insert(to.into(), f);
            Ok(())
        })
    }
}

/// Records every open; never launches anything.
#[derive(Debug, Default)]
pub struct FakeOpener {
    pub calls: Mutex<Vec<(PathBuf, OpenWith)>>,
    /// One-shot override of the next result.
    pub next: Mutex<Option<Result<ChooseOutcome, String>>>,
}

impl FakeOpener {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn calls(&self) -> Vec<(PathBuf, OpenWith)> {
        self.calls.lock().unwrap().clone()
    }

    pub fn set_next(&self, r: Result<ChooseOutcome, String>) {
        *self.next.lock().unwrap() = Some(r);
    }
}

impl EditOpener for FakeOpener {
    fn open(&self, path: &Path, with: &OpenWith) -> Result<ChooseOutcome, OpenError> {
        assert!(path.exists(), "opened file must exist");
        self.calls
            .lock()
            .unwrap()
            .push((path.to_path_buf(), with.clone()));
        if let Some(r) = self.next.lock().unwrap().take() {
            return r.map_err(|message| OpenError::Failed {
                program: "fake".into(),
                message,
            });
        }
        Ok(match with {
            OpenWith::Default => ChooseOutcome::Opened { app: None },
            OpenWith::App(a) => ChooseOutcome::Opened {
                app: Some(a.clone()),
            },
            OpenWith::Choose => ChooseOutcome::Opened {
                app: Some(AppRef::Name("Chosen".into())),
            },
        })
    }
}
