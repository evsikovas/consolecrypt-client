//! Vault key trust (protocol 1.11, security audit 2026-10-21, finding H2).
//!
//! Every profile pins the key commitment of its vault,
//! `C = HKDF(VRK, vault_id, "cc-vrk-commit-v1")`, in the key file and the
//! profile database. Every path that yields a VRK — passphrase (including the
//! retry with a fresh server envelope), Recovery Key, device unlock,
//! approval, backup restore, enable sync — checks it against the pin
//! **before** the vault is installed. A different key is a hard stop
//! ([`AppError::VaultKeyMismatch`]): nothing is decrypted or encrypted with
//! it, no attestation or envelope replacement is sent, sync does not start,
//! the event lands in the profile's security journal. There is no "accept
//! the new key": only a profile reset changes the pin.
//!
//! Pins taken without an independent check (device unlock after the update,
//! approval through an older server or approver) are marked unconfirmed;
//! the next passphrase or Recovery Key unlock confirms them — the server
//! cannot forge an envelope that opens with a passphrase it does not know.
//!
//! Synced profiles on a server with `capabilities.vrk_commitment` publish a
//! signed key-commitment record and compare the records of the account's
//! other devices: a different `C` raises "Device X received a different
//! vault key" and never changes this device's key.

use crate::app::AppCore;
use crate::dto::{AppEvent, VaultKeyConflictDto, VaultKeyStatusDto};
use crate::error::{AppError, AppResult};
use crate::keyfile::PinnedVaultKey;
use crate::secrets::blocking;
use crate::session::{Session, Unlocked};
use cc_protocol::envelopes::{KeyEnvelope, NewEnvelope};
use cc_protocol::{Bytes, DeviceId, VaultId};
use cc_storage_core::local_key_envelope;
use cc_vault_core::UnlockedVault;
use std::sync::Arc;

/// Database copy of the pinned key (the key file holds the same value).
pub(crate) const VAULT_KEY_SETTING: &str = "app_core.vault_key";
/// Security journal of a profile (JSON lines, metadata only).
const JOURNAL_FILE: &str = "security-journal.jsonl";
/// Upper bound of the journal before it is rotated (keeps the newest half).
const JOURNAL_MAX_BYTES: u64 = 256 * 1024;

/// Lower-case hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Where a VRK came from: decides whether a first pin counts as confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeySource {
    /// This device created the vault.
    Created,
    /// The vault passphrase opened a password envelope.
    Passphrase,
    /// The Recovery Key opened a recovery envelope.
    RecoveryKey,
    /// A backup opened with its passphrase or Recovery Key.
    Backup,
    /// OS-protected device unlock (local envelope sealed by this device).
    DeviceUnlock,
    /// Approval v2 verified with the approver key the user confirmed.
    ApprovalVerified,
    /// Approval without v2 (older server or approver): trust on first use.
    ApprovalUnverified,
}

impl KeySource {
    /// The VRK was obtained independently of the server.
    fn confirms(self) -> bool {
        matches!(
            self,
            KeySource::Created
                | KeySource::Passphrase
                | KeySource::RecoveryKey
                | KeySource::Backup
                | KeySource::ApprovalVerified
        )
    }

    fn name(self) -> &'static str {
        match self {
            KeySource::Created => "created",
            KeySource::Passphrase => "passphrase",
            KeySource::RecoveryKey => "recovery_key",
            KeySource::Backup => "backup",
            KeySource::DeviceUnlock => "device_unlock",
            KeySource::ApprovalVerified => "approval_v2",
            KeySource::ApprovalUnverified => "approval_unverified",
        }
    }
}

/// The pin to write after the vault was accepted (`None` = unchanged).
#[must_use]
pub(crate) struct PinUpdate(Option<PinnedVaultKey>);

impl Session {
    /// The pinned vault key (key file first, else the database).
    pub(crate) async fn pinned_vault_key(&self) -> Option<PinnedVaultKey> {
        if let Some(pin) = self.keyfile().and_then(|k| k.vault_key) {
            return Some(pin);
        }
        let st = self.storage_opt()?;
        st.setting_get::<PinnedVaultKey>(VAULT_KEY_SETTING)
            .await
            .ok()
            .flatten()
    }

