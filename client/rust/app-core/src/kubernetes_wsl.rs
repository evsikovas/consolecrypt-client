//! WSL receives a private, revocable loopback capability, never cluster
//! credentials. NAT is deliberately rejected by a live host reachability test.
use crate::kubernetes_cli::{inspect, run, Binary};
use crate::kubernetes_proxy::CliProxyLease;
use crate::{AppError, AppResult};
use cc_terminal_core::{local::LocalCommand, ShellLifetime};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

const PROBE: &str = r#"exec 3<>/dev/tcp/127.0.0.1/"$1" || exit 1
printf %s "$2" >&3"#;
const STAGE: &str = r#"set -eu
umask 077
d=$(/usr/bin/mktemp -d /tmp/consolecrypt-kube.XXXXXXXXXXXX)
trap '/bin/rm -rf -- "$d"' EXIT
/bin/chmod 700 "$d"
/bin/cat -- "$1" > "$d/config"
/bin/chmod 600 "$d/config"
printf %s "$d"
trap - EXIT"#;
const SHELL: &str = r#"set -eu
d=$1
trap '/bin/rm -rf -- "$d"' EXIT
export KUBECONFIG="$d/config"
HOME=$(cd ~ && pwd -P)
export HOME
cd "$HOME"
/bin/bash --noprofile --norc -i"#;
const CLEAN: &str = r#"/bin/rm -rf -- "$1""#;
const PATH: &str = "PATH=/usr/local/bin:/usr/bin:/bin";

pub(crate) struct WslCapability {
    binary: Binary,
    distribution: String,
    directory: String,
    closed: AtomicBool,
}
impl WslCapability {
    pub(crate) fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        // The owning CliProxyLease is revoked before this best-effort cleanup.
        // Do not shut down a distro: other local terminals may be using it.
        let binary = self.binary.clone();
        let args = shell_args(
            &self.distribution,
            CLEAN,
            std::slice::from_ref(&self.directory),
        );
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = run(
                    &binary,
                    &args,
                    None,
                    Duration::from_secs(10),
                    CancellationToken::new(),
                )
                .await;
            });
        }
    }
}
impl Drop for WslCapability {
    fn drop(&mut self) {
        self.close();
    }
}
impl ShellLifetime for WslCapability {
    fn close(&self) {
        self.close();
    }
}

fn failure(rule: &'static str) -> AppError {
    AppError::invalid("kubernetes", rule)
}
fn shell_args(distribution: &str, script: &str, args: &[String]) -> Vec<String> {
    [
        "--distribution",
        distribution,
        "--exec",
        "/usr/bin/env",
        "-i",
        PATH,
        "TERM=xterm-256color",
        "LC_ALL=C.UTF-8",
        "/bin/bash",
        "--noprofile",
        "--norc",
        "-c",
        script,
        "consolecrypt-kube",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(args.iter().cloned())
    .collect()
}
fn private_directory(value: &str) -> bool {
    value
        .strip_prefix("/tmp/consolecrypt-kube.")
        .is_some_and(|suffix| {
            suffix.len() == 12 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}
fn translated_path(bytes: &[u8]) -> AppResult<String> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| failure("wsl_path_invalid"))?
        .trim_end_matches(['\r', '\n']);
    if value.len() > 4096 || !value.starts_with('/') || value.chars().any(char::is_control) {
        return Err(failure("wsl_path_invalid"));
    }
    Ok(value.into())
}

/// The nonce has no authority. Only a process that can reach the host's exact
/// 127.0.0.1 listener can complete this test; a WSL NAT loopback is insufficient.
async fn reachable(
    binary: &Binary,
    distribution: &str,
    cancel: CancellationToken,
) -> AppResult<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| failure("wsl_loopback_unavailable"))?;
    let port = listener
        .local_addr()
        .map_err(|_| failure("wsl_loopback_unavailable"))?
        .port();
    let nonce = uuid::Uuid::new_v4().to_string();
    let args = shell_args(distribution, PROBE, &[port.to_string(), nonce.clone()]);
    let verify = async {
        let (socket, _) = listener
            .accept()
            .await
            .map_err(|_| failure("wsl_loopback_unavailable"))?;
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        socket
            .take(128)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| failure("wsl_loopback_unavailable"))?;
        if bytes.as_slice() != nonce.as_bytes() {
            return Err(failure("wsl_loopback_unavailable"));
        }
        Ok(())
    };
    let launch = async {
        let result = run(binary, &args, None, Duration::from_secs(20), cancel.clone())
            .await
            .map_err(|_| failure("wsl_loopback_unavailable"))?;
        if result.exit_code != 0 {
            return Err(failure("wsl_loopback_unavailable"));
        }
        Ok(())
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AppError::VaultLocked),
        result = tokio::time::timeout(Duration::from_secs(20), async { tokio::try_join!(verify, launch)?; Ok(()) }) =>
            result.map_err(|_|failure("wsl_loopback_unavailable"))?,
    }
}

