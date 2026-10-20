//! Flux application boundary: unlock-scoped consent, protected output and
//! visible PTYs. Only ephemeral loopback capabilities reach external CLIs.
use crate::kubernetes_cli::{
    CliError, CliVersion, FluxAction, FluxDiscovery, InstallMethod, InstallOperation, InstallReview,
};
use crate::kubernetes_proxy::CliProxyLease;
use crate::{AppCore, AppError, AppResult};
use cc_terminal_core::{local::LocalCommand, ShellLifetime, TerminalSize, TerminalTarget};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, serde::Serialize)]
pub struct ContextShellProfile {
    pub id: String,
    pub label: String,
    pub shell: String,
}
#[derive(Debug, serde::Serialize)]
pub struct ContextTerminalProfiles {
    pub profiles: Vec<ContextShellProfile>,
    pub wsl_supported: bool,
}

/// Native shells can consume this machine's loopback capability. WSL2 NAT
/// and Windows/WSL path translation are not silently treated as supported.
fn context_shells() -> Vec<(ContextShellProfile, PathBuf, Vec<String>)> {
    let mut profiles = Vec::new();
    if cfg!(target_os = "macos") {
        profiles.push((
            "macos:zsh",
            "zsh",
            PathBuf::from("/bin/zsh"),
            vec!["-f".into(), "-i".into()],
        ));
    } else if cfg!(target_os = "linux") {
        profiles.push((
            "linux:bash",
            "bash",
            PathBuf::from("/bin/bash"),
            vec!["--noprofile".into(), "--norc".into(), "-i".into()],
        ));
    } else if cfg!(windows) {
        if let Some(system) = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            profiles.push((
                "windows:powershell",
                "Windows PowerShell",
                system.join("System32/WindowsPowerShell/v1.0/powershell.exe"),
                vec!["-NoLogo".into(), "-NoProfile".into()],
            ));
            profiles.push((
                "windows:cmd",
                "Command Prompt",
                system.join("System32/cmd.exe"),
                vec!["/D".into()],
            ));
        }
        if let Some(programs) = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            profiles.insert(
                0,
                (
                    "windows:pwsh",
                    "PowerShell 7",
                    programs.join("PowerShell/7/pwsh.exe"),
                    vec!["-NoLogo".into(), "-NoProfile".into()],
                ),
            );
        }
    }
    profiles
        .into_iter()
        .filter(|(_, _, path, _)| path.is_file())
        .map(|(id, label, path, args)| {
            (
                ContextShellProfile {
                    id: id.into(),
                    label: label.into(),
                    shell: if id == "windows:cmd" {
                        "cmd"
                    } else if id.starts_with("windows:") {
                        "powershell"
                    } else {
                        label
                    }
                    .into(),
                },
                path,
                args,
            )
        })
        .collect()
}

