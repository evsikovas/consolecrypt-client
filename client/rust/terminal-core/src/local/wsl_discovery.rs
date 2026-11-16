//! Data-only result of the existing fixed `wsl --list --quiet` invocation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WslDiscoveryStatus {
    ExecutableMissing,
    SpawnFailed,
    TimedOut,
    IoFailed,
    OutputTooLarge,
    NonzeroExit,
    Empty,
    Available,
}

#[cfg(any(target_os = "windows", test))]
const MAX_LIST_BYTES: usize = 128 * 1024;

#[cfg(any(target_os = "windows", test))]
pub(super) struct WslList {
    pub names: Vec<String>,
    pub status: WslDiscoveryStatus,
}

#[cfg(any(target_os = "windows", test))]
impl WslList {
    pub fn failed(status: WslDiscoveryStatus) -> Self {
        Self {
            names: Vec::new(),
            status,
        }
    }
    pub fn succeeded(&self) -> bool {
        matches!(
            self.status,
            WslDiscoveryStatus::Empty | WslDiscoveryStatus::Available
        )
    }
}

/// Bound the SAME stdout and exit observation; neither partial output nor a
/// failed process may be reported as a successful empty distribution list.
#[cfg(any(target_os = "windows", test))]
pub(super) async fn read_list<R, W>(stdout: R, wait: W, deadline: std::time::Duration) -> WslList
where
    R: tokio::io::AsyncRead + Unpin,
    W: std::future::Future<Output = std::io::Result<bool>>,
{
    use tokio::io::AsyncReadExt;
    let collected = tokio::time::timeout(deadline, async {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_LIST_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| WslDiscoveryStatus::IoFailed)?;
        if bytes.len() > MAX_LIST_BYTES {
            return Err(WslDiscoveryStatus::OutputTooLarge);
        }
        if !wait.await.map_err(|_| WslDiscoveryStatus::IoFailed)? {
            return Err(WslDiscoveryStatus::NonzeroExit);
        }
        decode_wsl_names(&bytes)
    })
    .await;
    match collected {
        Err(_) => WslList::failed(WslDiscoveryStatus::TimedOut),
        Ok(Err(status)) => WslList::failed(status),
        Ok(Ok(names)) => WslList {
            status: if names.is_empty() {
                WslDiscoveryStatus::Empty
            } else {
                WslDiscoveryStatus::Available
            },
            names,
        },
    }
}

