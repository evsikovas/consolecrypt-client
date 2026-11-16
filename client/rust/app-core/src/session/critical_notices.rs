//! Original-owner, bounded metadata projection on the existing app event bus.
//! No runtime command, raw error, host, path, output or credential is retained.

use crate::dto::{
    AppEvent, CriticalBackupStateDto, CriticalDeviceStateDto, CriticalEditStateDto,
    CriticalRuntimeChangeDto, CriticalSecurityStateDto, CriticalSyncStateDto, CriticalTeamStateDto,
    CriticalTerminalStateDto, CriticalTransferStateDto, CriticalTunnelStateDto, SyncPhaseDto,
    SyncStatusDto, SyncStopReasonDto, TerminalStatusDto,
};
use cc_models::ObjectId;
use cc_storage_core::ProfileId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use tokio::sync::broadcast;
use uuid::Uuid;

const MAX_REVISION: u64 = 9_007_199_254_740_991;
const MAX_RECORDS: usize = 2048;
const MAX_SNAPSHOT_RECORDS: usize = 64;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    domain: u8,
    id: String,
}

#[derive(Clone, Copy)]
struct Record {
    change: CriticalRuntimeChangeDto,
    // Zero is an unreported healthy baseline, never a startup notice.
    revision: u64,
}

#[derive(Default)]
struct State {
    closed: bool,
    revision: u64,
    gap_sent: bool,
    records: HashMap<Key, Record>,
    sync_generation: u64,
    sync_active: bool,
}

pub(crate) struct RuntimeNoticeEmitter {
    profile_id: ProfileId,
    scope_id: Uuid,
    events: broadcast::Sender<AppEvent>,
    max_records: usize,
    state: Mutex<State>,
}

/// One installed engine only. A replaced/disabled engine cannot publish later
/// status into the same unlocked owner's current synchronization source.
#[derive(Clone)]
pub(crate) struct SyncNoticeSource {
    owner: Arc<RuntimeNoticeEmitter>,
    generation: Option<u64>,
}

fn canonical_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

fn domain(change: CriticalRuntimeChangeDto) -> u8 {
    match change {
        CriticalRuntimeChangeDto::Sync { .. } => 0,
        CriticalRuntimeChangeDto::Tunnel { .. } => 1,
        CriticalRuntimeChangeDto::Terminal { .. } => 2,
        CriticalRuntimeChangeDto::Security { .. } => 3,
        CriticalRuntimeChangeDto::Device { .. } => 4,
        CriticalRuntimeChangeDto::Backup { .. } => 5,
        CriticalRuntimeChangeDto::Team { .. } => 6,
        CriticalRuntimeChangeDto::Edit { .. } => 7,
        CriticalRuntimeChangeDto::Transfer { .. } => 8,
    }
}

fn starts_notice(change: CriticalRuntimeChangeDto) -> bool {
    !matches!(
        change,
        CriticalRuntimeChangeDto::Sync {
            state: CriticalSyncStateDto::Idle
        } | CriticalRuntimeChangeDto::Tunnel {
            state: CriticalTunnelStateDto::Running | CriticalTunnelStateDto::Stopped
        } | CriticalRuntimeChangeDto::Terminal {
            state: CriticalTerminalStateDto::Connected
        } | CriticalRuntimeChangeDto::Backup {
            state: CriticalBackupStateDto::Completed
        } | CriticalRuntimeChangeDto::Edit {
            state: CriticalEditStateDto::Synced
        } | CriticalRuntimeChangeDto::Transfer {
            state: CriticalTransferStateDto::Completed
        }
    )
}

/// One exact synchronous boundary for engine identity and metadata source.
/// Neither callback nor callers may await while this guard is held. All
/// transitions acquire engine first, then the emitter's metadata mutex.
fn engine_transition<T, R>(
    slot: &RwLock<Option<T>>,
    update: impl FnOnce(&mut Option<T>) -> R,
) -> R {
    let mut slot = slot
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    update(&mut slot)
}

impl RuntimeNoticeEmitter {
    pub(crate) fn replace_engine<T>(
        self: &Arc<Self>,
        slot: &RwLock<Option<T>>,
        engine: T,
    ) -> SyncNoticeSource {
        engine_transition(slot, |current| {
            *current = Some(engine);
            self.sync_source()
        })
    }

    pub(crate) fn take_engine<T>(&self, slot: &RwLock<Option<T>>) -> Option<T> {
        engine_transition(slot, |current| {
            self.stop_sync_source();
            current.take()
        })
    }

