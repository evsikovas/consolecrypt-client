use crate::{AutomationError, HelperPeer, Result, TargetHandler};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;

const BOOTSTRAP: &str = "import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module('consolecrypt_runner',run_name='__main__')";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelperLaunch {
    Native {
        python: PathBuf,
        module_dir: PathBuf,
        root: PathBuf,
    },
    Wsl {
        distribution: String,
        user: String,
        python: String,
        module_dir: String,
        root: String,
    },
}

fn unix_path(value: &str) -> bool {
    value.starts_with('/') && value.len() <= 4096 && !value.chars().any(|c| c.is_control())
}

impl HelperLaunch {
    pub fn command(&self) -> Result<std::process::Command> {
        let mut command = self.base_command()?;
        command.args(["--stdio", "--root"]);
        match self {
            Self::Native { root, .. } => command.arg(root),
            Self::Wsl { root, .. } => command.arg(root),
        };
        Ok(command)
    }

    pub(crate) fn base_command(&self) -> Result<std::process::Command> {
        match self {
            Self::Native {
                python,
                module_dir,
                root,
            } => {
                if cfg!(target_os = "windows") {
                    return Err(AutomationError::Invalid(
                        "Windows requires explicit WSL2 or remote Linux",
                    ));
                }
                if [python, module_dir, root]
                    .iter()
                    .any(|path| !path.is_absolute())
                {
                    return Err(AutomationError::Invalid("helper paths must be absolute"));
                }
                let mut command = std::process::Command::new(python);
                command.args(["-I", "-c", BOOTSTRAP]).arg(module_dir);
                Ok(command)
            }
            Self::Wsl {
                distribution,
                user,
                python,
                module_dir,
                root,
            } => {
                if !cfg!(target_os = "windows") {
                    return Err(AutomationError::Invalid("WSL is available on Windows only"));
                }
                if distribution.is_empty()
                    || distribution.len() > 128
                    || distribution.starts_with('-')
                    || distribution.chars().any(|c| c.is_control())
                    || user.is_empty()
                    || user == "root"
                    || user.len() > 64
                    || user.starts_with('-')
                    || !user
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
                    || ![python, module_dir, root]
                        .iter()
                        .all(|value| unix_path(value))
                {
                    return Err(AutomationError::Invalid(
                        "invalid explicit WSL distribution, user or Unix paths",
                    ));
                }
                let system = PathBuf::from(
                    std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
                );
                let mut command = std::process::Command::new(system.join("System32/wsl.exe"));
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                command.args([
                    "--distribution",
                    distribution,
                    "--user",
                    user,
                    "--exec",
                    python,
                    "-I",
                    "-c",
                    BOOTSTRAP,
                    module_dir,
                ]);
                Ok(command)
            }
        }
    }

    pub async fn spawn(&self) -> Result<HelperProcess> {
        self.spawn_with_targets(None).await
    }

    pub async fn spawn_with_targets(
        &self,
        targets: Option<Arc<dyn TargetHandler>>,
    ) -> Result<HelperProcess> {
        Self::spawn_command(self.command()?, targets).await
    }

    /// Private enrolled Linux service; ordinary helper command is unchanged.
    /// Bootstrap carries public pins/metadata through the OWNED existing pipe,
    /// never command arguments or a public network enrollment endpoint.
    pub async fn spawn_service(
        &self,
        targets: Arc<dyn TargetHandler>,
        bootstrap: serde_json::Value,
    ) -> Result<HelperProcess> {
        self.spawn_service_control(targets, bootstrap, "service_bootstrap")
            .await
    }
    pub(crate) async fn spawn_service_existing(
        &self,
        targets: Arc<dyn TargetHandler>,
        bootstrap: serde_json::Value,
    ) -> Result<HelperProcess> {
        self.spawn_service_control(targets, bootstrap, "service_resume_bootstrap")
            .await
    }
    /// Fixed metadata-only initial frame. No TargetHandler, vault callback,
    /// job admission or live helper is returned, even on successful retirement.
    pub(crate) async fn retire_service_existing(
        &self,
        bootstrap: serde_json::Value,
    ) -> Result<bool> {
        if !matches!(self, Self::Native { .. }) || !cfg!(target_os = "linux") {
            return Err(AutomationError::Invalid(
                "service retirement requires original Linux backend",
            ));
        }
        let Self::Native { root, .. } = self else {
            unreachable!()
        };
        let mut command = self.base_command()?;
        command.arg("--service-stdio").arg("--root").arg(root);
        let process = Self::spawn_command(command, None).await?;
        let response = process
            .peer
            .request(
                "retire_expired_bootstrap",
                bootstrap,
                Duration::from_secs(30),
            )
            .await;
        process.close().await;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct Observation {
            retired: bool,
            already_retired: bool,
        }
        let observed: Observation = serde_json::from_value(response?)
            .map_err(|_| AutomationError::Protocol("invalid service retirement observation"))?;
        if observed.already_retired && !observed.retired {
            return Err(AutomationError::Protocol(
                "invalid service retirement observation",
            ));
        }
        Ok(observed.retired)
    }
    async fn spawn_service_control(
        &self,
        targets: Arc<dyn TargetHandler>,
        bootstrap: serde_json::Value,
        operation: &'static str,
    ) -> Result<HelperProcess> {
        if !matches!(self, Self::Native { .. }) || !cfg!(target_os = "linux") {
            return Err(AutomationError::Invalid(
                "service requires Linux local native backend",
            ));
        }
        let Self::Native { root, .. } = self else {
            unreachable!()
        };
        let mut command = self.base_command()?;
        command.arg("--service-stdio").arg("--root").arg(root);
        let process = Self::spawn_command(command, Some(targets)).await?;
        match process
            .peer
            .request(operation, bootstrap, Duration::from_secs(30))
            .await
        {
            Ok(value) if value.get("bootstrapped") == Some(&serde_json::json!(true)) => Ok(process),
            _ => {
                process.close().await;
                Err(AutomationError::Invalid("service bootstrap denied"))
            }
        }
    }

