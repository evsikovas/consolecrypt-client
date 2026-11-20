//! Local-only service owner metadata on the ORIGINAL profile SQLCipher connection.
//! This storage seam neither signs, authorizes, enrolls nor reconstructs a grant.
//! AppCore must retain its original WeakSession/WeakUnlocked/nonce, validate trust
//! and referenced SQL revisions inside the write closure, and publish NOTHING on
//! Unknown. No nonce is persisted or reconstructed from this record.
use super::{Database, ReaderAuthorityWriteError, Tx};
use crate::{ProfileId, ProfileKind, Result, StorageError};
use cc_protocol::{DeviceId, VaultId};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const KEY: &str = "cc.native.service.owner.v1";
const MAX_BYTES: usize = 256 * 1024;
const MAX_RECORDS: usize = 32;

/// Native-authenticated PERSONAL metadata pin, not an account authorization.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePersonalOwnerPin {
    pub user_id: cc_protocol::UserId,
    pub server_origin_fingerprint: String,
    pub vault_commitment: String,
}

/// An original Native owner supplied by AppCore, NOT a Node LocalProfile ID.
/// The fingerprint is compared to the stored pin, not authenticated here.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceOwnerIdentity {
    pub profile_id: ProfileId,
    pub vault_id: VaultId,
    pub device_id: DeviceId,
    pub signing_fingerprint: String,
    // Missing remains the OLD Local-only format; never defaults to Personal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personal: Option<ServicePersonalOwnerPin>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceOwnerDelivery {
    PendingEnrollment,
    Acknowledged,
    DeliveryUnknown,
    RevokedLocally,
}

/// Closed public metadata only: no arbitrary JSON, signatures, secret refs,
/// variables, credentials, or private key material. Signing/head payloads are a
/// separate future typed seam; these digests cannot reconstruct a lease/grant.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceOwnerRecord {
    pub service_id: Uuid,
    /// Opaque Node-local namespace; NEVER UUID-cast to the Native profile ID.
    pub node_local_profile: String,
    pub owner_epoch: u64,
    pub policy_revision: u64,
    pub revocation_epoch: u64,
    pub clock_floor: u64,
    pub signer_fingerprint: String,
    pub recipient_fingerprint: String,
    pub route_trust_digest: String,
    pub policy_digest: String,
    pub status_digest: String,
    pub delivery: ServiceOwnerDelivery,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u8,
    revision: u64,
    owner: ServiceOwnerIdentity,
    // Array prevents duplicate service IDs being silently overwritten by a map.
    #[serde(deserialize_with = "read_records")]
    records: Vec<ServiceOwnerRecord>,
}

fn read_records<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<ServiceOwnerRecord>, D::Error> {
    struct Records;
    impl<'de> serde::de::Visitor<'de> for Records {
        type Value = Vec<ServiceOwnerRecord>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bounded service owner records")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut records = Vec::new();
            while let Some(record) = seq.next_element::<ServiceOwnerRecord>()? {
                if records.len() == MAX_RECORDS {
                    return Err(serde::de::Error::custom("service owner record budget"));
                }
                records.push(record);
            }
            Ok(records)
        }
    }
    d.deserialize_seq(Records)
}

/// Opaque exact-read CAS expectation. Absence is NOT an enrolled owner, and is
/// never converted to default epochs. No Deserialize / raw-byte constructor.
pub struct ServiceOwnerSnapshot {
    owner: ServiceOwnerIdentity,
    raw: Option<Vec<u8>>,
    ledger: Option<Ledger>,
}
impl ServiceOwnerSnapshot {
    pub fn revision(&self) -> Option<u64> {
        self.ledger.as_ref().map(|l| l.revision)
    }
    pub fn records(&self) -> &[ServiceOwnerRecord] {
        self.ledger.as_ref().map_or(&[], |l| l.records.as_slice())
    }
}
impl std::fmt::Debug for ServiceOwnerSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceOwnerSnapshot(<private metadata>)")
    }
}