    /// TR-3: compare the key of `vault` with the pin. A mismatch is the
    /// hard stop of TR-9; otherwise the returned update (first pin, or the
    /// confirmation of an unconfirmed one) is written with
    /// [`Session::store_pin`] once the vault was accepted.
    pub(crate) async fn check_vault_key(
        &self,
        vault: &UnlockedVault,
        source: KeySource,
    ) -> AppResult<PinUpdate> {
        let received = vault.key_commitment();
        match self.pinned_vault_key().await {
            Some(pin) => {
                let pinned = pin.commitment();
                if pinned != Some(received) {
                    return Err(self
                        .key_mismatch(vault.vault_id(), pinned, Some(received), "commitment")
                        .await);
                }
                if pin.unconfirmed && source.confirms() {
                    tracing::info!(source = source.name(), "vault key confirmed");
                    return Ok(PinUpdate(Some(PinnedVaultKey {
                        unconfirmed: false,
                        ..pin
                    })));
                }
                Ok(PinUpdate(None))
            }
            None => {
                tracing::info!(source = source.name(), "vault key pinned");
                Ok(PinUpdate(Some(PinnedVaultKey {
                    commitment: Bytes::new(received.to_vec()),
                    unconfirmed: !source.confirms(),
                    published: false,
                })))
            }
        }
    }

    /// Persist a pin update: the database (when open) and the key file
    /// (when present). [`Session::seal_storage`] copies the database value
    /// into a new key file.
    pub(crate) async fn store_pin(&self, update: PinUpdate) -> AppResult<()> {
        let Some(pin) = update.0 else {
            return Ok(());
        };
        if let Some(st) = self.storage_opt() {
            st.setting_set(VAULT_KEY_SETTING, pin.clone()).await?;
        }
        self.update_keyfile(|k| k.vault_key = Some(pin)).await
    }

    /// Mark the pin as published (or not) to the server.
    async fn set_pin_published(&self, published: bool) -> AppResult<()> {
        let Some(mut pin) = self.pinned_vault_key().await else {
            return Ok(());
        };
        if pin.published == published {
            return Ok(());
        }
        pin.published = published;
        self.store_pin(PinUpdate(Some(pin))).await
    }

    /// TR-9: log, journal and announce a key mismatch; the caller returns
    /// the error without using the key.
    pub(crate) async fn key_mismatch(
        &self,
        vault_id: VaultId,
        pinned: Option<[u8; 32]>,
        received: Option<[u8; 32]>,
        reason: &str,
    ) -> AppError {
        tracing::error!(
            target: "security",
            vault_id = %vault_id,
            reason,
            "vault key mismatch: the server offered a key this device does not trust"
        );
        let server = self
            .profile()
            .await
            .ok()
            .and_then(|p| p.server_url)
            .unwrap_or_default();
        self.journal(serde_json::json!({
            "event": "vault_key_mismatch",
            "vault_id": vault_id.to_string(),
            "reason": reason,
            "pinned": pinned.map(|c| hex(&c)),
            "received": received.map(|c| hex(&c)),
            "server": server,
        }))
        .await;
        self.ctx.emit(AppEvent::VaultKeyMismatch {
            vault_id: vault_id.to_string(),
            reason: reason.to_owned(),
        });
        AppError::VaultKeyMismatch {
            vault_id: vault_id.to_string(),
            reason: reason.to_owned(),
            pinned: pinned.map(|c| hex(&c)),
            received: received.map(|c| hex(&c)),
        }
    }

    /// Append one line to the profile's security journal (best effort,
    /// metadata only: ids, commitments, server URL, time).
    pub(crate) async fn journal(&self, mut entry: serde_json::Value) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        entry["at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        let r = blocking(move || {
            use std::io::Write as _;
            let path = dir.join(JOURNAL_FILE);
            let io = |e: std::io::Error| AppError::Storage(format!("security journal: {e}"));
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > JOURNAL_MAX_BYTES) {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let keep: Vec<&str> = text.lines().collect();
                let tail = keep[keep.len() / 2..].join("\n");
                std::fs::write(&path, format!("{tail}\n")).map_err(io)?;
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(io)?;
            writeln!(f, "{entry}").map_err(io)?;
            Ok(())
        })
        .await;
        if let Err(e) = r {
            tracing::warn!(error = %e, "security journal not written");
        }
    }
}

/// TR-4: what the server echoes is cached only if it is exactly what this
/// device sent (then with the server's envelope id); otherwise the device's
/// own envelope is cached.
pub(crate) fn adopt_own(
    vault_id: VaultId,
    own: &NewEnvelope,
    server: Option<KeyEnvelope>,
    device: DeviceId,
) -> KeyEnvelope {
    match server {
        Some(env) if env.vault_id == vault_id && env.to_new() == *own => env,
        Some(_) => {
            tracing::warn!(
                "server envelope differs from the one this device sent; keeping our own"
            );
            local_key_envelope(vault_id, own.clone(), Some(device))
        }
        None => local_key_envelope(vault_id, own.clone(), Some(device)),
    }
}

