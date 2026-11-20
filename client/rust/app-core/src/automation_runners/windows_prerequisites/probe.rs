use super::*;
use tokio::io::AsyncReadExt;

const MAX_OUTPUT: usize = 16 * 1024;
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ElevationState {
    Elevated,
    NotElevated,
    Unknown,
}
#[derive(Serialize)]
pub(super) struct Elevation {
    state: ElevationState,
    reason: Option<&'static str>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum FeatureState {
    Enabled,
    Disabled,
    EnablePending,
    DisablePending,
    Unknown,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Feature {
    state: FeatureState,
    reason: Value,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Features {
    subsystem_linux: Feature,
    virtual_machine_platform: Feature,
}
#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum RebootState {
    IndicatorsPresent,
    NotObserved,
    Unknown,
}
#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Indicator {
    ComponentServicing,
    WindowsUpdate,
    PendingFileRename,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reboot {
    state: RebootState,
    reason: Value,
    indicators: Vec<Indicator>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    version: u8,
    features: Features,
    reboot: Reboot,
}
pub(super) struct Observed {
    pub elevation: Elevation,
    pub features: Features,
    pub reboot: Reboot,
}
fn parse(raw: &[u8], elevation: ElevationState) -> AppResult<Observed> {
    if raw.len() > MAX_OUTPUT {
        return Err(denied("invalid_observation"));
    }
    let r: Raw = serde_json::from_slice(raw).map_err(|_| denied("invalid_observation"))?;
    if r.version != 1 {
        return Err(denied("invalid_observation"));
    }
    for feature in [
        &r.features.subsystem_linux,
        &r.features.virtual_machine_platform,
    ] {
        let valid = match feature.state {
            FeatureState::Unknown => match elevation {
                ElevationState::NotElevated => feature.reason == json!("requiresElevation"),
                _ => feature.reason == json!("unavailable"),
            },
            _ => elevation == ElevationState::Elevated && feature.reason.is_null(),
        };
        if !valid {
            return Err(denied("invalid_observation"));
        }
    }
    let mut seen = Vec::new();
    for indicator in &r.reboot.indicators {
        if seen.contains(&indicator) {
            return Err(denied("invalid_observation"));
        }
        seen.push(indicator);
    }
    if r.reboot.indicators.len() > 3
        || match r.reboot.state {
            RebootState::IndicatorsPresent => {
                r.reboot.indicators.is_empty() || !r.reboot.reason.is_null()
            }
            RebootState::NotObserved => {
                !r.reboot.indicators.is_empty() || !r.reboot.reason.is_null()
            }
            RebootState::Unknown => {
                !r.reboot.indicators.is_empty() || r.reboot.reason != json!("unavailable")
            }
        }
    {
        return Err(denied("invalid_observation"));
    }
    Ok(Observed {
        elevation: Elevation {
            state: elevation,
            reason: (elevation == ElevationState::Unknown).then_some("unavailable"),
        },
        features: r.features,
        reboot: r.reboot,
    })
}

/// Cancellation never abandons an unobserved child behind a released slot.
/// This worker survives caller-future Drop and holds the existing registration.
async fn collect(
    command: &mut tokio::process::Command,
    cancel: &CancellationToken,
    dropped: &CancellationToken,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
    expires: Instant,
) -> AppResult<Vec<u8>> {
    if cancel.is_cancelled()
        || dropped.is_cancelled()
        || *shutdown.borrow()
        || Instant::now() >= expires
    {
        return Err(AppError::Cancelled);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| denied("observation_unavailable"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| denied("observation_unavailable"))?;
    let observed = {
        let read_and_join = async {
            let mut bytes = Vec::with_capacity(1024);
            (&mut stdout)
                .take((MAX_OUTPUT + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| denied("observation_unavailable"))?;
            if bytes.len() > MAX_OUTPUT {
                return Err(denied("output_budget"));
            }
            let status = child
                .wait()
                .await
                .map_err(|_| denied("observation_unavailable"))?;
            if !status.success() {
                return Err(denied("observation_unavailable"));
            }
            Ok(bytes)
        };
        tokio::select! { biased;
            _=cancel.cancelled()=>Err(AppError::Cancelled),
            _=dropped.cancelled()=>Err(AppError::Cancelled),
            _=shutdown.changed()=>Err(AppError::VaultLocked),
            _=tokio::time::sleep_until(expires.into())=>Err(denied("observation_timeout")),
            result=read_and_join=>result,
        }
    };
    if observed.is_err() {
        let _ = child.start_kill();
        // Keep original slot/worker until the actual OS process is reaped.
        let _ = child.wait().await;
    }
    observed
}

#[cfg(windows)]
fn command() -> AppResult<(tokio::process::Command, ElevationState)> {
    use base64::Engine;
    let context = cc_platform_core::windows_prerequisite_context()
        .map_err(|_| denied("observation_unavailable"))?;
    let directory = context.system_directory();
    let elevation = match context.token_elevation() {
        cc_platform_core::WindowsTokenElevation::Elevated => ElevationState::Elevated,
        cc_platform_core::WindowsTokenElevation::NotElevated => ElevationState::NotElevated,
        cc_platform_core::WindowsTokenElevation::Unknown => ElevationState::Unknown,
    };
    let windows = directory
        .parent()
        .ok_or_else(|| denied("observation_unavailable"))?;
    let executable = directory.join("WindowsPowerShell/v1.0/powershell.exe");
    let script = include_str!("../windows_prerequisites.ps1");
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut command = tokio::process::Command::new(executable);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
        ])
        .arg(base64::engine::general_purpose::STANDARD.encode(bytes))
        .current_dir(&directory)
        .env_clear()
        .env("SystemRoot", windows)
        .env("WINDIR", windows)
        .env("PATH", &directory)
        .env(
            "PSModulePath",
            directory.join("WindowsPowerShell/v1.0/Modules"),
        )
        .env(
            "CC_PREREQUISITE_ELEVATION",
            match elevation {
                ElevationState::Elevated => "elevated",
                ElevationState::NotElevated => "notElevated",
                ElevationState::Unknown => "unknown",
            },
        )
        .creation_flags(0x08000000);
    Ok((command, elevation))
}
#[cfg(windows)]
pub(super) async fn windows(
    cancel: &CancellationToken,
    dropped: &CancellationToken,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
    expires: Instant,
) -> AppResult<Observed> {
    let (mut command, elevation) = command()?;
    let raw = collect(&mut command, cancel, dropped, shutdown, expires).await?;
    parse(&raw, elevation)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body() -> Value {
        json!({"version":1,"features":{
        "subsystemLinux":{"state":"unknown","reason":"requiresElevation"},
        "virtualMachinePlatform":{"state":"unknown","reason":"requiresElevation"}},
        "reboot":{"state":"notObserved","reason":null,"indicators":[]}})
    }
    #[test]
    fn closed_observation_never_infers_disabled_from_unavailable_or_an_unknown_token() {
        let raw = body();
        assert!(
            parse(
                &serde_json::to_vec(&raw).unwrap(),
                ElevationState::NotElevated
            )
            .is_ok()
        );
        assert!(parse(&serde_json::to_vec(&raw).unwrap(), ElevationState::Elevated).is_err());
        for field in ["subsystemLinux", "virtualMachinePlatform"] {
            let mut changed = body();
            changed["features"][field] = json!({"state":"disabled","reason":null});
            assert!(
                parse(
                    &serde_json::to_vec(&changed).unwrap(),
                    ElevationState::NotElevated
                )
                .is_err()
            );
        }
        let mut unknown = body();
        for field in ["subsystemLinux", "virtualMachinePlatform"] {
            unknown["features"][field]["reason"] = json!("unavailable");
        }
        assert!(
            parse(
                &serde_json::to_vec(&unknown).unwrap(),
                ElevationState::Unknown
            )
            .is_ok()
        );
        unknown["future"] = json!(true);
        assert!(
            parse(
                &serde_json::to_vec(&unknown).unwrap(),
                ElevationState::Unknown
            )
            .is_err()
        );
    }
    #[test]
    fn reboot_evidence_is_closed_and_requires_observed_indicators_or_all_reads() {
        for reboot in [
            json!({"state":"indicatorsPresent","reason":null,"indicators":[]}),
            json!({"state":"notObserved","reason":null,"indicators":["windowsUpdate"]}),
            json!({"state":"unknown","reason":"unavailable","indicators":["windowsUpdate"]}),
            json!({"state":"indicatorsPresent","reason":null,"indicators":["windowsUpdate","windowsUpdate"]}),
            json!({"state":"notObserved","indicators":[]}),
            json!({"state":"rebootRequired","reason":null,"indicators":[]}),
        ] {
            let mut raw = body();
            raw["reboot"] = reboot;
            assert!(
                parse(
                    &serde_json::to_vec(&raw).unwrap(),
                    ElevationState::NotElevated
                )
                .is_err()
            );
        }
        for reboot in [
            json!({"state":"indicatorsPresent","reason":null,"indicators":["windowsUpdate"]}),
            json!({"state":"unknown","reason":"unavailable","indicators":[]}),
        ] {
            let mut raw = body();
            raw["reboot"] = reboot;
            assert!(
                parse(
                    &serde_json::to_vec(&raw).unwrap(),
                    ElevationState::NotElevated
                )
                .is_ok()
            );
        }
        assert!(parse(&vec![b' '; MAX_OUTPUT + 1], ElevationState::Unknown).is_err());
        assert!(parse(b"\xff", ElevationState::Unknown).is_err());
    }
    #[test]
    #[ignore = "literal owned child only, invoked by lifecycle test"]
    fn prerequisite_fixture_child() {
        let Ok(mode) = std::env::var("CC_OWNED_PREREQUISITE_CHILD") else {
            return;
        };
        match mode.as_str() {
            "success" => println!("OWNED_PREREQUISITE_CHILD"),
            "oversize" => {
                use std::io::Write;
                let _ = std::io::stdout().write_all(&vec![b'x'; MAX_OUTPUT + 1]);
                std::thread::sleep(Duration::from_secs(30));
            }
            "wait" => std::thread::sleep(Duration::from_secs(30)),
            _ => panic!("unknown owned child mode"),
        }
    }
    fn child(mode: &str) -> tokio::process::Command {
        let mut c = tokio::process::Command::new(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "automation_runners::windows_prerequisites::probe::tests::prerequisite_fixture_child",
            "--ignored",
            "--nocapture",
        ])
        .env("CC_OWNED_PREREQUISITE_CHILD", mode);
        c
    }
    #[tokio::test]
    async fn real_child_success_output_budget_cancel_deadline_and_launch_failure_join() {
        for mode in [
            "success", "oversize", "cancel", "drop", "deadline", "launch",
        ] {
            let cancel = CancellationToken::new();
            let dropped = CancellationToken::new();
            let (_sender, mut shutdown) = tokio::sync::watch::channel(false);
            let owned = tempfile::tempdir().unwrap();
            let mut command = if mode == "launch" {
                tokio::process::Command::new(owned.path().join("absent.exe"))
            } else {
                child(if mode == "success" {
                    "success"
                } else if mode == "oversize" {
                    "oversize"
                } else {
                    "wait"
                })
            };
            if mode == "cancel" || mode == "drop" {
                let signal = if mode == "cancel" {
                    cancel.clone()
                } else {
                    dropped.clone()
                };
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    signal.cancel();
                });
            }
            let expires = Instant::now()
                + if mode == "deadline" {
                    Duration::from_millis(50)
                } else {
                    Duration::from_secs(5)
                };
            let result = collect(&mut command, &cancel, &dropped, &mut shutdown, expires).await;
            assert_eq!(result.is_ok(), mode == "success", "owned child mode {mode}");
        }
    }
}
