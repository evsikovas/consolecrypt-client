//! New-only source scaffolding. No SSH, release trust, execution or grants.
use super::*;
use serde_json::json;
use std::path::PathBuf;

const MAX_ARTIFACT: u64 = 32 * 1024 * 1024;
const MAX_MANIFEST: u64 = 64 * 1024;
const MAX_STATIC: usize = 1024 * 1024;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstallOperation {
    Install,
    Update,
    Rollback,
    Remove,
    Status,
}
impl AgentInstallOperation {
    fn name(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Rollback => "rollback",
            Self::Remove => "remove",
            Self::Status => "status",
        }
    }
    pub fn needs_release(self) -> bool {
        matches!(self, Self::Install | Self::Update)
    }
}

/// Actual read-only public input bytes; no signature or desired-version grant.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallClaimedRelease {
    artifact_sha256: String,
    artifact_bytes: u64,
    manifest_sha256: String,
    signature_verified: bool,
}
pub struct AgentInstallerSeed {
    namespace: namespace::ManagedSeedNamespace,
    root: ProjectRoot,
    path: PathBuf,
    revision: ProjectRevision,
    claimed: Option<AgentInstallClaimedRelease>,
}
impl AgentInstallerSeed {
    /// Files always land in a NEW original-profile UUID. The Native caller
    /// publishes its catalog row only AFTER recheck(), under original guards.
    pub fn create(
        profile: &Path,
        project: Uuid,
        operation: AgentInstallOperation,
        start: bool,
        release: Option<(&Path, &Path)>,
        validate_manifest: impl FnOnce(&[u8], &str, u64) -> Result<()>,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        require(!project.is_nil() && operation.needs_release() == release.is_some())?;
        require(
            !start
                || matches!(
                    operation,
                    AgentInstallOperation::Install
                        | AgentInstallOperation::Update
                        | AgentInstallOperation::Rollback
                ),
        )?;
        check()?;
        // Input type/length bounds BEFORE allocation, mkdir or copying.
        let mut captured = release
            .map(|(a, m)| {
                Ok::<_, AutomationError>((capture(a, MAX_ARTIFACT)?, capture(m, MAX_MANIFEST)?))
            })
            .transpose()?;
        // Input syntax/bindings only. The one fixed AppCore validator reuses
        // the actual public wire parser; it MUST NOT assert signer trust.
        let validated = if let Some((artifact, manifest)) = captured.as_mut() {
            use std::io::{Seek, SeekFrom};
            let mut magic = [0u8; 4];
            artifact.file.read_exact(&mut magic)?;
            require(magic == *b"PK\x03\x04")?;
            artifact.file.seek(SeekFrom::Start(0))?;
            let mut h = Sha256::new();
            let mut seen = 0u64;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                check()?;
                let n = artifact.file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                seen += n as u64;
                require(seen <= artifact.bytes)?;
                h.update(&buffer[..n]);
            }
            require(seen == artifact.bytes)?;
            artifact.file.seek(SeekFrom::Start(0))?;
            let actual = hex::encode(h.finalize());
            let mut raw = Vec::with_capacity(manifest.bytes as usize);
            (&mut manifest.file)
                .take(MAX_MANIFEST + 1)
                .read_to_end(&mut raw)?;
            require(raw.len() as u64 == manifest.bytes)?;
            manifest.file.seek(SeekFrom::Start(0))?;
            validate_manifest(&raw, &actual, seen)?;
            Some((actual, hex::encode(Sha256::digest(&raw))))
        } else {
            None
        };
        check()?;
        let playbook=serde_json::to_string(&json!([{"name":"Linux Agent software delivery","hosts":"all","gather_facts":false,
            "become":true,"vars":{"cc_agent_install_operation":operation.name(),"cc_agent_install_start":start,
                "cc_agent_install_approved":operation!=AgentInstallOperation::Status,
                "cc_agent_release_artifact":"{{ playbook_dir }}/release/candidate.zip",
                "cc_agent_release_manifest":"{{ playbook_dir }}/release/candidate-release.json"},
            "roles":["consolecrypt_agent_install"]}])).map_err(|_|recovery())?;
        Self::write_fixed(
            profile,
            project,
            FixedTree {
                files: static_files().to_vec(),
                directories: vec![
                    "roles",
                    "roles/consolecrypt_agent_install",
                    "roles/consolecrypt_agent_install/defaults",
                    "roles/consolecrypt_agent_install/tasks",
                    "roles/consolecrypt_agent_install/meta",
                    "roles/consolecrypt_agent_install/files",
                    "release",
                ],
                playbook,
                captured,
                validated,
            },
            check,
        )
    }
    /// Second fixed source-only purpose. No archive, trust, Actor or install.
    pub fn create_monitoring_node(
        profile: &Path,
        project: Uuid,
        operation: crate::monitoring_node_plan::MonitoringNodeOperation,
        plan: &crate::monitoring_node_plan::MonitoringNodePlan,
        check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        require(!project.is_nil())?;
        plan.validate()?;
        Self::write_fixed(
            profile,
            project,
            FixedTree {
                files: monitoring_files().to_vec(),
                directories: vec![],
                playbook: plan.playbook(operation)?,
                captured: None,
                validated: None,
            },
            check,
        )
    }
    fn write_fixed(
        profile: &Path,
        project: Uuid,
        tree: FixedTree,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        require(
            !tree.files.is_empty()
                && tree.files.len() <= 16
                && tree
                    .files
                    .iter()
                    .all(|(_, b)| !b.is_empty() && b.len() <= 256 * 1024)
                && tree.files.iter().map(|(_, b)| b.len()).sum::<usize>() <= MAX_STATIC,
        )?;
        require(!tree.playbook.is_empty() && tree.playbook.len() <= 64 * 1024)?;
        check()?;
        let space = namespace::ManagedSeedNamespace::create(profile, project)?;
        let root = ProjectRoot {
            root: space.root()?,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            publication_boundary: true,
        };
        for directory in tree.directories {
            check()?;
            space.check()?;
            root.import_directory(directory)?;
        }
        for (name, bytes) in tree.files {
            write(&space, &root, name, bytes, &mut check)?;
        }
        // JSON is valid YAML; values are serialized, never YAML-interpolated.
        write(
            &space,
            &root,
            "playbook.yml",
            tree.playbook.as_bytes(),
            &mut check,
        )?;
        let claimed = if let Some((artifact, manifest)) = tree.captured {
            let a = copy(&space, &root, "release/candidate.zip", artifact, &mut check)?;
            let m = copy(
                &space,
                &root,
                "release/candidate-release.json",
                manifest,
                &mut check,
            )?;
            require(
                tree.validated
                    .as_ref()
                    .is_some_and(|(ah, mh)| ah == &a.0 && mh == &m.0),
            )?;
            Some(AgentInstallClaimedRelease {
                artifact_sha256: a.0,
                artifact_bytes: a.1,
                manifest_sha256: m.0,
                signature_verified: false,
            })
        } else {
            None
        };
        check()?;
        space.check()?;
        let revision = root.revision_for_publication()?;
        require(revision.bytes <= MAX_ARTIFACT + MAX_MANIFEST + MAX_STATIC as u64 + 64 * 1024)?;
        sync_directory(&root.root)?;
        space.check()?;
        check()?;
        Ok(Self {
            namespace: space,
            root,
            path: profile
                .join("automation")
                .join("projects")
                .join(project.to_string()),
            revision,
            claimed,
        })
    }
    /// Cheap retained namespace/name identity only. No project-byte hash walk.
    /// Safe for the short final guarded catalog callback; not a content grant.
    pub fn recheck_identity(&self) -> Result<()> {
        self.namespace.check()
    }
    pub fn recheck(&self) -> Result<()> {
        self.namespace.check()?;
        require(self.root.revision_for_publication()? == self.revision)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn revision(&self) -> &ProjectRevision {
        &self.revision
    }
    pub fn claimed_release(&self) -> Option<&AgentInstallClaimedRelease> {
        self.claimed.as_ref()
    }
}

