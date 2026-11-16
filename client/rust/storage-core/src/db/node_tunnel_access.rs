//! Bounded LocalOnly observations in the original encrypted profile database.
//! These rows never reconstruct a grant or claim remote drainage/delivery.
use super::{NodeAssociationOwner, Result, StorageError, Tx};
use cc_models::ObjectId;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const KEY: &str = "cc.native.agent.tunnel-access.v1";
const MAX_BYTES: usize = 128 * 1024;
const MAX_ROWS: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeTunnelAccessEvent {
    OpenRequested,
    Opened,
    StopRequested,
    Cancelled,
    Revoked,
    Unknown,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeTunnelAccessRecord {
    pub operation_id: Uuid,
    pub association_id: Uuid,
    pub host_id: ObjectId,
    pub host_revision: i64,
    pub grant_id: Uuid,
    pub grant_digest: String,
    pub policy_digest: String,
    pub agent_id: String,
    pub key_epoch: u64,
    pub protocol: String,
    pub destination_ip: String,
    pub destination_port: u16,
    pub expires_at: u64,
    pub observed_at: u64,
    pub event: NodeTunnelAccessEvent,
    pub sent: u64,
    pub received: u64,
    pub counters_exact: bool,
    pub native_control_finished: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeTunnelAccessRow {
    pub sequence: u64,
    pub profile_id: crate::ProfileId,
    pub device_id: cc_protocol::DeviceId,
    pub personal_user_id: Option<cc_protocol::UserId>,
    pub record: NodeTunnelAccessRecord,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u8,
    revision: u64,
    owner: NodeAssociationOwner,
    rows: Vec<NodeTunnelAccessRow>,
}

fn invalid() -> StorageError {
    StorageError::Invalid("tunnel access observations unavailable".into())
}
fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate(record: &NodeTunnelAccessRecord) -> Result<()> {
    let agent = Uuid::parse_str(&record.agent_id).map_err(|_| invalid())?;
    let ip: std::net::IpAddr = record.destination_ip.parse().map_err(|_| invalid())?;
    if record.operation_id.is_nil() || record.association_id.is_nil() || record.grant_id.is_nil()
        || agent.is_nil() || agent.to_string() != record.agent_id || record.host_revision <= 0
        || record.key_epoch == 0 || !hash(&record.grant_digest) || !hash(&record.policy_digest)
        || record.protocol != "tcp" || !ip.is_loopback() || ip.to_string() != record.destination_ip || record.destination_port == 0
        || !(1..=253_402_300_799).contains(&record.expires_at)
        || !(1..=253_402_300_799).contains(&record.observed_at)
        || record.sent.checked_add(record.received).is_none_or(|n| n > 32 * 1024 * 1024)
        || matches!(record.event, NodeTunnelAccessEvent::OpenRequested | NodeTunnelAccessEvent::Opened)
            && (record.native_control_finished || record.sent != 0 || record.received != 0)
    { return Err(invalid()); }
    Ok(())
}

impl Tx<'_> {
    fn node_tunnel_access_raw(&self) -> Result<Option<Vec<u8>>> {
        let row: Option<(i64, Option<Vec<u8>>)> = self.c().query_row(
            "SELECT length(CAST(value AS BLOB)), CASE WHEN length(CAST(value AS BLOB)) <= ?2 THEN CAST(value AS BLOB) ELSE NULL END FROM local_settings WHERE key=?1",
            params![KEY, MAX_BYTES as i64], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        match row {
            None => Ok(None),
            Some((length, Some(raw))) if length > 0 && length as usize == raw.len() && raw.len() <= MAX_BYTES => Ok(Some(raw)),
            _ => Err(invalid()),
        }
    }
    fn node_tunnel_access_journal(&self, owner: &NodeAssociationOwner) -> Result<Journal> {
        // Reuses the actual SQL profile/device/OWNER/Active checks, not UI data.
        self.node_association_snapshot(owner)?;
        let Some(raw) = self.node_tunnel_access_raw()? else {
            return Ok(Journal { version: 1, revision: 0, owner: owner.clone(), rows: Vec::new() });
        };
        let value: Journal = serde_json::from_slice(&raw).map_err(|_| invalid())?;
        if value.version != 1 || value.owner != *owner || value.revision == 0 || value.rows.is_empty() || value.rows.len() > MAX_ROWS
            || value.rows.windows(2).any(|r| r[0].sequence >= r[1].sequence)
            || value.rows.last().is_some_and(|r| r.sequence != value.revision)
        { return Err(invalid()); }
        for row in &value.rows {
            validate(&row.record)?;
            if row.profile_id != owner.profile_id || row.device_id != owner.device_id
                || row.personal_user_id != owner.personal.as_ref().map(|p| p.user_id)
            { return Err(invalid()); }
        }
        Ok(value)
    }
    pub fn node_tunnel_access_records(&self, owner: &NodeAssociationOwner) -> Result<Vec<NodeTunnelAccessRow>> {
        Ok(self.node_tunnel_access_journal(owner)?.rows)
    }
    pub fn node_tunnel_access_append(&self, owner: &NodeAssociationOwner, record: NodeTunnelAccessRecord) -> Result<()> {
        validate(&record)?;
        let associations = self.node_association_snapshot(owner)?;
        if !associations.records().iter().any(|r| r.public.association_id == record.association_id
            && r.public.saved_host_id == record.host_id && r.public.saved_host_revision == record.host_revision
            && r.public.agent_id == record.agent_id)
        { return Err(invalid()); }
        let mut journal = self.node_tunnel_access_journal(owner)?;
        journal.revision = journal.revision.checked_add(1).ok_or_else(invalid)?;
        journal.rows.push(NodeTunnelAccessRow { sequence: journal.revision, profile_id: owner.profile_id,
            device_id: owner.device_id, personal_user_id: owner.personal.as_ref().map(|p| p.user_id), record });
        while journal.rows.len() > MAX_ROWS { journal.rows.remove(0); }
        let mut raw = serde_json::to_vec(&journal)?;
        while raw.len() > MAX_BYTES && journal.rows.len() > 1 {
            journal.rows.remove(0);
            raw = serde_json::to_vec(&journal)?;
        }
        if raw.len() > MAX_BYTES { return Err(invalid()); }
        let text = std::str::from_utf8(&raw).map_err(|_| invalid())?;
        self.c().execute("INSERT OR REPLACE INTO local_settings(key,value,updated_at) VALUES(?1,?2,?3)",
            params![KEY, text, crate::sql::ts(&crate::sql::now())])?;
        if self.node_tunnel_access_raw()?.as_deref() != Some(raw.as_slice()) { return Err(invalid()); }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
