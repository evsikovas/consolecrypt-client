//! Child of collector_service: exact LocalOnly intent/CAS, never signature authority.
use super::*;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCollectorServiceCredentialSet {
    pub authorization_json: Vec<u8>,
    pub authorization_sha256: String,
    pub ordered_capsule_sha256: Vec<String>,
    pub minimum_actor_revision: u64,
    pub signed_admission_receipt_json: Option<Vec<u8>>,
}
impl NodeCollectorServiceCredentialSet {
    pub(super) fn validate(&self) -> Result<()> {
        if self.authorization_json.is_empty() || self.authorization_json.len() > 12288
            || !hash(&self.authorization_sha256) || self.minimum_actor_revision == 0
            || self.ordered_capsule_sha256.is_empty() || self.ordered_capsule_sha256.len() > 2
            || self.ordered_capsule_sha256.iter().any(|h| !hash(h))
            || self.signed_admission_receipt_json.as_ref().is_some_and(|v| v.is_empty() || v.len() > 4096)
        { return Err(invalid()); }
        Ok(())
    }
}
impl Tx<'_> {
    pub fn node_association_begin_service_credential_set(&self, snapshot: &NodeAssociationSnapshot,
        id: Uuid, expected: &NodeCollectorService, intent: NodeCollectorServiceCredentialSet) -> Result<()> {
        intent.validate()?;
        if expected.credential_set.is_some() || expected.revocation.is_some() || intent.signed_admission_receipt_json.is_some() {
            return Err(invalid());
        }
        let mut rows = snapshot.records().to_vec();
        let row = rows.iter_mut().find(|r| r.public.association_id == id).ok_or_else(invalid)?;
        if row.phase != NodeAssociationPhase::Associated || row.revocation.is_some()
            || row.rotation.as_ref().is_some_and(|r| !r.selected_observed)
            || row.collector_service.as_ref() != Some(expected) { return Err(invalid()); }
        let mut next = expected.clone(); next.credential_set = Some(intent); next.validate()?;
        row.collector_service = Some(next);
        self.node_association_put(snapshot, rows)
    }
    pub fn node_association_complete_service_credential_set(&self, snapshot: &NodeAssociationSnapshot,
        id: Uuid, expected: &NodeCollectorService, receipt: Vec<u8>) -> Result<()> {
        let mut rows = snapshot.records().to_vec();
        let row = rows.iter_mut().find(|r| r.public.association_id == id).ok_or_else(invalid)?;
        if row.phase != NodeAssociationPhase::Associated || row.revocation.is_some()
            || row.rotation.as_ref().is_some_and(|r| !r.selected_observed)
            || row.collector_service.as_ref() != Some(expected) || expected.revocation.is_some() { return Err(invalid()); }
        let mut next = expected.clone();
        let set = next.credential_set.as_mut().ok_or_else(invalid)?;
        if set.signed_admission_receipt_json.is_some() { return Err(invalid()); }
        set.signed_admission_receipt_json = Some(receipt); next.validate()?;
        row.collector_service = Some(next);
        self.node_association_put(snapshot, rows)
    }
}
