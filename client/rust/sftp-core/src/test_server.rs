//! Minimal SFTP server over a local directory (tests only), built on the
//! russh-sftp server module. Paths are confined to `root`.
//!
//! By default it behaves like OpenSSH's `sftp-server` where it matters for
//! atomic replace: plain `RENAME` refuses to overwrite an existing target and
//! `posix-rename@openssh.com` (which does overwrite) is announced; like
//! OpenSSH it reads the `SYMLINK` arguments as (target, link).
//!
//! Available to other crates' tests with the `test-harness` feature
//! ([`connect_local`]). Never used in production code.

use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Packet, Status, StatusCode, Version,
};
use russh_sftp::server::{Handler, StatusReply};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

enum Open {
    File(std::fs::File),
    Dir(Option<Vec<File>>),
}

/// An in-process SFTP server over `root` and a client connected to it
/// through an in-memory pipe (no SSH involved).
pub async fn connect_local(root: PathBuf) -> Result<crate::SftpClient, crate::SftpError> {
    let (a, b) = tokio::io::duplex(1 << 20);
    tokio::spawn(russh_sftp::server::run(b, LocalFsServer::new(root)));
    crate::SftpClient::from_stream(a).await
}

/// russh-sftp [`Handler`] serving a local directory.
#[allow(missing_debug_implementations)]
pub struct LocalFsServer {
    root: PathBuf,
    handles: HashMap<String, Open>,
    next: u64,
    posix_rename: bool,
}

impl LocalFsServer {
    /// Serve `root` (paths outside it are refused).
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            handles: HashMap::new(),
            next: 0,
            posix_rename: true,
        }
    }

    /// Do not announce / implement `posix-rename@openssh.com`.
    pub fn without_posix_rename(mut self) -> Self {
        self.posix_rename = false;
        self
    }

    fn resolve(&self, p: &str) -> Result<PathBuf, StatusCode> {
        let mut out = self.root.clone();
        for c in Path::new(p).components() {
            match c {
                Component::Normal(n) => out.push(n),
                Component::ParentDir => {
                    let popped = out.pop();
                    if !popped || !out.starts_with(&self.root) {
                        return Err(StatusCode::PermissionDenied);
                    }
                }
                _ => {}
            }
        }
        if !out.starts_with(&self.root) {
            return Err(StatusCode::PermissionDenied);
        }
        Ok(out)
    }

    fn handle(&mut self, o: Open) -> String {
        self.next += 1;
        let h = format!("h{}", self.next);
        self.handles.insert(h.clone(), o);
        h
    }
}

fn io_code(e: std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".into(),
        language_tag: "en-US".into(),
    }
}

