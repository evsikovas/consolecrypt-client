use super::*;
use cc_storage_core::{NodeAssociationRecord, NodeCollectorService};
impl Ledger {
    pub(in super::super) async fn begin_service_revocation_original(&self,id:Uuid,
        expected:NodeCollectorService,intent:cc_storage_core::NodeCollectorServiceRevocation)->AppResult<()> {
        let end=self.deadline()?;let snapshot=self.snapshot_original().await?;
        let scope=self.scope.clone();let storage=self.storage_original()?;
        storage.write_node_association(move|tx|scope.with_io_current(|_|{
            consent_live(end)?;
            tx.node_association_begin_service_revocation(&snapshot,id,&expected,intent).map_err(AppError::from)?;
            consent_live(end)
        })).await.map_err(|_|unknown())?;self.current_original()
    }
    pub(in super::super) async fn complete_service_revocation_original(&self,id:Uuid,
        expected:cc_storage_core::NodeCollectorServiceRevocation,receipt:Vec<u8>)->AppResult<()> {
        let end=self.deadline()?;let snapshot=self.snapshot_original().await?;
        let scope=self.scope.clone();let storage=self.storage_original()?;
        storage.write_node_association(move|tx|scope.with_io_current(|_|{
            consent_live(end)?;
            tx.node_association_complete_service_revocation(&snapshot,id,&expected,receipt).map_err(AppError::from)?;
            consent_live(end)
        })).await.map_err(|_|unknown())?;self.current_original()
    }
    pub(in super::super) async fn begin_service_original(
        &self,
        expected: NodeAssociationRecord,
        intent: NodeCollectorService,
    ) -> AppResult<()> {
        let end = self.deadline()?;
        let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone();
        let storage = self.storage_original()?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(end)?;
                    tx.node_association_begin_collector_service(&snapshot, &expected, intent)
                        .map_err(AppError::from)?;
                    consent_live(end)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current_original()
    }
    pub(in super::super) async fn complete_service_original(
        &self,
        id: Uuid,
        expected: NodeCollectorService,
        claim: Vec<u8>,
        receipt: Vec<u8>,
    ) -> AppResult<()> {
        let end = self.deadline()?;
        let snapshot = self.snapshot_original().await?;
        let scope = self.scope.clone();
        let storage = self.storage_original()?;
        storage
            .write_node_association(move |tx| {
                scope.with_io_current(|_| {
                    consent_live(end)?;
                    tx.node_association_complete_collector_service(
                        &snapshot, id, &expected, claim, receipt,
                    )
                    .map_err(AppError::from)?;
                    consent_live(end)
                })
            })
            .await
            .map_err(|_| unknown())?;
        self.current_original()
    }
}