// Internal mechanics only: both callers above select compile-owned fixed
// assets and closed generated values. No public arbitrary-tree constructor.
struct FixedTree {
    files: Vec<(&'static str, &'static [u8])>,
    directories: Vec<&'static str>,
    playbook: String,
    captured: Option<(Input, Input)>,
    validated: Option<(String, String)>,
}
struct Input {
    parent: ProjectRoot,
    parent_path: PathBuf,
    name: String,
    file: cap_std::fs::File,
    bytes: u64,
}
fn capture(path: &Path, max: u64) -> Result<Input> {
    require(
        path.is_absolute()
            && path
                .to_str()
                .is_some_and(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
            && !path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            }),
    )?;
    let parent_path = path.parent().ok_or_else(recovery)?.to_owned();
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(recovery)?
        .to_owned();
    let parent = ProjectRoot::open(&parent_path)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    namespace::check_local_directory(&parent.root)?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        require(windows::is_local_ntfs(parent.root.as_handle())?)?;
    }
    let file = parent.open_file(&name)?;
    let bytes = file.metadata()?.len();
    require(bytes > 0 && bytes <= max)?;
    parent.reopen_same_directory(&parent_path)?;
    Ok(Input {
        parent,
        parent_path,
        name,
        file,
        bytes,
    })
}
fn write(
    space: &namespace::ManagedSeedNamespace,
    root: &ProjectRoot,
    name: &str,
    bytes: &[u8],
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    check()?;
    space.check()?;
    let mut target = root.import_file(name, 0o600)?;
    for part in bytes.chunks(64 * 1024) {
        check()?;
        space.check()?;
        target.write_all(part)?;
    }
    root.sync_import_file(&target)?;
    check()?;
    space.check()
}
fn copy(
    space: &namespace::ManagedSeedNamespace,
    root: &ProjectRoot,
    name: &str,
    mut input: Input,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<(String, u64)> {
    check()?;
    space.check()?;
    let mut target = root.import_file(name, 0o600)?;
    let mut count = 0u64;
    let mut h = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        check()?;
        space.check()?;
        let n = input.file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count = count.checked_add(n as u64).ok_or_else(recovery)?;
        require(count <= input.bytes)?;
        h.update(&buffer[..n]);
        target.write_all(&buffer[..n])?;
    }
    require(count == input.bytes && input.file.metadata()?.len() == input.bytes)?;
    // Require the selected NAME still points to the SAME file, then hash the
    // held source again. Same-length edits cannot silently replace review bytes.
    let named = input.parent.open_file(&input.name)?;
    #[cfg(windows)]
    require(crate::project_identity_windows::same(
        &named.try_clone()?.into_std(),
        &input.file.try_clone()?.into_std(),
    )?)?;
    #[cfg(unix)]
    require(
        named.metadata()?.dev() == input.file.metadata()?.dev()
            && named.metadata()?.ino() == input.file.metadata()?.ino(),
    )?;
    #[cfg(not(any(unix, windows)))]
    return Err(AutomationError::PublicationUnsupported(
        "unsupported selected release file identity",
    ));
    input.parent.reopen_same_directory(&input.parent_path)?;
    use std::io::{Seek, SeekFrom};
    input.file.seek(SeekFrom::Start(0))?;
    let first = hex::encode(h.finalize());
    let mut again = Sha256::new();
    let mut seen = 0u64;
    loop {
        check()?;
        let n = input.file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        seen += n as u64;
        require(seen <= input.bytes)?;
        again.update(&buffer[..n]);
    }
    require(seen == count && hex::encode(again.finalize()) == first)?;
    root.sync_import_file(&target)?;
    check()?;
    space.check()?;
    Ok((first, count))
}