impl Handler for LocalFsServer {
    type Error = StatusReply;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported.into()
    }

    async fn init(&mut self, _v: u32, _e: HashMap<String, String>) -> Result<Version, Self::Error> {
        let mut v = Version::new();
        if self.posix_rename {
            v.extensions
                .insert("posix-rename@openssh.com".into(), "1".into());
        }
        Ok(v)
    }

    async fn extended(
        &mut self,
        id: u32,
        request: String,
        data: Vec<u8>,
    ) -> Result<Packet, Self::Error> {
        if request != "posix-rename@openssh.com" || !self.posix_rename {
            return Err(StatusCode::OpUnsupported.into());
        }
        fn take(d: &mut &[u8]) -> Option<String> {
            let n = u32::from_be_bytes(d.get(..4)?.try_into().ok()?) as usize;
            let s = String::from_utf8(d.get(4..4 + n)?.to_vec()).ok()?;
            *d = &d[4 + n..];
            Some(s)
        }
        let mut d = data.as_slice();
        let (Some(from), Some(to)) = (take(&mut d), take(&mut d)) else {
            return Err(StatusCode::BadMessage.into());
        };
        std::fs::rename(self.resolve(&from)?, self.resolve(&to)?).map_err(io_code)?;
        Ok(Packet::Status(ok(id)))
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _a: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let path = self.resolve(&filename)?;
        let f = std::fs::OpenOptions::from(pflags)
            .open(&path)
            .map_err(io_code)?;
        let handle = self.handle(Open::File(f));
        Ok(Handle { id, handle })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.handles.remove(&handle);
        Ok(ok(id))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let Some(Open::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure.into());
        };
        f.seek(SeekFrom::Start(offset)).map_err(io_code)?;
        let mut buf = vec![0u8; len as usize];
        let n = f.read(&mut buf).map_err(io_code)?;
        if n == 0 {
            return Err(StatusCode::Eof.into());
        }
        buf.truncate(n);
        Ok(Data { id, data: buf })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let Some(Open::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure.into());
        };
        f.seek(SeekFrom::Start(offset)).map_err(io_code)?;
        f.write_all(&data).map_err(io_code)?;
        Ok(ok(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let p = self.resolve(&path)?;
        let m = std::fs::symlink_metadata(p).map_err(io_code)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&m),
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let p = self.resolve(&path)?;
        let m = std::fs::metadata(p).map_err(io_code)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&m),
        })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let Some(Open::File(f)) = self.handles.get(&handle) else {
            return Err(StatusCode::Failure.into());
        };
        let m = f.metadata().map_err(io_code)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&m),
        })
    }

    async fn setstat(
        &mut self,
        id: u32,
        path: String,
        attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        let p = self.resolve(&path)?;
        #[cfg(unix)]
        if let Some(mode) = attrs.permissions {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode & 0o7777))
                .map_err(io_code)?;
        }
        let _ = (&p, &attrs);
        Ok(ok(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let p = self.resolve(&path)?;
        let mut files = Vec::new();
        for e in std::fs::read_dir(&p).map_err(io_code)? {
            let e = e.map_err(io_code)?;
            let m = e.metadata().map_err(io_code)?;
            files.push(File::new(
                e.file_name().to_string_lossy().to_string(),
                FileAttributes::from(&m),
            ));
        }
        let handle = self.handle(Open::Dir(Some(files)));
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        match self.handles.get_mut(&handle) {
            Some(Open::Dir(files)) => match files.take() {
                Some(files) if !files.is_empty() => Ok(Name { id, files }),
                _ => Err(StatusCode::Eof.into()),
            },
            _ => Err(StatusCode::Failure.into()),
        }
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        std::fs::remove_file(self.resolve(&filename)?).map_err(io_code)?;
        Ok(ok(id))
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _a: FileAttributes,
    ) -> Result<Status, Self::Error> {
        std::fs::create_dir(self.resolve(&path)?).map_err(io_code)?;
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        std::fs::remove_dir(self.resolve(&path)?).map_err(io_code)?;
        Ok(ok(id))
    }

    async fn readlink(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let target = std::fs::read_link(self.resolve(&path)?).map_err(io_code)?;
        Ok(Name {
            id,
            files: vec![File::dummy(target.to_string_lossy().to_string())],
        })
    }

    async fn symlink(
        &mut self,
        id: u32,
        linkpath: String,
        targetpath: String,
    ) -> Result<Status, Self::Error> {
        // OpenSSH order: the first argument is the target, the second the
        // link (russh-sftp names them the other way round).
        let (target, link) = (linkpath, targetpath);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, self.resolve(&link)?).map_err(io_code)?;
            Ok(ok(id))
        }
        #[cfg(not(unix))]
        {
            let _ = (id, target, link);
            Err(StatusCode::OpUnsupported.into())
        }
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let p = self.resolve(&path)?;
        let rel = p.strip_prefix(&self.root).unwrap_or(Path::new(""));
        let s = format!("/{}", rel.to_string_lossy());
        Ok(Name {
            id,
            files: vec![File::dummy(s)],
        })
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        let to = self.resolve(&newpath)?;
        // OpenSSH implements RENAME with link()+unlink(): no overwrite.
        if std::fs::symlink_metadata(&to).is_ok() {
            return Err(StatusCode::Failure.into());
        }
        std::fs::rename(self.resolve(&oldpath)?, to).map_err(io_code)?;
        Ok(ok(id))
    }
}
