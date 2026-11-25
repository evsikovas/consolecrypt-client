use super::*;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::{Instant, timeout, timeout_at};

const LIMIT: Duration = Duration::from_secs(5);

fn destination(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(bare) = value.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return bare
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .map(|a| a.to_string());
    }
    if let Ok(ip) = value.parse::<IpAddr>() {
        return Some(ip.to_string());
    }
    let dns = value.strip_suffix('.').unwrap_or(value);
    let valid = dns.len() <= 253
        && !dns.bytes().all(|c| c.is_ascii_digit() || c == b'.')
        && dns.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        });
    valid.then(|| value.to_owned())
}

fn failure(e: std::io::Error) -> &'static str {
    match e.kind() {
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset => "refused",
        ErrorKind::NetworkUnreachable | ErrorKind::HostUnreachable => "unreachable",
        ErrorKind::TimedOut => "timeout",
        _ => "socket_error",
    }
}

async fn local(host: &str, port: u16, udp: bool) -> &'static str {
    let end = Instant::now() + LIMIT;
    let addresses: Vec<_> = match timeout_at(end, tokio::net::lookup_host((host, port))).await {
        Ok(Ok(iter)) => iter.collect(),
        _ => return "dns_error",
    };
    let Some(first) = addresses.first() else {
        return "dns_error";
    };
    if !udp {
        let mut result = "timeout";
        for (i, address) in addresses.iter().enumerate() {
            let remaining = end.saturating_duration_since(Instant::now());
            let attempt = remaining / (addresses.len() - i) as u32;
            result = match timeout(attempt, TcpStream::connect(address)).await {
                Ok(Ok(_)) => return "connected",
                Ok(Err(e)) => failure(e),
                Err(_) => "timeout",
            };
        }
        return result;
    }
    let bind: SocketAddr = if first.is_ipv6() {
        ([0u16; 8], 0).into()
    } else {
        ([0u8; 4], 0).into()
    };
    let exchange = async {
        let socket = UdpSocket::bind(bind).await?;
        socket.connect(first).await?;
        socket.send(&[0]).await?;
        socket.recv(&mut vec![0u8; 65535]).await
    };
    match timeout_at(end, exchange).await {
        Ok(Ok(_)) => "reply",
        Ok(Err(e)) => failure(e),
        Err(_) => "no_reply",
    }
}

impl AppCore {
    pub async fn port_check(
        &self,
        source_id: Option<String>,
        host: String,
        port: u16,
        udp: bool,
    ) -> AppResult<&'static str> {
        let host =
            destination(&host).ok_or_else(|| AppError::invalid("host", "invalid destination"))?;
        if port == 0 {
            return Err(AppError::invalid("port", "invalid port"));
        }
        let Some(source) = source_id else {
            let (_, u) = self.unlocked().await?;
            let mut shutdown = u.sharing_shutdown.subscribe();
            if *shutdown.borrow() {
                return Err(AppError::VaultLocked);
            }
            return tokio::select! {
                biased;
                _ = shutdown.changed() => Err(AppError::VaultLocked),
                value = local(&host, port, udp) => Ok(value),
            };
        };
        let prepared = self.sharing_prepare_connection(&source).await?;
        let script = include_str!("port_check.py")
            .replace("\r\n", "\n")
            .replace('\'', "'\\''");
        let protocol = if udp { "udp" } else { "tcp" };
        let command = format!(
            "command -v python3 >/dev/null || exit 127; exec python3 -c '{script}' '{host}' {port} {protocol}"
        );
        prepared
            .run(async {
                let session = prepared
                    .unlocked
                    .ssh
                    .connector
                    .connect(&prepared.plan)
                    .await?;
                let result = session
                    .exec_bounded(&command, 2048, tokio::time::sleep(Duration::from_secs(10)))
                    .await;
                let _ = timeout(Duration::from_secs(1), session.disconnect()).await;
                let Some(out) = result? else {
                    return Ok("remote_error");
                };
                if out.exit_status == Some(127) {
                    return Ok("no_python");
                }
                if out.exit_status != Some(0) {
                    return Ok("remote_error");
                }
                Ok(match out.stdout_lossy().trim() {
                    "connected" if !udp => "connected",
                    "reply" if udp => "reply",
                    "no_reply" if udp => "no_reply",
                    "refused" => "refused",
                    "unreachable" => "unreachable",
                    "timeout" => "timeout",
                    "dns_error" => "dns_error",
                    "socket_error" => "socket_error",
                    _ => "remote_error",
                })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_a_single_destination() {
        for bad in [
            "",
            "a,b",
            "-n",
            "a;ls",
            "a'",
            "10.0.0.0/24",
            "https://host",
            "127.1",
            "[name]",
        ] {
            assert!(destination(bad).is_none(), "{bad}");
        }
        for host in [
            "localhost",
            "api.example.",
            "192.0.2.1",
            "[::1]",
            "2001:db8::1",
        ] {
            assert!(destination(host).is_some());
        }
    }

    #[tokio::test]
    async fn distinguishes_tcp_connect_and_udp_reply() {
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = tcp.local_addr().unwrap().port();
        assert_eq!(local("127.0.0.1", port, false).await, "connected");
        drop(tcp);
        assert_eq!(local("127.0.0.1", port, false).await, "refused");
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = udp.local_addr().unwrap().port();
        let echo = tokio::spawn(async move {
            let (n, peer) = udp.recv_from(&mut [0; 8]).await.unwrap();
            assert_eq!(n, 1);
            udp.send_to(b"ok", peer).await.unwrap();
            udp
        });
        assert_eq!(local("127.0.0.1", port, true).await, "reply");
        let _silent = echo.await.unwrap();
        assert_eq!(local("127.0.0.1", port, true).await, "no_reply");
    }
}