// Compile-owned exact role and adapters. These include paths deliberately
// resolve to this product checkout, never to a caller-selected template.
fn static_files() -> [(&'static str, &'static [u8]); 16] {
    [
        (
            "install_linux.py",
            include_bytes!("../../../../../../agent/delivery/install_linux.py"),
        ),
        (
            "installed_runtime_policy.py",
            include_bytes!("../../../../../../agent/delivery/installed_runtime_policy.py"),
        ),
        (
            "release_platforms.py",
            include_bytes!("../../../../../../agent/delivery/release_platforms.py"),
        ),
        (
            "collector_provision.py",
            include_bytes!("../../../../../../agent/delivery/collector_provision.py"),
        ),
        (
            "collector_namespace_probe.py",
            include_bytes!("../../../../../../agent/delivery/collector_namespace_probe.py"),
        ),
        (
            "collector_disk_namespace_probe.py",
            include_bytes!("../../../../../../agent/delivery/collector_disk_namespace_probe.py"),
        ),
        (
            "stage.py",
            include_bytes!("../../../../../../agent/delivery/stage.py"),
        ),
        (
            "release_verifier.py",
            include_bytes!("../../../../../../agent/delivery/release_verifier.py"),
        ),
        (
            "notice_inventory.py",
            include_bytes!("../../../../../../agent/delivery/notice_inventory.py"),
        ),
        (
            "prototype_package.py",
            include_bytes!("../../../../../../agent/delivery/prototype_package.py"),
        ),
        (
            "package_wheel.py",
            include_bytes!("../../../../../../agent/delivery/package_wheel.py"),
        ),
        (
            "roles/consolecrypt_agent_install/README.md",
            include_bytes!(
                "../../../../../../agent/delivery/roles/consolecrypt_agent_install/README.md"
            ),
        ),
        (
            "roles/consolecrypt_agent_install/defaults/main.yml",
            include_bytes!(
                "../../../../../../agent/delivery/roles/consolecrypt_agent_install/defaults/main.yml"
            ),
        ),
        (
            "roles/consolecrypt_agent_install/tasks/main.yml",
            include_bytes!(
                "../../../../../../agent/delivery/roles/consolecrypt_agent_install/tasks/main.yml"
            ),
        ),
        (
            "roles/consolecrypt_agent_install/meta/main.yml",
            include_bytes!(
                "../../../../../../agent/delivery/roles/consolecrypt_agent_install/meta/main.yml"
            ),
        ),
        (
            "roles/consolecrypt_agent_install/files/consolecrypt-agent-installed.service",
            include_bytes!(
                "../../../../../../agent/delivery/roles/consolecrypt_agent_install/files/consolecrypt-agent-installed.service"
            ),
        ),
    ]
}

