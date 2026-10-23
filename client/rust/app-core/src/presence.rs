//! Step-up confirmations with the OS user-presence check (0.9.2): "Confirm
//! with Touch ID / Windows Hello / fingerprint" instead of typing the vault
//! passphrase.
//!
//! * Offered only while device unlock (S5) is on for the profile on this
//!   device, its key is protected by the OS (`system`, never the Linux
//!   keyring convenience mode) and the user kept "Confirm actions with
//!   biometrics" on (the default).
//! * [`AppCore::verify_user_presence`] asks the OS to release the device
//!   unlock key (the OS shows its own prompt with the app's reason), opens
//!   the local unlock envelope with it inside the core and mints a
//!   single-use presence token bound to the action kind and the object
//!   (session, container, …). The key never leaves the core; nothing the UI
//!   claims replaces the OS check. Android passes the material its
//!   `BiometricPrompt` `CryptoObject` decrypted (as for unlock).
//! * Core-enforced step-ups ([`StepUp`]) accept the passphrase or such a
//!   token; [`PASSPHRASE_ONLY`] actions never accept a token.
//! * Tokens live in memory of the unlocked vault only: locking, switching
//!   the profile or re-unlocking drops them.

use crate::app::AppCore;
use crate::error::{AppError, AppResult};
use crate::keyfile::{self, DeviceUnlockRecord};
use crate::secrets::blocking;
use crate::session::{Session, Unlocked};
use base64::Engine;
use cc_platform_core::{OsAuthError, UnlockProtection};
use cc_vault_core::{unlock_with_unlock_key, UnlockKey, VaultError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// How long a presence token can be redeemed.
pub(crate) const PRESENCE_TTL: Duration = Duration::from_secs(30);
const MAX_TOKENS: usize = 16;
const MAX_OBJECT: usize = 512;

/// What a step-up confirmation is for (wire names cross the bridge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepUpAction {
    /// A change the app confirms before it calls the core (Kubernetes
    /// changes, reviewed database statements, …): the app redeems the
    /// token right away with [`AppCore::confirm_step_up`].
    Confirm,
    /// The production write window of a database session (object: the
    /// session id).
    DbProductionWrite,
    /// A container's environment or unmasked inspect (object:
    /// `<session>/<container>`).
    DockerReveal,
    /// Editing a Compose project (object: `<session>/<project>`).
    DockerCompose,
    ChangePassphrase,
    RecoveryKit,
    DisableDeviceUnlock,
    BackupExport,
    BackupRestore,
    ApproveDevice,
    AccountSecurity,
    EnterpriseAccount,
}

/// Actions confirmed with the vault passphrase only, never with the OS
/// check. They change who or what can open the vault (passphrase, Recovery
/// Kit, device unlock itself, a new device), move the whole vault out or
/// in (backups) or change the account / organisation sign-in: a stolen
/// unlocked device with a known device PIN must not be enough for them.
/// The app keeps the same list (`lib/vault/step_up.dart`).
pub const PASSPHRASE_ONLY: &[StepUpAction] = &[
    StepUpAction::ChangePassphrase,
    StepUpAction::RecoveryKit,
    StepUpAction::DisableDeviceUnlock,
    StepUpAction::BackupExport,
    StepUpAction::BackupRestore,
    StepUpAction::ApproveDevice,
    StepUpAction::AccountSecurity,
    StepUpAction::EnterpriseAccount,
];

impl StepUpAction {
    const ALL: [(StepUpAction, &'static str); 12] = [
        (Self::Confirm, "confirm"),
        (Self::DbProductionWrite, "db_production_write"),
        (Self::DockerReveal, "docker_reveal"),
        (Self::DockerCompose, "docker_compose"),
        (Self::ChangePassphrase, "change_passphrase"),
        (Self::RecoveryKit, "recovery_kit"),
        (Self::DisableDeviceUnlock, "disable_device_unlock"),
        (Self::BackupExport, "backup_export"),
        (Self::BackupRestore, "backup_restore"),
        (Self::ApproveDevice, "approve_device"),
        (Self::AccountSecurity, "account_security"),
        (Self::EnterpriseAccount, "enterprise_account"),
    ];

    pub fn parse(wire: &str) -> AppResult<Self> {
        Self::ALL
            .iter()
            .find(|(_, name)| *name == wire)
            .map(|(action, _)| *action)
            .ok_or_else(|| AppError::invalid("action", "unknown step-up action"))
    }

    pub fn wire(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(action, _)| *action == self)
            .map(|(_, name)| *name)
            .unwrap_or("confirm")
    }

