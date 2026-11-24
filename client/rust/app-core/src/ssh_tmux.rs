//! tmux operations run on a separate channel of the active SSH connection.
use super::{parse_terminal_id, AppCore, AppError, AppResult};
use cc_terminal_core::{OpenedShell, TerminalSize, TerminalTarget};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    id: u32,
    pid: u32,
    created: u64,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Probe,
    List,
    Create { name: String },
    Rename { session: Session, name: String },
    Kill { session: Session },
    Attach { session: Session },
}

fn name_arg(name: &str) -> AppResult<String> {
    if name.is_empty()
        || name.len() > 128
        || name.trim() != name
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
    {
        return Err(AppError::invalid("tmux", "invalid session name"));
    }
    Ok(format!("'{name}'"))
}

const LIST: &str = "tmux -u list-sessions -F '#{pid}\t#{session_id}\t#{session_created}\t#{session_windows}\t#{session_attached}\t#{session_activity}\t#{session_name}\t#{session_path}'";

impl Action {
    fn command(&self) -> AppResult<String> {
        // SSH executes with the account's login shell (which may be fish).
        // Wrap the fixed script in POSIX sh; only numbers and validated names
        // are interpolated. Existing names are never used as command arguments.
        let script = match self {
            Self::Probe => "command -v tmux >/dev/null || exit 45; exec tmux -V".to_string(),
            Self::List => format!("LC_ALL=C; export LC_ALL; exec {LIST}"),
            Self::Create { name } => format!("exec tmux new-session -d -s {}", name_arg(name)?),
            Self::Rename { session, .. } | Self::Kill { session } | Self::Attach { session } => {
                let target = format!("'${}'", session.id);
                let command = match self {
                    Self::Rename { name, .. } => {
                        format!("rename-session -t {target} {}", name_arg(name)?)
                    }
                    Self::Kill { .. } => format!("kill-session -t {target}"),
                    Self::Attach { .. } => format!("attach-session -t {target}"),
                    _ => unreachable!(),
                };
                // IDs restart at $0 when the tmux server restarts.
                format!("identity=$(tmux display-message -p -t '${}:' '#{{pid}}:#{{session_id}}:#{{session_created}}') && [ \"$identity\" = '{}:${}:{}' ] || exit 46; exec tmux -u {command}",
                    session.id, session.pid, session.id, session.created)
            }
        };
        Ok(format!("exec sh -c '{}'", script.replace('\'', "'\\''")))
    }
}

fn sessions(output: &str) -> AppResult<Vec<Value>> {
    let mut rows = Vec::new();
    let invalid = || AppError::invalid("tmux", "invalid session list");
    for line in output.lines() {
        let mut parts = line.splitn(8, '\t');
        let mut next = || parts.next().ok_or_else(invalid);
        let pid = next()?.parse::<u32>().map_err(|_| invalid())?;
        let id = next()?
            .strip_prefix('$')
            .ok_or_else(invalid)?
            .parse::<u32>()
            .map_err(|_| invalid())?;
        let created = next()?.parse::<u64>().map_err(|_| invalid())?;
        let windows = next()?.parse::<u32>().map_err(|_| invalid())?;
        let clients = next()?.parse::<u32>().map_err(|_| invalid())?;
        let activity = next()?.parse::<u64>().map_err(|_| invalid())?;
        let name = next()?;
        let path = next()?;
        rows.push(
            json!({"id":id,"pid":pid,"created":created,"name":name,"path":path,
            "windows":windows,"clients":clients,"activity":activity}),
        );
    }
    Ok(rows)
}

