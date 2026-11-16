//! Explicit original saved Password refs. No global resolver/default/fallback.
use super::*;
use crate::working_set::VaultCredentialMetadata;
use cc_agent_node_wire::{
    CollectorCredentialScope, CollectorCredentialTarget, CollectorRecipientAttestation,
};
use cc_crypto_core::{
    collector_credential_binding, CollectorCredentialEnvelope, PreparedCollectorCredentialOwner,
};
use sha2::Digest as _;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    target: CollectorCredentialTarget,
    credential_id: String,
    expected_credential_revision: String,
}
#[derive(Clone)]
pub(super) struct Original {
    pub(super) target: CollectorCredentialTarget,
    metadata: VaultCredentialMetadata,
    revision: i64,
}
impl Original {
    pub(super) fn view(&self) -> Value {
        json!({"target":self.target,"credentialId":self.metadata.credential(),"credentialRevision":self.revision.to_string()})
    }
    fn check_held(&self, u: &crate::session::Unlocked) -> AppResult<()> {
        if u.working()
            .vault_credential_metadata(self.metadata.credential())
            .as_ref()
            != Some(&self.metadata)
            || self
                .metadata
                .fences()
                .iter()
                .any(|(id, rev)| u.working().revision_of(*id) != Some(*rev))
        {
            return Err(unavailable());
        }
        Ok(())
    }
    pub(super) async fn check(&self, context: &Context) -> AppResult<()> {
        let app = context.app()?;
        context
            .scope
            .with_resources(&app, |_, u, _| {
                if u.working()
                    .vault_credential_metadata(self.metadata.credential())
                    .as_ref()
                    != Some(&self.metadata)
                    || self
                        .metadata
                        .fences()
                        .iter()
                        .any(|(id, rev)| u.working().revision_of(*id) != Some(*rev))
                {
                    return Err(unavailable());
                }
                Ok(())
            })
            .await
    }
}
pub(super) async fn capture(context: &Context, requests: &[Request]) -> AppResult<Vec<Original>> {
    if requests.len() > 2 {
        return Err(unavailable());
    }
    let app = context.app()?;
    context
        .scope
        .with_resources(&app, |_, u, _| {
            let mut result = Vec::new();
            for request in requests {
                let id = crate::dto::parse_id("credential", &request.credential_id)?;
                let rev = revision(&request.expected_credential_revision)?;
                if id.to_string() != request.credential_id
                    || u.working().revision_of(id) != Some(rev)
                    || result
                        .iter()
                        .any(|old: &Original| old.target == request.target)
                {
                    return Err(unavailable());
                }
                let metadata = u
                    .working()
                    .vault_credential_metadata(id)
                    .ok_or_else(unavailable)?;
                let username = metadata.username().ok_or_else(unavailable)?;
                if username.is_empty()
                    || username.len() > 256
                    || username.chars().any(char::is_control)
                {
                    return Err(unavailable());
                }
                result.push(Original {
                    target: request.target.clone(),
                    metadata,
                    revision: rev,
                });
            }
            Ok(result)
        })
        .await
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RecipientReference {
    version: u8,
    namespace_id: String,
    attestation: Signed<CollectorRecipientAttestation>,
}
impl RecipientReference {
    pub(super) fn checked(
        &self,
        facts: &ServiceReviewFacts,
        base: &Signed<CollectorServiceDelegation>,
        context: &Context,
    ) -> AppResult<()> {
        let r = &self.attestation.body;
        cc_agent_node_wire::verify_collector_recipient(
            &facts.pin,
            &facts.certificate,
            base,
            &self.attestation,
            r.challenge,
            context.scope.now()?,
        )
        .map_err(|_| unavailable())?;
        if self.version != 1
            || self.namespace_id != hex::encode(r.original_start_nonce.as_bytes())
            || self.namespace_id.len() != 32
        {
            return Err(unavailable());
        }
        facts.live(context)
    }
}
async fn seal_one(
    context: &Context,
    facts: &ServiceReviewFacts,
    base: &Signed<CollectorServiceDelegation>,
    proof: &Signed<CollectorRecipientAttestation>,
    original: &Original,
) -> AppResult<CollectorCredentialEnvelope> {
    original.check(context).await?;
    facts.live(context)?;
    let username = original
        .metadata
        .username()
        .ok_or_else(unavailable)?
        .to_owned();
    let target_url = match &original.target {
        CollectorCredentialTarget::Receiver {} => {
            facts.catalog.manifest["receiver"]["url"].as_str()
        }
        CollectorCredentialTarget::Scrape { target_id } => facts.catalog.manifest["targets"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row["id"].as_str() == Some(target_id.as_str()))
            })
            .and_then(|row| row["url"].as_str()),
    }
    .ok_or_else(unavailable)?;
    let now = context.scope.now()?;
    let device = context.scope.with_current_resources(|_, _, id| {
        ResourceId::new(&id.device_id().to_string()).map_err(|_| unavailable())
    })?;
    let scope = CollectorCredentialScope {
        header: facts.policy.header.clone(),
        scope_id: ResourceId::new(&Uuid::new_v4().to_string()).map_err(|_| unavailable())?,
        owner_device_id: device,
        service_id: facts.policy.service_id,
        base_delegation_hash: facts.policy.digest().map_err(|_| unavailable())?,
        original_start_nonce: proof.body.original_start_nonce,
        recipient_attestation_digest: proof.body.digest().map_err(|_| unavailable())?,
        recipient_challenge: proof.body.challenge,
        policy_revision: facts.policy.policy_revision,
        revocation_epoch: facts.policy.revocation_epoch,
        agent_id: facts.policy.agent_id,
        key_epoch: facts.policy.key_epoch,
        certificate_hash: facts.policy.certificate_hash,
        collector_id: facts.policy.collector_id,
        manifest_sha256: facts.policy.manifest_sha256,
        target: original.target.clone(),
        endpoint_sha256: Hash::from_bytes(sha2::Sha256::digest(target_url.as_bytes()).into()),
        username,
        credential_id: ResourceId::new(&original.metadata.credential().to_string())
            .map_err(|_| unavailable())?,
        credential_revision: original.revision.try_into().map_err(|_| unavailable())?,
        recipient_id: proof.body.recipient_id,
        recipient_key_sha256: Hash::from_bytes(
            sha2::Sha256::digest(proof.body.recipient_public_key.as_bytes()).into(),
        ),
        recipient_key_epoch: proof.body.recipient_key_epoch,
        recipient_certificate_epoch: proof.body.recipient_certificate_epoch,
        issued_at: now.0,
        expires_at: facts.policy.expires_at,
    };
    let prepared = PreparedCollectorCredentialOwner::prepare(
        &facts.pin,
        &facts.certificate,
        base,
        proof,
        proof.body.challenge,
        scope,
        &facts.catalog.manifest,
        now,
    )
    .map_err(|_| unavailable())?;
    let signed = {
        let app = context.app()?;
        context
            .scope
            .with_loaded(&app, |id| {
                consent_live(context.scope.deadline())?;
                facts.live(context)?;
                id.sign_agent_collector_credential_scope_v1(prepared, context.scope.now()?)
                    .map_err(|_| unavailable())
            })
            .await?
    };
    let writer = {
        let app = context.app()?;
        context
            .scope
            .with_resources(&app, |_, u, _| {
                original.check_held(u)?;
                Ok(u.writer.clone())
            })
            .await?
    };
    let secret = writer
        .read_secret(original.metadata.secret())
        .await
        .map_err(|_| unavailable())?;
    drop(writer);
    if secret.id != original.metadata.secret()
        || secret.kind != cc_models::secret::SecretKind::Password
        || !cc_crypto_core::collector_basic_password_supported(
            secret.value.expose_secret().as_bytes(),
        )
    {
        return Err(unavailable());
    }
    let app = context.app()?;
    let capsule = context
        .scope
        .with_loaded(&app, |id| {
            consent_live(context.scope.deadline())?;
            facts.live(context)?;
            let now = context.scope.now()?;
            let binding = collector_credential_binding(&signed, &facts.pin, now)
                .map_err(|_| unavailable())?;
            let capsule = id
                .seal_service_credential(
                    binding,
                    Zeroizing::new(secret.value.expose_secret().as_bytes().to_vec()),
                    proof.body.recipient_public_key.as_bytes(),
                    now.0.seconds(),
                    facts.policy.expires_at.seconds(),
                )
                .map_err(|_| unavailable())?;
            consent_live(context.scope.deadline())?;
            facts.live(context)?;
            Ok(capsule)
        })
        .await?;
    CollectorCredentialEnvelope::new(signed, capsule).map_err(|_| unavailable())
}
pub(super) async fn deliver(
    context: &Context,
    facts: &ServiceReviewFacts,
    peer: &mut transport::Peer,
    base: &Signed<CollectorServiceDelegation>,
) -> AppResult<()> {
    if facts.credentials.is_empty() {
        return Ok(());
    }
    let reference: RecipientReference = peer
        .exchange_weak(&context.core, "collectorServiceRecipient", json!({}), None)
        .await?;
    reference.checked(facts, base, context)?;
    for (slot, original) in facts.credentials.iter().enumerate() {
        reference.checked(facts, base, context)?;
        original.check(context).await?;
        let envelope = seal_one(context, facts, base, &reference.attestation, original).await?;
        envelope
            .check_metadata_only(
                &facts.pin,
                &facts.certificate,
                base,
                &reference.attestation,
                reference.attestation.body.challenge,
                &facts.catalog.manifest,
                context.scope.now()?,
            )
            .map_err(|_| unavailable())?;
        let response:HandoffAck=peer.exchange_weak(&context.core,"deliverCollectorServiceCredential",
            json!({"version":1,"namespaceId":reference.namespace_id,"slot":slot,"envelope":envelope}),None).await?;
        if response.kind != "collectorCredentialHandoff"
            || response.slot != slot
            || response.service_id != facts.policy.service_id
            || response.delegation_hash != facts.policy.digest().map_err(|_| unavailable())?
            || !response.encrypted_accepted
            || response.material_state != "notObserved"
            || response.runtime_state != "notObserved"
        {
            return Err(unavailable());
        }
        original.check(context).await?;
        facts.live(context)?;
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HandoffAck {
    kind: String,
    slot: usize,
    service_id: ResourceId,
    delegation_hash: Hash,
    encrypted_accepted: bool,
    material_state: String,
    runtime_state: String,
}

impl AppCore {
    /// Explicit LOCAL metadata choice read. No SSH, secret value, implicit
    /// prepare, chosen credential, key material or collector authority here.
    pub async fn agent_collector_service_credential_choices_json(
        self,
        raw: &str,
    ) -> AppResult<String> {
        let frame: Frame = parse(raw)?;
        let context = Context::capture(&self, frame).await?;
        drop(self);
        bounded_original(&context.scope, async {
            let app = context.app()?;
            let rows = context
                .scope
                .with_resources(&app, |_, u, _| {
                    u.working()
                        .collector_service_credential_choices()
                        .ok_or_else(unavailable)
                })
                .await?;
            drop(app);
            context.publish(
                json!({"kind":"finiteCollectorServiceCredentialChoices",
                "credentials":rows.into_iter().map(|(id,revision,name)|json!({"credentialId":id,
                    "credentialRevision":revision.to_string(),"name":name})).collect::<Vec<_>>()}),
                16384,
            )
        })
        .await
    }
}

mod service_lifetime;
pub(super) use service_lifetime::{prepare_and_seal_original,OriginalLifetimeSet,prepare_original_from_peer};
