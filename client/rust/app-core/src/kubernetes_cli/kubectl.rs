//! kubectl is optional. Package operations share the reviewed, revocable CLI
//! plan lifecycle; discovery never executes an executable found on disk.
use super::*;
use crate::{AppCore, AppResult};

#[derive(Debug, Serialize)]
pub struct KubectlDiscovery {
    pub supported: bool,
    pub candidates: Vec<CliCandidate>,
    pub package_managers: Vec<InstallMethod>,
    pub documentation: String,
}

fn documentation() -> &'static str {
    if cfg!(windows) {
        "https://kubernetes.io/docs/tasks/tools/install-kubectl-windows/"
    } else if cfg!(target_os = "macos") {
        "https://kubernetes.io/docs/tasks/tools/install-kubectl-macos/"
    } else {
        "https://kubernetes.io/docs/tasks/tools/install-kubectl-linux/"
    }
}

fn arguments(method: InstallMethod) -> Result<Vec<String>> {
    Ok(match method {
        InstallMethod::Homebrew => vec!["install", "kubectl"],
        InstallMethod::Winget => vec!["install", "--exact", "--id", "Kubernetes.kubectl"],
        InstallMethod::Scoop => vec!["install", "kubectl"],
        InstallMethod::Chocolatey => vec!["install", "kubernetes-cli"],
        InstallMethod::OfficialRelease => return Err(CliError::Invalid),
    }
    .into_iter()
    .map(str::to_owned)
    .collect())
}

impl FluxCliRuntime {
    pub fn discover_kubectl(&self) -> Result<KubectlDiscovery> {
        if mobile() {
            return Ok(KubectlDiscovery {
                supported: false,
                candidates: vec![],
                package_managers: vec![],
                documentation: documentation().into(),
            });
        }
        self.ensure_open()?;
        let mut paths = std::collections::BTreeSet::new();
        for directory in search_directories() {
            if let Ok(path) = executable(&directory.join(binary_name("kubectl"))) {
                paths.insert(path);
            }
        }
        Ok(KubectlDiscovery {
            supported: true,
            candidates: paths
                .into_iter()
                .map(|path| CliCandidate { path })
                .collect(),
            package_managers: [
                InstallMethod::Homebrew,
                InstallMethod::Winget,
                InstallMethod::Scoop,
                InstallMethod::Chocolatey,
            ]
            .into_iter()
            .filter(|method| manager(*method).is_some())
            .collect(),
            documentation: documentation().into(),
        })
    }

    pub async fn probe_kubectl(
        &self,
        path: &Path,
        cancel: CancellationToken,
    ) -> Result<CliVersion> {
        self.ensure_open()?;
        let binary = inspect(path)?;
        let output = self
            .run(
                &binary,
                &["version".into(), "--client".into(), "--output=json".into()],
                None,
                Duration::from_secs(10),
                cancel,
            )
            .await?;
        if output.exit_code != 0 {
            return Err(CliError::Process);
        }
        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| CliError::Invalid)?;
        let version = response
            .pointer("/clientVersion/gitVersion")
            .and_then(|v| v.as_str())
            .ok_or(CliError::Invalid)?;
        let version = semver::Version::parse(version.strip_prefix('v').unwrap_or(version))
            .map_err(|_| CliError::Invalid)?;
        if version.major != 1 || !version.pre.is_empty() || !version.build.is_empty() {
            return Err(CliError::Invalid);
        }
        Ok(CliVersion {
            path: binary.path,
            version: version.to_string(),
            sha256: binary.hash,
        })
    }

    pub fn prepare_kubectl_install(&self, method: InstallMethod) -> Result<InstallReview> {
        self.ensure_open()?;
        let arguments = arguments(method)?;
        let binary = inspect(&manager(method).ok_or(CliError::NotFound)?)?;
        let ticket = Uuid::new_v4().to_string();
        let review = InstallReview {
            tool: "kubectl".into(),
            ticket: ticket.clone(),
            method,
            operation: InstallOperation::Install,
            // The package manager owns its destination/version. Do not promise
            // a directory or release that the app cannot enforce.
            version: String::new(),
            executable: Some(binary.path.clone()),
            arguments,
            source: documentation().into(),
            destination: PathBuf::new(),
            verifier: None,
            executable_sha256: Some(binary.hash.clone()),
        };
        let mut plans = self.plans.lock().map_err(|_| CliError::Cancelled)?;
        plans.retain(|_, plan| plan.created.elapsed() < PLAN_LIFETIME);
        if plans.len() >= 4 {
            return Err(CliError::Invalid);
        }
        plans.insert(
            ticket,
            Plan {
                created: Instant::now(),
                review: review.clone(),
                binary,
                previous: None,
            },
        );
        Ok(review)
    }
}

impl AppCore {
    pub async fn kube_kubectl_discover(&self) -> AppResult<KubectlDiscovery> {
        let (owner, unlocked) = self.unlocked().await?;
        let result = unlocked
            .kube
            .cli
            .discover_kubectl()
            .map_err(crate::kubernetes_flux::error)?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(result)
    }
    pub async fn kube_kubectl_probe(&self, path: String) -> AppResult<CliVersion> {
        let (owner, unlocked) = self.unlocked().await?;
        let result = unlocked
            .kube
            .cli
            .probe_kubectl(Path::new(&path), unlocked.kube.cli.cancellation())
            .await
            .map_err(crate::kubernetes_flux::error)?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(result)
    }
    pub async fn kube_kubectl_review(&self, method: InstallMethod) -> AppResult<InstallReview> {
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        unlocked
            .kube
            .cli
            .prepare_kubectl_install(method)
            .map_err(crate::kubernetes_flux::error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kubectl_package_commands_are_fixed_and_never_download_or_invoke_a_shell() {
        assert_eq!(
            arguments(InstallMethod::Homebrew).unwrap(),
            ["install", "kubectl"]
        );
        assert_eq!(
            arguments(InstallMethod::Winget).unwrap(),
            ["install", "--exact", "--id", "Kubernetes.kubectl"]
        );
        assert_eq!(
            arguments(InstallMethod::Scoop).unwrap(),
            ["install", "kubectl"]
        );
        assert_eq!(
            arguments(InstallMethod::Chocolatey).unwrap(),
            ["install", "kubernetes-cli"]
        );
        assert!(arguments(InstallMethod::OfficialRelease).is_err());
    }

    #[test]
    fn kubectl_plans_share_the_unlock_lifetime() {
        let runtime = FluxCliRuntime::default();
        runtime.close();
        assert!(runtime.discover_kubectl().is_err());
        assert!(runtime
            .prepare_kubectl_install(InstallMethod::Homebrew)
            .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn kubectl_version_probe_uses_only_client_metadata_and_an_empty_kubeconfig() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("kubectl");
        std::fs::write(&executable, concat!(
            "#!/bin/sh\n",
            "test \"$1\" = version && test \"$2\" = --client && test \"$3\" = --output=json || exit 2\n",
            "test \"$KUBECONFIG\" = /dev/null || exit 3\n",
            "printf '{\"clientVersion\":{\"gitVersion\":\"v1.32.4\"}}'\n",
        )).unwrap();
        make_executable(&executable).unwrap();
        let runtime = FluxCliRuntime::default();
        let version = runtime
            .probe_kubectl(&executable, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(version.version, "1.32.4");
        runtime.close();
        assert!(matches!(
            runtime
                .probe_kubectl(&executable, CancellationToken::new())
                .await,
            Err(CliError::Cancelled)
        ));
    }
}