    /// The original producer observed upstream loss, with no raw reason or
    /// replacement owner. A gap is metadata incompleteness, never recovery.
    pub(crate) fn source_gap(&self) {
        let mut state = self.lock();
        self.gap(&mut state);
    }
    pub(crate) fn new(profile_id: ProfileId, events: broadcast::Sender<AppEvent>) -> Arc<Self> {
        Self::create(profile_id, events, MAX_RECORDS)
    }

    fn create(
        profile_id: ProfileId,
        events: broadcast::Sender<AppEvent>,
        max_records: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            profile_id,
            scope_id: Uuid::new_v4(),
            events,
            max_records,
            state: Mutex::new(State {
                closed: !canonical_id(&profile_id.to_string()),
                ..State::default()
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn scope_id(&self) -> Option<String> {
        (!self.lock().closed).then(|| self.scope_id.to_string())
    }

    /// Read-only recovery of metadata emitted before the UI learned this
    /// unlock's nonce. Nonce and records are captured under the SAME lock.
    /// Re-reading does not allocate revisions or recreate dismissed notices.
    pub(crate) fn profile_snapshot(&self) -> Option<serde_json::Value> {
        let state = self.lock();
        if state.closed {
            return None;
        }
        let mut records: Vec<_> = state
            .records
            .iter()
            .filter(|(_, record)| record.revision != 0)
            .collect();
        records.sort_unstable_by(|a, b| b.1.revision.cmp(&a.1.revision));
        let incomplete = state.gap_sent || records.len() > MAX_SNAPSHOT_RECORDS;
        let records: Vec<_> = records
            .into_iter()
            .take(MAX_SNAPSHOT_RECORDS)
            .map(|(key, record)| {
                serde_json::json!({
                    "type": "critical_state_changed",
                    "profile_id": self.profile_id.to_string(),
                    "scope_id": self.scope_id.to_string(),
                    "record_id": key.id,
                    "revision": record.revision,
                    "change": record.change,
                })
            })
            .collect();
        Some(serde_json::json!({
            "version": 1,
            "profile_id": self.profile_id.to_string(),
            "scope_id": self.scope_id.to_string(),
            "records": records,
            "incomplete": incomplete,
        }))
    }

    /// Serialized with publication. After this returns no old-owner event can
    /// be sent, even if an aborted async forwarder was already running.
    pub(crate) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.sync_active = false;
        state.records.clear();
    }

    pub(crate) fn stop_sync_source(&self) {
        self.lock().sync_active = false;
    }

    pub(crate) fn sync_source(self: &Arc<Self>) -> SyncNoticeSource {
        let mut state = self.lock();
        let generation = if state.closed {
            None
        } else if let Some(next) = state.sync_generation.checked_add(1) {
            state.sync_generation = next;
            state.sync_active = true;
            Some(next)
        } else {
            state.sync_active = false;
            self.gap(&mut state);
            None
        };
        SyncNoticeSource {
            owner: self.clone(),
            generation,
        }
    }

    fn gap(&self, state: &mut State) {
        if state.closed || state.gap_sent {
            return;
        }
        state.gap_sent = true;
        let _ = self.events.send(AppEvent::CriticalNoticeGap {
            profile_id: self.profile_id.to_string(),
            scope_id: self.scope_id.to_string(),
        });
    }

    fn publish(
        &self,
        record_id: String,
        change: CriticalRuntimeChangeDto,
        sync_generation: Option<u64>,
    ) {
        let mut state = self.lock();
        if state.closed || !canonical_id(&record_id) {
            return;
        }
        if (domain(change) == 0 || sync_generation.is_some())
            && (!state.sync_active || sync_generation != Some(state.sync_generation))
        {
            return;
        }
        let key = Key {
            domain: domain(change),
            id: record_id.clone(),
        };
        let previous = state.records.get(&key).map(|record| record.change);
        if previous == Some(change) {
            return;
        }
        if previous.is_none() && state.records.len() >= self.max_records {
            self.gap(&mut state);
            return;
        }
        if previous.is_none() && !starts_notice(change) {
            state.records.insert(
                key,
                Record {
                    change,
                    revision: 0,
                },
            );
            return;
        }
        let Some(revision) = state.revision.checked_add(1).filter(|n| *n <= MAX_REVISION) else {
            self.gap(&mut state);
            return;
        };
        state.revision = revision;
        state.records.insert(key, Record { change, revision });
        // Keep the mutex through the nonblocking send: close and concurrent
        // changes cannot reorder publication after their state update.
        let _ = self.events.send(AppEvent::CriticalStateChanged {
            profile_id: self.profile_id.to_string(),
            scope_id: self.scope_id.to_string(),
            record_id,
            revision,
            change,
        });
    }

    pub(crate) fn terminal(&self, id: cc_terminal_core::TerminalId, status: &TerminalStatusDto) {
        let state = match status {
            TerminalStatusDto::Connecting => return,
            TerminalStatusDto::Connected => CriticalTerminalStateDto::Connected,
            TerminalStatusDto::Closed { .. } => CriticalTerminalStateDto::Closed,
            TerminalStatusDto::Failed { .. } => CriticalTerminalStateDto::Failed,
        };
        self.publish(
            id.to_string(),
            CriticalRuntimeChangeDto::Terminal { state },
            None,
        );
    }

    pub(crate) fn tunnel(&self, id: ObjectId, state: CriticalTunnelStateDto) {
        self.publish(
            id.to_string(),
            CriticalRuntimeChangeDto::Tunnel { state },
            None,
        );
    }

    /// Factual observations from the original unlock's existing object store.
    /// No claim about current engine recovery or latest-profile attribution.
    pub(crate) fn security(&self, id: impl ToString, state: CriticalSecurityStateDto) {
        self.publish(
            id.to_string(),
            CriticalRuntimeChangeDto::Security { state },
            None,
        );
    }
    pub(crate) fn device(&self, id: impl ToString, state: CriticalDeviceStateDto) {
        self.publish(
            id.to_string(),
            CriticalRuntimeChangeDto::Device { state },
            None,
        );
    }
    pub(crate) fn backup(&self, state: CriticalBackupStateDto) {
        self.publish(
            self.profile_id.to_string(),
            CriticalRuntimeChangeDto::Backup { state },
            None,
        );
    }
    pub(crate) fn team(&self, id: impl ToString, state: CriticalTeamStateDto) {
        self.publish(
            id.to_string(),
            CriticalRuntimeChangeDto::Team { state },
            None,
        );
    }
    pub(crate) fn edit(&self, id: impl ToString, state: CriticalEditStateDto) {
        self.publish(
            id.to_string(),
            CriticalRuntimeChangeDto::Edit { state },
            None,
        );
    }
    pub(crate) fn edit_leftovers(&self) {
        self.publish(
            self.profile_id.to_string(),
            CriticalRuntimeChangeDto::Edit {
                state: CriticalEditStateDto::RecoveryAvailable,
            },
            None,
        );
    }
    pub(crate) fn transfer(&self, native_id: Uuid, state: CriticalTransferStateDto) {
        self.publish(
            native_id.to_string(),
            CriticalRuntimeChangeDto::Transfer { state },
            None,
        );
    }
}

impl SyncNoticeSource {
    pub(crate) fn device(&self, id: impl ToString, state: CriticalDeviceStateDto) {
        let Some(generation) = self.generation else {
            return;
        };
        self.owner.publish(
            id.to_string(),
            CriticalRuntimeChangeDto::Device { state },
            Some(generation),
        );
    }
    pub(crate) fn recovery_changed(&self) {
        let Some(generation) = self.generation else {
            return;
        };
        self.owner.publish(
            self.owner.profile_id.to_string(),
            CriticalRuntimeChangeDto::Device {
                state: CriticalDeviceStateDto::RecoveryChanged,
            },
            Some(generation),
        );
    }
    pub(crate) fn source_gap(&self) {
        let mut state = self.owner.lock();
        if state.sync_active && self.generation == Some(state.sync_generation) {
            self.owner.gap(&mut state);
        }
    }
    pub(crate) fn status(&self, status: &SyncStatusDto) {
        let state = match status.phase {
            SyncPhaseDto::Stopped => match status.stop_reason {
                Some(SyncStopReasonDto::Shutdown) => return,
                Some(SyncStopReasonDto::DeviceRevoked) => CriticalSyncStateDto::DeviceRevoked,
                Some(SyncStopReasonDto::ReauthRequired) => CriticalSyncStateDto::ReauthRequired,
                Some(SyncStopReasonDto::AccountDisabled) => CriticalSyncStateDto::AccountDisabled,
                Some(SyncStopReasonDto::NotTrusted) => CriticalSyncStateDto::NotTrusted,
                Some(SyncStopReasonDto::VaultGone) => CriticalSyncStateDto::VaultGone,
                Some(SyncStopReasonDto::UpgradeRequired) => CriticalSyncStateDto::UpgradeRequired,
                None => CriticalSyncStateDto::Stopped,
            },
            _ if status.failed > 0 => CriticalSyncStateDto::Error,
            SyncPhaseDto::Offline => CriticalSyncStateDto::Offline,
            SyncPhaseDto::Error => CriticalSyncStateDto::Error,
            SyncPhaseDto::Idle => CriticalSyncStateDto::Idle,
            _ => return,
        };
        self.owner.publish(
            self.owner.profile_id.to_string(),
            CriticalRuntimeChangeDto::Sync { state },
            self.generation,
        );
    }
}

#[cfg(test)]
#[path = "critical_notices_tests.rs"]
mod tests;
