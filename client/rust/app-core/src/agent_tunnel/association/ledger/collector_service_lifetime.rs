//! SAME original scope and existing closed SQL snapshot/CAS, before network byte.
use super::*;
use cc_storage_core::{NodeCollectorService, NodeCollectorServiceCredentialSet};
impl Ledger {
    pub(in super::super) async fn begin_lifetime_set_original(&self, original: &Arc<OriginalScope>,
        id: Uuid, expected: NodeCollectorService, intent: NodeCollectorServiceCredentialSet) -> AppResult<NodeCollectorService> {
        if !Arc::ptr_eq(original, &self.scope) { return Err(unavailable()); }
        let end = self.deadline()?; let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone(); let storage = self.storage_original()?;
        let previous = expected.clone(); let staged = intent.clone();
        storage.write_node_association(move |tx| scope.with_io_current(|_| {
            consent_live(end)?;
            tx.node_association_begin_service_credential_set(&snapshot, id, &previous, staged).map_err(AppError::from)?;
            consent_live(end)
        })).await.map_err(|_| unknown())?;
        self.current_original()?;
        let mut next = expected; next.credential_set = Some(intent);
        let readback = self.snapshot_original().await?;
        if readback.records().iter().find(|r| r.public.association_id == id).and_then(|r|r.collector_service.as_ref()) != Some(&next) {
            return Err(unknown());
        }
        self.current_original()?;
        Ok(next)
    }
    pub(in super::super) async fn complete_lifetime_set_original(&self, original: &Arc<OriginalScope>,
        id: Uuid, expected: NodeCollectorService, receipt: Vec<u8>) -> AppResult<NodeCollectorService> {
        if !Arc::ptr_eq(original, &self.scope) { return Err(unavailable()); }
        let end = self.deadline()?; let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone(); let storage = self.storage_original()?;
        let previous = expected.clone(); let encoded = receipt.clone();
        storage.write_node_association(move |tx| scope.with_io_current(|_| {
            consent_live(end)?;
            tx.node_association_complete_service_credential_set(&snapshot, id, &previous, encoded).map_err(AppError::from)?;
            consent_live(end)
        })).await.map_err(|_| unknown())?;
        self.current_original()?;
        let mut next = expected;
        next.credential_set.as_mut().ok_or_else(unknown)?.signed_admission_receipt_json = Some(receipt);
        let readback = self.snapshot_original().await?;
        if readback.records().iter().find(|r|r.public.association_id == id).and_then(|r|r.collector_service.as_ref()) != Some(&next) {
            return Err(unknown());
        }
        self.current_original()?;
        Ok(next)
    }
}