    pub(crate) fn dependency_command(
        &self,
        operation: uuid::Uuid,
    ) -> Result<(
        std::process::Command,
        crate::dependency_launch::DependencyExpectation,
    )> {
        let root = match self {
            Self::Native { root, .. } => root
                .to_str()
                .ok_or(AutomationError::Invalid("dependency root must be UTF-8"))?,
            Self::Wsl { root, .. } => root.as_str(),
        };
        let expected = crate::dependency_launch::DependencyExpectation::new(root, operation)?;
        let mut command = self.base_command()?;
        command.args([
            "--dependency-stdio",
            "--root",
            root,
            "--operation-id",
            &operation.to_string(),
        ]);
        Ok((command, expected))
    }

    /// New private process/root; never reuses a job peer or target handler.
    /// Explicit Native profile/source/lifecycle gates are required upstream.
    pub async fn spawn_dependency(
        &self,
        operation: uuid::Uuid,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<crate::DependencyHelperConnection> {
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        let (command, expected) = self.dependency_command(operation)?;
        let local = Self::spawn_command(command, None).await?;
        crate::DependencyHelperConnection::local(local, expected, cancel).await
    }

    async fn spawn_command(
        command: std::process::Command,
        targets: Option<Arc<dyn TargetHandler>>,
    ) -> Result<HelperProcess> {
        Self::spawn_command_channels(command, targets, None).await
    }
    pub(crate) async fn spawn_command_channels(
        command: std::process::Command,
        targets: Option<Arc<dyn TargetHandler>>,
        vault: Option<Arc<dyn crate::VaultValidationHandler>>,
    ) -> Result<HelperProcess> {
        let mut command = tokio::process::Command::from(command);
        command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().ok_or(AutomationError::Closed)?;
        let stdin = child.stdin.take().ok_or(AutomationError::Closed)?;
        if let Some(stderr) = child.stderr.take() {
            // Drain to avoid a blocked helper; keep no raw diagnostic/secret text.
            tokio::spawn(async move {
                let mut reader = stderr;
                let mut chunk = [0u8; 4096];
                while let Ok(count) = reader.read(&mut chunk).await {
                    if count == 0 {
                        break;
                    }
                }
            });
        }
        Ok(HelperProcess {
            peer: match vault {
                Some(handler) => HelperPeer::connect_with_vault(stdout, stdin, handler),
                None => HelperPeer::connect_with_targets(stdout, stdin, targets),
            },
            child: tokio::sync::Mutex::new(child),
        })
    }
}

pub struct HelperProcess {
    pub peer: Arc<HelperPeer>,
    child: tokio::sync::Mutex<tokio::process::Child>,
}

impl std::fmt::Debug for HelperProcess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HelperProcess")
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

impl HelperProcess {
    pub async fn close(&self) {
        // EOF asks only this helper to cancel its work. Never stop a distro or
        // a terminal. The helper's own process groups enforce cancellation.
        self.peer.close().await;
        let mut child = self.child.lock().await;
        if tokio::time::timeout(Duration::from_secs(20), child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

#[cfg(test)]
mod dependency_launch_tests {
    use super::*;
    #[test]
    #[ignore = "subprocess fixture; invoked only by dependency_failed_probe_awaits_owned_child_exit"]
    fn dependency_cleanup_fixture_child() {
        use std::io::Read;
        let token = std::env::var("CC_DEPENDENCY_CLEANUP_TOKEN").unwrap();
        let id = uuid::Uuid::parse_str(&token).unwrap();
        assert_eq!(id.to_string(), token);
        assert!(!id.is_nil());
        let root = PathBuf::from(std::env::var_os("CC_DEPENDENCY_CLEANUP_ROOT").unwrap());
        assert_eq!(root.file_name().unwrap().to_str().unwrap(), token);
        assert!(root.is_dir());
        std::fs::write(root.join("ready"), b"fixture-entered").unwrap();
        // Rust's own test harness emits non-protocol stdout. That deliberately
        // exercises failed probe parsing, while the child remains alive until
        // the owned control stdin is actually closed.
        let mut input = Vec::new();
        std::io::stdin().take(4097).read_to_end(&mut input).unwrap();
        assert!(input.len() <= 4096);
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(root.join("closed"), b"eof+delayed-cleanup").unwrap();
    }
    #[tokio::test]
    async fn dependency_failed_probe_awaits_owned_child_exit() {
        for cancelled in [false, true] {
            let own = tempfile::tempdir().unwrap();
            let operation = uuid::Uuid::new_v4();
            let root = own.path().join(operation.to_string());
            std::fs::create_dir(&root).unwrap();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--ignored",
                    "--exact",
                    "launch::dependency_launch_tests::dependency_cleanup_fixture_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("CC_DEPENDENCY_CLEANUP_TOKEN", operation.to_string())
                .env("CC_DEPENDENCY_CLEANUP_ROOT", &root);
            // libtest writes its banner before entering the fixture. Wait for
            // this owned child's readiness before connecting the protocol
            // parser: otherwise an early malformed-banner error can close its
            // stdout while libtest is still printing the test-name prefix,
            // making the child exit before it can observe stdin EOF.
            let mut command = tokio::process::Command::from(command);
            command
                .kill_on_drop(true)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut child = command.spawn().unwrap();
            let ready = root.join("ready");
            tokio::time::timeout(Duration::from_secs(10), async {
                while !ready.is_file() {
                    assert!(
                        child.try_wait().unwrap().is_none(),
                        "fixture exited before readiness"
                    );
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("owned cleanup fixture did not enter its body");
            let local = HelperProcess {
                peer: HelperPeer::connect_with_targets(
                    child.stdout.take().unwrap(),
                    child.stdin.take().unwrap(),
                    None,
                ),
                child: tokio::sync::Mutex::new(child),
            };
            let peer = local.peer.clone();
            let cancel = tokio_util::sync::CancellationToken::new();
            if cancelled {
                cancel.cancel();
            }
            let expected =
                crate::dependency_launch::DependencyExpectation::new("/fixture/jobs", operation)
                    .unwrap();
            let result = crate::DependencyHelperConnection::local(local, expected, &cancel).await;
            assert!(result.is_err());
            assert!(peer.is_closed());
            // A mere kill_on_drop or scheduled cleanup returns before this
            // delayed EOF marker can be produced; explicit wait must finish.
            assert_eq!(
                std::fs::read(root.join("closed")).unwrap(),
                b"eof+delayed-cleanup"
            );
        }
    }
    #[test]
    fn dependency_argv_is_fixed_and_ordinary_stdio_is_unchanged() {
        let launch = if cfg!(windows) {
            HelperLaunch::Wsl {
                distribution: "explicit-unused-fixture".into(),
                user: "runner".into(),
                python: "/opt/python".into(),
                module_dir: "/opt/runner/src".into(),
                root: "/home/runner/jobs".into(),
            }
        } else {
            HelperLaunch::Native {
                python: "/opt/python".into(),
                module_dir: "/opt/runner/src".into(),
                root: "/home/runner/jobs".into(),
            }
        };
        let id = uuid::Uuid::new_v4();
        let (command, expected) = launch.dependency_command(id).unwrap();
        let args = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            &args[args.len() - 5..],
            &[
                "--dependency-stdio",
                "--root",
                "/home/runner/jobs",
                "--operation-id",
                &id.to_string()
            ]
        );
        assert_eq!(expected.operation, id);
        assert!(!args.iter().any(|arg| arg == "--stdio"));
        let normal = launch.command().unwrap();
        let normal = normal
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            !normal
                .iter()
                .any(|arg| arg == "--dependency-stdio" || arg == "--operation-id")
        );
        assert_eq!(
            &normal[normal.len() - 3..],
            &["--stdio", "--root", "/home/runner/jobs"]
        );
        assert!(launch.dependency_command(uuid::Uuid::nil()).is_err());
    }
}
