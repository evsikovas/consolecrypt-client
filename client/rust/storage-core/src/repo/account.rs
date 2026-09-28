//! `profile` and `vaults` repositories.

use crate::db::Tx;
use crate::error::Result;
use crate::model::{Profile, ProfileKind, VaultRecord};
use crate::repo::objects::OptionalStorage;
use crate::sql::{
    enum_str, now, opt_json, parse_enum, parse_id, parse_opt_id, parse_opt_json, parse_opt_ts,
    parse_ts, ts,
};
use cc_protocol::envelopes::KeyEnvelope;
use cc_protocol::vaults::VaultInfo;
use cc_protocol::VaultId;
use rusqlite::{params, Row};

const VAULT_COLS: &str = "vault_id, role, state, owner_user_id, caller_trusted, \
     server_latest_sequence, password_envelope, recovery_envelope, device_envelope, \
     server_created_at, last_unlocked_at, updated_at";

fn vault_from_row(r: &Row<'_>) -> Result<VaultRecord> {
    const T: &str = "vaults";
    Ok(VaultRecord {
        vault_id: parse_id(&r.get::<_, String>(0)?, T, "vault_id")?,
        role: parse_enum(&r.get::<_, String>(1)?, T, "role")?,
        state: parse_enum(&r.get::<_, String>(2)?, T, "state")?,
        owner_user_id: parse_opt_id(r.get(3)?, T, "owner_user_id")?,
        caller_trusted: r.get::<_, i64>(4)? != 0,
        server_latest_sequence: r.get(5)?,
        password_envelope: parse_opt_json(r.get(6)?, T, "password_envelope")?,
        recovery_envelope: parse_opt_json(r.get(7)?, T, "recovery_envelope")?,
        device_envelope: parse_opt_json(r.get(8)?, T, "device_envelope")?,
        server_created_at: parse_opt_ts(r.get(9)?, T, "server_created_at")?,
        last_unlocked_at: parse_opt_ts(r.get(10)?, T, "last_unlocked_at")?,
        updated_at: parse_ts(&r.get::<_, String>(11)?, T, "updated_at")?,
    })
}

