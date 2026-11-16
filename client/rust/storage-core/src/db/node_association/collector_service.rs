//! Public signed intent in the SAME LocalOnly association row. These bytes
//! neither admit an Agent action nor constitute a verified service ACK.
use super::*;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCollectorService {
    pub service_id: Uuid,
    pub delegation_sha256: String,
    pub signed_delegation_json: Vec<u8>,
    pub manifest_json: Vec<u8>,
    pub owner_pin_json: Vec<u8>,
    pub expected_actor_revision: u64,
    pub signed_claim_json: Option<Vec<u8>>,
    pub signed_receipt_json: Option<Vec<u8>>,
    pub configuration_recorded: bool,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub credential_set: Option<NodeCollectorServiceCredentialSet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation: Option<NodeCollectorServiceRevocation>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCollectorServiceRevocation {
    pub signed_revocation_json: Vec<u8>,
    pub expected_actor_revision: u64,
    pub signed_receipt_json: Option<Vec<u8>>,
}
impl std::fmt::Debug for NodeCollectorServiceRevocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NodeCollectorServiceRevocation([redacted])")
    }
}
impl std::fmt::Debug for NodeCollectorService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NodeCollectorService([redacted])")
    }
}
impl NodeCollectorService {
    pub(super) fn validate(&self) -> Result<()> {
        if let Some(set)=&self.credential_set {set.validate()?;}
        if self.revocation.as_ref().is_some_and(|r| {
            r.expected_actor_revision == 0
                || r.signed_revocation_json.is_empty()
                || r.signed_revocation_json.len() > 16384
                || r.signed_receipt_json
                    .as_ref()
                    .is_some_and(|v| v.is_empty() || v.len() > 16384)
        }) {
            return Err(invalid());
        }
        if self.service_id.is_nil()
            || !hash(&self.delegation_sha256)
            || self.expected_actor_revision == 0
            || self.signed_delegation_json.is_empty()
            || self.signed_delegation_json.len() > 16384
            || self.manifest_json.is_empty()
            || self.manifest_json.len() > 8192
            || self.owner_pin_json.is_empty()
            || self.owner_pin_json.len() > 4096
            || self.configuration_recorded != self.signed_claim_json.is_some()
            || self.signed_claim_json.is_some() != self.signed_receipt_json.is_some()
            || [&self.signed_claim_json, &self.signed_receipt_json]
                .iter()
                .any(|raw| {
                    raw.as_ref()
                        .is_some_and(|v| v.is_empty() || v.len() > 16384)
                })
        {
            return Err(invalid());
        }
        Ok(())
    }
}
impl Tx<'_> {
    pub fn node_association_begin_service_revocation(
        &self,
        snapshot: &NodeAssociationSnapshot,
        id: Uuid,
        expected: &NodeCollectorService,
        intent: NodeCollectorServiceRevocation,
    ) -> Result<()> {
        let mut rows = snapshot.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.public.association_id == id)
            .ok_or_else(invalid)?;
        if row.phase != NodeAssociationPhase::Associated
            || row.revocation.is_some()
            || row.collector_service.as_ref() != Some(expected)
            || expected.revocation.is_some()
            || intent.signed_receipt_json.is_some()
        {
            return Err(invalid());
        }
        let mut next = expected.clone();
        next.revocation = Some(intent);
        next.validate()?;
        row.collector_service = Some(next);
        self.node_association_put(snapshot, rows)
    }
    pub fn node_association_complete_service_revocation(
        &self,
        snapshot: &NodeAssociationSnapshot,
        id: Uuid,
        expected: &NodeCollectorServiceRevocation,
        receipt: Vec<u8>,
    ) -> Result<()> {
        let mut rows = snapshot.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.public.association_id == id)
            .ok_or_else(invalid)?;
        let service = row.collector_service.as_mut().ok_or_else(invalid)?;
        if row.phase != NodeAssociationPhase::Associated
            || row.revocation.is_some()
            || service.revocation.as_ref() != Some(expected)
            || expected.signed_receipt_json.is_some()
        {
            return Err(invalid());
        }
        service
            .revocation
            .as_mut()
            .ok_or_else(invalid)?
            .signed_receipt_json = Some(receipt);
        service.validate()?;
        self.node_association_put(snapshot, rows)
    }
    /// Durable Unknown BEFORE the first possibly mutating remote byte. An old
    /// Unknown/recorded intent cannot be overwritten or silently retried.
    pub fn node_association_begin_collector_service(
        &self,
        snapshot: &NodeAssociationSnapshot,
        expected: &NodeAssociationRecord,
        intent: NodeCollectorService,
    ) -> Result<()> {
        intent.validate()?;
        let mut rows = snapshot.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.public.association_id == expected.public.association_id)
            .ok_or_else(invalid)?;
        if &*row != expected
            || row.phase != NodeAssociationPhase::Associated
            || row.public.certificate.is_none()
            || row.revocation.is_some()
            || row.rotation.as_ref().is_some_and(|r| !r.selected_observed)
            || row.collector_service.is_some()
            || intent.configuration_recorded
        {
            return Err(invalid());
        }
        row.collector_service = Some(intent);
        self.node_association_put(snapshot, rows)
    }
    /// Native calls ONLY after verifying the SAME Node-signed claim+receipt.
    /// Database shape checking deliberately never claims signature authority.
    pub fn node_association_complete_collector_service(
        &self,
        snapshot: &NodeAssociationSnapshot,
        id: Uuid,
        expected: &NodeCollectorService,
        claim: Vec<u8>,
        receipt: Vec<u8>,
    ) -> Result<()> {
        let mut rows = snapshot.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.public.association_id == id)
            .ok_or_else(invalid)?;
        if row.phase != NodeAssociationPhase::Associated
            || row.revocation.is_some()
            || row.rotation.as_ref().is_some_and(|r| !r.selected_observed)
            || row.collector_service.as_ref() != Some(expected)
            || expected.configuration_recorded
        {
            return Err(invalid());
        }
        let mut next = expected.clone();
        next.signed_claim_json = Some(claim);
        next.signed_receipt_json = Some(receipt);
        next.configuration_recorded = true;
        next.validate()?;
        row.collector_service = Some(next);
        self.node_association_put(snapshot, rows)
    }
}

#[path="collector_service_lifetime.rs"]
mod lifetime;
pub use lifetime::NodeCollectorServiceCredentialSet;
