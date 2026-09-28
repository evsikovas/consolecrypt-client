//! Minimal SOCKS5 (RFC 1928/1929) and HTTP CONNECT client used to reach the
//! first hop through a network proxy.

use crate::error::SshError;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use cc_models::host::{Proxy, ProxyKind};
use secrecy::{ExposeSecret, SecretString};
use std::net::IpAddr;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use zeroize::Zeroizing;

/// Connect to `target_host:target_port` through `proxy`.
pub async fn connect_via_proxy(
    proxy: &Proxy,
    password: Option<&SecretString>,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, SshError> {
    let mut stream = TcpStream::connect((proxy.address.as_str(), proxy.port))
        .await
        .map_err(|source| SshError::Connect {
            host: proxy.address.clone(),
            port: proxy.port,
            source,
        })?;
    let _ = stream.set_nodelay(true);
    let creds = proxy
        .username
        .as_deref()
        .map(|u| (u, password.map(|p| p.expose_secret()).unwrap_or("")));
    match proxy.kind {
        ProxyKind::Socks5 => socks5_connect(&mut stream, creds, target_host, target_port).await?,
        ProxyKind::HttpConnect => {
            stream = http_connect(stream, creds, target_host, target_port).await?
        }
    }
    Ok(stream)
}

fn perr(msg: impl Into<String>) -> SshError {
    SshError::Proxy(msg.into())
}

/// SOCKS5 CONNECT handshake on an established stream.
pub async fn socks5_connect<S>(
    stream: &mut S,
    creds: Option<(&str, &str)>,
    host: &str,
    port: u16,
) -> Result<(), SshError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let methods: &[u8] = if creds.is_some() {
        &[0x00, 0x02]
    } else {
        &[0x00]
    };
    let mut hello = vec![0x05, methods.len() as u8];
    hello.extend_from_slice(methods);
    stream.write_all(&hello).await?;
    let mut resp = [0u8; 2];
    stream.read_exact(&mut resp).await?;
    if resp[0] != 0x05 {
        return Err(perr("not a SOCKS5 proxy"));
    }
    match resp[1] {
        0x00 => {}
        0x02 => {
            let (user, pass) = creds.ok_or_else(|| perr("proxy requires authentication"))?;
            if user.len() > 255 || pass.len() > 255 {
                return Err(perr("proxy username/password too long"));
            }
            let mut msg = Zeroizing::new(Vec::with_capacity(3 + user.len() + pass.len()));
            msg.push(0x01);
            msg.push(user.len() as u8);
            msg.extend_from_slice(user.as_bytes());
            msg.push(pass.len() as u8);
            msg.extend_from_slice(pass.as_bytes());
            stream.write_all(&msg).await?;
            let mut r = [0u8; 2];
            stream.read_exact(&mut r).await?;
            if r[1] != 0x00 {
                return Err(perr("proxy authentication failed"));
            }
        }
        0xff => return Err(perr("proxy accepted none of our authentication methods")),
        m => return Err(perr(format!("proxy selected unsupported method {m:#x}"))),
    }

    let mut req = vec![0x05, 0x01, 0x00];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            req.push(0x01);
            req.extend_from_slice(&v4.octets());
        }
        Ok(IpAddr::V6(v6)) => {
            req.push(0x04);
            req.extend_from_slice(&v6.octets());
        }
        Err(_) => {
            if host.len() > 255 {
                return Err(perr("target host name too long for SOCKS5"));
            }
            req.push(0x03);
            req.push(host.len() as u8);
            req.extend_from_slice(host.as_bytes());
        }
    }
    req.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&req).await?;

    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await?;
    if head[1] != 0x00 {
        return Err(perr(format!(
            "proxy refused CONNECT to {host}:{port}: {}",
            socks5_reply_text(head[1])
        )));
    }
    let skip = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut l = [0u8; 1];
            stream.read_exact(&mut l).await?;
            l[0] as usize
        }
        t => return Err(perr(format!("bad SOCKS5 address type {t:#x}"))),
    };
    let mut rest = vec![0u8; skip + 2];
    stream.read_exact(&mut rest).await?;
    Ok(())
}

/// Human-readable SOCKS5 reply code.
pub fn socks5_reply_text(code: u8) -> &'static str {
    match code {
        0x01 => "general failure",
        0x02 => "connection not allowed by ruleset",
        0x03 => "network unreachable",
        0x04 => "host unreachable",
        0x05 => "connection refused",
        0x06 => "TTL expired",
        0x07 => "command not supported",
        0x08 => "address type not supported",
        _ => "unknown error",
    }
}

