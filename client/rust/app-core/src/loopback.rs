//! RFC 8252 loopback redirect listener shared by the browser sign-ins
//! (Kubernetes OIDC, enterprise sign-in to the sync server).
//!
//! Accepts only `GET` requests from a loopback peer whose single `Host`
//! header names the listener exactly, and returns the first callback URL the
//! caller's `accept` check approves. The request bytes (which carry the
//! authorization code) are zeroized; the browser gets a static page.

use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

/// Why no callback arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopbackError {
    /// The listener failed.
    Unavailable,
    /// Cancelled by the user / the flow was discarded.
    Cancelled,
    /// The deadline passed.
    Timeout,
}

const OK_PAGE: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 27\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\nReturn to ConsoleCrypt now.";
const BAD_REQUEST: &str =
    "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";

/// `host:port` the browser must send for `redirect` (`127.0.0.1:p` or
/// `[::1]:p`).
fn expected_host(redirect: &Url) -> Option<String> {
    let port = redirect.port()?;
    Some(match redirect.host()? {
        url::Host::Ipv6(ip) => format!("[{ip}]:{port}"),
        host => format!("{host}:{port}"),
    })
}

/// Parse one HTTP request head into the callback URL, if acceptable.
pub(crate) fn parse_request(head: &[u8], redirect: &Url) -> Option<Url> {
    let header = std::str::from_utf8(head).ok()?;
    if !header.ends_with("\r\n\r\n") {
        return None;
    }
    let mut words = header.lines().next()?.split_whitespace();
    if words.next()? != "GET" {
        return None;
    }
    let path = words.next()?;
    if !path.starts_with('/') || path.starts_with("//") {
        return None;
    }
    let expected = expected_host(redirect)?;
    let hosts: Vec<_> = header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
        .collect();
    if hosts.len() != 1 || hosts[0].1.trim() != expected {
        return None;
    }
    redirect.join(path).ok()
}

/// Wait for the browser redirect on `listener` until `deadline` or `cancel`.
pub(crate) async fn await_callback(
    listener: &TcpListener,
    redirect: &Url,
    accept: impl Fn(&Url) -> bool,
    cancel: CancellationToken,
    deadline: Instant,
) -> Result<Url, LoopbackError> {
    let work = async {
        loop {
            let (mut stream, peer) = listener
                .accept()
                .await
                .map_err(|_| LoopbackError::Unavailable)?;
            if !peer.ip().is_loopback() {
                continue;
            }
            let request = tokio::time::timeout(Duration::from_secs(5), async {
                let mut bytes = Zeroizing::new(Vec::new());
                let mut byte = [0u8; 1];
                while bytes.len() < 32 * 1024 && !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).await?;
                    bytes.push(byte[0]);
                }
                Ok::<_, std::io::Error>(bytes)
            })
            .await;
            let callback = request
                .ok()
                .and_then(Result::ok)
                .and_then(|bytes| parse_request(&bytes, redirect))
                .filter(|url| accept(url));
            let response = if callback.is_some() {
                OK_PAGE
            } else {
                BAD_REQUEST
            };
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                stream.write_all(response.as_bytes()),
            )
            .await;
            if let Some(callback) = callback {
                return Ok(callback);
            }
        }
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(LoopbackError::Cancelled),
        result = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), work) =>
            result.map_err(|_| LoopbackError::Timeout)?,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_head_parsing() {
        let v4 = Url::parse("http://127.0.0.1:5000/auth/callback").unwrap();
        let ok =
            b"GET /auth/callback?login_code=x&state=s HTTP/1.1\r\nHost: 127.0.0.1:5000\r\n\r\n";
        let url = parse_request(ok, &v4).unwrap();
        assert_eq!(url.path(), "/auth/callback");
        assert_eq!(url.query(), Some("login_code=x&state=s"));
        // Wrong host / duplicated host / method / absolute-path tricks.
        for bad in [
            &b"GET /auth/callback HTTP/1.1\r\nHost: localhost:5000\r\n\r\n"[..],
            b"GET /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:5000\r\nHost: 127.0.0.1:5000\r\n\r\n",
            b"POST /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:5000\r\n\r\n",
            b"GET //evil.example/auth/callback HTTP/1.1\r\nHost: 127.0.0.1:5000\r\n\r\n",
            b"GET /auth/callback HTTP/1.1\r\nHost: 127.0.0.1:5000\r\n",
        ] {
            assert!(parse_request(bad, &v4).is_none());
        }
        let v6 = Url::parse("http://[::1]:6000/auth/callback").unwrap();
        let ok6 = b"GET /auth/callback?state=s HTTP/1.1\r\nHost: [::1]:6000\r\n\r\n";
        assert!(parse_request(ok6, &v6).is_some());
        assert!(parse_request(ok, &v6).is_none());
    }

    #[tokio::test]
    async fn cancel_and_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect = Url::parse(&format!(
            "http://127.0.0.1:{}/auth/callback",
            listener.local_addr().unwrap().port()
        ))
        .unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let r = await_callback(
            &listener,
            &redirect,
            |_| true,
            cancel,
            Instant::now() + Duration::from_secs(5),
        )
        .await;
        assert_eq!(r, Err(LoopbackError::Cancelled));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let r = await_callback(
            &listener,
            &redirect,
            |_| true,
            CancellationToken::new(),
            Instant::now() + Duration::from_millis(20),
        )
        .await;
        assert_eq!(r, Err(LoopbackError::Timeout));
    }
}
