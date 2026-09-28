//! Minimal SOCKS5 server side (RFC 1928): no authentication, `CONNECT`
//! only, IPv4 / IPv6 / domain-name targets. Used by dynamic forwarding.

use std::net::{Ipv4Addr, Ipv6Addr};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// SOCKS5 reply codes.
pub mod reply {
    pub const SUCCEEDED: u8 = 0x00;
    pub const GENERAL_FAILURE: u8 = 0x01;
    pub const NOT_ALLOWED: u8 = 0x02;
    pub const NETWORK_UNREACHABLE: u8 = 0x03;
    pub const HOST_UNREACHABLE: u8 = 0x04;
    pub const CONNECTION_REFUSED: u8 = 0x05;
    pub const COMMAND_NOT_SUPPORTED: u8 = 0x07;
    pub const ADDRESS_TYPE_NOT_SUPPORTED: u8 = 0x08;
}

/// Handshake failures (the client has already been answered when possible).
#[derive(Debug, thiserror::Error)]
pub enum Socks5Error {
    #[error("not a SOCKS5 client (version byte {0:#x})")]
    Version(u8),
    #[error("client offered no acceptable authentication method")]
    NoAcceptableMethod,
    #[error("unsupported SOCKS5 command {0:#x}")]
    Command(u8),
    #[error("unsupported address type {0:#x}")]
    AddressType(u8),
    #[error("invalid domain name")]
    Domain,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Requested destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocksTarget {
    /// IP literal (IPv6 without brackets) or domain name.
    pub host: String,
    pub port: u16,
}

/// Run the server side of the handshake up to (not including) the final
/// reply. After connecting upstream call [`send_reply`].
pub async fn accept<S>(s: &mut S) -> Result<SocksTarget, Socks5Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let ver = s.read_u8().await?;
    if ver != 0x05 {
        return Err(Socks5Error::Version(ver));
    }
    let n = s.read_u8().await? as usize;
    let mut methods = vec![0u8; n];
    s.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        s.write_all(&[0x05, 0xff]).await?;
        return Err(Socks5Error::NoAcceptableMethod);
    }
    s.write_all(&[0x05, 0x00]).await?;

    let mut head = [0u8; 4];
    s.read_exact(&mut head).await?;
    if head[0] != 0x05 {
        return Err(Socks5Error::Version(head[0]));
    }
    let host = match head[3] {
        0x01 => {
            let mut a = [0u8; 4];
            s.read_exact(&mut a).await?;
            Ipv4Addr::from(a).to_string()
        }
        0x04 => {
            let mut a = [0u8; 16];
            s.read_exact(&mut a).await?;
            Ipv6Addr::from(a).to_string()
        }
        0x03 => {
            let len = s.read_u8().await? as usize;
            let mut name = vec![0u8; len];
            s.read_exact(&mut name).await?;
            match String::from_utf8(name) {
                Ok(n)
                    if !n.is_empty() && !n.chars().any(|c| c.is_control() || c.is_whitespace()) =>
                {
                    n
                }
                _ => {
                    send_reply(s, reply::GENERAL_FAILURE).await?;
                    return Err(Socks5Error::Domain);
                }
            }
        }
        t => {
            // Cannot know the address length: answer and give up.
            send_reply(s, reply::ADDRESS_TYPE_NOT_SUPPORTED).await?;
            return Err(Socks5Error::AddressType(t));
        }
    };
    let port = s.read_u16().await?;
    if head[1] != 0x01 {
        send_reply(s, reply::COMMAND_NOT_SUPPORTED).await?;
        return Err(Socks5Error::Command(head[1]));
    }
    Ok(SocksTarget { host, port })
}

/// Send the final reply (bound address reported as 0.0.0.0:0).
pub async fn send_reply<S>(s: &mut S, code: u8) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    s.write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    s.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn run(client_bytes: Vec<u8>) -> (Result<SocksTarget, Socks5Error>, Vec<u8>) {
        let (mut c, mut s) = duplex(1024);
        c.write_all(&client_bytes).await.unwrap();
        let r = accept(&mut s).await;
        drop(s);
        let mut out = Vec::new();
        c.read_to_end(&mut out).await.unwrap();
        (r, out)
    }

    #[tokio::test]
    async fn ipv4_domain_ipv6() {
        let (r, out) = run(vec![5, 1, 0, 5, 1, 0, 1, 10, 0, 0, 1, 0x1f, 0x90]).await;
        assert_eq!(
            r.unwrap(),
            SocksTarget {
                host: "10.0.0.1".into(),
                port: 8080
            }
        );
        assert_eq!(out, vec![5, 0]);

        let mut req = vec![5, 2, 2, 0, 5, 1, 0, 3, 9];
        req.extend_from_slice(b"localhost");
        req.extend_from_slice(&443u16.to_be_bytes());
        let (r, _) = run(req).await;
        assert_eq!(
            r.unwrap(),
            SocksTarget {
                host: "localhost".into(),
                port: 443
            }
        );

        let mut req = vec![5, 1, 0, 5, 1, 0, 4];
        req.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        req.extend_from_slice(&22u16.to_be_bytes());
        let (r, _) = run(req).await;
        assert_eq!(
            r.unwrap(),
            SocksTarget {
                host: "::1".into(),
                port: 22
            }
        );
    }

    #[tokio::test]
    async fn rejects_bad_requests() {
        // only user/pass offered
        let (r, out) = run(vec![5, 1, 2]).await;
        assert!(matches!(r, Err(Socks5Error::NoAcceptableMethod)));
        assert_eq!(out, vec![5, 0xff]);
        // BIND command
        let (r, out) = run(vec![5, 1, 0, 5, 2, 0, 1, 1, 2, 3, 4, 0, 80]).await;
        assert!(matches!(r, Err(Socks5Error::Command(2))));
        assert_eq!(out[2..4], [5, reply::COMMAND_NOT_SUPPORTED]);
        // unknown address type
        let (r, _) = run(vec![5, 1, 0, 5, 1, 0, 9]).await;
        assert!(matches!(r, Err(Socks5Error::AddressType(9))));
        // SOCKS4
        let (r, _) = run(vec![4, 1, 0, 80, 1, 2, 3, 4, 0]).await;
        assert!(matches!(r, Err(Socks5Error::Version(4))));
    }
}