pub(crate) async fn prepare(
    executable: &Path,
    distribution: &str,
    lease: &Arc<CliProxyLease>,
) -> AppResult<(LocalCommand, Arc<WslCapability>)> {
    let binary = inspect(executable).map_err(|_| failure("context_shell_unavailable"))?;
    let cancel = lease.cancellation();
    reachable(&binary, distribution, cancel.clone()).await?;
    let args = vec![
        "--distribution".into(),
        distribution.into(),
        "--exec".into(),
        "/usr/bin/wslpath".into(),
        "-u".into(),
        lease.path().to_string_lossy().into_owned(),
    ];
    let output = run(
        &binary,
        &args,
        None,
        Duration::from_secs(10),
        cancel.clone(),
    )
    .await
    .map_err(|_| failure("wsl_path_invalid"))?;
    if output.exit_code != 0 {
        return Err(failure("wsl_path_invalid"));
    }
    let translated = translated_path(&output.stdout)?;
    let output = run(
        &binary,
        &shell_args(distribution, STAGE, &[translated]),
        None,
        Duration::from_secs(10),
        cancel.clone(),
    )
    .await
    .map_err(|_| failure("wsl_config_failed"))?;
    let directory =
        std::str::from_utf8(&output.stdout).map_err(|_| failure("wsl_config_failed"))?;
    if output.exit_code != 0 || !private_directory(directory) {
        return Err(failure("wsl_config_failed"));
    }
    let capability = Arc::new(WslCapability {
        binary,
        distribution: distribution.into(),
        directory: directory.into(),
        closed: AtomicBool::new(false),
    });
    if cancel.is_cancelled() {
        return Err(AppError::VaultLocked);
    }
    let command = LocalCommand {
        executable: executable.to_owned(),
        arguments: shell_args(
            distribution,
            SHELL,
            std::slice::from_ref(&capability.directory),
        ),
        environment: crate::kubernetes_cli::clean_environment(),
        directory: executable
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| failure("context_shell_unavailable"))?,
    };
    Ok((command, capability))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_paths_and_distributions_are_single_argv_values() {
        let distro = "a distro; $(printf injected)";
        let args = shell_args(distro, STAGE, &["/mnt/c/User's path/a b".into()]);
        assert_eq!(args[1], distro);
        assert_eq!(args.last().unwrap(), "/mnt/c/User's path/a b");
        assert!(!args.iter().any(|arg| arg == "--shell-type"));
        assert_eq!(
            translated_path(b"/mnt/c/a b/config\r\n").unwrap(),
            "/mnt/c/a b/config"
        );
        assert!(translated_path(b"/mnt/c/a\ninjected").is_err());
        assert!(private_directory("/tmp/consolecrypt-kube.A123b56789cd"));
        assert!(!private_directory("/tmp/consolecrypt-kube.A123b56789cd/.."));
        assert!(!private_directory("/tmp/consolecrypt-kube.A123b56789cd\n"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wsl_scripts_probe_host_and_stage_only_private_capability() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        // Run the exact fixed probe/staging scripts using the host's bash. The
        // distro transport itself is exercised on the Windows runner.
        let config = temp.path().join("capability with ' spaces");
        std::fs::write(&config, b"synthetic revocable proxy capability").unwrap();
        let result = tokio::process::Command::new("/bin/bash")
            .args(["--noprofile", "--norc", "-c", STAGE, "test"])
            .arg(&config)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .await
            .unwrap();
        assert!(result.status.success());
        let directory = String::from_utf8(result.stdout).unwrap();
        assert!(private_directory(&directory));
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let target = PathBuf::from(&directory).join("config");
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::read(&config).unwrap(),
            std::fs::read(target).unwrap()
        );
        let status = tokio::process::Command::new("/bin/bash")
            .args(["--noprofile", "--norc", "-c", CLEAN, "test", &directory])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .status()
            .await
            .unwrap();
        assert!(status.success());
        assert!(!Path::new(&directory).exists());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let child = tokio::process::Command::new("/bin/bash")
            .args(["--noprofile", "--norc", "-c", PROBE, "test", &port, "nonce"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .spawn()
            .unwrap();
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).await.unwrap();
        assert!(child.wait_with_output().await.unwrap().status.success());
        assert_eq!(bytes, b"nonce");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn actual_wsl_command_boundary_checks_host_loopback_and_revocation() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("fixture-wsl");
        // Emulate only wsl.exe's transport prefix, then execute the exact env
        // and fixed bash argv. No parser interprets the distro name as code.
        std::fs::write(&executable, "#!/bin/sh\nshift 3\nexec \"$@\"\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let binary = inspect(&executable).unwrap();
        reachable(
            &binary,
            "distro with ; characters",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            reachable(&binary, "fixture", cancelled).await,
            Err(AppError::VaultLocked)
        ));

        std::fs::write(&executable, "#!/bin/sh\nexit 1\n").unwrap();
        let binary = inspect(&executable).unwrap();
        let error = reachable(&binary, "nat-fixture", CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::InvalidInput { .. }));
    }
}
