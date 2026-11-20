//! Exact pull data witness, NOT a profile/writer admission capability.
use super::*;
use crate::api::{MonitoringAccountPolicy, MonitoringPolicyState};
use cc_protocol::UserId;
#[derive(Debug)]
pub struct MonitoringCheckpointApplied {
    account: UserId,
    vault: VaultId,
    epoch: uuid::Uuid,
    sequence: i64,
}
impl MonitoringCheckpointApplied {
    pub fn account_id(&self) -> UserId {
        self.account
    }
    pub fn vault_id(&self) -> VaultId {
        self.vault
    }
    pub fn epoch(&self) -> uuid::Uuid {
        self.epoch
    }
    pub fn sequence(&self) -> i64 {
        self.sequence
    }
}
impl SyncEngine {
    /// Pull-only checkpoint recapture through the existing cycle mutex/codec/
    /// atomic store. No push/conflict resolution/recovery upload/auto retry.
    ///32total pages/32MiB serialized typed pages/10s, shutdown cancels; no grant.
    pub async fn pull_monitoring_checkpoint(
        &self,
        policy: &MonitoringAccountPolicy,
    ) -> Result<MonitoringCheckpointApplied, SyncError> {
        let mut shutdown = self.inner.shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(SyncError::Stopped(StopReason::Shutdown));
        }
        tokio::select! {
            biased;
            _=shutdown.changed()=>Err(SyncError::Stopped(StopReason::Shutdown)),
            result=tokio::time::timeout(Duration::from_secs(10),self.pull_monitoring_checkpoint_inner(policy))=>{
                result.map_err(|_|SyncError::MalformedResponse("compatibility pull deadline exceeded".into()))?
            }
        }
    }
    async fn pull_monitoring_checkpoint_inner(
        &self,
        policy: &MonitoringAccountPolicy,
    ) -> Result<MonitoringCheckpointApplied, SyncError> {
        if policy.state() != MonitoringPolicyState::Enabled
            || !policy.matches_origin(&self.inner.api)
            || policy.checkpoint().vault_id() != self.vault_id()
        {
            return Err(checkpoint_changed());
        }
        let _cycle = self.inner.cycle.lock().await;
        if let Some(reason) = self.stop_reason() {
            return Err(SyncError::Stopped(reason));
        }
        let checkpoint = policy.checkpoint();
        let initial = self.cursor().await?;
        if initial.recovery.is_some()
            || initial
                .epoch
                .is_some_and(|epoch| epoch != checkpoint.epoch())
        {
            return Err(checkpoint_changed());
        }
        self.require_clean_checkpoint_outbox().await?;
        let mut report = SyncReport::default();
        let mut pages = CheckpointPages::strict(checkpoint.epoch());
        if !initial.snapshot_complete {
            self.snapshot_budget(&mut report, &mut pages).await?;
        }
        self.pull_budget(&mut report, &mut pages).await?;
        let cursor = self.cursor().await?;
        if let Some(reason) = self.stop_reason() {
            return Err(SyncError::Stopped(reason));
        }
        self.require_clean_checkpoint_outbox().await?;
        if !cursor.snapshot_complete
            || cursor.recovery.is_some()
            || cursor.epoch != Some(checkpoint.epoch())
            || cursor.last_sequence < checkpoint.sequence()
            || report.stashed != 0
        {
            return Err(checkpoint_changed());
        }
        Ok(MonitoringCheckpointApplied {
            account: policy.account_id(),
            vault: self.vault_id(),
            epoch: checkpoint.epoch(),
            sequence: cursor.last_sequence,
        })
    }
    async fn require_clean_checkpoint_outbox(&self) -> Result<(), SyncError> {
        let vault = self.vault_id();
        let clean = self
            .inner
            .store
            .storage()
            .read(move |tx| -> Result<bool, StorageError> {
                let counts = tx.outbox_counts(vault)?;
                Ok(counts.total() == 0 && tx.conflicted_objects(vault)?.is_empty())
            })
            .await?;
        if !clean {
            return Err(SyncError::MalformedResponse(
                "compatibility pull requires explicit pending-edit reconciliation".into(),
            ));
        }
        Ok(())
    }
}
