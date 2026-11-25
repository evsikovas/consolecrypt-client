use super::*;
use std::collections::BTreeSet;
use std::time::Duration;
use tokio::{process::Command, time::timeout};

const SCRIPT: &str = include_str!("listening_ports.sh");
const TIMEOUT: Duration = Duration::from_secs(15);

fn parse(output: &[u8], low: u16, high: u16) -> AppResult<Vec<u16>> {
    let invalid = || AppError::Io("invalid socket table output".into());
    let text = std::str::from_utf8(output).map_err(|_| invalid())?;
    let ports = text
        .lines()
        .map(|line| {
            line.trim()
                .parse::<u16>()
                .ok()
                .filter(|p| (low..=high).contains(p))
                .ok_or_else(invalid)
        })
        .collect::<AppResult<BTreeSet<_>>>()?;
    Ok(ports.into_iter().collect())
}

async fn local(low: u16, high: u16, udp: bool) -> AppResult<Vec<u16>> {
    let mut command = if cfg!(windows) {
        let root = std::env::var_os("SystemRoot")
            .ok_or_else(|| AppError::Io("missing Windows system directory".into()))?;
        let ps =
            std::path::PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let method = if udp {
            "GetActiveUdpListeners"
        } else {
            "GetActiveTcpListeners"
        };
        let mut cmd = Command::new(ps);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &format!(
            "$ErrorActionPreference='Stop'; [Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().{method}() | ForEach-Object {{ $_.Port }} | Where-Object {{ $_ -ge {low} -and $_ -le {high} }} | Sort-Object -Unique"
        )]);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);
        cmd
    } else if cfg!(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd"
    )) {
        let mut cmd = Command::new("/bin/sh");
        cmd.args([
            "-c",
            &SCRIPT.replace("\r\n", "\n"),
            "cc-ports",
            if udp { "udp" } else { "tcp" },
            &low.to_string(),
            &high.to_string(),
        ]);
        cmd
    } else {
        return Err(AppError::Unsupported("local socket table".into()));
    };
    let out = timeout(TIMEOUT, command.kill_on_drop(true).output())
        .await
        .map_err(|_| AppError::Io("socket table timed out".into()))??;
    if !out.status.success() {
        return Err(AppError::Io("could not read local socket table".into()));
    }
    parse(&out.stdout, low, high)
}

impl AppCore {
    pub async fn socket_owners(
        &self,
        host_id: Option<String>,
        port: u16,
        udp: bool,
    ) -> AppResult<Vec<String>> {
        if port == 0 {
            return Err(AppError::invalid("port", "invalid port"));
        }
        let script = include_str!("socket_owner.sh").replace("\r\n", "\n");
        let protocol = if udp { "udp" } else { "tcp" };
        let names = |data: &[u8]| -> Vec<String> {
            String::from_utf8_lossy(data)
                .lines()
                .map(str::trim)
                .filter(|line| {
                    !line.is_empty()
                        && line.chars().count() <= 120
                        && !line.chars().any(char::is_control)
                })
                .take(16)
                .map(str::to_owned)
                .collect()
        };
        if let Some(id) = host_id {
            let prepared = self.sharing_prepare_connection(&id).await?;
            let quoted = script.replace('\'', "'\\''");
            let command = format!("/bin/sh -c '{quoted}' cc-owner {protocol} {port}");
            return prepared
                .run(async {
                    let ssh = prepared
                        .unlocked
                        .ssh
                        .connector
                        .connect(&prepared.plan)
                        .await?;
                    let result = ssh
                        .exec_bounded(&command, 4096, tokio::time::sleep(TIMEOUT))
                        .await;
                    let _ = timeout(Duration::from_secs(1), ssh.disconnect()).await;
                    Ok(result?
                        .filter(|r| r.exit_status == Some(0))
                        .map_or_else(Vec::new, |r| names(&r.stdout)))
                })
                .await;
        }
        let (_, unlocked) = self.unlocked().await?;
        let mut shutdown = unlocked.sharing_shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let mut command = if cfg!(windows) {
            let root = std::env::var_os("SystemRoot")
                .ok_or_else(|| AppError::Io("missing Windows system directory".into()))?;
            let mut cmd = Command::new(
                std::path::PathBuf::from(root)
                    .join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
            );
            let list = if udp {
                "Get-NetUDPEndpoint"
            } else {
                "Get-NetTCPConnection -State Listen"
            };
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", &format!(
                "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); {list} -LocalPort {port} -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique | ForEach-Object {{ (Get-Process -Id $_ -ErrorAction SilentlyContinue).ProcessName }} | Sort-Object -Unique"
            )]);
            #[cfg(windows)]
            cmd.creation_flags(0x08000000);
            cmd
        } else if cfg!(any(
            target_os = "macos",
            target_os = "linux",
            target_os = "freebsd"
        )) {
            let mut cmd = Command::new("/bin/sh");
            cmd.args(["-c", &script, "cc-owner", protocol, &port.to_string()]);
            cmd
        } else {
            return Ok(vec![]);
        };
        let out = tokio::select! {
            biased;
            _ = shutdown.changed() => return Err(AppError::VaultLocked),
            result = timeout(TIMEOUT, command.kill_on_drop(true).output()) => result,
        };
        Ok(out
            .ok()
            .and_then(Result::ok)
            .filter(|o| o.status.success())
            .map_or_else(Vec::new, |o| names(&o.stdout)))
    }

    /// Read listening TCP sockets or bound UDP sockets on the chosen destination.
    pub async fn listening_ports(
        &self,
        host_id: Option<String>,
        low: u16,
        high: u16,
        udp: bool,
    ) -> AppResult<Vec<u16>> {
        if low == 0 || low > high {
            return Err(AppError::invalid("port", "invalid port interval"));
        }
        let Some(id) = host_id else {
            let (_, unlocked) = self.unlocked().await?;
            let mut shutdown = unlocked.sharing_shutdown.subscribe();
            if *shutdown.borrow() {
                return Err(AppError::VaultLocked);
            }
            return tokio::select! {
                biased;
                _ = shutdown.changed() => Err(AppError::VaultLocked),
                value = local(low, high, udp) => value,
            };
        };
        let prepared = self.sharing_prepare_connection(&id).await?;
        let script = SCRIPT.replace("\r\n", "\n").replace('\'', "'\\''");
        let protocol = if udp { "udp" } else { "tcp" };
        let command = format!("/bin/sh -c '{script}' cc-ports {protocol} {low} {high}");
        prepared
            .run(async {
                let ssh = prepared
                    .unlocked
                    .ssh
                    .connector
                    .connect(&prepared.plan)
                    .await?;
                // At most 65,535 distinct five-digit ports and one newline each.
                let result = ssh
                    .exec_bounded(&command, 65535 * 6, tokio::time::sleep(TIMEOUT))
                    .await;
                let _ = timeout(Duration::from_secs(1), ssh.disconnect()).await;
                let output =
                    result?.ok_or_else(|| AppError::Io("remote socket table timed out".into()))?;
                match output.exit_status {
                    Some(0) => parse(&output.stdout, low, high),
                    Some(69) => Err(AppError::Unsupported("remote socket table".into())),
                    _ => Err(AppError::Io("could not read remote socket table".into())),
                }
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_only_the_bound_port_in_an_inclusive_interval() {
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = tcp.local_addr().unwrap().port();
        assert_eq!(local(port, port, false).await.unwrap(), vec![port]);
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = udp.local_addr().unwrap().port();
        assert_eq!(local(port, port, true).await.unwrap(), vec![port]);
    }
}
