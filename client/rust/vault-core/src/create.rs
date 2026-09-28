//! Vault creation (ADR-0002 key hierarchy; synced or local-only, ADR-0106).

use crate::error::Result;
use crate::identity::DeviceIdentity;
use crate::recovery_kit::RecoveryKit;
use crate::unlocked::{check_new_passphrase, UnlockedVault};
use cc_crypto_core::{Argon2Params, RecoveryKey, Vrk};
use cc_protocol::envelopes::NewEnvelope;
use cc_protocol::vaults::CreateVaultRequest;
use cc_protocol::{Timestamp, VaultId};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Result of [`create_vault`].
///
/// * Synced profile: send `create_request` (`POST /v1/vaults`).
/// * Local-only profile: do **not** send it; persist
///   [`CreatedVault::local_envelopes`] in the local database instead.
///   Nothing here depends on a server response (the vault id is
///   client-generated), so the vault is fully usable offline.
///
/// In both cases show `recovery_kit` once and run its onboarding check.
pub struct CreatedVault {
    pub unlocked: UnlockedVault,
    pub create_request: CreateVaultRequest,
    pub recovery_kit: RecoveryKit,
}

impl CreatedVault {
    /// The envelopes to persist locally (local-only profiles, and the cache
    /// for offline unlock of synced profiles).
    pub fn local_envelopes(&self) -> LocalVaultEnvelopes {
        LocalVaultEnvelopes {
            vault_id: self.create_request.vault_id,
            password: self.create_request.password_envelope.clone(),
            recovery: self.create_request.recovery_envelope.clone(),
            device: Some(self.create_request.device_envelope.clone()),
        }
    }
}

impl fmt::Debug for CreatedVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreatedVault")
            .field("vault_id", &self.create_request.vault_id)
            .field("unlocked", &self.unlocked)
            .field("recovery_kit", &self.recovery_kit)
            .finish_non_exhaustive()
    }
}

/// A vault's envelopes as persisted locally (no server-assigned fields).
/// Contains only ciphertext and public metadata — safe to store in the
/// local database and in backups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalVaultEnvelopes {
    pub vault_id: VaultId,
    pub password: NewEnvelope,
    pub recovery: NewEnvelope,
    /// This installation's device envelope (OS/biometric unlock), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<NewEnvelope>,
}

/// Create a new vault: random `vault_id` and VRK, password envelope
/// (Argon2id with `kdf`), a fresh Recovery Key + recovery envelope, and a
/// device envelope for `identity`.
///
/// `server_url` is printed on the Recovery Kit (`None` for local-only
/// profiles). Blocking (Argon2id).
pub fn create_vault(
    identity: &DeviceIdentity,
    passphrase: &SecretString,
    kdf: Argon2Params,
    server_url: Option<&str>,
    created_at: Timestamp,
) -> Result<CreatedVault> {
    check_new_passphrase(passphrase)?;
    let vault_id = VaultId::new();
    let unlocked = UnlockedVault::from_vrk(vault_id, Vrk::generate()?);
    let rk = RecoveryKey::generate()?;
    let create_request = CreateVaultRequest {
        vault_id,
        vault_access_key: unlocked.vault_access_key(),
        password_envelope: unlocked.password_envelope(passphrase, kdf)?,
        recovery_envelope: unlocked.recovery_envelope(&rk)?,
        device_envelope: unlocked.device_envelope_for(identity)?,
    };
    let recovery_kit = RecoveryKit::new(vault_id, &rk, server_url, created_at);
    Ok(CreatedVault {
        unlocked,
        create_request,
        recovery_kit,
    })
}