    /// See [`PASSPHRASE_ONLY`].
    pub fn passphrase_only(self) -> bool {
        PASSPHRASE_ONLY.contains(&self)
    }
}

/// The proof a core-enforced step-up gets: the vault passphrase or a
/// presence token from [`AppCore::verify_user_presence`].
pub enum StepUp {
    Passphrase(String),
    Presence(String),
}

impl std::fmt::Debug for StepUp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            StepUp::Passphrase(_) => "StepUp::Passphrase(<redacted>)",
            StepUp::Presence(_) => "StepUp::Presence(<redacted>)",
        })
    }
}

impl StepUp {
    /// Exactly one of the two; neither → `passphrase_required`.
    pub fn from_parts(
        passphrase: Option<String>,
        presence_token: Option<String>,
    ) -> AppResult<Self> {
        match (passphrase, presence_token) {
            (Some(p), None) => Ok(StepUp::Passphrase(p)),
            (None, Some(t)) => Ok(StepUp::Presence(t)),
            (None, None) => Err(AppError::PassphraseRequired),
            (Some(_), Some(_)) => Err(AppError::invalid(
                "step_up",
                "either the passphrase or a presence token",
            )),
        }
    }
}

struct Grant {
    action: StepUpAction,
    object: String,
    expires: Instant,
}

/// Presence tokens of one unlock lifetime (part of [`Unlocked`]).
#[derive(Default)]
pub(crate) struct PresenceTokens {
    grants: Mutex<HashMap<String, Grant>>,
}

impl std::fmt::Debug for PresenceTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresenceTokens").finish_non_exhaustive()
    }
}

impl PresenceTokens {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Grant>> {
        self.grants.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn mint(
        &self,
        action: StepUpAction,
        object: &str,
        now: Instant,
    ) -> AppResult<String> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        cc_crypto_core::fill_random(&mut bytes[..]).map_err(|e| AppError::Crypto(e.to_string()))?;
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes[..]);
        let mut grants = self.lock();
        grants.retain(|_, grant| grant.expires > now);
        while grants.len() >= MAX_TOKENS {
            let Some(oldest) = grants
                .iter()
                .min_by_key(|(_, grant)| grant.expires)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            grants.remove(&oldest);
        }
        grants.insert(
            token.clone(),
            Grant {
                action,
                object: object.to_owned(),
                expires: now + PRESENCE_TTL,
            },
        );
        Ok(token)
    }

    /// Redeems `token` for `action` on `object`. Any attempt uses the token
    /// up, a mismatching one included.
    pub(crate) fn redeem(
        &self,
        token: &str,
        action: StepUpAction,
        object: &str,
        now: Instant,
    ) -> bool {
        let Some(grant) = self.lock().remove(token) else {
            return false;
        };
        grant.action == action && grant.object == object && now < grant.expires
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }
}

fn check_object(object: &str) -> AppResult<()> {
    if object.len() > MAX_OBJECT || object.chars().any(char::is_control) {
        return Err(AppError::invalid("object", "unexpected step-up object"));
    }
    Ok(())
}

/// The device unlock record when it may confirm actions: on, OS-protected
/// and the user's "confirm actions" preference on.
fn presence_record(s: &Session) -> Option<DeviceUnlockRecord> {
    s.keyfile()
        .and_then(|k| k.device_unlock)
        .filter(|r| r.confirm_actions && r.protection == "system")
}

impl AppCore {
    /// Whether this device can confirm actions with the OS check now.
    pub(crate) fn presence_offered(&self, s: &Session) -> bool {
        let status = self.inner.ctx.device_keys.status();
        presence_record(s).is_some()
            && status.available()
            && status.protection == UnlockProtection::System
    }

    /// The OS prompt (with `reason`) releases the device unlock key; the
    /// core checks it against the local unlock envelope and returns a
    /// single-use token for `action` on `object`, valid for 30 s.
    /// `passphrase_required` when the OS check is not offered for it.
    pub async fn verify_user_presence(
        &self,
        action: String,
        object: String,
        reason: String,
    ) -> AppResult<String> {
        let (s, u, action, record) = self.presence_target(&action, &object).await?;
        if record.external {
            return Err(AppError::from(OsAuthError::ExternalRequired));
        }
        let keys = self.inner.ctx.device_keys.clone();
        let handle = record.handle.0.clone();
        let reason = crate::device_unlock::reason_or_default(reason);
        let released = blocking(move || Ok(keys.release(&handle, &reason))).await?;
        let material = match released {
            Ok(m) => m,
            Err(OsAuthError::Invalidated) => {
                self.invalidated(&s).await;
                return Err(AppError::OsAuthInvalidated);
            }
            Err(e) => return Err(e.into()),
        };
        self.presence_token(&s, &u, &record, &material, action, &object)
            .await
    }

