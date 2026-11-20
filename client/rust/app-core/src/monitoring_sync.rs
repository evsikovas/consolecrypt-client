//! Private MON07 compatibility diagnostics, not an upload/admission capability.
//! The local 1.2.0 candidate declares a preservation floor. Compatible roster
//! snapshots alone cannot enable synchronization; server opt-in is separate.
use crate::{AppCore, AppResult};
use cc_protocol::DeviceId;
use cc_protocol::devices::{DeviceInfo, DeviceStatus};
use cc_storage_core::ProfileKind;
use semver::Version;
use serde::Serialize;
use std::{collections::BTreeSet, time::Duration};

const MAX_DEVICES: usize = 256;
const MAX_VERSION_BYTES: usize = 64;
// Release-owned metadata only; never a bridge argument, user flag, guessed
// preview version or the unrelated database 0.9.2 compatibility floor.
const RELEASED_PRESERVATION_FLOOR: Option<&str> =
    crate::settings_preservation::RELEASED_SETTINGS_PRESERVATION_FLOOR;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MonitoringSyncState {
    LocalOnly,
    FloorUndeclared,
    RosterUnavailable,
    RosterInvalid,
    DevicesUnsupported,
    ServerAdmissionUnverified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DeviceCompatibilityReason {
    UnknownVersion,
    InvalidVersion,
    PrereleaseVersion,
    OlderVersion,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceCompatibilityBlocker {
    pub device_id: DeviceId,
    pub reason: DeviceCompatibilityReason,
}
/// Diagnostic only. No serde input, transferable grant, persisted ready bit or
/// boolean accepted from a caller. No remote names/raw version text are echoed.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MonitoringSyncReadiness {
    pub state: MonitoringSyncState,
    pub checked_active_devices: usize,
    pub blocked_devices: Vec<DeviceCompatibilityBlocker>,
}
impl MonitoringSyncReadiness {
    fn state(state: MonitoringSyncState) -> Self {
        Self {
            state,
            checked_active_devices: 0,
            blocked_devices: Vec::new(),
        }
    }
}
struct ReleasedFloor(Version);
impl ReleasedFloor {
    fn parse(text: &str) -> Option<Self> {
        let version = stable_version(text).ok()?;
        version.build.is_empty().then_some(Self(version))
    }
}
fn stable_version(text: &str) -> Result<Version, DeviceCompatibilityReason> {
    if text.len() > MAX_VERSION_BYTES || text.trim() != text || text.is_empty() {
        return Err(DeviceCompatibilityReason::InvalidVersion);
    }
    let version = Version::parse(text).map_err(|_| DeviceCompatibilityReason::InvalidVersion)?;
    if !version.pre.is_empty() {
        return Err(DeviceCompatibilityReason::PrereleaseVersion);
    }
    Ok(version)
}
fn evaluate(
    devices: &[DeviceInfo],
    own: DeviceId,
    floor: &ReleasedFloor,
) -> MonitoringSyncReadiness {
    let mut ids = BTreeSet::new();
    if devices.is_empty()
        || devices.len() > MAX_DEVICES
        || devices
            .iter()
            .any(|device| device.device_id == DeviceId::NIL || !ids.insert(device.device_id))
        || devices.iter().filter(|device| device.is_current).count() != 1
        || !devices.iter().any(|device| {
            device.device_id == own && device.is_current && device.status == DeviceStatus::Active
        })
    {
        return MonitoringSyncReadiness::state(MonitoringSyncState::RosterInvalid);
    }
    let mut result = MonitoringSyncReadiness::state(MonitoringSyncState::ServerAdmissionUnverified);
    // Only actual Revoked is excluded. Unknown/missing/old last_seen is NEVER
    // treated as inactive/revoked; dormant active clients can reconnect later.
    for device in devices
        .iter()
        .filter(|device| device.status == DeviceStatus::Active)
    {
        result.checked_active_devices += 1;
        let reason = match device.client_version.as_deref() {
            None => Some(DeviceCompatibilityReason::UnknownVersion),
            Some(text) => match stable_version(text) {
                Err(reason) => Some(reason),
                Ok(version) if version < floor.0 => Some(DeviceCompatibilityReason::OlderVersion),
                Ok(_) => None,
            },
        };
        if let Some(reason) = reason {
            result.blocked_devices.push(DeviceCompatibilityBlocker {
                device_id: device.device_id,
                reason,
            });
        }
    }
    if !result.blocked_devices.is_empty() {
        result.state = MonitoringSyncState::DevicesUnsupported;
    }
    // Even an all-compatible fresh roster cannot prevent a later old-device
    // enrollment/login/downgrade. Current pinned protocol has no verified
    // scoped admission opt-in; NEVER manufacture a Ready state here.
    result
}
impl AppCore {
    #[allow(dead_code)] // private prerequisite; no public bridge or enable gate yet.
    pub(crate) async fn monitoring_settings_sync_readiness(
        &self,
    ) -> AppResult<MonitoringSyncReadiness> {
        self.monitoring_sync_readiness_with_floor(
            RELEASED_PRESERVATION_FLOOR.and_then(ReleasedFloor::parse),
        )
        .await
    }
    async fn monitoring_sync_readiness_with_floor(
        &self,
        floor: Option<ReleasedFloor>,
    ) -> AppResult<MonitoringSyncReadiness> {
        let (owner, u) = self.unlocked().await?;
        let weak = std::sync::Arc::downgrade(&u);
        let (kind, own) = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            (owner.kind().await?, owner.device_id())
        };
        drop(u);
        if kind == ProfileKind::Local {
            return Ok(MonitoringSyncReadiness::state(
                MonitoringSyncState::LocalOnly,
            ));
        }
        let Some(floor) = floor else {
            return Ok(MonitoringSyncReadiness::state(
                MonitoringSyncState::FloorUndeclared,
            ));
        };
        // Same authenticated ApiClient/list_devices as db_sync_readiness. No
        // second transport, caller URL, plaintext roster cache or weak error-to-
        // empty conversion. A hard ceiling bounds the observation's duration.
        let response =
            tokio::time::timeout(Duration::from_secs(10), owner.api()?.list_devices()).await;
        let u = weak.upgrade().ok_or(crate::AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if owner.device_id() != own {
            return Err(crate::AppError::invalid(
                "monitoring_sync",
                "context_changed",
            ));
        }
        Ok(match response {
            Ok(Ok(response)) => evaluate(&response.devices, own, &floor),
            _ => MonitoringSyncReadiness::state(MonitoringSyncState::RosterUnavailable),
        })
    }
}
#[cfg(test)]
mod tests;

// Private staged prerequisite, no bridge/readiness permission or writer bypass.
#[allow(dead_code)]
mod admission;
pub(crate) use admission::diagnostics::catalog_snapshot;

pub use admission::MonitoringSettingsConsent;

pub(crate) use admission::{PanelWriteAdmission, PanelWritePolicy, SettingsWriteAdmission};
