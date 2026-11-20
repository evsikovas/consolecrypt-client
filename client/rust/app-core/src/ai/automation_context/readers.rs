//! Fixed bounded observations. No caller command, path, URL or version claim.
use super::*;
use cc_automation_core::EnvironmentProbe;

const OS_COMMAND: &str = "LC_ALL=C; export LC_ALL; printf '__CC_OS_BEGIN__\\n'; if [ -r /etc/os-release ]; then head -c 4096 /etc/os-release; fi; printf '\\n__CC_ARCH__\\n'; uname -m; printf '__CC_KERNEL__\\n'; uname -r; printf '__CC_OS_END__\\n'";
fn text(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(invalid("invalid_fact_response"));
    }
    Ok(value.to_owned())
}
fn os_fields(raw: &[u8]) -> AppResult<Value> {
    if raw.len() > 8192 {
        return Err(invalid("fact_budget"));
    }
    let raw = std::str::from_utf8(raw).map_err(|_| invalid("invalid_fact_response"))?;
    for marker in [
        "__CC_OS_BEGIN__\n",
        "\n__CC_ARCH__\n",
        "__CC_KERNEL__\n",
        "__CC_OS_END__\n",
    ] {
        if raw.matches(marker).count() != 1 {
            return Err(invalid("invalid_fact_response"));
        }
    }
    let raw = raw
        .strip_prefix("__CC_OS_BEGIN__\n")
        .ok_or_else(|| invalid("invalid_fact_response"))?;
    let (release, tail) = raw
        .split_once("\n__CC_ARCH__\n")
        .ok_or_else(|| invalid("invalid_fact_response"))?;
    let (arch, tail) = tail
        .split_once("__CC_KERNEL__\n")
        .ok_or_else(|| invalid("invalid_fact_response"))?;
    let kernel = tail
        .strip_suffix("__CC_OS_END__\n")
        .ok_or_else(|| invalid("invalid_fact_response"))?;
    let mut result = serde_json::Map::new();
    for line in release.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let field = match key {
            "ID" => "osId",
            "VERSION_ID" => "osVersion",
            _ => continue,
        };
        if result.contains_key(field) {
            return Err(invalid("invalid_fact_response"));
        }
        // Parse data only. NEVER source/eval os-release or expand its contents.
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        if !value.is_empty() {
            result.insert(field.into(), json!(text(value)?));
        }
    }
    result.insert("architecture".into(), json!(text(arch)?));
    result.insert("kernelRelease".into(), json!(text(kernel)?));
    Ok(Value::Object(result))
}

