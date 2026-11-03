//! Local, independently authenticated KDBX composite keys. Never synced.
use crate::{
    app::AppCore,
    error::{AppError, AppResult},
    keyfile::PasswordVaultKeyRecord,
    secrets::blocking,
};
use cc_crypto_core::{open_password_vault_key, seal_password_vault_key, UnlockKey};
use cc_platform_core::UnlockProtection;
use cc_protocol::Bytes;
use zeroize::Zeroizing;

impl AppCore {
    /// `info`, `enable`, `release`, `forget`. An id is SHA-256 of the canonical
    /// source identity, never a user-supplied OS credential name.
    pub async fn password_vault_key(
        &self,
        action: String,
        id: String,
        method: String,
        secret: Vec<u8>,
        pin: Option<String>,
    ) -> AppResult<Vec<u8>> {
        let secret = Zeroizing::new(secret);
        let pin = pin.map(Zeroizing::new);
        if id.len() != 64
            || !id.bytes().all(|b| b.is_ascii_hexdigit())
            || !matches!(method.as_str(), "hello" | "security_key")
        {
            return Err(AppError::invalid("password_vault", "source or method"));
        }
        let s = self.session().await?;
        let u = s.unlocked().await?;
        let name = format!("{id}.{method}");
        let context = format!("kdbx:{}:{name}", s.profile_id).into_bytes();
        let old = s
            .keyfile()
            .and_then(|k| k.password_vault_keys.get(&name).cloned());
        let device = self.inner.ctx.device_keys.clone();
        let keys = self.security_keys();
        let external = method == "hello" && device.status().external;
        let available = if method == "hello" {
            let status = device.status();
            status.available() && status.protection == UnlockProtection::System
        } else {
            keys.pin().is_some()
        };
        if action == "info" {
            return serde_json::to_vec(&serde_json::json!({"enabled":old.is_some(), "available":available, "external": external, "app_pin": keys.pin() == Some(cc_platform_core::SecurityKeyPin::App)})).map_err(|_| AppError::internal("kdbx info"));
        }
        // Android's CryptoObject prompt lives in the Activity. As with vault
        // device unlock, possession of material is checked cryptographically;
        // no boolean authentication result can release a composite key.
        if external && action == "external_handle" {
            let _guard = s.hold_unlocked(&u).await?;
            return old
                .map(|r| r.handle.0)
                .ok_or_else(|| AppError::invalid("password_vault", "not enrolled"));
        }
        if action == "forget" {
            let _guard = s.hold_unlocked(&u).await?;
            s.update_keyfile(|k| {
                k.password_vault_keys.remove(&name);
            })
            .await?;
            if method == "hello" {
                if let Some(r) = old {
                    blocking(move || device.destroy(&r.handle.0).map_err(AppError::from)).await?;
                }
            }
            return Ok(Vec::new());
        }
        if !available {
            return Err(AppError::Unsupported(
                "KDBX authenticator unavailable".into(),
            ));
        }
        if external && action == "enable_external" {
            // composite key (32) || OS material (32) || opaque handle (1..4096).
            if !(65..=4160).contains(&secret.len()) || !s.has_keyfile() {
                return Err(AppError::invalid("password_vault", "external key payload"));
            }
            let _guard = s.hold_unlocked(&u).await?;
            let envelope = seal_password_vault_key(
                &UnlockKey::from_material(&secret[32..64])?,
                u.vault_id,
                &context,
                &secret[..32],
            )?;
            s.update_keyfile(|k| {
                k.password_vault_keys.insert(
                    name,
                    PasswordVaultKeyRecord {
                        handle: Bytes::new(secret[64..].to_vec()),
                        salt: Bytes::new(vec![]),
                        envelope,
                    },
                );
            })
            .await?;
            return Ok(vec![]);
        }
        if external && action == "release_external" {
            if secret.len() != 32 {
                return Err(AppError::invalid(
                    "password_vault",
                    "external material length",
                ));
            }
            let _guard = s.hold_unlocked(&u).await?;
            let record = old.ok_or_else(|| AppError::invalid("password_vault", "not enrolled"))?;
            return Ok(open_password_vault_key(
                &UnlockKey::from_material(&secret)?,
                u.vault_id,
                &context,
                &record.envelope,
            )?
            .to_vec());
        }
        if external || action.ends_with("_external") || action == "external_handle" {
            return Err(AppError::Unsupported("KDBX authenticator operation".into()));
        }
        let reason = "Unlock your KDBX password database".to_owned();
        if action == "enable" {
            if secret.len() != 32 || !s.has_keyfile() {
                return Err(AppError::invalid("password_vault", "key length or profile"));
            }
            let label = format!("kdbx.{}.{}", s.profile_id, uuid::Uuid::new_v4());
            let (handle, salt, material) = if method == "hello" {
                let dev = device.clone();
                let k =
                    blocking(move || dev.create(&label, &reason).map_err(AppError::from)).await?;
                (k.handle, vec![], k.material)
            } else {
                let mut salt = [0u8; 32];
                cc_crypto_core::fill_random(&mut salt)
                    .map_err(|e| AppError::Crypto(e.to_string()))?;
                blocking(move || {
                    let p = pin.as_deref().map(String::as_str);
                    let handle = keys
                        .register("ConsoleCrypt KDBX", p)
                        .map_err(crate::security_key::failure)?;
                    let material = keys
                        .prf(&handle, &salt, p)
                        .map_err(crate::security_key::failure)?;
                    Ok((handle, salt.to_vec(), Zeroizing::new(material.to_vec())))
                })
                .await?
            };
            let result = async {
                if !std::sync::Arc::ptr_eq(&self.session().await?, &s) {
                    return Err(AppError::VaultLocked);
                }
                let _guard = s.hold_unlocked(&u).await?;
                let envelope = seal_password_vault_key(
                    &UnlockKey::from_material(&material)?,
                    u.vault_id,
                    &context,
                    &secret,
                )?;
                let record = PasswordVaultKeyRecord {
                    handle: Bytes::new(handle.clone()),
                    salt: Bytes::new(salt),
                    envelope,
                };
                s.update_keyfile(|k| {
                    k.password_vault_keys.insert(name, record);
                })
                .await
            }
            .await;
            if result.is_err() && method == "hello" {
                let _ = device.destroy(&handle);
            }
            result?;
            if method == "hello" {
                if let Some(r) = old {
                    let _ =
                        blocking(move || device.destroy(&r.handle.0).map_err(AppError::from)).await;
                }
            }
            return Ok(vec![]);
        }
        if action != "release" {
            return Err(AppError::invalid("password_vault", "action"));
        }
        let record = old.ok_or_else(|| AppError::invalid("password_vault", "not enrolled"))?;
        let material = if method == "hello" {
            let handle = record.handle.0.clone();
            blocking(move || device.release(&handle, &reason).map_err(AppError::from)).await?
        } else {
            let salt: [u8; 32] = record
                .salt
                .0
                .as_slice()
                .try_into()
                .map_err(|_| AppError::internal("kdbx salt"))?;
            let handle = record.handle.0.clone();
            blocking(move || {
                keys.prf(&handle, &salt, pin.as_deref().map(String::as_str))
                    .map(|material| Zeroizing::new(material.to_vec()))
                    .map_err(crate::security_key::failure)
            })
            .await?
        };
        if !std::sync::Arc::ptr_eq(&self.session().await?, &s) {
            return Err(AppError::VaultLocked);
        }
        let _guard = s.hold_unlocked(&u).await?;
        if s.keyfile()
            .and_then(|k| k.password_vault_keys.get(&name).cloned())
            .as_ref()
            != Some(&record)
        {
            return Err(AppError::invalid("password_vault", "enrollment changed"));
        }
        let opened = open_password_vault_key(
            &UnlockKey::from_material(&material)?,
            u.vault_id,
            &context,
            &record.envelope,
        )?;
        Ok(opened.to_vec())
    }
}
