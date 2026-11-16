//! Read-only native local selection facts. No authority DB, clock or repair.
use crate::*;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PrivateLayout {
    LegacyV1,
    PreparedV2,
    ActivationV3,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionState {
    Legacy1,
    Prepared1,
    ActivationPending,
    Selected2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GuardIdentity {
    pub dev: u64,
    pub ino: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalNodeInspection {
    pub format: u32,
    pub agent_id: AgentId,
    pub private_layout: PrivateLayout,
    pub required_reader_schema: u32,
    pub actual_selected_epoch: Option<u64>,
    pub selection_state: SelectionState,
    pub guard: GuardIdentity,
}
/// Existing native shared lock plus facts, not an authority capability.
/// No public constructor, serialization, clone, seed or descriptor getter.
/// ```compile_fail
/// use cc_agent_identity_policy::NodeInspectionGuard;
/// let guard:NodeInspectionGuard=serde_json::from_str("{}").unwrap();
/// ```
pub struct NodeInspectionGuard {
    #[cfg(target_os = "linux")]
    guard: crate::node_prepare::Guard,
    facts: LocalNodeInspection,
}
impl NodeInspectionGuard {
    pub fn facts(&self) -> &LocalNodeInspection {
        &self.facts
    }
    pub fn recheck_guard_identity(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            let directory =
                std::fs::symlink_metadata(self.guard.directory()).map_err(|_| Error::Storage)?;
            if !directory.is_dir() || directory.permissions().mode() & 0o777 != 0o700 {
                return Err(Error::Storage);
            }
            let (dev, ino) = crate::node_prepare::guard_identity(&self.guard)?;
            if self.facts.guard != (GuardIdentity { dev, ino }) {
                return Err(Error::Storage);
            }
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(Error::CapabilityDenied)
        }
    }
}
pub fn inspect_node_state(
    directory: &Path,
    expected_agent: AgentId,
) -> Result<NodeInspectionGuard> {
    #[cfg(target_os = "linux")]
    {
        linux::inspect(directory, expected_agent)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (directory, expected_agent);
        Err(Error::CapabilityDenied)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    const NAMES: [&str; 7] = [
        ".node-keys.lock",
        "node-identity.key",
        "node-rotation.intent.v2",
        "node-rotation.pending.v2",
        "node-rotation.state.v2",
        "node-activation.intent.v3",
        "node-activation.state.v3",
    ];
    pub(super) fn inspect(directory: &Path, expected: AgentId) -> Result<NodeInspectionGuard> {
        let guard = crate::node_prepare::inspection_read_lock(directory)?;
        let owner = fs::symlink_metadata(guard.directory())
            .map_err(|_| Error::Storage)?
            .uid();
        let mut names = BTreeSet::new();
        for item in fs::read_dir(guard.directory()).map_err(|_| Error::Storage)? {
            let item = item.map_err(|_| Error::Storage)?;
            let name = item.file_name().into_string().map_err(|_| Error::Storage)?;
            if !NAMES.contains(&name.as_str()) {
                return Err(Error::UncertainCommit);
            }
            let metadata = fs::symlink_metadata(item.path()).map_err(|_| Error::Storage)?;
            if !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o777 != 0o600
                || metadata.uid() != owner
            {
                return Err(Error::Storage);
            }
            names.insert(name);
        }
        if !names.contains(NAMES[0]) || !names.contains(NAMES[1]) {
            return Err(Error::Storage);
        }
        let old = crate::provision::load_existing_under_guard(&guard, expected)?;
        let prepared = crate::node_prepare::check(&guard, Some(&old))?;
        let phase = crate::node_activation::inspection_phase(&guard, &old)?;
        let (layout, reader, epoch, state, count) = match (prepared, phase) {
            (None, None) => (
                PrivateLayout::LegacyV1,
                1,
                Some(1),
                SelectionState::Legacy1,
                2,
            ),
            (Some(_), None) => (
                PrivateLayout::PreparedV2,
                1,
                Some(1),
                SelectionState::Prepared1,
                5,
            ),
            (Some(_), Some(false)) => (
                PrivateLayout::ActivationV3,
                3,
                None,
                SelectionState::ActivationPending,
                6,
            ),
            (Some(_), Some(true)) => (
                PrivateLayout::ActivationV3,
                3,
                Some(2),
                SelectionState::Selected2,
                7,
            ),
            _ => return Err(Error::UncertainCommit),
        };
        if names.len() != count {
            return Err(Error::UncertainCommit);
        }
        let (dev, ino) = crate::node_prepare::guard_identity(&guard)?;
        let observation = NodeInspectionGuard {
            guard,
            facts: LocalNodeInspection {
                format: 1,
                agent_id: expected,
                private_layout: layout,
                required_reader_schema: reader,
                actual_selected_epoch: epoch,
                selection_state: state,
                guard: GuardIdentity { dev, ino },
            },
        };
        observation.recheck_guard_identity()?;
        Ok(observation)
    }
}