impl AppCore {
    /// Vault key trust of the open profile (pin, confirmation, conflicts
    /// reported by other devices of the account).
    pub async fn vault_key_status(&self) -> AppResult<VaultKeyStatusDto> {
        let s = self.session().await?;
        let pin = s.pinned_vault_key().await;
        let conflicts = match s.unlocked_opt().await {
            Some(u) => u.key_conflicts(),
            None => Vec::new(),
        };
        Ok(VaultKeyStatusDto {
            pinned: pin.is_some(),
            unconfirmed: pin.as_ref().is_some_and(|p| p.unconfirmed),
            fingerprint: pin
                .as_ref()
                .and_then(PinnedVaultKey::commitment)
                .map(|c| hex(&c[..8])),
            conflicts,
        })
    }

    /// TR-7: confirm an unconfirmed vault key with the vault passphrase.
    /// The stored password envelope is opened (and, if it no longer opens,
    /// a fresh one from the server); its key must equal the pinned one.
    /// `Ok(false)` = wrong passphrase.
    pub async fn confirm_vault_key(&self, passphrase: String) -> AppResult<bool> {
        let s = self.session().await?;
        let (vault_id, cached, _) = s.unlock_envelopes().await?;
        let pass = Arc::new(crate::app::secret(passphrase));
        let mut candidates: Vec<KeyEnvelope> = cached.into_iter().collect();
        if let Some(api) = s.api_opt() {
            if let Ok(m) = api.recovery_vault_envelope(vault_id).await {
                if let Some(env) = m.password_envelope {
                    if !candidates.contains(&env) {
                        candidates.push(env);
                    }
                }
            }
        }
        for env in candidates {
            let p = pass.clone();
            match blocking(move || Ok(cc_vault_core::unlock_with_passphrase(vault_id, &env, &p)?))
                .await
            {
                Ok(vault) => {
                    let update = s.check_vault_key(&vault, KeySource::Passphrase).await?;
                    vault.lock();
                    s.store_pin(update).await?;
                    return Ok(true);
                }
                Err(AppError::WrongPassphrase) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(false)
    }
}

/// TR-5, best effort after unlock (synced profiles on a 1.11 server):
/// publish this device's signed key-commitment record once and compare the
/// records of the account's other devices with the pin.
pub(crate) async fn reconcile_key_commitments(
    s: &Arc<Session>,
    u: &Arc<Unlocked>,
) -> AppResult<()> {
    let Some(api) = s.api_opt() else {
        return Ok(());
    };
    if !api
        .meta()
        .await
        .is_ok_and(|m| m.capabilities.vrk_commitment)
    {
        return Ok(());
    }
    let Some(pin) = s.pinned_vault_key().await else {
        return Ok(());
    };
    let Some(own) = pin.commitment() else {
        return Ok(());
    };
    let vault_id = u.vault_id;
    let identity = s.identity();
    if !pin.published {
        let record = identity.key_commitment_record(vault_id, &own, chrono::Utc::now().timestamp());
        api.put_key_commitment(vault_id, &record).await?;
        s.set_pin_published(true).await?;
    }
    let records = api.list_key_commitments(vault_id).await?.commitments;
    let devices = api.list_devices().await?.devices;
    let mut conflicts = Vec::new();
    for r in records {
        if r.device_id == identity.device_id() {
            continue;
        }
        let Some(dev) = devices.iter().find(|d| d.device_id == r.device_id) else {
            continue;
        };
        let (Some(signing), Some(commitment)) = (
            dev.signing_public_key.to_array::<32>(),
            r.commitment.to_array::<32>(),
        ) else {
            continue;
        };
        // An invalid signature is ignored: records only raise alarms.
        if cc_crypto_core::verify_key_commitment(
            &signing,
            vault_id,
            &commitment,
            r.device_id,
            r.issued_at,
            r.signature.as_slice(),
        )
        .is_err()
        {
            continue;
        }
        if commitment != own {
            conflicts.push(VaultKeyConflictDto {
                device_id: r.device_id.to_string(),
                device_name: dev.name.clone(),
                fingerprint: hex(&commitment[..8]),
            });
        }
    }
    let known = u.key_conflicts();
    for c in &conflicts {
        if !known.iter().any(|k| k.device_id == c.device_id) {
            tracing::warn!(
                target: "security",
                device_id = %c.device_id,
                "a device of this account reported a different vault key"
            );
            s.journal(serde_json::json!({
                "event": "vault_key_conflict",
                "vault_id": vault_id.to_string(),
                "device_id": c.device_id,
                "reported": c.fingerprint,
                "pinned": hex(&own[..8]),
            }))
            .await;
            s.ctx.emit(AppEvent::VaultKeyConflict {
                device_id: c.device_id.clone(),
                device_name: c.device_name.clone(),
            });
        }
    }
    u.set_key_conflicts(conflicts);
    Ok(())
}
