use super::*;
use cc_terminal_core::TerminalTarget;
use std::net::IpAddr;

fn capture_command(peer: Option<&str>, sudo: bool) -> AppResult<String> {
    let peer = match peer {
        None => String::new(),
        Some(raw) => {
            let raw = raw.trim();
            let bare = raw
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(raw);
            if let Ok(ip) = bare.parse::<IpAddr>() {
                ip.to_string()
            } else {
                let valid_label = |label: &str| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                };
                if raw.len() > 253
                    || !raw
                        .strip_suffix('.')
                        .unwrap_or(raw)
                        .split('.')
                        .all(valid_label)
                    || raw.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                {
                    return Err(AppError::invalid("peer", "invalid server address"));
                }
                raw.to_owned()
            }
        }
    };
    let script = include_str!("tcpdump.sh")
        .replace("\r\n", "\n")
        .replace('\'', "'\\''");
    let mode = if sudo { "sudo" } else { "direct" };
    Ok(format!("sh -c '{script}' cc-tcpdump '{peer}' {mode}"))
}

impl AppCore {
    pub async fn open_tcpdump(
        &self,
        host_id: String,
        peer_id: Option<String>,
        sudo: bool,
    ) -> AppResult<TerminalInfoDto> {
        let prepared = self.sharing_prepare_connection(&host_id).await?;
        let u = &prepared.unlocked;
        let peer = peer_id
            .as_deref()
            .map(|id| -> AppResult<_> {
                let id = parse_id("peer_id", id)?;
                if id == prepared.plan.host_id {
                    return Err(AppError::invalid("peer", "select a different server"));
                }
                Ok(host_of(u, id)?.address)
            })
            .transpose()?;
        let command = capture_command(peer.as_deref(), sudo)?;
        let opened = prepared
            .run(u.ssh.open_command_pty(&prepared.plan, command))
            .await?;
        let terminals = u.ssh.terminals.clone();
        let mut shutdown = u.sharing_shutdown.subscribe();
        let id = terminals
            .open_background_channel(
                TerminalTarget::Ssh {
                    host_id: prepared.plan.host_id,
                },
                "tcpdump".into(),
                TerminalSize {
                    cols: 240,
                    rows: 24,
                },
                opened,
            )
            .await?;
        if *shutdown.borrow() {
            let _ = terminals.remove(id).await;
            return Err(AppError::VaultLocked);
        }
        let info = terminals.info(id)?;
        tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = shutdown.changed() => { let _ = terminals.remove(id).await; }
                _ = terminals.wait_closed(id) => {}
            }
        });
        Ok(info.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcpdump_accepts_only_a_single_peer() {
        for invalid in [
            "",
            "-i",
            "x;id",
            "db.test or tcp",
            "10.0",
            "a..b",
            "a..",
            "x/24",
            "a\nb",
            "x'",
        ] {
            assert!(capture_command(Some(invalid), false).is_err(), "{invalid}");
        }
        assert!(
            capture_command(Some("[2001:db8::1]"), true)
                .unwrap()
                .ends_with("'2001:db8::1' sudo")
        );
        assert!(
            capture_command(Some("db.example."), false)
                .unwrap()
                .ends_with("'db.example.' direct")
        );
        assert!(capture_command(None, false).unwrap().ends_with("'' direct"));
    }
}