pub(super) async fn capture(
    app: &AppCore,
    owner: &Arc<Session>,
    original: &Original,
    project: &ProjectFence,
    kind: Kind,
    subject: &str,
    cancel: &CancellationToken,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
) -> AppResult<Fact> {
    let u = original.unlock.upgrade().ok_or(AppError::VaultLocked)?;
    original.check(owner, &u)?;
    match kind {
        Kind::HostOs => {
            let host_id = crate::dto::parse_id("host_id", subject)?;
            let (rev, protocol) = u
                .working()
                .correlation_host_metadata(host_id)
                .ok_or_else(|| invalid("host_unavailable"))?;
            if protocol != "ssh" || rev <= 0 {
                return Err(invalid("host_unavailable"));
            }
            revision(rev as u64)?;
            let metadata = MetadataFence::capture(&u, vec![host_id])?;
            if !u
                .working()
                .watch_revisions(metadata.projection.revisions.clone(), cancel.clone())
            {
                return Err(invalid("subject_changed"));
            }
            drop(u);
            let session = app
                .open_private_runner_session(subject, &original.unlock, cancel)
                .await?;
            let observed = tokio::select! {biased;
                _=cancel.cancelled()=>Err(AppError::Cancelled),
                _=shutdown.changed()=>Err(AppError::VaultLocked),
                value=tokio::time::timeout(Duration::from_secs(15),session.exec_with_private_input(OS_COMMAND,&[],8192))=>
                    value.map_err(|_|invalid("host_read_timeout")).and_then(|v|v.map_err(|_|invalid("host_read_failed"))),
            };
            // This session is dedicated, never a pooled terminal/job/Runner.
            let _ = tokio::time::timeout(Duration::from_secs(5), session.disconnect()).await;
            let observed = observed?;
            if observed.exit_status != Some(0) || observed.exit_signal.is_some() {
                return Err(invalid("host_read_failed"));
            }
            let fields = os_fields(&observed.stdout)?;
            Ok(Fact {
                kind,
                subject: subject.into(),
                subject_revision: rev as u64,
                fields,
                metadata,
                runner: None,
                pins: None,
            })
        }
        Kind::RunnerVersions => {
            let connection = owner
                .automation
                .connections
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(subject)
                .cloned()
                .filter(|v| !v.peer.is_closed())
                .ok_or_else(|| invalid("runner_not_connected"))?;
            revision(connection.revision)?;
            let current =
                automation_runners::dependency::capture_runner(owner, subject, connection.revision)
                    .await?;
            let roots = match &current.configuration {
                automation_runners::AutomationRunnerConfig::Remote { host_id, .. }
                | automation_runners::AutomationRunnerConfig::ServiceRemote { host_id, .. } => {
                    vec![crate::dto::parse_id("host_id", host_id)?]
                }
                automation_runners::AutomationRunnerConfig::Local { .. } => vec![],
            };
            let metadata = MetadataFence::capture(&u, roots)?;
            if !u
                .working()
                .watch_revisions(metadata.projection.revisions.clone(), cancel.clone())
            {
                return Err(invalid("subject_changed"));
            }
            let runner = RunnerFence {
                id: subject.into(),
                revision: connection.revision,
                stamp: current.stamp,
                connection: Arc::downgrade(&connection),
            };
            drop(u);
            // A FRESH RPC on the already connected peer. Never connect/install,
            // never relabel the old connection.probe with a new timestamp.
            let raw = tokio::select! {biased;
                _=cancel.cancelled()=>return Err(AppError::Cancelled),
                _=shutdown.changed()=>return Err(AppError::VaultLocked),
                result=connection.peer.request("probe",json!({}),Duration::from_secs(20))=>result.map_err(automation_projects::automation_error)?,
            };
            let probe =
                EnvironmentProbe::parse(raw).map_err(automation_projects::automation_error)?;
            let pins = project.pins(owner).await?;
            let mut fields = serde_json::Map::new();
            fields.insert("python".into(), json!(text(&probe.python)?));
            if let Some(actual) = probe
                .packages
                .get("ansible-core")
                .and_then(|v| v.actual.as_deref())
            {
                fields.insert("ansibleCore".into(), json!(text(actual)?));
            }
            fields.insert("collections".into(), pins.pins.clone());
            Ok(Fact {
                kind,
                subject: subject.into(),
                subject_revision: connection.revision,
                fields: Value::Object(fields),
                metadata,
                runner: Some(runner),
                pins: Some(pins),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_os_data_is_not_sourced_or_expanded() {
        let fields=os_fields(b"__CC_OS_BEGIN__\nID=ubuntu\nVERSION_ID=\"24.04\"\nPRETTY_NAME=$(touch /tmp/forbidden)\n\n__CC_ARCH__\nx86_64\n__CC_KERNEL__\n6.8.0\n__CC_OS_END__\n").unwrap();
        assert_eq!(
            fields,
            json!({"osId":"ubuntu","osVersion":"24.04","architecture":"x86_64","kernelRelease":"6.8.0"})
        );
    }
    #[test]
    fn duplicate_boundary_and_oversized_output_fail_closed() {
        assert!(os_fields(b"__CC_OS_BEGIN__\n__CC_OS_BEGIN__\n").is_err());
        assert!(os_fields(&vec![b'x'; 8193]).is_err());
        assert!(os_fields(b"__CC_OS_BEGIN__\nID=x\nID=y\n\n__CC_ARCH__\nx86_64\n__CC_KERNEL__\n6.8\n__CC_OS_END__\n").is_err());
    }
}