impl AppCore {
    pub async fn terminal_tmux(&self, terminal_id: String, request: String) -> AppResult<Value> {
        let action: Action = serde_json::from_str(&request)
            .map_err(|_| AppError::invalid("tmux", "invalid operation"))?;
        let command = action.command()?;
        let (_, u) = self.unlocked().await?;
        let terminals = &u.ssh.terminals;
        let parent = parse_terminal_id(&terminal_id)?;
        let TerminalTarget::Ssh { host_id } = terminals.info(parent)?.target else {
            return Err(AppError::invalid("tmux", "SSH session required"));
        };
        let ssh = terminals
            .connected_ssh(parent, host_id)
            .ok_or(AppError::Cancelled)?;
        let shutdown = u.sharing_shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let mut signals = shutdown.clone();
        let stopped = async move {
            tokio::select! {
                _ = terminals.wait_closed(parent) => {},
                _ = signals.wait_for(|locked| *locked) => {},
                _ = tokio::time::sleep(Duration::from_secs(10)) => {},
            }
        };
        tokio::pin!(stopped);
        if let Action::Attach { session } = action {
            // Check before opening the new tab: a stale selection must not displace
            // the current client. The PTY command checks again before attaching.
            let check = format!(
                "tmux display-message -p -t '${}:' '#{{pid}}:#{{session_id}}:#{{session_created}}'",
                session.id
            );
            let status = ssh
                .exec_bounded(&check, 256, &mut stopped)
                .await?
                .ok_or(AppError::Cancelled)?;
            if status.exit_status != Some(0)
                || status.stdout_lossy().trim()
                    != format!("{}:${}:{}", session.pid, session.id, session.created)
            {
                return Err(AppError::invalid("tmux", "session changed"));
            }
            let channel = tokio::select! {
                biased;
                _ = stopped => return Err(AppError::Cancelled),
                result = ssh.open_command_pty(cc_ssh_core::PtyRequest::default(), &command) => result?,
            };
            let id = terminals
                .open_channel(
                    TerminalTarget::Ssh { host_id },
                    format!("tmux · ${}", session.id),
                    TerminalSize {
                        cols: 100,
                        rows: 30,
                    },
                    OpenedShell {
                        channel,
                        session: None,
                        lifetime: None,
                    },
                )
                .await?;
            // The parent owns the SSH transport. Closing this client closes only its PTY.
            if let Err(error) = terminals.suppress_command_tracking(id) {
                let _ = terminals.remove(id).await;
                return Err(error.into());
            }
            if *shutdown.borrow() || terminals.connected_ssh(parent, host_id).is_none() {
                let _ = terminals.remove(id).await;
                return Err(AppError::Cancelled);
            }
            return Ok(json!({"terminal":id.to_string()}));
        }
        let out = ssh
            .exec_bounded(&command, 256 * 1024, stopped)
            .await?
            .ok_or(AppError::Cancelled)?;
        let stdout = out.stdout_lossy();
        if *shutdown.borrow() || terminals.connected_ssh(parent, host_id).is_none() {
            return Err(AppError::Cancelled);
        }
        if out.exit_status == Some(0) && out.exit_signal.is_none() {
            return match action {
                Action::Probe => Ok(json!({"version":stdout.trim()})),
                Action::List => Ok(json!({"sessions":sessions(&stdout)?})),
                _ => Ok(json!({})),
            };
        }
        if matches!(action, Action::Probe) && out.exit_status == Some(45) {
            return Ok(json!({"version":null}));
        }
        let stderr = out.stderr_lossy();
        if matches!(action, Action::List)
            && (stderr.contains("no server running")
                || stderr.contains("No such file or directory")
                || stderr.trim() == "no sessions")
        {
            return Ok(json!({"sessions":[]}));
        }
        Err(AppError::invalid(
            "tmux",
            if out.exit_status == Some(46) {
                "session changed"
            } else {
                "command failed"
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_targets_are_separate_from_shell_syntax() {
        assert_eq!(name_arg("сборка 3").unwrap(), "'сборка 3'");
        for invalid in ["", "-a", "foo:bar", "a;b", "a'b", "a\nb", "$(pwd)"] {
            assert!(name_arg(invalid).is_err());
        }
        let session = Session {
            id: 9,
            pid: 334,
            created: 229,
        };
        let cmd = Action::Kill { session }.command().unwrap();
        assert!(cmd.contains("334:$9:229") && cmd.contains("kill-session"));
        assert!(!cmd.contains("kill-server"));
    }

    #[test]
    fn session_list_keeps_exact_identity_and_directory() {
        let rows = sessions("420\t$0\t123\t2\t1\t199\tbuild\t/work/in progress\n").unwrap();
        assert_eq!(rows[0]["clients"], 1);
        assert_eq!(rows[0]["created"], 123);
        assert_eq!(rows[0]["path"], "/work/in progress");
        assert!(sessions("").unwrap().is_empty());
        assert!(sessions("bad data").is_err());
    }
}
