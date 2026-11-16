//! Encrypted LocalOnly user preference, never consent, a grant, or resume proof.
//! Native revalidates original context, current signatures and V2 admission.
use super::{NodeAssociationOwner, NodeAssociationPhase, NodeAssociationRecord, Tx};
use crate::{Result, StorageError};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const KEY: &str = "cc.native.agent.tunnel-resume-preferences.v1";
const MAX_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: usize = 128;

/// Exact TCP loopback destination. There is no global/default preference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeTunnelDestination {
    pub association_id: Uuid,
    pub destination_ip: String,
    pub destination_port: u16,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    destination: NodeTunnelDestination,
    // Exact complete public metadata: includes route/host revision, initial
    // certificate, current selected rotation and owner-pin readback. No hash
    // truncation, latest-context substitution or interpretation as authority.
    association: NodeAssociationRecord,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    version: u8,
    owner: NodeAssociationOwner,
    entries: Vec<Entry>,
}

fn invalid() -> StorageError {
    StorageError::Invalid("tunnel resume preference unavailable or changed".into())
}

fn destination_valid(d: &NodeTunnelDestination) -> bool {
    d.destination_ip.parse::<std::net::IpAddr>().is_ok_and(|ip|
        ip.is_loopback() && ip.to_string() == d.destination_ip)
        && !d.association_id.is_nil() && d.destination_port != 0
}

// Local JSON reader rejects duplicates recursively before Value can merge them.
// Raw input is bounded before deserialization; serde_json also bounds depth.
struct Unique(serde_json::Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Unique;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("duplicate-free bounded JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(serde_json::Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Unique, A::Error> {
                let mut values = Vec::new();
                while let Some(Unique(value)) = a.next_element::<Unique>()? { values.push(value); }
                Ok(Unique(serde_json::Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> std::result::Result<Unique, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = a.next_key::<String>()? {
                    if values.contains_key(&key) { return Err(serde::de::Error::custom("duplicate field")); }
                    let Unique(value) = a.next_value::<Unique>()?;
                    values.insert(key, value);
                }
                Ok(Unique(serde_json::Value::Object(values)))
            }
        }
        d.deserialize_any(Visitor)
    }
}

pub(super) fn unique(raw: &[u8]) -> Result<serde_json::Value> {
    if raw.is_empty() || raw.len() > MAX_BYTES { return Err(invalid()); }
    serde_json::from_slice::<Unique>(raw).map(|v| v.0).map_err(|_| invalid())
}

fn tunnel_row(row: &NodeAssociationRecord) -> Result<()> {
    if row.phase != NodeAssociationPhase::Associated || row.revocation.is_some() { return Err(invalid()); }
    let initial = &row.public.certificate.as_ref().ok_or_else(invalid)?.signed_certificate_json;
    let current = match &row.rotation {
        None => initial,
        Some(r) if r.selected_observed => r.signed_certificate_json.as_ref().ok_or_else(invalid)?,
        Some(_) => return Err(invalid()),
    };
    // Metadata gate ONLY. Existing Native current-certificate signature/chain
    // verification remains mandatory before any network operation.
    for raw in [initial, current] {
        let value = unique(raw)?;
        let body = value.get("body").ok_or_else(invalid)?;
        let expected = serde_json::json!(["tunnel.open"]);
        if body.get("scopes") != Some(&expected)
            || body.get("capabilities").and_then(|c| c.get("version")).and_then(|n| n.as_u64()) != Some(1)
            || body.get("capabilities").and_then(|c| c.get("scopes")) != Some(&expected)
        { return Err(invalid()); }
    }
    Ok(())
}

