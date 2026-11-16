//! Closed HTTP/1.1 codec for one Reader GET over the approved SSH peer.
//! No URI, redirect, cookies, decompression, retry or general HTTP client.
use super::{ReaderTransportError as Error, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::io::Write;
use tokio::io::{AsyncRead, AsyncReadExt};
use zeroize::Zeroizing;

pub(super) const HEAD_BYTES: usize = 16384;

pub(super) fn request(path: &'static str, port: u16, bundle: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if !matches!(path, "/v1/heartbeat" | "/v1/history")
        || port == 0
        || bundle.is_empty()
        || bundle.len() > 8192
    {
        return Err(Error::Rejected);
    }
    let encoded = Zeroizing::new(URL_SAFE_NO_PAD.encode(bundle));
    // Host never comes from a URL or response. The approved broker endpoint is
    // always Agent-local IPv4 loopback in this first slice.
    let mut out = Zeroizing::new(Vec::with_capacity(12000));
    write!(&mut *out, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-ConsoleCrypt-Reader: {}\r\nConnection: close\r\n\r\n", &*encoded)
        .map_err(|_| Error::Rejected)?;
    if out.len() > HEAD_BYTES {
        return Err(Error::Rejected);
    }
    Ok(out)
}

pub(super) struct HttpResponse {
    pub status: u16,
    pub body: Zeroizing<Vec<u8>>,
}

fn framing(head: &[u8], cap: usize) -> Result<(u16, usize)> {
    if head.len() > HEAD_BYTES || !head.ends_with(b"\r\n\r\n") || !head.is_ascii() {
        return Err(Error::Malformed);
    }
    let text = std::str::from_utf8(head).map_err(|_| Error::Malformed)?;
    let mut lines = text[..text.len() - 4].split("\r\n");
    let status = lines.next().ok_or(Error::Malformed)?;
    let bytes = status.as_bytes();
    // Current Agent BaseHTTPRequestHandler emits HTTP/1.0 responses even to
    // an HTTP/1.1 request. Both literal versions require explicit close/length.
    if bytes.len() < 13
        || (&bytes[..9] != b"HTTP/1.1 " && &bytes[..9] != b"HTTP/1.0 ")
        || bytes[12..].iter().any(|b| *b < 32 || *b == 127)
        || !bytes[9..12].iter().all(u8::is_ascii_digit)
        || bytes[12] != b' '
    {
        return Err(Error::Malformed);
    }
    let code: u16 = status[9..12].parse().map_err(|_| Error::Malformed)?;
    if !(200..=599).contains(&code) {
        return Err(Error::Malformed);
    }
    let mut length = None;
    let mut json = false;
    let mut connection = false;
    let mut seen = std::collections::BTreeSet::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(Error::Malformed)?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.bytes().any(|b| b < 32 || b == 127)
        {
            return Err(Error::Malformed);
        }
        let name = name.to_ascii_lowercase();
        // Duplicate fields and all transfer/content encodings are rejected,
        // even if a permissive HTTP implementation would combine them.
        if !seen.insert(name.clone()) {
            return Err(Error::Malformed);
        }
        let value = value.trim_matches(' ');
        match name.as_str() {
            "content-length" => {
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(Error::Malformed);
                }
                let n = value.parse::<usize>().map_err(|_| Error::Malformed)?;
                if n == 0 || n > cap {
                    return Err(Error::Malformed);
                }
                length = Some(n);
            }
            "content-type" => {
                json = value.eq_ignore_ascii_case("application/json")
                    || value.eq_ignore_ascii_case("application/json; charset=utf-8")
            }
            "connection" => {
                if !value.eq_ignore_ascii_case("close") {
                    return Err(Error::Malformed);
                }
                connection = true;
            }
            "transfer-encoding" | "content-encoding" => return Err(Error::Malformed),
            _ => {}
        }
    }
    if !json || !connection {
        return Err(Error::Malformed);
    }
    Ok((code, length.ok_or(Error::Malformed)?))
}

pub(super) async fn response<S: AsyncRead + Unpin>(
    stream: &mut S,
    cap: usize,
) -> Result<HttpResponse> {
    if !matches!(cap, 65536 | 262144) {
        return Err(Error::Rejected);
    }
    let mut head = Vec::with_capacity(1024);
    loop {
        if head.len() == HEAD_BYTES {
            return Err(Error::Malformed);
        }
        let mut byte = [0];
        if stream
            .read(&mut byte)
            .await
            .map_err(|_| Error::DataOutcomeUnknown)?
            != 1
        {
            return Err(Error::Malformed);
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let (status, length) = framing(&head, cap)?;
    let mut body = Zeroizing::new(vec![0; length]);
    stream
        .read_exact(&mut body)
        .await
        .map_err(|_| Error::DataOutcomeUnknown)?;
    let mut extra = [0];
    if stream
        .read(&mut extra)
        .await
        .map_err(|_| Error::DataOutcomeUnknown)?
        != 0
    {
        return Err(Error::Malformed);
    }
    Ok(HttpResponse { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_has_only_fixed_path_and_reader_family() {
        for p in ["/v1/heartbeat", "/v1/history"] {
            let b = request(p, 8765, b"{}").unwrap();
            let s = std::str::from_utf8(&b).unwrap();
            assert!(s.starts_with(&format!("GET {p} HTTP/1.1\r\n")));
            assert!(s.contains("X-ConsoleCrypt-Reader: e30\r\n"));
            assert!(!s.contains("X-ConsoleCrypt-Proof") && !s.contains("Authorization"));
            assert!(s.ends_with("\r\n\r\n"));
        }
        assert!(request("/v1/history?x=1", 1, b"{}").is_err());
        assert!(request("/v1/history", 0, b"{}").is_err());
        assert!(request("/v1/history", 1, &vec![0; 8193]).is_err());
    }
    #[test]
    fn strict_http_framing_cannot_smuggle_or_inflate() {
        let valid=b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n";
        assert_eq!(framing(valid, 65536).unwrap(), (200, 2));
        let mut v10 = valid.to_vec();
        v10[7] = b'0';
        assert_eq!(framing(&v10, 65536).unwrap(), (200, 2));
        for bad in [
            "HTTP/1.2 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\ncontent-length: 2\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 65537\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\n",
            "HTTP/1.1 100 Continue\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
        ] { assert!(framing(bad.as_bytes(),65536).is_err()); }
    }
    #[tokio::test]
    async fn complete_body_requires_exact_eof_no_extra_response() {
        let head="HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n";
        for (tail, ok) in [("{}", true), ("{", false), ("{}x", false)] {
            let wire = format!("{head}{tail}").into_bytes();
            let mut input = wire.as_slice();
            assert_eq!(response(&mut input, 65536).await.is_ok(), ok);
        }
    }
}