    /// Android: [`Self::verify_user_presence`] with the material the app's
    /// `BiometricPrompt` `CryptoObject` decrypted.
    pub async fn verify_user_presence_with_material(
        &self,
        action: String,
        object: String,
        material: Vec<u8>,
    ) -> AppResult<String> {
        let material = Zeroizing::new(material);
        crate::device_unlock::check_material(&material)?;
        let (s, u, action, record) = self.presence_target(&action, &object).await?;
        self.presence_token(&s, &u, &record, &material, action, &object)
            .await
    }

    /// Step-ups the app enforces itself (UI-only confirmations): checks
    /// the passphrase or redeems a presence token right away.
    pub async fn confirm_step_up(
        &self,
        action: String,
        object: String,
        proof: StepUp,
    ) -> AppResult<()> {
        let action = StepUpAction::parse(&action)?;
        check_object(&object)?;
        self.check_step_up(action, &object, proof).await
    }

    /// "Confirm actions with biometrics" for the open profile on this
    /// device (on by default once device unlock is on).
    pub async fn set_confirm_actions_with_presence(&self, enabled: bool) -> AppResult<()> {
        let s = self.session().await?;
        s.unlocked().await?;
        if s.keyfile().and_then(|k| k.device_unlock).is_none() {
            return Err(AppError::Unsupported(
                "Device unlock is disabled for this profile".into(),
            ));
        }
        s.update_keyfile(|k| {
            if let Some(record) = k.device_unlock.as_mut() {
                record.confirm_actions = enabled;
            }
        })
        .await
    }

    /// A core-enforced step-up: the passphrase, or a presence token minted
    /// for exactly this `action` and `object` in this unlock lifetime.
    pub(crate) async fn check_step_up(
        &self,
        action: StepUpAction,
        object: &str,
        proof: StepUp,
    ) -> AppResult<()> {
        match proof {
            StepUp::Passphrase(passphrase) => {
                if self.verify_passphrase(passphrase).await? {
                    Ok(())
                } else {
                    Err(AppError::WrongPassphrase)
                }
            }
            StepUp::Presence(token) => {
                if action.passphrase_only() {
                    return Err(AppError::PassphraseRequired);
                }
                let (s, u) = self.unlocked().await?;
                // Redeem first: a token is used up by any attempt.
                let redeemed = u.presence.redeem(&token, action, object, Instant::now());
                if redeemed && presence_record(&s).is_some() {
                    Ok(())
                } else {
                    Err(AppError::PassphraseRequired)
                }
            }
        }
    }

    async fn presence_target(
        &self,
        action: &str,
        object: &str,
    ) -> AppResult<(
        Arc<Session>,
        Arc<Unlocked>,
        StepUpAction,
        DeviceUnlockRecord,
    )> {
        let action = StepUpAction::parse(action)?;
        check_object(object)?;
        if action.passphrase_only() {
            return Err(AppError::PassphraseRequired);
        }
        let (s, u) = self.unlocked().await?;
        if !self.presence_offered(&s) {
            return Err(AppError::PassphraseRequired);
        }
        let record = presence_record(&s).ok_or(AppError::PassphraseRequired)?;
        Ok((s, u, action, record))
    }