/// `wsl --list --quiet` emits UTF-16LE on many Windows releases, UTF-8 on
/// others. Preserve exact distro names and pass them as one argv element.
#[cfg(any(target_os = "windows", test))]
pub(super) fn decode_wsl_names(bytes: &[u8]) -> Result<Vec<String>, WslDiscoveryStatus> {
    let text =
        if bytes.starts_with(&[0xff, 0xfe]) || bytes.iter().skip(1).step_by(2).any(|b| *b == 0) {
            let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
            if bytes.len() % 2 != 0 {
                return Err(WslDiscoveryStatus::IoFailed);
            }
            String::from_utf16(
                &bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| WslDiscoveryStatus::IoFailed)?
        } else {
            std::str::from_utf8(bytes)
                .map_err(|_| WslDiscoveryStatus::IoFailed)?
                .to_owned()
        };
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return Err(WslDiscoveryStatus::IoFailed);
    }
    let mut names = Vec::new();
    for name in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if name.chars().any(char::is_control) {
            return Err(WslDiscoveryStatus::IoFailed);
        }
        if !name.is_empty() && !names.iter().any(|s| s == name) {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Context, Poll},
        time::Duration,
    };
    use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};

    const LIMIT: Duration = Duration::from_secs(1);

    #[tokio::test]
    async fn malformed_encodings_never_claim_empty_or_available() {
        for bytes in [
            vec![0xff],
            vec![0xff, 0xfe, 0x41],
            vec![0xff, 0xfe, 0x00, 0xd8],
        ] {
            let result = read_list(bytes.as_slice(), async { Ok(true) }, LIMIT).await;
            assert_eq!(result.status, WslDiscoveryStatus::IoFailed);
            assert!(!result.succeeded());
            assert!(result.names.is_empty());
        }
    }

    #[tokio::test]
    async fn invalid_control_line_discards_other_valid_names() {
        for bytes in [
            b"\0".as_slice(),
            b"Ubuntu\n\0\n".as_slice(),
            b"Ubuntu\nBad\tName\n".as_slice(),
        ] {
            let result = read_list(bytes, async { Ok(true) }, LIMIT).await;
            assert_eq!(result.status, WslDiscoveryStatus::IoFailed);
            assert!(result.names.is_empty());
        }
    }

    #[tokio::test]
    async fn bom_utf16_unicode_only_name_remains_available() {
        let name = "\u{0414}\u{0435}\u{0431}\u{0438}\u{0430}\u{043d}";
        let bytes: Vec<_> = [0xff, 0xfe]
            .into_iter()
            .chain(name.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let result = read_list(bytes.as_slice(), async { Ok(true) }, LIMIT).await;
        assert_eq!(result.status, WslDiscoveryStatus::Available);
        assert_eq!(result.names, vec![name]);
    }

    #[tokio::test]
    async fn successful_utf8_and_utf16_preserve_exact_names() {
        let text = "Ubuntu\r\nDistro with space\r\n\u{0414}\u{0435}\u{0431}\u{0438}\u{0430}\u{043d}\r\nUbuntu\r\n";
        let utf16: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        for bytes in [text.as_bytes(), utf16.as_slice()] {
            let result = read_list(bytes, async { Ok(true) }, LIMIT).await;
            assert_eq!(result.status, WslDiscoveryStatus::Available);
            assert!(result.succeeded());
            assert_eq!(
                result.names,
                vec![
                    "Ubuntu",
                    "Distro with space",
                    "\u{0414}\u{0435}\u{0431}\u{0438}\u{0430}\u{043d}"
                ]
            );
        }
    }

    #[tokio::test]
    async fn only_successful_empty_output_means_empty() {
        let empty = read_list(b" \r\n".as_slice(), async { Ok(true) }, LIMIT).await;
        assert_eq!(empty.status, WslDiscoveryStatus::Empty);
        assert!(empty.succeeded());
        for bytes in [b"".as_slice(), b"Ubuntu\n".as_slice()] {
            let failed = read_list(bytes, async { Ok(false) }, LIMIT).await;
            assert_eq!(failed.status, WslDiscoveryStatus::NonzeroExit);
            assert!(!failed.succeeded());
            assert!(failed.names.is_empty());
        }
    }

    #[tokio::test]
    async fn exit_observation_error_discards_collected_names() {
        let result = read_list(
            b"Ubuntu\n".as_slice(),
            async { Err(std::io::Error::other("synthetic")) },
            LIMIT,
        )
        .await;
        assert_eq!(result.status, WslDiscoveryStatus::IoFailed);
        assert!(result.names.is_empty());
    }

    struct FailingRead;
    impl AsyncRead for FailingRead {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Err(std::io::Error::other("synthetic")))
        }
    }
    #[tokio::test]
    async fn stdout_error_does_not_observe_exit_or_claim_empty() {
        let result = read_list(
            FailingRead,
            async { panic!("wait must not run after read failure") },
            LIMIT,
        )
        .await;
        assert_eq!(result.status, WslDiscoveryStatus::IoFailed);
        assert!(!result.succeeded());
        assert!(result.names.is_empty());
    }

    #[tokio::test]
    async fn oversize_stops_before_wait_and_discards_names() {
        let bytes = vec![b'x'; MAX_LIST_BYTES + 1];
        let result = read_list(
            bytes.as_slice(),
            async { panic!("oversize must not await exit") },
            LIMIT,
        )
        .await;
        assert_eq!(result.status, WslDiscoveryStatus::OutputTooLarge);
        assert!(result.names.is_empty());
    }

    #[tokio::test]
    async fn exact_output_limit_is_still_supported() {
        let bytes = vec![b'x'; MAX_LIST_BYTES];
        let result = read_list(bytes.as_slice(), async { Ok(true) }, LIMIT).await;
        assert_eq!(result.status, WslDiscoveryStatus::Available);
        assert_eq!(result.names[0].len(), MAX_LIST_BYTES);
    }

    #[tokio::test]
    async fn blocked_stdout_timeout_closes_reader_without_waiting_for_exit() {
        let (mut writer, reader) = tokio::io::duplex(16);
        let result = read_list(
            reader,
            async { panic!("no EOF: exit must not yet be observed") },
            Duration::from_millis(10),
        )
        .await;
        assert_eq!(result.status, WslDiscoveryStatus::TimedOut);
        assert!(result.names.is_empty());
        assert!(
            writer.write_all(b"late").await.is_err(),
            "timeout must drop the pipe reader"
        );
    }

    #[tokio::test]
    async fn exit_wait_timeout_discards_real_collected_output_and_waiter() {
        struct Guard(Arc<AtomicUsize>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let guard = Guard(drops.clone());
        let polls = Arc::new(AtomicUsize::new(0));
        let observed_polls = polls.clone();
        let wait = async move {
            let _guard = guard;
            std::future::poll_fn(|_| {
                observed_polls.fetch_add(1, Ordering::SeqCst);
                Poll::<std::io::Result<bool>>::Pending
            })
            .await
        };
        let result = read_list(b"Ubuntu\n".as_slice(), wait, Duration::from_millis(10)).await;
        assert!(
            polls.load(Ordering::SeqCst) > 0,
            "actual wait future must be pending"
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(result.status, WslDiscoveryStatus::TimedOut);
        assert!(result.names.is_empty());
    }

    #[test]
    fn unavailable_provenance_never_succeeds_or_carries_names() {
        for status in [
            WslDiscoveryStatus::ExecutableMissing,
            WslDiscoveryStatus::SpawnFailed,
        ] {
            let result = WslList::failed(status);
            assert_eq!(result.status, status);
            assert!(!result.succeeded());
            assert!(result.names.is_empty());
        }
    }
}