pub enum ServiceOwnerWriteError<E> {
    BeforeWrite(StorageError),
    Rejected(E),
    Unknown,
}
impl<E> std::fmt::Debug for ServiceOwnerWriteError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BeforeWrite(_) => "ServiceOwnerWriteError::BeforeWrite",
            Self::Rejected(_) => "ServiceOwnerWriteError::Rejected",
            Self::Unknown => "ServiceOwnerWriteError::Unknown",
        })
    }
}
impl<E> std::fmt::Display for ServiceOwnerWriteError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl<E> std::error::Error for ServiceOwnerWriteError<E> {}
impl<E> From<StorageError> for ServiceOwnerWriteError<E> {
    fn from(_: StorageError) -> Self {
        Self::Unknown
    }
}

impl Database {
    /// Same FULL/IMMEDIATE/restore kernel; requires file-backed storage.
    /// Only this result after COMMIT can acknowledge durability. A Tx-level CAS
    /// result alone is NOT a durable receipt or export permission.
    pub fn service_owner_write<T, E>(
        &mut self,
        f: impl FnOnce(&Tx<'_>) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, ServiceOwnerWriteError<E>>
    where
        E: From<StorageError>,
    {
        self.durable_write_kernel(true, f).map_err(|e| match e {
            ReaderAuthorityWriteError::BeforeWrite(e) => ServiceOwnerWriteError::BeforeWrite(e),
            ReaderAuthorityWriteError::Rejected(e) => ServiceOwnerWriteError::Rejected(e),
            ReaderAuthorityWriteError::Unknown => ServiceOwnerWriteError::Unknown,
        })
    }
}

fn invalid() -> StorageError {
    StorageError::Invalid("service owner ledger unavailable or changed".into())
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate_owner(owner: &ServiceOwnerIdentity) -> Result<()> {
    if owner.profile_id.0.is_nil()
        || owner.device_id.to_string() == Uuid::nil().to_string()
        || owner.vault_id.to_string() == Uuid::nil().to_string()
        || !hash(&owner.signing_fingerprint)
        || owner.personal.as_ref().is_some_and(|p| {
            p.user_id == cc_protocol::UserId::NIL
                || !hash(&p.server_origin_fingerprint)
                || !hash(&p.vault_commitment)
        })
    {
        return Err(invalid());
    }
    Ok(())
}
fn validate(ledger: &Ledger) -> Result<()> {
    validate_owner(&ledger.owner)?;
    if ledger.version != 1
        || ledger.revision == 0
        || ledger.records.is_empty()
        || ledger.records.len() > MAX_RECORDS
    {
        return Err(invalid());
    }
    let mut ids = std::collections::BTreeSet::new();
    for r in &ledger.records {
        if r.service_id.is_nil()
            || !ids.insert(r.service_id)
            || r.node_local_profile.is_empty()
            || r.node_local_profile.len() > 128
            || !r
                .node_local_profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || r.owner_epoch == 0
            || r.policy_revision == 0
            || r.revocation_epoch == 0
            || r.clock_floor == 0
            || ![
                &r.signer_fingerprint,
                &r.recipient_fingerprint,
                &r.route_trust_digest,
                &r.policy_digest,
                &r.status_digest,
            ]
            .into_iter()
            .all(|s| hash(s))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

impl Tx<'_> {
    fn service_owner_current(&self, owner: &ServiceOwnerIdentity) -> Result<()> {
        validate_owner(owner)?;
        let profile = self.get_profile()?.ok_or_else(invalid)?;
        let vault = self.get_vault(owner.vault_id)?.ok_or_else(invalid)?;
        let matches = match &owner.personal {
            None => {
                profile.kind == ProfileKind::Local
                    && profile.user_id.is_none()
                    && profile.server_url.is_none()
                    && vault.owner_user_id.is_none()
            }
            Some(personal) => {
                profile.kind == ProfileKind::Synced
                    && profile.user_id == Some(personal.user_id)
                    && profile.server_url.as_ref().is_some_and(|s| !s.is_empty())
                    && vault.owner_user_id == Some(personal.user_id)
                    && vault.caller_trusted
            }
        };
        // Native additionally proves PERSONAL list membership, original canonical
        // origin and confirmed commitment. SQL rows alone never attest these.
        if !matches
            || profile.profile_id != owner.profile_id
            || profile.device_id != owner.device_id
            || vault.role != cc_protocol::vaults::VaultRole::Owner
            || vault.state != cc_protocol::vaults::VaultState::Active
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn service_owner_bytes(&self) -> Result<Option<Vec<u8>>> {
        // SQL bounds BEFORE Rust allocation, including corrupt oversized rows.
        let row: Option<(i64, Option<Vec<u8>>)> = self.c().query_row(
            "SELECT length(CAST(value AS BLOB)), CASE WHEN length(CAST(value AS BLOB)) <= ?2 THEN CAST(value AS BLOB) ELSE NULL END FROM local_settings WHERE key=?1",
            params![KEY, MAX_BYTES as i64], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;
        match row {
            None => Ok(None),
            Some((n, Some(raw))) if n > 0 && n as usize <= MAX_BYTES && raw.len() == n as usize => {
                Ok(Some(raw))
            }
            _ => Err(invalid()),
        }
    }
    pub fn service_owner_snapshot(
        &self,
        owner: &ServiceOwnerIdentity,
    ) -> Result<ServiceOwnerSnapshot> {
        self.service_owner_current(owner)?;
        let raw = self.service_owner_bytes()?;
        let ledger = raw
            .as_ref()
            .map(|raw| {
                let ledger: Ledger = serde_json::from_slice(raw).map_err(|_| invalid())?;
                validate(&ledger)?;
                if ledger.owner != *owner {
                    return Err(invalid());
                }
                Ok(ledger)
            })
            .transpose()?;
        Ok(ServiceOwnerSnapshot {
            owner: owner.clone(),
            raw,
            ledger,
        })
    }
    /// Explicit bounded metadata transition; NOT deserializable approval. Caller
    /// must check original runtime owner and genuine policy provenance in this
    /// SAME transaction before calling. Tombstones/floors cannot be removed.
    pub fn service_owner_compare_exchange(
        &self,
        expected: &ServiceOwnerSnapshot,
        records: Vec<ServiceOwnerRecord>,
    ) -> Result<()> {
        self.service_owner_current(&expected.owner)?;
        if self.service_owner_bytes()? != expected.raw {
            return Err(invalid());
        }
        let revision = expected
            .ledger
            .as_ref()
            .map_or(Some(1), |l| l.revision.checked_add(1))
            .ok_or_else(invalid)?;
        let next = Ledger {
            version: 1,
            revision,
            owner: expected.owner.clone(),
            records,
        };
        validate(&next)?;
        for old in expected.records() {
            let new = next
                .records
                .iter()
                .find(|r| r.service_id == old.service_id)
                .ok_or_else(invalid)?;
            if new.node_local_profile != old.node_local_profile
                || new.owner_epoch != old.owner_epoch
                || new.signer_fingerprint != old.signer_fingerprint
                || new.recipient_fingerprint != old.recipient_fingerprint
                || new.policy_revision < old.policy_revision
                || new.revocation_epoch < old.revocation_epoch
                || new.clock_floor < old.clock_floor
                || (old.delivery == ServiceOwnerDelivery::RevokedLocally
                    && new.delivery != ServiceOwnerDelivery::RevokedLocally)
                || (new.policy_revision == old.policy_revision
                    && (new.policy_digest != old.policy_digest
                        || new.route_trust_digest != old.route_trust_digest))
            {
                return Err(invalid());
            }
        }
        let raw = serde_json::to_vec(&next).map_err(|_| invalid())?;
        if raw.len() > MAX_BYTES {
            return Err(invalid());
        }
        let value = std::str::from_utf8(&raw).map_err(|_| invalid())?;
        self.c().execute(
            "INSERT OR REPLACE INTO local_settings(key,value,updated_at) VALUES(?1,?2,?3)",
            params![KEY, value, crate::sql::ts(&crate::sql::now())],
        )?;
        if self.service_owner_bytes()?.as_deref() != Some(raw.as_slice()) {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