impl Tx<'_> {
    /// The profile row of this database, if set.
    pub fn get_profile(&self) -> Result<Option<Profile>> {
        const T: &str = "profile";
        self.c()
            .query_row_and_then(
                "SELECT profile_id, kind, display_name, server_url, user_id, device_id, email, \
                 created_at, updated_at FROM profile WHERE id = 1",
                [],
                |r| -> Result<Profile> {
                    Ok(Profile {
                        profile_id: parse_id(&r.get::<_, String>(0)?, T, "profile_id")?,
                        kind: parse_enum(&r.get::<_, String>(1)?, T, "kind")?,
                        display_name: r.get(2)?,
                        server_url: r.get(3)?,
                        user_id: parse_opt_id(r.get(4)?, T, "user_id")?,
                        device_id: parse_id(&r.get::<_, String>(5)?, T, "device_id")?,
                        email: r.get(6)?,
                        created_at: parse_ts(&r.get::<_, String>(7)?, T, "created_at")?,
                        updated_at: parse_ts(&r.get::<_, String>(8)?, T, "updated_at")?,
                    })
                },
            )
            .optional_storage()
    }

    /// Kind of this profile; databases without a profile row behave as
    /// `Synced` (the sync engine and tests set up rows explicitly).
    pub fn profile_kind(&self) -> Result<ProfileKind> {
        Ok(self.get_profile()?.map_or(ProfileKind::Synced, |p| p.kind))
    }

    /// Create or replace the profile row.
    pub fn put_profile(&self, p: &Profile) -> Result<()> {
        if p.kind == ProfileKind::Synced && p.server_url.is_none() {
            return Err(crate::StorageError::Invalid(
                "synced profile needs a server url".into(),
            ));
        }
        self.c().execute(
            "INSERT OR REPLACE INTO profile (id, profile_id, kind, display_name, server_url, user_id, \
             device_id, email, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                p.profile_id.to_string(),
                enum_str(&p.kind)?,
                p.display_name,
                p.server_url,
                p.user_id.map(|u| u.to_string()),
                p.device_id.to_string(),
                p.email,
                ts(&p.created_at),
                ts(&p.updated_at),
            ],
        )?;
        Ok(())
    }

    /// Remove the profile row (sign-out).
    pub fn clear_profile(&self) -> Result<()> {
        self.c().execute("DELETE FROM profile", [])?;
        Ok(())
    }

    /// One vault.
    pub fn get_vault(&self, vault_id: VaultId) -> Result<Option<VaultRecord>> {
        self.c()
            .query_row_and_then(
                &format!("SELECT {VAULT_COLS} FROM vaults WHERE vault_id = ?1"),
                params![vault_id.to_string()],
                vault_from_row,
            )
            .optional_storage()
    }

    /// All vaults, ordered by id.
    pub fn list_vaults(&self) -> Result<Vec<VaultRecord>> {
        let mut stmt = self.c().prepare(&format!(
            "SELECT {VAULT_COLS} FROM vaults ORDER BY vault_id"
        ))?;
        let rows = stmt.query_and_then([], vault_from_row)?;
        rows.collect()
    }

    /// Insert or replace a vault record.
    pub fn put_vault(&self, v: &VaultRecord) -> Result<()> {
        self.c().execute(
            &format!(
                "INSERT OR REPLACE INTO vaults ({VAULT_COLS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
            ),
            params![
                v.vault_id.to_string(),
                enum_str(&v.role)?,
                enum_str(&v.state)?,
                v.owner_user_id.map(|u| u.to_string()),
                v.caller_trusted as i64,
                v.server_latest_sequence,
                opt_json(&v.password_envelope)?,
                opt_json(&v.recovery_envelope)?,
                opt_json(&v.device_envelope)?,
                v.server_created_at.as_ref().map(ts),
                v.last_unlocked_at.as_ref().map(ts),
                ts(&v.updated_at),
            ],
        )?;
        Ok(())
    }

    /// Merge server-side vault metadata, keeping cached envelopes.
    pub fn upsert_vault_info(&self, info: &VaultInfo) -> Result<VaultRecord> {
        let mut rec = self
            .get_vault(info.vault_id)?
            .unwrap_or_else(|| VaultRecord::new(info.vault_id, info.role));
        rec.role = info.role;
        rec.state = info.state;
        rec.owner_user_id = Some(info.owner_user_id);
        rec.caller_trusted = info.caller_trusted;
        rec.server_latest_sequence = info.latest_sequence;
        rec.server_created_at = Some(info.created_at);
        rec.updated_at = now();
        self.put_vault(&rec)?;
        Ok(rec)
    }

    /// Cache envelopes for offline unlock. `None` leaves a slot unchanged.
    pub fn cache_vault_envelopes(
        &self,
        vault_id: VaultId,
        password: Option<&KeyEnvelope>,
        recovery: Option<&KeyEnvelope>,
        device: Option<&KeyEnvelope>,
    ) -> Result<()> {
        let Some(mut rec) = self.get_vault(vault_id)? else {
            return Err(crate::StorageError::Invalid("unknown vault".into()));
        };
        if let Some(e) = password {
            rec.password_envelope = Some(e.clone());
        }
        if let Some(e) = recovery {
            rec.recovery_envelope = Some(e.clone());
        }
        if let Some(e) = device {
            rec.device_envelope = Some(e.clone());
        }
        rec.updated_at = now();
        self.put_vault(&rec)
    }

    /// Record a successful unlock (for UI "last unlocked").
    pub fn mark_vault_unlocked(&self, vault_id: VaultId) -> Result<()> {
        let t = ts(&now());
        self.c().execute(
            "UPDATE vaults SET last_unlocked_at = ?2, updated_at = ?2 WHERE vault_id = ?1",
            params![vault_id.to_string(), t],
        )?;
        Ok(())
    }

    /// Remove a vault and all of its local data (objects, outbox, cursors).
    pub fn purge_vault(&self, vault_id: VaultId) -> Result<()> {
        let v = vault_id.to_string();
        for table in [
            "objects",
            "remote_objects",
            "outbox",
            "sync_state",
            "snapshot_seen",
            "vaults",
        ] {
            self.c().execute(
                &format!("DELETE FROM {table} WHERE vault_id = ?1"),
                params![v],
            )?;
        }
        Ok(())
    }
}
