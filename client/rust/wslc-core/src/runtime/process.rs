use crate::models::*;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

pub fn resolve_cli(explicit: Option<&str>) -> AppResult<PathBuf> {
    let candidates = if let Some(path) = explicit {
        vec![PathBuf::from(path)]
    } else {
        // Fixed trusted installation locations only; never current directory or relative PATH.
        vec![
            PathBuf::from(r"C:\Program Files\WSL\wslc.exe"),
            PathBuf::from(r"C:\Windows\System32\wslc.exe"),
        ]
    };
    candidates
        .into_iter()
        .find_map(|p| {
            if !p.is_absolute() || p.file_name()?.to_str()?.to_lowercase() != "wslc.exe" {
                return None;
            }
            let canonical = crate::desktop::simplify_path(&p.canonicalize().ok()?);
            canonical.is_file().then_some(canonical)
        })
        .ok_or_else(|| {
            AppError::new(
                "cli_missing",
                "resolve_cli",
                "Укажите абсолютный путь к установленному wslc.exe",
            )
        })
}

pub fn command(path: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(path);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    cmd
}

async fn bounded_read(stream: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    bounded_read_limit(stream, OUTPUT_LIMIT).await
}

async fn bounded_read_limit(
    mut stream: impl AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(data);
        }
        if data.len() + n > limit {
            return Err(std::io::Error::other("output_limit"));
        }
        data.extend_from_slice(&chunk[..n]);
    }
}

/// Raw stdout bytes with optional stdin data, for file transfer through `wslc exec`.
pub async fn run_bytes(
    path: &Path,
    args: &[String],
    input: Option<&[u8]>,
    budget: Duration,
    operation: &str,
    limit: usize,
) -> AppResult<Vec<u8>> {
    let mut cmd = command(path, args);
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().map_err(|e| {
        let mut error = AppError::new("spawn", operation, "Не удалось запустить WSLc");
        error.diagnostics = Some(e.to_string());
        error
    })?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let write = async {
        if let (Some(mut pipe), Some(data)) = (stdin, input) {
            pipe.write_all(data).await?;
            pipe.shutdown().await?;
        }
        Ok::<_, std::io::Error>(())
    };
    let result = tokio::time::timeout(budget, async {
        tokio::try_join!(
            child.wait(),
            bounded_read_limit(stdout, limit),
            bounded_read_limit(stderr, 64 * 1024),
            write
        )
    })
    .await;
    let (status, out, err, ()) = match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) if e.to_string() == "output_limit" => {
            let _ = child.kill().await;
            return Err(AppError::new(
                "too_large",
                operation,
                "Файл больше допустимого размера",
            ));
        }
        other => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(AppError::new(
                if other.is_err() {
                    "timeout"
                } else {
                    "output_or_io"
                },
                operation,
                "Передача данных через WSLc не завершена",
            ));
        }
    };
    if !status.success() {
        let mut error = AppError::new("runtime", operation, "WSLc завершился с ошибкой");
        error.diagnostics = Some(String::from_utf8_lossy(&err).chars().take(4096).collect());
        return Err(error);
    }
    Ok(out)
}

pub fn decode(bytes: &[u8], operation: &str) -> AppResult<String> {
    // wsl --version uses UTF-16LE; wslc redirects UTF-8. Reject damaged encodings.
    let utf16 = bytes.starts_with(&[0xff, 0xfe])
        || bytes
            .iter()
            .skip(1)
            .step_by(2)
            .take(32)
            .filter(|&&b| b == 0)
            .count()
            > 8;
    if utf16 {
        let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
        if !bytes.len().is_multiple_of(2) {
            return Err(AppError::new(
                "encoding",
                operation,
                "Повреждён UTF-16 вывод CLI",
            ));
        }
        let words: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&words)
            .map_err(|_| AppError::new("encoding", operation, "Повреждён UTF-16 вывод CLI"))
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(bytes)
                .to_vec(),
        )
        .map_err(|_| AppError::new("encoding", operation, "Повреждён UTF-8 вывод CLI"))
    }
}

pub async fn execute(
    path: &Path,
    args: &[String],
    budget: Duration,
    operation: &str,
) -> AppResult<String> {
    let started = Instant::now();
    let mut child = command(path, args).spawn().map_err(|e| {
        let mut error = AppError::new(
            "spawn",
            operation,
            "Не удалось запустить WSLc: проверьте путь и политики организации",
        );
        error.diagnostics = Some(e.to_string());
        error
    })?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let result = tokio::time::timeout(budget, async {
        tokio::try_join!(child.wait(), bounded_read(stdout), bounded_read(stderr))
    })
    .await;
    let (status, out, err) = match result {
        Ok(Ok(output)) => output,
        other => {
            // Only our CLI process is ended. No taskkill, WSL shutdown or container kill.
            let _ = child.kill().await;
            let _ = child.wait().await;
            let code = if other.is_err() {
                "timeout"
            } else {
                "output_or_io"
            };
            tracing::warn!(
                operation,
                code,
                elapsed_ms = started.elapsed().as_millis() as u64
            );
            return Err(AppError::new(
                code,
                operation,
                "Чтение CLI не завершено; результат изменяющей операции требует перепроверки",
            ));
        }
    };
    tracing::info!(
        operation,
        exit_code = status.code(),
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    if !status.success() {
        let mut error = AppError::new("runtime", operation, "WSLc завершился с ошибкой");
        // Preserve a bounded source diagnostic, never log it with tracing.
        let stderr = decode(&err, operation)?;
        error.diagnostics = Some(stderr.chars().take(4096).collect());
        return Err(error);
    }
    decode(&out, operation)
}
