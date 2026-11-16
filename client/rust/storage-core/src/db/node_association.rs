//! Fixed Native LocalOnly association metadata, never Node/Reader authority.
//! Native must verify SAME original SSH transaction + real signed claim/cert
//! and exact Actor committed checkpoint before the completion transition.
use super::{Database, ReaderAuthorityWriteError, Tx};
use crate::{ProfileId, ProfileKind, Result, StorageError};
use cc_models::ObjectId;
use cc_protocol::{DeviceId, UserId, VaultId};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
mod collector_service;
pub use collector_service::{NodeCollectorService,NodeCollectorServiceCredentialSet,NodeCollectorServiceRevocation};
const KEY: &str = "cc.native.agent.node-associations.v1";
const MAX_BYTES: usize = 256 * 1024;
const MAX_RECORDS: usize = 32;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAssociationPersonalOwner {
    pub user_id: UserId,
    pub server_url: String,
    pub origin_fingerprint: String,
    pub vault_commitment: String,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAssociationOwner {
    pub profile_id: ProfileId,
    pub device_id: DeviceId,
    pub vault_id: VaultId,
    pub signing_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personal: Option<NodeAssociationPersonalOwner>,
}
/// Public data frozen BEFORE certificate apply. IDs stay in their real domains;
/// the Agent ID/context are never derived by casting a Native profile ID.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAssociationPublic {
    pub association_id: Uuid,
    pub saved_host_id: ObjectId,
    pub saved_host_revision: i64,
    pub route_trust_digest: String,
    pub agent_id: String,
    pub node_key_fingerprint: String,
    pub owner_epoch: u64,
    pub checkpoint: u64,
    pub key_epoch: u64,
    pub expected_actor_revision: u64,
    pub client_tls_identity_id: Uuid,
    pub client_tls_certificate_sha256: String,
    pub offer_digest: String,
    pub signed_offer_json: Vec<u8>,
    pub owner_pin_json: Vec<u8>,
    pub certificate: Option<NodeAssociationCertificate>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAssociationCertificate {
    pub request_digest: String,
    pub certificate_digest: String,
    pub signed_certificate_json: Vec<u8>,
    pub committed_owner_pin_json: Vec<u8>,
    pub expected_actor_revision: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeAssociationPhase {
    Pending,
    Unknown,
    Associated,
    Blocked,
    Revoked,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAssociationRecord {
    pub public: NodeAssociationPublic,
    pub phase: NodeAssociationPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation: Option<NodeAssociationRevocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<NodeAssociationRotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collector_service: Option<NodeCollectorService>,
}
/// Bounded public rotation intent in the EXISTING row, not a new journal or
/// authority. Only a private same-peer observed completion sets selected=true.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAssociationRotation {
    pub candidate_json: Option<Vec<u8>>,
    pub signed_certificate_json: Option<Vec<u8>>,
    pub old_proof_json: Option<Vec<u8>>,
    pub new_proof_json: Option<Vec<u8>>,
    pub committed_owner_pin_json: Option<Vec<u8>>,
    pub expected_actor_revision: u64,
    pub observed_owner_pin_json: Option<Vec<u8>>,
    pub observed_actor_revision: Option<u64>,
    pub selected_observed: bool,
}
/// Existing LocalOnly row intent only; no signer/grant or portable ACK.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAssociationRevocation {
    pub signed_revocation_json: Vec<u8>,
    pub committed_owner_pin_json: Vec<u8>,
    pub expected_actor_revision: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u8,
    revision: u64,
    owner: NodeAssociationOwner,
    records: Vec<NodeAssociationRecord>,
}
pub struct NodeAssociationSnapshot {
    owner: NodeAssociationOwner,
    raw: Option<Vec<u8>>,
    ledger: Option<Ledger>,
}
macro_rules! redacted {($($t:ty),*)=>{$(impl std::fmt::Debug for $t{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(concat!(stringify!($t),"([redacted])"))}})*}}
redacted!(
    NodeAssociationOwner,
    NodeAssociationPersonalOwner,
    NodeAssociationPublic,
    NodeAssociationCertificate,
    NodeAssociationPhase,
    NodeAssociationRecord,
    NodeAssociationRevocation,
    NodeAssociationRotation,
    NodeAssociationSnapshot
);
impl NodeAssociationSnapshot {
    pub fn revision(&self) -> Option<u64> {
        self.ledger.as_ref().map(|l| l.revision)
    }
    pub fn records(&self) -> &[NodeAssociationRecord] {
        self.ledger.as_ref().map_or(&[], |l| &l.records)
    }
}
fn invalid() -> StorageError {
    StorageError::Invalid("node association unavailable or changed".into())
}
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn canonical_id(s: &str) -> bool {
    Uuid::parse_str(s).is_ok_and(|id| !id.is_nil() && id.to_string() == s)
}
fn validate_public(p: &NodeAssociationPublic) -> Result<()> {
    if p.association_id.is_nil()
        || !canonical_id(&p.saved_host_id.to_string())
        || p.saved_host_revision <= 0
        || !canonical_id(&p.agent_id)
        || p.client_tls_identity_id.is_nil()
        || p.owner_epoch == 0
        || p.key_epoch == 0
        || [
            &p.route_trust_digest,
            &p.node_key_fingerprint,
            &p.client_tls_certificate_sha256,
            &p.offer_digest,
        ]
        .iter()
        .any(|s| !hash(s))
        || p.signed_offer_json.is_empty()
        || p.signed_offer_json.len() > 16384
        || p.owner_pin_json.is_empty()
        || p.owner_pin_json.len() > 4096
    {
        return Err(invalid());
    }
    if let Some(c) = &p.certificate {
        if !hash(&c.request_digest)
            || !hash(&c.certificate_digest)
            || c.expected_actor_revision == 0
            || c.signed_certificate_json.is_empty()
            || c.signed_certificate_json.len() > 16384
            || c.committed_owner_pin_json.is_empty()
            || c.committed_owner_pin_json.len() > 4096
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn validate_owner(o: &NodeAssociationOwner) -> Result<()> {
    if o.profile_id.0.is_nil()
        || !canonical_id(&o.device_id.to_string())
        || !canonical_id(&o.vault_id.to_string())
        || !hash(&o.signing_fingerprint)
        || o.personal.as_ref().is_some_and(|p| {
            p.user_id == UserId::NIL
                || p.server_url.is_empty()
                || p.server_url.len() > 2048
                || p.server_url.chars().any(char::is_control)
                || !hash(&p.origin_fingerprint)
                || !hash(&p.vault_commitment)
        })
    {
        return Err(invalid());
    }
    Ok(())
}
fn validate_ledger(l: &Ledger) -> Result<()> {
    validate_owner(&l.owner)?;
    if l.version != 1 || l.revision == 0 || l.records.is_empty() || l.records.len() > MAX_RECORDS {
        return Err(invalid());
    }
    let mut ids = std::collections::BTreeSet::new();
    for row in &l.records {
        validate_public(&row.public)?;
        if row.phase == NodeAssociationPhase::Associated && row.public.certificate.is_none() {
            return Err(invalid());
        }
        if let Some(service) = &row.collector_service {
            if row.public.certificate.is_none() { return Err(invalid()); }
            service.validate()?;
        }
        if let Some(rotation) = &row.rotation {
            if row.public.certificate.is_none()
                || !matches!(row.phase,NodeAssociationPhase::Blocked|NodeAssociationPhase::Associated|NodeAssociationPhase::Revoked)
                || rotation.expected_actor_revision==0 {return Err(invalid())}
            for raw in [&rotation.candidate_json,&rotation.signed_certificate_json,&rotation.old_proof_json,
                &rotation.new_proof_json,&rotation.committed_owner_pin_json,&rotation.observed_owner_pin_json] {
                if raw.as_ref().is_some_and(|v|v.is_empty()||v.len()>16384) {return Err(invalid())}
            }
            if rotation.old_proof_json.is_some()!=rotation.new_proof_json.is_some()
                || rotation.signed_certificate_json.is_some()!=rotation.committed_owner_pin_json.is_some()
                || rotation.old_proof_json.is_some() && rotation.signed_certificate_json.is_none()
                || rotation.signed_certificate_json.is_some() && rotation.candidate_json.is_none()
                || rotation.selected_observed && (rotation.old_proof_json.is_none()
                    || rotation.observed_owner_pin_json.is_none() || rotation.observed_actor_revision.is_none())
                || !rotation.selected_observed && (rotation.observed_owner_pin_json.is_some()
                    || rotation.observed_actor_revision.is_some() || row.phase!=NodeAssociationPhase::Blocked) {
                return Err(invalid())
            }
        }
        if let Some(revoke) = &row.revocation {
            if row.public.certificate.is_none()
                || !matches!(row.phase, NodeAssociationPhase::Blocked | NodeAssociationPhase::Revoked)
                || revoke.expected_actor_revision == 0 || revoke.signed_revocation_json.is_empty()
                || revoke.signed_revocation_json.len() > 16384
                || revoke.committed_owner_pin_json.is_empty() || revoke.committed_owner_pin_json.len() > 4096 {
                return Err(invalid());
            }
        }
        if row.phase == NodeAssociationPhase::Revoked && row.revocation.is_none() { return Err(invalid()); }
        if !ids.insert(row.public.association_id) {
            return Err(invalid());
        }
    }
    Ok(())
}
impl Database {
    pub fn node_association_write<T, E>(
        &mut self,
        f: impl FnOnce(&Tx<'_>) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, ReaderAuthorityWriteError<E>>
    where
        E: From<StorageError>,
    {
        self.durable_write_kernel(true, f)
    }
}
impl Tx<'_> {
    fn node_association_current(&self, o: &NodeAssociationOwner) -> Result<()> {
        validate_owner(o)?;
        let p = self.get_profile()?.ok_or_else(invalid)?;
        let v = self.get_vault(o.vault_id)?.ok_or_else(invalid)?;
        let owner = match &o.personal {
            None => {
                p.kind == ProfileKind::Local
                    && p.user_id.is_none()
                    && p.server_url.is_none()
                    && v.owner_user_id.is_none()
            }
            Some(actual) => {
                p.kind == ProfileKind::Synced
                    && p.user_id == Some(actual.user_id)
                    && p.server_url.as_deref() == Some(actual.server_url.as_str())
                    && v.owner_user_id == Some(actual.user_id)
                    && v.caller_trusted
            }
        };
        if !owner
            || p.profile_id != o.profile_id
            || p.device_id != o.device_id
            || v.role != cc_protocol::vaults::VaultRole::Owner
            || v.state != cc_protocol::vaults::VaultState::Active
        {
            return Err(invalid());
        }
        // Native additionally checks actual /auth/me +Personal membership,
        // canonical origin/confirmed commitment, original nonce and route refs.
        Ok(())
    }
    fn node_association_raw(&self) -> Result<Option<Vec<u8>>> {
        let row:Option<(i64,Option<Vec<u8>>)>=self.c().query_row(
            "SELECT length(CAST(value AS BLOB)), CASE WHEN length(CAST(value AS BLOB)) <= ?2 THEN CAST(value AS BLOB) ELSE NULL END FROM local_settings WHERE key=?1",
            params![KEY,MAX_BYTES as i64],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        match row {
            None => Ok(None),
            Some((n, Some(raw))) if n > 0 && n as usize <= MAX_BYTES && raw.len() == n as usize => {
                Ok(Some(raw))
            }
            _ => Err(invalid()),
        }
    }
    pub fn node_association_snapshot(
        &self,
        o: &NodeAssociationOwner,
    ) -> Result<NodeAssociationSnapshot> {
        self.node_association_current(o)?;
        let raw = self.node_association_raw()?;
        let ledger = raw
            .as_ref()
            .map(|bytes| {
                let l: Ledger = serde_json::from_slice(bytes).map_err(|_| invalid())?;
                validate_ledger(&l)?;
                if l.owner != *o {
                    return Err(invalid());
                }
                Ok(l)
            })
            .transpose()?;
        Ok(NodeAssociationSnapshot {
            owner: o.clone(),
            raw,
            ledger,
        })
    }
    fn node_association_put(
        &self,
        s: &NodeAssociationSnapshot,
        records: Vec<NodeAssociationRecord>,
    ) -> Result<()> {
        self.node_association_current(&s.owner)?;
        if self.node_association_raw()? != s.raw {
            return Err(invalid());
        }
        let revision = s
            .revision()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(invalid)?;
        let l = Ledger {
            version: 1,
            revision,
            owner: s.owner.clone(),
            records,
        };
        validate_ledger(&l)?;
        let raw = serde_json::to_vec(&l)?;
        if raw.len() > MAX_BYTES {
            return Err(invalid());
        }
        let text = std::str::from_utf8(&raw).map_err(|_| invalid())?;
        self.c().execute(
            "INSERT OR REPLACE INTO local_settings(key,value,updated_at) VALUES(?1,?2,?3)",
            params![KEY, text, crate::sql::ts(&crate::sql::now())],
        )?;
        if self.node_association_raw()?.as_deref() != Some(raw.as_slice()) {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn node_association_reserve(
        &self,
        s: &NodeAssociationSnapshot,
        public: NodeAssociationPublic,
    ) -> Result<()> {
        validate_public(&public)?;
        if public.certificate.is_some() {
            return Err(invalid());
        }
        let mut rows = s.records().to_vec();
        if rows.len() >= MAX_RECORDS
            || rows.iter().any(|r| {
                r.public.association_id == public.association_id
                    || (r.public.saved_host_id == public.saved_host_id
                        && r.public.agent_id == public.agent_id)
            })
        {
            return Err(invalid());
        }
        rows.push(NodeAssociationRecord {
            public,
            phase: NodeAssociationPhase::Pending,
            revocation: None,
            rotation: None,
            collector_service: None,
        });
        self.node_association_put(s, rows)
    }
    /// UNKNOWN is durable BEFORE forwarding certificate apply. A lost ACK or
    /// worker reply never becomes Associated or permits a retry/replacement.
    pub fn node_association_mark_unknown(
        &self,
        s: &NodeAssociationSnapshot,
        id: Uuid,
    ) -> Result<()> {
        self.node_association_transition(
            s,
            id,
            NodeAssociationPhase::Pending,
            NodeAssociationPhase::Unknown,
        )
    }
    /// Only after the actual same-channel Claim is cryptographically verified.
    /// No token is retained; Unknown remains sticky before certificate forwarding.
    pub fn node_association_attach_certificate(
        &self,
        s: &NodeAssociationSnapshot,
        id: Uuid,
        certificate: NodeAssociationCertificate,
    ) -> Result<()> {
        let mut rows = s.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.public.association_id == id)
            .ok_or_else(invalid)?;
        if row.phase != NodeAssociationPhase::Unknown || row.public.certificate.is_some() {
            return Err(invalid());
        }
        row.public.certificate = Some(certificate);
        self.node_association_put(s, rows)
    }
    /// Only the Native SAME-transaction typed verified ACK may invoke this.
    /// Metadata is not a grant; Native must reverify signatures on future use.
    pub fn node_association_complete(&self, s: &NodeAssociationSnapshot, id: Uuid) -> Result<()> {
        self.node_association_transition(
            s,
            id,
            NodeAssociationPhase::Unknown,
            NodeAssociationPhase::Associated,
        )
    }
    pub fn node_association_begin_rotation(&self,s:&NodeAssociationSnapshot,expected:&NodeAssociationRecord,
        intent:NodeAssociationRotation)->Result<()> {
        let mut rows=s.records().to_vec();let row=rows.iter_mut().find(|r|r.public.association_id==expected.public.association_id).ok_or_else(invalid)?;
        if &*row!=expected || row.phase!=NodeAssociationPhase::Associated || row.public.certificate.is_none()
            || row.revocation.is_some() || row.rotation.is_some() || intent.selected_observed {return Err(invalid())}
        row.phase=NodeAssociationPhase::Blocked;row.rotation=Some(intent);self.node_association_put(s,rows)
    }
    pub fn node_association_update_rotation(&self,s:&NodeAssociationSnapshot,id:Uuid,
        expected:&NodeAssociationRotation,next:NodeAssociationRotation)->Result<()> {
        let mut rows=s.records().to_vec();let row=rows.iter_mut().find(|r|r.public.association_id==id).ok_or_else(invalid)?;
        if row.phase!=NodeAssociationPhase::Blocked || row.rotation.as_ref()!=Some(expected)
            || row.revocation.is_some() || expected.selected_observed {return Err(invalid())}
        if next.expected_actor_revision!=expected.expected_actor_revision
            || expected.candidate_json.is_some() && next.candidate_json!=expected.candidate_json
            || expected.signed_certificate_json.is_some() && next.signed_certificate_json!=expected.signed_certificate_json
            || expected.old_proof_json.is_some() && next.old_proof_json!=expected.old_proof_json
            || expected.new_proof_json.is_some() && next.new_proof_json!=expected.new_proof_json
            || expected.committed_owner_pin_json.is_some() && next.committed_owner_pin_json!=expected.committed_owner_pin_json {
            return Err(invalid())
        }
        if next.selected_observed {row.phase=NodeAssociationPhase::Associated;}
        row.rotation=Some(next);self.node_association_put(s,rows)
    }
    /// Local fail-closed intent BEFORE remote dispatch. Lost ACK keeps this
    /// row Blocked+intent; no enrollment reset or automatic retry is allowed.
    pub fn node_association_begin_revocation(&self, s: &NodeAssociationSnapshot, expected: &NodeAssociationRecord,
        intent: NodeAssociationRevocation) -> Result<()> {
        let mut rows = s.records().to_vec();
        let row = rows.iter_mut().find(|r| r.public.association_id == expected.public.association_id).ok_or_else(invalid)?;
        if &*row != expected || row.phase != NodeAssociationPhase::Associated || row.public.certificate.is_none()
            || row.revocation.is_some() { return Err(invalid()); }
        row.phase = NodeAssociationPhase::Blocked;
        row.revocation = Some(intent);
        self.node_association_put(s, rows)
    }
    /// Only SAME original typed revoked readback may complete this local row.
    pub fn node_association_complete_revocation(&self, s: &NodeAssociationSnapshot, id: Uuid,
        expected: &NodeAssociationRevocation) -> Result<()> {
        let mut rows = s.records().to_vec();
        let row = rows.iter_mut().find(|r| r.public.association_id == id).ok_or_else(invalid)?;
        if row.phase != NodeAssociationPhase::Blocked || row.revocation.as_ref() != Some(expected) {
            return Err(invalid());
        }
        row.phase = NodeAssociationPhase::Revoked;
        self.node_association_put(s, rows)
    }
    pub fn node_association_block(&self, s: &NodeAssociationSnapshot, id: Uuid) -> Result<()> {
        let mut rows = s.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.public.association_id == id)
            .ok_or_else(invalid)?;
        row.phase = NodeAssociationPhase::Blocked;
        self.node_association_put(s, rows)
    }
    fn node_association_transition(
        &self,
        s: &NodeAssociationSnapshot,
        id: Uuid,
        from: NodeAssociationPhase,
        to: NodeAssociationPhase,
    ) -> Result<()> {
        let mut rows = s.records().to_vec();
        let row = rows
            .iter_mut()
            .find(|r| r.public.association_id == id && r.phase == from)
            .ok_or_else(invalid)?;
        row.phase = to;
        self.node_association_put(s, rows)
    }
}
#[cfg(test)]
mod tests;
