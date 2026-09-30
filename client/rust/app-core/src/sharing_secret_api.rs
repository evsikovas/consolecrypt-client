//! Explicit secret actions. Plaintext never becomes an ordinary item DTO.
use super::*;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::VaultObject;

impl AppCore {
    pub async fn sharing_edit_secret(
        &self,
        share_id: String,
        value: SecretValue,
    ) -> AppResult<SharingOutboxDto> {
        if value.is_empty() {
            return Err(AppError::invalid("secret", "must not be empty"));
        }
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), false).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        env.ensure_not_pending(id)?;
        let binding = env.binding(id)?;
        if binding.kind != SharedItemKind::Secret {
            return Err(invalid("the shared item has another kind"));
        }
        env.require_kind(binding.kind)?;
        let store = env.store()?;
        let previous = env
            .wait(async { store.load(&binding).await.map_err(integrity) })
            .await?
            .ok_or_else(|| invalid("shared owner has not been accepted"))?;
        let mut projection = env
            .wait(async {
                store
                    .open_cached(&previous, env.pin.device, env.identity.as_ref())
                    .await
                    .map_err(integrity)
            })
            .await?
            .ok_or_else(|| invalid("deleted shared items cannot be changed"))?;
        let SharedProjection::Secret(secret) = &mut projection else {
            return Err(invalid("the shared item has another kind"));
        };
        secret.value = value;
        // Validate the typed projection before producing a signed ciphertext.
        let encoded = projection.encode()?;
        let projection = SharedProjection::decode(binding.kind, encoded.as_slice())?;
        let manifest = trusted_manifest(&previous)?;
        let mut context = previous.state().revision.signed.mutation.context.clone();
        context.revision = context
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("shared revision limit reached"))?;
        let state = SharedItemState {
            access: previous.state().access.clone(),
            revision: make_revision(&env, &manifest, context, Some(&projection), Some(&previous))?,
        };
        self.sharing_check(&env).await?;
        env.queue(QueuedKind::Put, binding, state, Some(&previous))?;
        self.sharing_save(&mut env).await?;
        Ok(env.outbox_dto())
    }

    /// An explicit independent private credential. The old share remains
    /// separate and revocation cannot erase this already accepted copy.
    pub async fn sharing_copy_secret_credential(
        &self,
        share_id: String,
    ) -> AppResult<crate::CredentialDto> {
        self.sharing_with_verified_projection(
            share_id,
            SharedItemKind::Secret,
            |u, _, projection| {
                Box::pin(async move {
                    let SharedProjection::Secret(public) = projection else {
                        return Err(invalid("the shared item has another kind"));
                    };
                    let kind = match public.secret_kind {
                        SecretKind::Password => CredentialKind::Password,
                        SecretKind::SshPrivateKey => CredentialKind::SshPrivateKey,
                        _ => return Err(invalid("this secret must be used explicitly")),
                    };
                    let mut credential = Credential::new(public.name.clone(), kind);
                    if kind == CredentialKind::SshPrivateKey {
                        // Inspect without decrypting or removing original key
                        // protection. A passphrase remains a separate action.
                        let info = cc_ssh_core::keys::inspect_private_key(
                            public.value.expose_secret(),
                            None,
                        )?;
                        cc_ssh_core::keys::apply_key_metadata(&mut credential, &info);
                    }
                    let secret = Secret::new(public.secret_kind, public.value.clone());
                    credential.secret_id = Some(secret.id);
                    u.writer.put(VaultObject::Secret(secret)).await?;
                    crate::inventory::validate_refs(
                        u.working(),
                        &VaultObject::Credential(credential.clone()),
                    )?;
                    let dto = crate::CredentialDto::from_model(&credential);
                    u.writer.put(VaultObject::Credential(credential)).await?;
                    Ok(dto)
                })
            },
        )
        .await
    }
}