/// HTTP CONNECT handshake. Returns the stream positioned after the response
/// headers.
pub async fn http_connect(
    stream: TcpStream,
    creds: Option<(&str, &str)>,
    host: &str,
    port: u16,
) -> Result<TcpStream, SshError> {
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut req = Zeroizing::new(format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: ConsoleCrypt\r\n"
    ));
    if let Some((u, p)) = creds {
        let token = Zeroizing::new(B64.encode(format!("{u}:{p}")));
        req.push_str("Proxy-Authorization: Basic ");
        req.push_str(&token);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    let mut reader = BufReader::new(stream);
    reader.get_mut().write_all(req.as_bytes()).await?;

    let mut status = String::new();
    reader.read_line(&mut status).await?;
    let code = status
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| perr("malformed HTTP proxy response"))?;
    // Drain headers.
    let mut total = 0usize;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        total += n;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if total > 64 * 1024 {
            return Err(perr("HTTP proxy response headers too large"));
        }
    }
    if !(200..300).contains(&code) {
        return Err(perr(format!(
            "HTTP proxy refused CONNECT to {authority}: {}",
            status.trim()
        )));
    }
    if !reader.buffer().is_empty() {
        return Err(perr("HTTP proxy sent unexpected data after CONNECT"));
    }
    Ok(reader.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    fn proxy(kind: ProxyKind, port: u16, user: Option<&str>) -> Proxy {
        let now = chrono::Utc::now();
        Proxy {
            id: cc_models::ObjectId::new(),
            name: "p".into(),
            kind,
            address: "127.0.0.1".into(),
            port,
            username: user.map(Into::into),
            password_secret_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Tiny SOCKS5 server supporting user/pass; connects to the requested target.
    async fn socks_server(expect_auth: Option<(&'static str, &'static str)>) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut h = [0u8; 2];
            s.read_exact(&mut h).await.unwrap();
            let mut m = vec![0u8; h[1] as usize];
            s.read_exact(&mut m).await.unwrap();
            if let Some((u, p)) = expect_auth {
                s.write_all(&[5, 2]).await.unwrap();
                let mut v = [0u8; 2];
                s.read_exact(&mut v).await.unwrap();
                let mut user = vec![0u8; v[1] as usize];
                s.read_exact(&mut user).await.unwrap();
                let mut pl = [0u8; 1];
                s.read_exact(&mut pl).await.unwrap();
                let mut pass = vec![0u8; pl[0] as usize];
                s.read_exact(&mut pass).await.unwrap();
                let ok = user == u.as_bytes() && pass == p.as_bytes();
                s.write_all(&[1, if ok { 0 } else { 1 }]).await.unwrap();
                if !ok {
                    return;
                }
            } else {
                s.write_all(&[5, 0]).await.unwrap();
            }
            let mut head = [0u8; 4];
            s.read_exact(&mut head).await.unwrap();
            let host = match head[3] {
                1 => {
                    let mut a = [0u8; 4];
                    s.read_exact(&mut a).await.unwrap();
                    std::net::Ipv4Addr::from(a).to_string()
                }
                3 => {
                    let mut l = [0u8; 1];
                    s.read_exact(&mut l).await.unwrap();
                    let mut n = vec![0u8; l[0] as usize];
                    s.read_exact(&mut n).await.unwrap();
                    String::from_utf8(n).unwrap()
                }
                _ => panic!("unexpected atyp"),
            };
            let mut p = [0u8; 2];
            s.read_exact(&mut p).await.unwrap();
            let port = u16::from_be_bytes(p);
            let mut up = TcpStream::connect((host.as_str(), port)).await.unwrap();
            s.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                .await
                .unwrap();
            let _ = tokio::io::copy_bidirectional(&mut s, &mut up).await;
        });
        port
    }

    async fn echo_server() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
        port
    }

    async fn roundtrip(mut s: TcpStream) {
        s.write_all(b"ping").await.unwrap();
        let mut b = [0u8; 4];
        s.read_exact(&mut b).await.unwrap();
        assert_eq!(&b, b"ping");
    }

    #[tokio::test]
    async fn socks5_no_auth_and_domain_target() {
        let echo = echo_server().await;
        let sp = socks_server(None).await;
        let s = connect_via_proxy(&proxy(ProxyKind::Socks5, sp, None), None, "localhost", echo)
            .await
            .unwrap();
        roundtrip(s).await;
    }

    #[tokio::test]
    async fn socks5_user_pass() {
        let echo = echo_server().await;
        let sp = socks_server(Some(("alice", "pw"))).await;
        let pw = SecretString::from("pw");
        let s = connect_via_proxy(
            &proxy(ProxyKind::Socks5, sp, Some("alice")),
            Some(&pw),
            "127.0.0.1",
            echo,
        )
        .await
        .unwrap();
        roundtrip(s).await;

        let sp = socks_server(Some(("alice", "pw"))).await;
        let bad = SecretString::from("wrong");
        let err = connect_via_proxy(
            &proxy(ProxyKind::Socks5, sp, Some("alice")),
            Some(&bad),
            "127.0.0.1",
            echo,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SshError::Proxy(_)), "{err}");
        assert!(!err.to_string().contains("wrong"));
    }

    #[tokio::test]
    async fn http_connect_ok_and_refused() {
        let echo = echo_server().await;
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hp = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            for i in 0..2 {
                let (s, _) = l.accept().await.unwrap();
                let mut r = BufReader::new(s);
                let mut first = String::new();
                r.read_line(&mut first).await.unwrap();
                let mut auth = false;
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).await.unwrap();
                    if line.starts_with("Proxy-Authorization: Basic ") {
                        auth = line.trim().ends_with(&B64.encode("bob:pw"));
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut s = r.into_inner();
                if i == 0 && auth {
                    let target = first.split_whitespace().nth(1).unwrap().to_string();
                    let mut up = TcpStream::connect(target).await.unwrap();
                    s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await
                        .unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut s, &mut up).await;
                } else {
                    s.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                        .await
                        .unwrap();
                }
            }
        });
        let pw = SecretString::from("pw");
        let p = proxy(ProxyKind::HttpConnect, hp, Some("bob"));
        let s = connect_via_proxy(&p, Some(&pw), "127.0.0.1", echo)
            .await
            .unwrap();
        roundtrip(s).await;
        let err = connect_via_proxy(&p, None, "127.0.0.1", echo)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("407"), "{err}");
    }
}