fn monitoring_files() -> [(&'static str, &'static [u8]); 8] {
    [
        (
            "monitoring-node-install.yml",
            include_bytes!("../../../../../../agent/delivery/monitoring-node-install.yml"),
        ),
        (
            "monitoring-node-restore.yml",
            include_bytes!("../../../../../../agent/delivery/monitoring-node-restore.yml"),
        ),
        (
            "monitoring_backup.py",
            include_bytes!("../../../../../../agent/delivery/monitoring_backup.py"),
        ),
        (
            "prometheus.service.j2",
            include_bytes!("../../../../../../agent/delivery/prometheus.service.j2"),
        ),
        (
            "rules.yml",
            include_bytes!("../../../../../../agent/delivery/rules.yml"),
        ),
        (
            "monitoring_plan.py",
            include_bytes!("../../../../../../agent/delivery/monitoring_plan.py"),
        ),
        (
            "MONITORING_NODE_RECOVERY.md",
            include_bytes!("../../../../../../agent/delivery/MONITORING_NODE_RECOVERY.md"),
        ),
        (
            "MONITORING_CONFIG_BACKUP.md",
            include_bytes!("../../../../../../agent/delivery/MONITORING_CONFIG_BACKUP.md"),
        ),
    ]
}

#[cfg(test)]
mod tests;