impl Tx<'_> {
    fn node_tunnel_preference_raw(&self) -> Result<Option<Vec<u8>>> {
        let row: Option<(i64, Option<Vec<u8>>)> = self.c().query_row(
            "SELECT length(CAST(value AS BLOB)), CASE WHEN length(CAST(value AS BLOB)) <= ?2 THEN CAST(value AS BLOB) ELSE NULL END FROM local_settings WHERE key=?1",
            params![KEY, MAX_BYTES as i64], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        match row {
            None => Ok(None),
            Some((n, Some(raw))) if n > 0 && n as usize == raw.len() && raw.len() <= MAX_BYTES => Ok(Some(raw)),
            _ => Err(invalid()),
        }
    }

    fn node_tunnel_preferences(&self, owner: &NodeAssociationOwner) -> Result<Preferences> {
        let Some(raw) = self.node_tunnel_preference_raw()? else {
            return Ok(Preferences { version: 1, owner: owner.clone(), entries: Vec::new() });
        };
        let prefs: Preferences = serde_json::from_value(unique(&raw)?).map_err(|_| invalid())?;
        if prefs.version != 1 || prefs.owner != *owner || prefs.entries.len() > MAX_ENTRIES { return Err(invalid()); }
        let mut destinations = std::collections::BTreeSet::new();
        for e in &prefs.entries {
            if !destination_valid(&e.destination) || e.destination.association_id != e.association.public.association_id
                || !destinations.insert((e.destination.association_id, e.destination.destination_ip.clone(), e.destination.destination_port))
            { return Err(invalid()); }
            tunnel_row(&e.association)?;
        }
        Ok(prefs)
    }

    fn node_tunnel_preference_association(&self, owner: &NodeAssociationOwner, d: &NodeTunnelDestination) -> Result<NodeAssociationRecord> {
        if !destination_valid(d) { return Err(invalid()); }
        // Actual current SQL profile/device/vault OWNER and exact full Personal
        // owner tuple are checked by the existing association ledger.
        let snapshot = self.node_association_snapshot(owner)?;
        let row = snapshot.records().iter().find(|r| r.public.association_id == d.association_id).ok_or_else(invalid)?;
        tunnel_row(row)?;
        Ok(row.clone())
    }

    /// Reads never repair/prune stale snapshots. Absent/stale is false, invalid
    /// or unknown storage is an error and remains byte-for-byte untouched.
    pub fn node_tunnel_resume_preference(&self, owner: &NodeAssociationOwner, destination: &NodeTunnelDestination) -> Result<bool> {
        let row = self.node_tunnel_preference_association(owner, destination)?;
        let prefs = self.node_tunnel_preferences(owner)?;
        Ok(prefs.entries.iter().any(|e| e.destination == *destination && e.association == row))
    }

    /// Invoke inside Storage::write_node_association with the original Native
    /// owner/scope fence, never an ordinary cloud/outbox write. Explicit enable
    /// may replace this destination's stale known snapshot; disable removes
    /// only an exact CURRENT snapshot and cannot delete an unknown descriptor.
    pub fn node_tunnel_set_resume_preference(&self, owner: &NodeAssociationOwner, destination: &NodeTunnelDestination, enabled: bool) -> Result<()> {
        let row = self.node_tunnel_preference_association(owner, destination)?;
        let mut prefs = self.node_tunnel_preferences(owner)?;
        let existing = prefs.entries.iter().position(|e| e.destination == *destination);
        if enabled {
            if let Some(i) = existing {
                if prefs.entries[i].association == row { return Ok(()); }
                prefs.entries[i].association = row;
            } else {
                if prefs.entries.len() >= MAX_ENTRIES { return Err(invalid()); }
                prefs.entries.push(Entry { destination: destination.clone(), association: row });
            }
        } else if let Some(i) = existing {
            if prefs.entries[i].association != row { return Ok(()); }
            prefs.entries.remove(i);
        } else { return Ok(()); }
        let raw = serde_json::to_vec(&prefs)?;
        if raw.len() > MAX_BYTES { return Err(invalid()); }
        let text = std::str::from_utf8(&raw).map_err(|_| invalid())?;
        self.c().execute("INSERT OR REPLACE INTO local_settings(key,value,updated_at) VALUES(?1,?2,?3)",
            params![KEY, text, crate::sql::ts(&crate::sql::now())])?;
        if self.node_tunnel_preference_raw()?.as_deref() != Some(raw.as_slice()) { return Err(invalid()); }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