pub(crate) fn error(value: CliError) -> AppError {
    let rule = match value {
        CliError::UnsupportedPlatform => "desktop_only",
        CliError::Invalid => "cli_invalid",
        CliError::NotFound => "cli_not_found",
        CliError::Changed => "cli_changed",
        CliError::Cancelled => "cli_cancelled",
        CliError::Timeout => "cli_timeout",
        CliError::TooLarge => "cli_output_too_large",
        CliError::Process => "cli_failed",
        CliError::Download => "cli_download_failed",
        CliError::VerificationRequired => "cli_verifier_required",
        CliError::Verification => "cli_verification_failed",
        CliError::Install => "cli_install_failed",
    };
    AppError::invalid("kubernetes", rule)
}
impl AppCore {
    pub async fn kube_context_terminal_profiles(&self) -> AppResult<ContextTerminalProfiles> {
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(ContextTerminalProfiles {
            profiles: context_shells()
                .into_iter()
                .map(|(profile, _, _)| profile)
                .collect(),
            wsl_supported: false,
        })
    }
    pub async fn kube_context_terminal(
        &self,
        session_id: String,
        namespace: String,
        profile_id: String,
    ) -> AppResult<String> {
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        let (profile, executable, arguments) = context_shells()
            .into_iter()
            .find(|(profile, _, _)| profile.id == profile_id)
            .ok_or_else(|| AppError::invalid("kubernetes", "context_shell_unavailable"))?;
        let lease = self
            .kube_cli_proxy_in_namespace(session_id, namespace.clone())
            .await?;
        let mut environment = crate::kubernetes_cli::clean_environment();
        environment.insert("KUBECONFIG".into(), lease.path().into_os_string());
        let directory = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.is_dir())
            .unwrap_or_else(|| {
                executable
                    .parent()
                    .expect("absolute native shell")
                    .to_owned()
            });
        let opened = cc_terminal_core::local::open_command(
            LocalCommand {
                executable,
                arguments,
                environment,
                directory,
            },
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        )
        .await
        .map_err(|_| AppError::invalid("kubernetes", "cli_terminal_failed"))?;
        let lifetime = Arc::new(ScopedCliLifetime {
            inner: opened.lifetime.clone(),
            cancel: lease.cancellation(),
            lease: Some(lease),
        });
        watch_lifetime(&lifetime);
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if lifetime.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let title = format!(
            "{}{} · {} · {}",
            if live.info.production { "PROD · " } else { "" },
            profile.label,
            live.info.context_name,
            namespace
        );
        let target = TerminalTarget::Local {
            profile_id: "kubernetes-context".into(),
            shell: profile.shell,
            display_name: title.clone(),
            distribution: None,
        };
        let id = unlocked
            .ssh
            .terminals
            .open_channel(
                target,
                title,
                TerminalSize {
                    cols: 100,
                    rows: 30,
                },
                cc_terminal_core::OpenedShell {
                    lifetime: Some(lifetime),
                    ..opened
                },
            )
            .await?;
        Ok(id.to_string())
    }
    pub async fn kube_flux_discover(
        &self,
        manual_path: Option<String>,
    ) -> AppResult<FluxDiscovery> {
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        unlocked
            .kube
            .cli
            .discover(manual_path.as_deref().map(Path::new))
            .map_err(error)
    }
    pub async fn kube_flux_probe(&self, path: String) -> AppResult<CliVersion> {
        let (owner, unlocked) = self.unlocked().await?;
        let result = unlocked
            .kube
            .cli
            .probe(Path::new(&path), unlocked.kube.cli.cancellation())
            .await
            .map_err(error)?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(result)
    }
    pub async fn kube_flux_review(
        &self,
        version: String,
        method: InstallMethod,
        operation: InstallOperation,
    ) -> AppResult<InstallReview> {
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        match operation {
            InstallOperation::Install => unlocked.kube.cli.prepare_install(&version, method),
            InstallOperation::Update => unlocked.kube.cli.prepare_update(&version, method),
            InstallOperation::Remove => unlocked.kube.cli.prepare_remove(&version, method),
        }
        .map_err(error)
    }
    pub async fn kube_flux_cancel(&self, ticket: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        unlocked.kube.cli.cancel(&ticket);
        Ok(())
    }
    /// The caller supplies only the native ticket. Method, argv and destination
    /// are recovered from that immutable review, never accepted back from UI.
    pub async fn kube_flux_approve(&self, ticket: String) -> AppResult<Option<String>> {
        let (owner, unlocked) = self.unlocked().await?;
        let review = unlocked.kube.cli.review(&ticket).map_err(error)?;
        if review.method == InstallMethod::OfficialRelease {
            if review.operation == InstallOperation::Remove {
                let _profile = self.hold_session(&owner).await?;
                let _generation = owner.hold_unlocked(&unlocked).await?;
                unlocked.kube.cli.remove_release(&ticket).map_err(error)?;
            } else {
                unlocked
                    .kube
                    .cli
                    .install_release(&ticket, unlocked.kube.cli.cancellation())
                    .await
                    .map_err(error)?;
                let _profile = self.hold_session(&owner).await?;
                let _generation = owner.hold_unlocked(&unlocked).await?;
            }
            return Ok(None);
        }
        let package = unlocked
            .kube
            .cli
            .approve_package_install(&ticket)
            .map_err(error)?;
        let directory = package
            .executable
            .parent()
            .ok_or_else(|| AppError::invalid("kubernetes", "cli_invalid"))?
            .to_owned();
        let command = LocalCommand {
            executable: package.executable,
            arguments: package.arguments,
            environment: package.environment,
            directory,
        };
        let opened = cc_terminal_core::local::open_command(
            command,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        )
        .await
        .map_err(|_| AppError::invalid("kubernetes", "cli_terminal_failed"))?;
        let lifetime = Arc::new(ScopedCliLifetime {
            inner: opened.lifetime.clone(),
            lease: None,
            cancel: package.cancellation.clone(),
        });
        watch_lifetime(&lifetime);
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if package.cancellation.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let target = TerminalTarget::Local {
            profile_id: "flux-install".into(),
            shell: "CLI".into(),
            display_name: "Flux CLI".into(),
            distribution: None,
        };
        let id = unlocked
            .ssh
            .terminals
            .open_channel(
                target,
                "Flux CLI".into(),
                TerminalSize {
                    cols: 100,
                    rows: 30,
                },
                cc_terminal_core::OpenedShell {
                    lifetime: Some(lifetime),
                    ..opened
                },
            )
            .await?;
        Ok(Some(id.to_string()))
    }
    pub async fn kube_flux_run(
        &self,
        session_id: String,
        path: String,
        action: FluxAction,
    ) -> AppResult<Vec<u8>> {
        let (owner, unlocked) = self.unlocked().await?;
        let lease = self.kube_cli_proxy(session_id).await?;
        let result = unlocked
            .kube
            .cli
            .execute(
                Path::new(&path),
                action,
                &lease.path(),
                lease.cancellation(),
            )
            .await
            .map_err(error)?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if lease.cancellation().is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        output_bytes(result)
    }

    pub async fn kube_flux_terminal(&self, session_id: String, path: String) -> AppResult<String> {
        let (owner, unlocked) = self.unlocked().await?;
        let live = unlocked.kube.get(&session_id)?;
        // Verify the chosen executable is Flux before providing a terminal whose
        // PATH resolves that directory first. This action is explicit in UI.
        let version = unlocked
            .kube
            .cli
            .probe(Path::new(&path), live.cancel.child_token())
            .await
            .map_err(error)?;
        let lease = self.kube_cli_proxy(session_id).await?;
        let mut environment = crate::kubernetes_cli::clean_environment();
        environment.insert("KUBECONFIG".into(), lease.path().into_os_string());
        let mut paths = vec![version
            .path
            .parent()
            .ok_or_else(|| AppError::invalid("kubernetes", "cli_invalid"))?
            .to_owned()];
        if let Some(path) = environment.get(&OsString::from("PATH")) {
            paths.extend(std::env::split_paths(path).filter(|p| p.is_absolute()));
        }
        environment.insert(
            "PATH".into(),
            std::env::join_paths(paths)
                .map_err(|_| AppError::invalid("kubernetes", "cli_invalid"))?,
        );
        let (executable, arguments) = terminal_shell()?;
        let directory = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.is_dir())
            .unwrap_or_else(|| {
                executable
                    .parent()
                    .expect("absolute system shell")
                    .to_owned()
            });
        let opened = cc_terminal_core::local::open_command(
            LocalCommand {
                executable,
                arguments,
                environment,
                directory,
            },
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        )
        .await
        .map_err(|_| AppError::invalid("kubernetes", "cli_terminal_failed"))?;
        let lifetime = Arc::new(ScopedCliLifetime {
            inner: opened.lifetime.clone(),
            cancel: lease.cancellation(),
            lease: Some(lease),
        });
        watch_lifetime(&lifetime);
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        if lifetime.cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        let title = format!(
            "{}Flux · {}",
            if live.info.production { "PROD · " } else { "" },
            live.info.context_name
        );
        let target = TerminalTarget::Local {
            profile_id: "flux-context".into(),
            shell: "CLI".into(),
            display_name: title.clone(),
            distribution: None,
        };
        let id = unlocked
            .ssh
            .terminals
            .open_channel(
                target,
                title,
                TerminalSize {
                    cols: 100,
                    rows: 30,
                },
                cc_terminal_core::OpenedShell {
                    lifetime: Some(lifetime),
                    ..opened
                },
            )
            .await?;
        Ok(id.to_string())
    }
}
fn terminal_shell() -> AppResult<(PathBuf, Vec<String>)> {
    if cfg!(target_os = "macos") {
        Ok((PathBuf::from("/bin/zsh"), vec!["-f".into(), "-i".into()]))
    } else if cfg!(target_os = "linux") {
        Ok((
            PathBuf::from("/bin/bash"),
            vec!["--noprofile".into(), "--norc".into(), "-i".into()],
        ))
    } else if cfg!(windows) {
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| AppError::invalid("kubernetes", "cli_terminal_failed"))?;
        Ok((
            system.join("System32/WindowsPowerShell/v1.0/powershell.exe"),
            vec!["-NoLogo".into(), "-NoProfile".into()],
        ))
    } else {
        Err(AppError::invalid("kubernetes", "desktop_only"))
    }
}
struct ScopedCliLifetime {
    inner: Option<Arc<dyn ShellLifetime>>,
    lease: Option<Arc<CliProxyLease>>,
    cancel: CancellationToken,
}
impl ShellLifetime for ScopedCliLifetime {
    fn close(&self) {
        self.cancel.cancel();
        if let Some(lease) = &self.lease {
            lease.close();
        }
        if let Some(inner) = &self.inner {
            inner.close();
        }
    }
}
impl Drop for ScopedCliLifetime {
    fn drop(&mut self) {
        self.close();
    }
}
fn watch_lifetime(lifetime: &Arc<ScopedCliLifetime>) {
    let cancel = lifetime.cancel.clone();
    let weak = Arc::downgrade(lifetime);
    tokio::spawn(async move {
        cancel.cancelled().await;
        if let Some(lifetime) = weak.upgrade() {
            lifetime.close();
        }
    });
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    #[tokio::test]
    async fn lock_cancels_a_native_flux_probe_and_a_new_unlock_has_fresh_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("started");
        let binary = script(
            temp.path(),
            "flux",
            &format!(
                "touch '{}'\nsleep 20\nprintf '{{\"flux\":\"v2.9.6\"}}'",
                marker.display()
            ),
        );
        let app = AppCore::new(crate::AppConfig::for_tests(
            temp.path().join("app").to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("fixture".into(), "flux-test-passphrase".into())
            .await
            .unwrap();
        let (_, before) = app.unlocked().await.unwrap();
        let worker_app = app.clone();
        let path = binary.to_string_lossy().into_owned();
        let task = tokio::spawn(async move { worker_app.kube_flux_probe(path).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !marker.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        app.lock().await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert!(before.kube.cli.cancellation().is_cancelled());
        app.unlock_with_passphrase("flux-test-passphrase".into())
            .await
            .unwrap();
        let (_, after) = app.unlocked().await.unwrap();
        assert!(!after.kube.cli.cancellation().is_cancelled());
        assert!(!Arc::ptr_eq(&before.kube.cli, &after.kube.cli));
    }
    #[tokio::test]
    async fn direct_pty_preserves_literal_arguments_and_explicit_environment() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("must-not-exist");
        let binary=script(temp.path(),"fixture","printf 'ARG=%s\\nCONFIG=%s\\n' \"$1\" \"$KUBECONFIG\"\nif [ -n \"$HOME\" ]; then printf INHERITED_HOME; fi");
        let literal = format!("literal; touch {}", marker.display());
        let environment = [(
            OsString::from("KUBECONFIG"),
            OsString::from("/fixture/proxy/config"),
        )]
        .into_iter()
        .collect();
        let mut opened = cc_terminal_core::local::open_command(
            LocalCommand {
                executable: binary,
                arguments: vec![literal.clone()],
                environment,
                directory: temp.path().into(),
            },
            TerminalSize { cols: 80, rows: 24 },
        )
        .await
        .unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while let Some(event) = opened.channel.reader.recv().await {
                match event {
                    cc_ssh_core::ShellEvent::Data(chunk) => bytes.extend_from_slice(&chunk),
                    cc_ssh_core::ShellEvent::Closed => break,
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let output = String::from_utf8(bytes).unwrap();
        assert!(output.contains(&literal));
        assert!(output.contains("CONFIG=/fixture/proxy/config"));
        assert!(!output.contains("INHERITED_HOME"));
        assert!(!marker.exists());
    }
}

pub(crate) fn output_bytes(result: crate::kubernetes_cli::CliOutput) -> AppResult<Vec<u8>> {
    let stdout = zeroize::Zeroizing::new(String::from_utf8_lossy(&result.stdout).into_owned());
    let stderr = zeroize::Zeroizing::new(String::from_utf8_lossy(&result.stderr).into_owned());
    #[derive(serde::Serialize)]
    struct Output<'a> {
        exit_code: i32,
        stdout: &'a str,
        stderr: &'a str,
    }
    serde_json::to_vec(&Output {
        exit_code: result.exit_code,
        stdout: &stdout,
        stderr: &stderr,
    })
    .map_err(|_| AppError::invalid("kubernetes", "cli_output_invalid"))
}
