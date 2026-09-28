//! The remote file system as seen by the edit engine.

use crate::{
    copy_with_progress, io_to_sftp, map_err, CancelToken, RemoteEntry, SftpClient, SftpError,
    TransferOptions, TransferProgress,
};
use async_trait::async_trait;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncWrite, AsyncWriteExt};

/// Progress callback of a transfer.
pub type ProgressFn<'a> = &'a mut (dyn FnMut(TransferProgress) + Send);

/// Operations the edit engine needs from the server. Implemented for
/// [`SftpClient`] (so `Arc<SftpClient>` coerces to `Arc<dyn EditRemote>`);
/// tests use an in-memory fake.
#[async_trait]
pub trait EditRemote: Send + Sync + 'static {
    /// Stat, following symlinks.
    async fn stat(&self, path: &str) -> Result<RemoteEntry, SftpError>;
    /// Stat without following symlinks.
    async fn lstat(&self, path: &str) -> Result<RemoteEntry, SftpError>;
    /// Resolve `path` to an absolute path without symlinks (`realpath`).
    async fn canonicalize(&self, path: &str) -> Result<String, SftpError>;
    /// Read a whole file; fails once more than `max_len` bytes arrive.
    async fn read_file(
        &self,
        path: &str,
        max_len: u64,
        progress: ProgressFn<'_>,
    ) -> Result<Vec<u8>, SftpError>;
    /// Create `path` exclusively (must not exist) with owner-only permissions
    /// and write `data`; a partial file is removed on failure.
    async fn write_new_file(
        &self,
        path: &str,
        data: &[u8],
        progress: ProgressFn<'_>,
    ) -> Result<(), SftpError>;
    /// Truncate and rewrite `path` in place (keeps owner, mode and inode).
    async fn overwrite_file(
        &self,
        path: &str,
        data: &[u8],
        progress: ProgressFn<'_>,
    ) -> Result<(), SftpError>;
    async fn chmod(&self, path: &str, mode: u32) -> Result<(), SftpError>;
    async fn remove_file(&self, path: &str) -> Result<(), SftpError>;
    /// Plain SFTP rename (may refuse to overwrite, as OpenSSH does).
    async fn rename(&self, from: &str, to: &str) -> Result<(), SftpError>;
    /// Whether [`EditRemote::posix_rename`] is available.
    fn supports_posix_rename(&self) -> bool;
    /// Rename replacing `to` atomically (`posix-rename@openssh.com`).
    async fn posix_rename(&self, from: &str, to: &str) -> Result<(), SftpError>;
}

/// `Vec` writer with a size cap.
struct CappedBuf {
    buf: Vec<u8>,
    max: u64,
}

impl AsyncWrite for CappedBuf {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if (self.buf.len() + data.len()) as u64 > self.max {
            return Poll::Ready(Err(std::io::Error::other(
                "file exceeds the edit size limit",
            )));
        }
        self.buf.extend_from_slice(data);
        Poll::Ready(Ok(data.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl SftpClient {
    async fn write_opened(
        &self,
        path: &str,
        flags: OpenFlags,
        attrs: FileAttributes,
        data: &[u8],
        progress: ProgressFn<'_>,
        remove_on_error: bool,
    ) -> Result<(), SftpError> {
        let mut f = self
            .inner
            .open_with_flags_and_attributes(path, flags, attrs)
            .await
            .map_err(|e| map_err(path, e))?;
        let chunk = TransferOptions::default().chunk_size;
        let mut reader = data;
        let r = copy_with_progress(
            &mut reader,
            &mut f,
            Some(data.len() as u64),
            chunk,
            progress,
            &CancelToken::new(),
        )
        .await;
        let close = f.shutdown().await;
        let result = match r {
            Ok(_) => close.map_err(|e| io_to_sftp(path, e)),
            Err(Ok(())) => Err(SftpError::Cancelled),
            Err(Err(e)) => Err(io_to_sftp(path, e)),
        };
        if result.is_err() && remove_on_error {
            let _ = self.inner.remove_file(path).await;
        }
        result
    }
}

#[async_trait]
impl EditRemote for SftpClient {
    async fn stat(&self, path: &str) -> Result<RemoteEntry, SftpError> {
        SftpClient::stat(self, path).await
    }

    async fn lstat(&self, path: &str) -> Result<RemoteEntry, SftpError> {
        SftpClient::lstat(self, path).await
    }

    async fn canonicalize(&self, path: &str) -> Result<String, SftpError> {
        SftpClient::canonicalize(self, path).await
    }

    async fn read_file(
        &self,
        path: &str,
        max_len: u64,
        progress: ProgressFn<'_>,
    ) -> Result<Vec<u8>, SftpError> {
        let mut w = CappedBuf {
            buf: Vec::new(),
            max: max_len,
        };
        self.download_to(
            path,
            &mut w,
            &TransferOptions::default(),
            progress,
            &CancelToken::new(),
        )
        .await?;
        Ok(w.buf)
    }

    async fn write_new_file(
        &self,
        path: &str,
        data: &[u8],
        progress: ProgressFn<'_>,
    ) -> Result<(), SftpError> {
        let attrs = FileAttributes {
            permissions: Some(0o600),
            ..FileAttributes::empty()
        };
        let flags = OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE;
        self.write_opened(path, flags, attrs, data, progress, true)
            .await
    }

    async fn overwrite_file(
        &self,
        path: &str,
        data: &[u8],
        progress: ProgressFn<'_>,
    ) -> Result<(), SftpError> {
        let flags = OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE;
        self.write_opened(path, flags, FileAttributes::empty(), data, progress, false)
            .await
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<(), SftpError> {
        SftpClient::chmod(self, path, mode).await
    }

    async fn remove_file(&self, path: &str) -> Result<(), SftpError> {
        SftpClient::remove_file(self, path).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        SftpClient::rename(self, from, to).await
    }

    fn supports_posix_rename(&self) -> bool {
        SftpClient::supports_posix_rename(self)
    }

    async fn posix_rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        SftpClient::posix_rename(self, from, to).await
    }
}