    async fn presence_token(
        &self,
        s: &Arc<Session>,
        u: &Arc<Unlocked>,
        record: &DeviceUnlockRecord,
        material: &[u8],
        action: StepUpAction,
        object: &str,
    ) -> AppResult<String> {
        let kf = s.keyfile().ok_or(AppError::PassphraseRequired)?;
        let key = UnlockKey::from_material(material)?;
        match unlock_with_unlock_key(
            kf.vault_id,
            &record.envelope,
            &key,
            &keyfile::unlock_context(s.profile_id),
        ) {
            Ok(vault) if vault.vault_id() == u.vault_id => vault.lock(),
            Ok(vault) => {
                vault.lock();
                return Err(AppError::PassphraseRequired);
            }
            Err(VaultError::DeviceNotAuthorized) => {
                self.invalidated(s).await;
                return Err(AppError::OsAuthInvalidated);
            }
            Err(e) => return Err(e.into()),
        }
        // The vault may have been locked or re-unlocked during the prompt.
        match s.unlocked_opt().await {
            Some(current) if Arc::ptr_eq(&current, u) => {}
            _ => return Err(AppError::VaultLocked),
        }
        u.presence.mint(action, object, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_single_use_bound_and_expire() {
        let tokens = PresenceTokens::default();
        let now = Instant::now();
        let object = "session-1";
        let t = tokens
            .mint(StepUpAction::DbProductionWrite, object, now)
            .unwrap();
        assert_eq!(t.len(), 43, "32 random bytes, base64url");
        assert!(tokens.redeem(&t, StepUpAction::DbProductionWrite, object, now));
        // Single use.
        assert!(!tokens.redeem(&t, StepUpAction::DbProductionWrite, object, now));

        // Bound to the action kind and the object; a wrong attempt uses it up.
        let t = tokens
            .mint(StepUpAction::DbProductionWrite, object, now)
            .unwrap();
        assert!(!tokens.redeem(&t, StepUpAction::DockerReveal, object, now));
        assert!(!tokens.redeem(&t, StepUpAction::DbProductionWrite, object, now));
        let t = tokens
            .mint(StepUpAction::DbProductionWrite, object, now)
            .unwrap();
        assert!(!tokens.redeem(&t, StepUpAction::DbProductionWrite, "session-2", now));
        assert!(!tokens.redeem(&t, StepUpAction::DbProductionWrite, object, now));

        // Expiry after 30 s.
        let t = tokens.mint(StepUpAction::Confirm, "", now).unwrap();
        assert!(tokens.redeem(&t, StepUpAction::Confirm, "", now + Duration::from_secs(29)));
        let t = tokens.mint(StepUpAction::Confirm, "", now).unwrap();
        assert!(!tokens.redeem(&t, StepUpAction::Confirm, "", now + PRESENCE_TTL));

        // Unknown tokens never pass.
        assert!(!tokens.redeem("", StepUpAction::Confirm, "", now));
        assert!(!tokens.redeem("forged", StepUpAction::Confirm, "", now));
    }

    #[test]
    fn the_store_stays_small() {
        let tokens = PresenceTokens::default();
        let now = Instant::now();
        let first = tokens.mint(StepUpAction::Confirm, "", now).unwrap();
        for i in 0..40 {
            tokens
                .mint(
                    StepUpAction::Confirm,
                    "",
                    now + Duration::from_millis(i + 1),
                )
                .unwrap();
        }
        assert!(tokens.len() <= MAX_TOKENS);
        assert!(!tokens.redeem(&first, StepUpAction::Confirm, "", now));
        // Expired ones are dropped on the next mint.
        tokens
            .mint(StepUpAction::Confirm, "", now + Duration::from_secs(60))
            .unwrap();
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn passphrase_only_actions_and_wire_names() {
        for action in PASSPHRASE_ONLY {
            assert!(action.passphrase_only());
            assert_eq!(StepUpAction::parse(action.wire()).unwrap(), *action);
        }
        for action in [
            StepUpAction::Confirm,
            StepUpAction::DbProductionWrite,
            StepUpAction::DockerReveal,
            StepUpAction::DockerCompose,
        ] {
            assert!(!action.passphrase_only());
            assert_eq!(StepUpAction::parse(action.wire()).unwrap(), action);
        }
        assert_eq!(StepUpAction::ALL.len(), PASSPHRASE_ONLY.len() + 4);
        assert!(StepUpAction::parse("unlock_everything").is_err());
    }

    #[test]
    fn a_step_up_needs_exactly_one_proof() {
        assert!(matches!(
            StepUp::from_parts(None, None),
            Err(AppError::PassphraseRequired)
        ));
        assert!(matches!(
            StepUp::from_parts(Some("p".into()), Some("t".into())),
            Err(AppError::InvalidInput { .. })
        ));
        assert!(matches!(
            StepUp::from_parts(Some("p".into()), None),
            Ok(StepUp::Passphrase(_))
        ));
        assert!(matches!(
            StepUp::from_parts(None, Some("t".into())),
            Ok(StepUp::Presence(_))
        ));
        assert_eq!(
            format!("{:?}", StepUp::Passphrase("secret".into())),
            "StepUp::Passphrase(<redacted>)"
        );
    }
}
