use crate::{AppCore, AppError, AppResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::{sleep, timeout};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    List,
    UnitPath {
        unit: String,
    },
    UnitText {
        unit: String,
    },
    Journal {
        unit: String,
        terminal: Option<String>,
        after_cursor: Option<String>,
    },
    Restart {
        unit: String,
        sudo: bool,
    },
}

fn service_name(unit: &str) -> AppResult<&str> {
    // A literal unit only. systemctl and journalctl both accept glob patterns.
    if unit.len() <= 255
        && unit
            .strip_suffix(".service")
            .is_some_and(|name| !name.is_empty())
        && unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:@-\\".contains(&b))
    {
        Ok(unit)
    } else {
        Err(AppError::invalid(
            "unit",
            "expected one systemd service name",
        ))
    }
}

impl Operation {
    fn command(&self) -> AppResult<String> {
        let command = match self {
            Self::List => {
                return Ok("command -v systemctl >/dev/null && test -d /run/systemd/system || exit 69; LC_ALL=C SYSTEMD_COLORS=0 SYSTEMD_URLIFY=0 systemctl --no-pager --no-legend --plain --full list-units --all --type=service".into());
            }
            Self::UnitPath { unit } => format!(
                "systemctl --no-pager show --property=FragmentPath --value -- '{}'",
                service_name(unit)?
            ),
            Self::UnitText { unit } => {
                format!("systemctl --no-pager cat -- '{}'", service_name(unit)?)
            }
            Self::Journal {
                unit, after_cursor, ..
            } => {
                let mut read = format!(
                    "journalctl --no-pager --all --output=json --output-fields=MESSAGE,PRIORITY,SYSLOG_IDENTIFIER,_COMM,_PID -n 200 --unit='{}'",
                    service_name(unit)?
                );
                if let Some(cursor) = after_cursor {
                    if cursor.is_empty()
                        || cursor.len() > 2048
                        || !cursor.bytes().all(|b| b.is_ascii_graphic())
                    {
                        return Err(AppError::invalid("journal", "invalid cursor"));
                    }
                    read.push_str(" --after-cursor='");
                    read.push_str(&cursor.replace('\'', "'\\''"));
                    read.push('\'');
                }
                read
            }
            Self::Restart { unit, sudo } => {
                let prefix = if *sudo { "sudo -n -- " } else { "" };
                format!(
                    "{prefix}systemctl --no-ask-password restart -- '{}'",
                    service_name(unit)?
                )
            }
        };
        Ok(format!(
            "LC_ALL=C SYSTEMD_COLORS=0 SYSTEMD_URLIFY=0 {command}"
        ))
    }
}

impl AppCore {
    pub async fn host_systemd(
        &self,
        host_id: String,
        host_updated_at_ms: i64,
        request_json: String,
    ) -> AppResult<Value> {
        let operation: Operation = serde_json::from_str(&request_json)
            .map_err(|_| AppError::invalid("systemd", "invalid operation"))?;
        let command = operation.command()?;
        let prepared = self.sharing_prepare_connection(&host_id).await?;
        let host = crate::inventory::host_of(&prepared.unlocked, prepared.plan.host_id)?;
        if crate::dto::ms(&host.updated_at) != host_updated_at_ms {
            return Err(AppError::invalid("host", "host changed"));
        }
        prepared
            .run(async {
                let limit = if matches!(operation, Operation::Restart { .. }) {
                    120
                } else {
                    20
                };
                let output_limit = if matches!(operation, Operation::Journal { .. }) {
                    4 * 1024 * 1024
                } else {
                    1024 * 1024
                };
                let deadline = sleep(Duration::from_secs(limit));
                let result = if let Operation::Journal {
                    terminal: Some(id), ..
                } = &operation
                {
                    let terminals = &prepared.unlocked.ssh.terminals;
                    let id = crate::ssh_api::parse_terminal_id(id)?;
                    let ssh = terminals
                        .connected_ssh(id, prepared.plan.host_id)
                        .ok_or(AppError::Cancelled)?;
                    ssh.exec_bounded(&command, output_limit, async {
                        tokio::select! {
                            _ = deadline => {},
                            _ = terminals.wait_closed(id) => {},
                        }
                    })
                    .await
                } else {
                    let ssh = prepared
                        .unlocked
                        .ssh
                        .connector
                        .connect(&prepared.plan)
                        .await?;
                    let result = ssh.exec_bounded(&command, output_limit, deadline).await;
                    let _ = timeout(Duration::from_secs(1), ssh.disconnect()).await;
                    result
                };
                Ok(match result? {
                    Some(output) => json!({
                        "status": output.exit_status,
                        "output": output.stdout_lossy(),
                        "diagnostic": output.stderr_lossy(),
                    }),
                    None => json!({"timeout": true}),
                })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_literal_service_can_be_restarted() {
        for unit in [
            "",
            ".service",
            "*.service",
            "sshd.socket",
            "a\nb.service",
            "a' b.service",
            "a;id.service",
        ] {
            assert!(service_name(unit).is_err(), "{unit:?}");
        }
        let op = Operation::Restart {
            unit: r"worker@group\x2d1.service".into(),
            sudo: true,
        };
        assert!(op.command().unwrap().ends_with(
            r"sudo -n -- systemctl --no-ask-password restart -- 'worker@group\x2d1.service'"
        ));
        assert!(
            serde_json::from_str::<Operation>(r#"{"action":"restart","unit":"sshd.service"}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<Operation>(r#"{"action":"stop","unit":"sshd.service"}"#)
                .is_err()
        );
    }
}
