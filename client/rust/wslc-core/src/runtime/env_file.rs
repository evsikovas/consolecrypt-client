//! Exclusive random environment files, removed even when a runtime future is
//! cancelled by profile locking. The CLI reads by path: close the write handle
//! before spawning it (Windows readers may not request FILE_SHARE_WRITE).
use crate::models::{AppError, AppResult};
use std::io::Write;

pub(crate) fn create(body: &[u8]) -> AppResult<tempfile::TempPath> {
    let write = || -> std::io::Result<tempfile::TempPath> {
        let mut file = tempfile::Builder::new()
            .prefix("consolecrypt-wslc-")
            .suffix(".env")
            .tempfile()?;
        file.write_all(body)?;
        file.flush()?;
        Ok(file.into_temp_path())
    };
    write().map_err(|_| {
        AppError::new(
            "env_file",
            "run",
            "Не удалось подготовить переменные окружения",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_exclusive_readable_by_the_cli_and_removed_on_drop() {
        let first = create(b"TOKEN=synthetic-one\n").unwrap();
        let second = create(b"TOKEN=synthetic-two\n").unwrap();
        assert_ne!(first.to_path_buf(), second.to_path_buf());
        assert_eq!(std::fs::read(&first).unwrap(), b"TOKEN=synthetic-one\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&first).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        let path = first.to_path_buf();
        drop(first);
        assert!(!path.exists());
        assert_eq!(std::fs::read(&second).unwrap(), b"TOKEN=synthetic-two\n");
    }

    #[tokio::test]
    async fn cancellation_removes_the_file_before_the_operation_can_complete() {
        let (send, receive) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let file = create(b"TOKEN=synthetic-cancelled\n").unwrap();
            send.send(file.to_path_buf()).unwrap();
            std::future::pending::<()>().await;
            drop(file);
        });
        let path = receive.await.unwrap();
        assert!(path.exists());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!path.exists());
    }
}
