//! One device/profile-local TLS client identity. Never a Node enrollment/grant.
//! No secret bytes; original runtime auth must be checked by the Native caller.
use super::{Database, ReaderAuthorityWriteError, Tx};
use crate::{ProfileId, Result, StorageError};
use cc_protocol::DeviceId;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const KEY: &str = "cc.native.tunnel.client-tls.v1";
const MAX_BYTES: usize = 24 * 1024;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelTlsOwner {
    pub profile_id: ProfileId,
    pub device_id: DeviceId,
    pub signer_fingerprint: String,
    pub context_fingerprint: String,
}

/// Public certificate and fingerprints only. Identity existence is NOT trust.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelTlsPublic {
    pub identity_id: Uuid,
    pub certificate_der: Vec<u8>,
    pub certificate_sha256: String,
    pub private_key_sha256: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelTlsPhase {
    Pending,
    Stored,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    version: u8,
    owner: TunnelTlsOwner,
    public: TunnelTlsPublic,
    phase: TunnelTlsPhase,
}

/// Exact read, not deserializable approval. No replacement/delete transition.
pub struct TunnelTlsSnapshot {
    owner: TunnelTlsOwner,
    raw: Option<Vec<u8>>,
    row: Option<Row>,
}
macro_rules! redacted_debug {($($ty:ty),*)=>{$(impl std::fmt::Debug for $ty {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(concat!(stringify!($ty),"([redacted])"))}})*}}
redacted_debug!(TunnelTlsOwner, TunnelTlsPublic, TunnelTlsPhase);
impl std::fmt::Debug for TunnelTlsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelTlsSnapshot([redacted])")
    }
}
impl TunnelTlsSnapshot {
    pub fn phase(&self) -> Option<TunnelTlsPhase> {
        self.row.as_ref().map(|r| r.phase)
    }
    pub fn public(&self) -> Option<&TunnelTlsPublic> {
        self.row.as_ref().map(|r| &r.public)
    }
}
fn invalid() -> StorageError {
    StorageError::Invalid("tunnel TLS identity unavailable or changed".into())
}
fn hash(v: &str) -> bool {
    v.len() == 64
        && v.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate(owner: &TunnelTlsOwner, public: Option<&TunnelTlsPublic>) -> Result<()> {
    if owner.profile_id.0.is_nil()
        || owner.device_id.to_string() == Uuid::nil().to_string()
        || !hash(&owner.signer_fingerprint)
        || !hash(&owner.context_fingerprint)
    {
        return Err(invalid());
    }
    if public.is_some_and(|p| {
        p.identity_id.is_nil()
            || p.certificate_der.is_empty()
            || p.certificate_der.len() > 4096
            || !hash(&p.certificate_sha256)
            || !hash(&p.private_key_sha256)
    }) {
        return Err(invalid());
    }
    Ok(())
}
impl Database {
    /// SAME file-backed FULL/IMMEDIATE kernel, not an ordinary Settings write.
    pub fn tunnel_tls_write<T, E>(
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
    fn tunnel_tls_current(&self, o: &TunnelTlsOwner) -> Result<()> {
        validate(o, None)?;
        let p = self.get_profile()?.ok_or_else(invalid)?;
        if p.profile_id != o.profile_id || p.device_id != o.device_id {
            return Err(invalid());
        }
        Ok(())
    }
    fn tunnel_tls_raw(&self) -> Result<Option<Vec<u8>>> {
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
    pub fn tunnel_tls_snapshot(&self, o: &TunnelTlsOwner) -> Result<TunnelTlsSnapshot> {
        self.tunnel_tls_current(o)?;
        let raw = self.tunnel_tls_raw()?;
        let row = raw
            .as_ref()
            .map(|b| {
                let r: Row = serde_json::from_slice(b).map_err(|_| invalid())?;
                validate(&r.owner, Some(&r.public))?;
                if r.version != 1 || r.owner != *o {
                    return Err(invalid());
                }
                Ok(r)
            })
            .transpose()?;
        Ok(TunnelTlsSnapshot {
            owner: o.clone(),
            raw,
            row,
        })
    }
    /// First writer only; public metadata is frozen BEFORE SecureStore writes.
    pub fn tunnel_tls_reserve(&self, s: &TunnelTlsSnapshot, p: TunnelTlsPublic) -> Result<()> {
        if s.raw.is_some() {
            return Err(invalid());
        }
        self.tunnel_tls_put(s, p, TunnelTlsPhase::Pending)
    }
    /// Only original Pending -> Stored, preserving every frozen byte/ID/hash.
    /// Native must have read back matching key and OS witness before calling.
    pub fn tunnel_tls_complete(&self, s: &TunnelTlsSnapshot) -> Result<()> {
        let r = s
            .row
            .as_ref()
            .filter(|r| r.phase == TunnelTlsPhase::Pending)
            .ok_or_else(invalid)?;
        self.tunnel_tls_put(s, r.public.clone(), TunnelTlsPhase::Stored)
    }
    fn tunnel_tls_put(
        &self,
        s: &TunnelTlsSnapshot,
        p: TunnelTlsPublic,
        phase: TunnelTlsPhase,
    ) -> Result<()> {
        self.tunnel_tls_current(&s.owner)?;
        validate(&s.owner, Some(&p))?;
        if self.tunnel_tls_raw()? != s.raw {
            return Err(invalid());
        }
        let bytes = serde_json::to_vec(&Row {
            version: 1,
            owner: s.owner.clone(),
            public: p,
            phase,
        })?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
        self.c().execute(
            "INSERT OR REPLACE INTO local_settings(key,value,updated_at) VALUES(?1,?2,?3)",
            params![KEY, text, crate::sql::ts(&crate::sql::now())],
        )?;
        if self.tunnel_tls_raw()?.as_deref() != Some(bytes.as_slice()) {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
