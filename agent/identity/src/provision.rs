//! Explicit node-local initial identity generation. This is not enrollment,
//! recovery/rotation, owner signing, a credential resolver or a generic signer.
use crate::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Public hello only. Possessing these fields grants no scopes or trust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicNodeIdentity {
    pub format: u32,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub public_key: PublicKey,
    pub key_fingerprint: Hash,
}

/// Linux-only, explicit one-time provisioning beneath a private existing parent.
/// A pre-existing directory must already contain a valid matching key. Missing,
/// partial or corrupt state is never repaired or silently replaced. Only public
/// metadata is returned; private material has no serialization/signing API.
pub fn provision_node(state_dir: &Path, agent_id: AgentId) -> Result<PublicNodeIdentity> {
    #[cfg(target_os = "linux")]
    {
        linux::provision(state_dir, agent_id)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state_dir, agent_id);
        Err(Error::CapabilityDenied)
    }
}

/// Noncreating public identity observation for the fixed enrollment role.
/// Existing key/guard/selection only; missing or partial state is never repaired.
pub fn inspect_existing_node_identity(directory: &Path, agent: AgentId) -> Result<PublicNodeIdentity> {
    #[cfg(target_os = "linux")]
    { linux::inspect_existing(directory, agent) }
    #[cfg(not(target_os = "linux"))]
    { let _ = (directory, agent); Err(Error::CapabilityDenied) }
}

/// Construct only the enrollment claim for this existing local key. All owner
/// authority is pinned independently. This does not consume a token, register a
/// device, issue scopes or expose a general-purpose signing API.
pub fn create_node_claim(
    state_dir: &Path,
    agent_id: AgentId,
    owner_pin: &OwnerPin,
    offer: Signed<Offer>,
    now: Now,
) -> Result<Signed<Claim>> {
    #[cfg(target_os = "linux")]
    {
        linux::claim(state_dir, agent_id, owner_pin, offer, now)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state_dir, agent_id, owner_pin, offer, now);
        Err(Error::CapabilityDenied)
    }
}

/// Typed owner grant/certificate context for node-principal proof of possession.
/// Deserializing this DTO creates no authority. A desktop/client never receives
/// this node key; remote client-device/relay authorization is a separate flow.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeRequestBinding {
    pub certificate: Signed<Certificate>,
    pub lease: Signed<Lease>,
    pub freshness: Signed<Freshness>,
    pub operation: Operation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedRotationInput {
    pub previous_certificate: Signed<Certificate>,
    pub certificate: Signed<Certificate>,
}
/// The value of the existing Rotate mutation, never a commit/activation ack.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedRotationProofs {
    pub certificate: Signed<Certificate>,
    pub old: Signed<RotationProof>,
    pub new: Signed<NewKeyProof>,
}
/// Sign only old/new possession proofs for an exact owner-approved prepared key.
/// No key/file/DB mutation, activation, raw payload signing or key export.
pub fn sign_prepared_node_rotation(
    directory: &Path,
    agent_id: AgentId,
    rotation_id: &str,
    pin: &OwnerPin,
    input: PreparedRotationInput,
    now: Now,
) -> Result<PreparedRotationProofs> {
    #[cfg(target_os = "linux")]
    {
        linux::rotation(directory, agent_id, rotation_id, pin, input, now)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (directory, agent_id, rotation_id, pin, input, now);
        Err(Error::CapabilityDenied)
    }
}
/// CLOSED TunnelOpen counterpart. Same existing old/new possession domains,
/// actual private candidate and epoch1->2 guard; no owner grant or activation.
pub fn sign_prepared_tunnel_node_rotation(
    directory: &Path, agent_id: AgentId, rotation_id: &str, pin: &OwnerPin,
    input: PreparedRotationInput, now: Now,
) -> Result<PreparedRotationProofs> {
    #[cfg(target_os = "linux")]
    { linux::tunnel_rotation(directory, agent_id, rotation_id, pin, input, now) }
    #[cfg(not(target_os = "linux"))]
    { let _=(directory,agent_id,rotation_id,pin,input,now); Err(Error::CapabilityDenied) }
}
/// Sign only the exact telemetry lease/request for an existing initial node key.
/// No raw payload, private-key export, owner grant issuance or admission occurs.
/// An older-owner certificate body requires exact endorsement by the current
/// pinned owner's signed lease. Its historical wrapper is not verified here;
/// authoritative admission still requires the retained verified certificate.
pub fn create_node_request(
    state_dir: &Path,
    agent_id: AgentId,
    pin: &OwnerPin,
    binding: NodeRequestBinding,
    now: Now,
) -> Result<Signed<AgentRequest>> {
    #[cfg(target_os = "linux")]
    {
        linux::request(state_dir, agent_id, pin, binding, now)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state_dir, agent_id, pin, binding, now);
        Err(Error::CapabilityDenied)
    }
}

/// Explicit opt-in TunnelOpen-only claim for a separately owner-approved offer.
/// The legacy claim remains exact TelemetryRead; this never replaces a certificate.
pub fn create_tunnel_node_claim(
    directory: &Path,
    agent: AgentId,
    pin: &OwnerPin,
    offer: Signed<Offer>,
    now: Now,
) -> Result<Signed<Claim>> {
    #[cfg(target_os = "linux")]
    {
        linux::tunnel_claim(directory, agent, pin, offer, now)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (directory, agent, pin, offer, now);
        Err(Error::CapabilityDenied)
    }
}
/// Only an independently owner-signed full bound policy can request this proof.
/// No generic signing, Relay enrollment, key export or Telemetry capability widening.
pub fn create_tunnel_node_request(
    directory: &Path,
    agent: AgentId,
    pin: &OwnerPin,
    binding: NodeRequestBinding,
    policy: &TunnelRoutePolicy,
    now: Now,
) -> Result<Signed<AgentRequest>> {
    policy.check_binding(&binding)?;
    #[cfg(target_os = "linux")]
    {
        linux::tunnel_request(directory, agent, pin, binding, now)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (directory, agent, pin, binding, now);
        Err(Error::CapabilityDenied)
    }
}


/// V2 opt-in ORIGINAL lease only. Existing closed TunnelOpenBound signer,
/// current real Node key/pin/certificate and fresh OS nonce; no supplied nonce,
/// raw signing or new domain. Worker must retain THIS exact signed request
/// through the SAME TLS nonce/Device echo handshake before Actor resume.
pub fn create_original_tunnel_node_request(
    directory: &Path, agent: AgentId, pin: &OwnerPin,
    binding: NodeRequestBinding, policy: &TunnelRoutePolicyV2, now: Now,
) -> Result<Signed<AgentRequest>> {
    policy.check_binding(&binding)?;
    #[cfg(target_os = "linux")]
    { linux::tunnel_request(directory, agent, pin, binding, now) }
    #[cfg(not(target_os = "linux"))]
    { let _ = (directory, agent, pin, binding, now); Err(Error::CapabilityDenied) }
}


/// CLOSED MetricsCollect-only initial claim; exact installed capability is
/// checked by the fixed worker before this custody call. No scope selector.
pub fn create_metrics_node_claim(directory:&Path,agent:AgentId,pin:&OwnerPin,
    offer:Signed<Offer>,now:Now)->Result<Signed<Claim>> {
    #[cfg(target_os="linux")]
    { linux::metrics_claim(directory,agent,pin,offer,now) }
    #[cfg(not(target_os="linux"))]
    { let _=(directory,agent,pin,offer,now); Err(Error::CapabilityDenied) }
}
/// Existing AgentRequest domain, exact MetricsCollect resource and current
/// selected Node key. No owner signing, admission, supplied nonce or clock.
pub fn create_metrics_node_request(directory:&Path,agent:AgentId,pin:&OwnerPin,
    collector:ResourceId,binding:NodeRequestBinding,now:Now)->Result<Signed<AgentRequest>> {
    if binding.operation!=(Operation::MetricsCollect {collector}) {return Err(Error::ScopeDenied)}
    #[cfg(target_os="linux")]
    { linux::metrics_request(directory,agent,pin,binding,now) }
    #[cfg(not(target_os="linux"))]
    { let _=(directory,agent,pin,collector,binding,now); Err(Error::CapabilityDenied) }
}

pub fn create_exporter_node_claim(directory:&Path,agent:AgentId,pin:&OwnerPin,
    offer:Signed<Offer>,now:Now)->Result<Signed<Claim>> {
    #[cfg(target_os="linux")]
    { linux::exporter_claim(directory,agent,pin,offer,now) }
    #[cfg(not(target_os="linux"))]
    { let _=(directory,agent,pin,offer,now); Err(Error::CapabilityDenied) }
}
pub fn create_exporter_node_request(directory:&Path,agent:AgentId,pin:&OwnerPin,
    policy:&ManagedExporterPolicy,binding:NodeRequestBinding,now:Now)->Result<Signed<AgentRequest>> {
    policy.check_binding(&binding)?;
    #[cfg(target_os="linux")]
    { linux::exporter_request(directory,agent,pin,binding,now) }
    #[cfg(not(target_os="linux"))]
    { let _=(directory,agent,pin,policy,binding,now); Err(Error::CapabilityDenied) }
}

#[cfg(target_os = "linux")]
pub(crate) fn load_existing_under_guard(
    guard: &crate::node_prepare::Guard,
    agent_id: AgentId,
) -> Result<PublicNodeIdentity> {
    linux::load_existing(guard.directory(), agent_id)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use subtle::ConstantTimeEq;
    use zeroize::Zeroizing;

    include!("provision_collector_service.rs");
include!("provision_collector_credentials.rs");
include!("provision_collector_service_lifetime.rs");
    const MAGIC: &[u8; 16] = b"CC-AGENT-KEY-V1\0";
    const BYTES: usize = 148;
    const KEY_FILE: &str = "node-identity.key";
    fn id_string(id: AgentId) -> Result<String> {
        serde_json::to_value(id)
            .map_err(|_| Error::InvalidWire)?
            .as_str()
            .map(str::to_owned)
            .ok_or(Error::InvalidWire)
    }
    fn checksum(bytes: &[u8]) -> Hash {
        crate::wire::hash(b"consolecrypt.agent.node-key-file.v1", bytes)
    }
    fn public(agent_id: AgentId, key: PublicKey) -> PublicNodeIdentity {
        PublicNodeIdentity {
            format: 1,
            agent_id,
            key_epoch: 1,
            public_key: key,
            key_fingerprint: key.fingerprint(),
        }
    }
    pub(super) fn inspect_existing(directory: &Path, agent: AgentId) -> Result<PublicNodeIdentity> {
        let directory = crate::durable::absolute(directory)?;
        crate::durable::secure_path(&directory)?;
        let guard = crate::node_prepare::existing_read_lock(&directory)?;
        let public = load(&directory.join(KEY_FILE), agent)?;
        crate::node_prepare::check(&guard, Some(&public))?;
        let selected = crate::node_activation::selected(&guard, &public)?;
        let result = selected_public(&public, selected.as_ref());
        crate::node_prepare::guard_inode(&guard)?;
        Ok(result)
    }
    pub(super) fn provision(state_dir: &Path, agent_id: AgentId) -> Result<PublicNodeIdentity> {
        let directory = crate::durable::absolute(state_dir)?;
        let parent = directory.parent().ok_or(Error::Storage)?;
        crate::durable::secure_path(parent)?;
        let created = match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(_) => return Err(Error::Storage),
        };
        crate::durable::secure_path(&directory)?;
        #[cfg(test)]
        if created {
            crate::node_prepare::test_initial_directory_created()?;
        }
        let guard = if created {
            crate::node_prepare::lock(&directory)?
        } else {
            crate::node_prepare::read_lock(&directory)?
        };
        let path = directory.join(KEY_FILE);
        if !created {
            let public = load(&path, agent_id)?;
            crate::node_prepare::check(&guard, Some(&public))?;
            return Ok(selected_public(
                &public,
                crate::node_activation::selected(&guard, &public)?.as_ref(),
            ));
        }
        crate::node_prepare::check(&guard, None)?;
        let mut seed = Zeroizing::new([0u8; 32]);
        // Maintained OS entropy adapter; failure denies. No seeded test RNG,
        // environment/file entropy override, fallback or caller-provided seed.
        getrandom::fill(seed.as_mut()).map_err(|_| Error::Entropy)?;
        let signing = SigningKey::from_bytes(&seed);
        let key = PublicKey::from_bytes(signing.verifying_key().to_bytes());
        key.validate()?;
        let id = id_string(agent_id)?;
        if id.len() != 36 {
            return Err(Error::InvalidWire);
        }
        let mut record = Zeroizing::new([0u8; BYTES]);
        record[..16].copy_from_slice(MAGIC);
        record[16..52].copy_from_slice(id.as_bytes());
        record[52..84].copy_from_slice(seed.as_ref());
        record[84..116].copy_from_slice(key.as_bytes());
        let digest = checksum(&record[..116]);
        record[116..].copy_from_slice(digest.as_bytes());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| Error::Storage)?;
        file.write_all(record.as_ref())
            .map_err(|_| Error::Storage)?;
        file.sync_all().map_err(|_| Error::Storage)?;
        File::open(&directory)
            .and_then(|v| v.sync_all())
            .map_err(|_| Error::Storage)?;
        File::open(parent)
            .and_then(|v| v.sync_all())
            .map_err(|_| Error::Storage)?;
        // Re-read structural, key and private-permission checks before reporting
        // success. Seed/record buffers and SigningKey zeroize on drop.
        load(&path, agent_id)
    }
    struct Material {
        public: PublicNodeIdentity,
        signing: SigningKey,
    }
    fn load(path: &Path, expected: AgentId) -> Result<PublicNodeIdentity> {
        load_material(path, expected).map(|material| material.public)
    }
    pub(super) fn load_existing(directory: &Path, agent_id: AgentId) -> Result<PublicNodeIdentity> {
        load(&directory.join(KEY_FILE), agent_id)
    }
    fn load_material(path: &Path, expected: AgentId) -> Result<Material> {
        let metadata = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.nlink() != 1
            || metadata.len() != BYTES as u64
            || metadata.permissions().mode() & 0o777 != 0o600
        {
            return Err(Error::Storage);
        }
        let mut record = Zeroizing::new([0u8; BYTES]);
        let mut file = File::open(path).map_err(|_| Error::Storage)?;
        file.read_exact(record.as_mut())
            .map_err(|_| Error::Storage)?;
        let mut extra = [0u8; 1];
        if file.read(&mut extra).map_err(|_| Error::Storage)? != 0 {
            return Err(Error::Storage);
        }
        let digest = checksum(&record[..116]);
        if &record[..16] != MAGIC || !bool::from(digest.as_bytes().ct_eq(&record[116..])) {
            return Err(Error::Storage);
        }
        let id = std::str::from_utf8(&record[16..52]).map_err(|_| Error::Storage)?;
        let stored = AgentId::new(id).map_err(|_| Error::Storage)?;
        if stored != expected {
            return Err(Error::WrongContext);
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        seed.copy_from_slice(&record[52..84]);
        let signing = SigningKey::from_bytes(&seed);
        let key = PublicKey::from_bytes(signing.verifying_key().to_bytes());
        key.validate()?;
        if !bool::from(key.as_bytes().ct_eq(&record[84..116])) {
            return Err(Error::InvalidKey);
        }
        Ok(Material {
            public: public(stored, key),
            signing,
        })
    }
    pub(super) fn claim(
        state_dir: &Path,
        agent_id: AgentId,
        pin: &OwnerPin,
        offer: Signed<Offer>,
        now: Now,
    ) -> Result<Signed<Claim>> {
        claim_for_scope(state_dir, agent_id, pin, offer, now, Scope::TelemetryRead)
    }
    pub(super) fn tunnel_claim(
        state_dir: &Path,
        agent_id: AgentId,
        pin: &OwnerPin,
        offer: Signed<Offer>,
        now: Now,
    ) -> Result<Signed<Claim>> {
        claim_for_scope(state_dir, agent_id, pin, offer, now, Scope::TunnelOpen)
    }
    pub(super) fn metrics_claim(directory:&Path,agent:AgentId,pin:&OwnerPin,
        offer:Signed<Offer>,now:Now)->Result<Signed<Claim>> {
        claim_for_scope(directory,agent,pin,offer,now,Scope::MetricsCollect)
    }
    pub(super) fn exporter_claim(directory:&Path,agent:AgentId,pin:&OwnerPin,
        offer:Signed<Offer>,now:Now)->Result<Signed<Claim>> {
        claim_for_scope(directory,agent,pin,offer,now,Scope::ExportersManage)
    }
    fn claim_for_scope(
        state_dir: &Path,
        agent_id: AgentId,
        pin: &OwnerPin,
        offer: Signed<Offer>,
        now: Now,
        scope: Scope,
    ) -> Result<Signed<Claim>> {
        if !matches!(scope, Scope::TelemetryRead | Scope::TunnelOpen | Scope::MetricsCollect | Scope::ExportersManage) {
            return Err(Error::ScopeDenied);
        }
        let directory = crate::durable::absolute(state_dir)?;
        crate::durable::secure_path(&directory)?;
        let guard = crate::node_prepare::read_lock(&directory)?;
        crate::node_activation::deny_presence(&guard)?;
        let material = load_material(&directory.join(KEY_FILE), agent_id)?;
        crate::node_prepare::check(&guard, Some(&material.public))?;
        // Bootstrap validates explicit personal trust, owner key/expiry and time;
        // it creates no persisted actor, invitation or device registration.
        State::bootstrap(pin.clone(), now)?;
        if offer.body.header.context != pin.context {
            return Err(Error::WrongContext);
        }
        if offer.body.header.owner_epoch != pin.epoch {
            return Err(Error::WrongEpoch);
        }
        if offer.body.header.checkpoint
            != pin
                .checkpoint
                .checked_add(1)
                .ok_or(Error::WrongCheckpoint)?
        {
            return Err(Error::WrongCheckpoint);
        }
        offer.verify(pin.key)?;
        crate::wire::live(offer.body.issued_at, offer.body.expires_at, now)?;
        if offer.body.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        if offer.body.agent_id != agent_id || offer.body.agent_key != material.public.public_key {
            return Err(Error::WrongClaim);
        }
        // Initial node implementation supports this exact capability only.
        // Additional scopes require a separately reviewed implementation/update.
        if offer.body.scopes.0.as_slice() != [scope] {
            return Err(Error::CapabilityDenied);
        }
        let nonce = random_nonce()?;
        let body = Claim {
            header: offer.body.header.clone(),
            invitation_id: offer.body.invitation_id,
            offer_hash: offer.body.digest()?,
            agent_id,
            agent_key: material.public.public_key,
            capabilities: Capabilities {
                version: 1,
                scopes: Scopes(vec![scope]),
            },
            nonce,
            issued_at: now.0,
        };
        let signature =
            SignatureBytes::from_bytes(material.signing.sign(&body.signing_bytes()?).to_bytes());
        Ok(Signed { body, signature })
    }
    fn random_nonce() -> Result<Nonce> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| Error::Entropy)?;
        let nonce = Nonce::from_bytes(random);
        nonce.validate().map_err(|_| Error::Entropy)?;
        Ok(nonce)
    }
    fn current_header(header: &Header, pin: &OwnerPin) -> Result<()> {
        if header.context != pin.context {
            return Err(Error::WrongContext);
        }
        if header.owner_epoch != pin.epoch {
            return Err(Error::WrongEpoch);
        }
        if header.checkpoint != pin.checkpoint {
            return Err(Error::WrongCheckpoint);
        }
        Ok(())
    }
    fn selected_public(
        old: &PublicNodeIdentity,
        selected: Option<&crate::node_activation::Selection>,
    ) -> PublicNodeIdentity {
        match selected {
            Some(selected) => PublicNodeIdentity {
                format: 3,
                agent_id: old.agent_id,
                key_epoch: 2,
                public_key: selected.prepared.new_public_key,
                key_fingerprint: selected.prepared.new_key_fingerprint,
            },
            None => old.clone(),
        }
    }
    pub(super) fn request(
        state_dir: &Path,
        agent_id: AgentId,
        pin: &OwnerPin,
        binding: NodeRequestBinding,
        now: Now,
    ) -> Result<Signed<AgentRequest>> {
        request_for_scope(state_dir, agent_id, pin, binding, now, Scope::TelemetryRead)
    }
    pub(super) fn tunnel_request(
        state_dir: &Path,
        agent_id: AgentId,
        pin: &OwnerPin,
        binding: NodeRequestBinding,
        now: Now,
    ) -> Result<Signed<AgentRequest>> {
        request_for_scope(state_dir, agent_id, pin, binding, now, Scope::TunnelOpen)
    }
    pub(super) fn metrics_request(directory:&Path,agent:AgentId,pin:&OwnerPin,
        binding:NodeRequestBinding,now:Now)->Result<Signed<AgentRequest>> {
        request_for_scope(directory,agent,pin,binding,now,Scope::MetricsCollect)
    }
    pub(super) fn exporter_request(directory:&Path,agent:AgentId,pin:&OwnerPin,
        binding:NodeRequestBinding,now:Now)->Result<Signed<AgentRequest>> {
        request_for_scope(directory,agent,pin,binding,now,Scope::ExportersManage)
    }
    fn request_for_scope(
        state_dir: &Path,
        agent_id: AgentId,
        pin: &OwnerPin,
        binding: NodeRequestBinding,
        now: Now,
        scope: Scope,
    ) -> Result<Signed<AgentRequest>> {
        if !matches!(scope, Scope::TelemetryRead | Scope::TunnelOpen | Scope::MetricsCollect | Scope::ExportersManage) {
            return Err(Error::ScopeDenied);
        }
        let directory = crate::durable::absolute(state_dir)?;
        crate::durable::secure_path(&directory)?;
        let guard = crate::node_prepare::read_lock(&directory)?;
        let material = load_material(&directory.join(KEY_FILE), agent_id)?;
        crate::node_prepare::check(&guard, Some(&material.public))?;
        let selected = crate::node_activation::selected(&guard, &material.public)?;
        let public = selected_public(&material.public, selected.as_ref());
        State::bootstrap(pin.clone(), now)?;
        let cert = &binding.certificate.body;
        let lease = &binding.lease.body;
        let fresh = &binding.freshness.body;
        current_header(&lease.header, pin)?;
        current_header(&fresh.header, pin)?;
        // Endorsement authority comes only from the current independently
        // pinned owner. Verify before trusting its certificate-body commitment.
        binding.lease.verify(pin.key)?;
        binding.freshness.verify(pin.key)?;
        if lease.certificate_hash != cert.digest()? {
            return Err(Error::WrongClaim);
        }
        if cert.header.context != pin.context {
            return Err(Error::WrongContext);
        }
        if cert.header.owner_epoch > pin.epoch {
            return Err(Error::WrongEpoch);
        }
        if cert.header.checkpoint > pin.checkpoint {
            return Err(Error::WrongCheckpoint);
        }
        if cert.header.owner_epoch == pin.epoch {
            binding.certificate.verify(pin.key)?;
        }
        // For an older epoch the signed current lease endorses this exact body,
        // not its historical wrapper signature. No historical key fallback is
        // attempted. Actor must still match its fully verified stored body.
        crate::wire::live(cert.not_before, cert.expires_at, now)?;
        crate::wire::live(lease.issued_at, lease.expires_at, now)?;
        crate::wire::live(fresh.issued_at, fresh.expires_at, now)?;
        if fresh.expires_at > pin.expires_at
            || lease.expires_at > cert.expires_at
            || lease.expires_at > pin.expires_at
            || lease.expires_at > fresh.expires_at
        {
            return Err(Error::Expired);
        }
        if cert.agent_id != agent_id
            || cert.agent_key != public.public_key
            || lease.agent_id != agent_id
        {
            return Err(Error::WrongClaim);
        }
        if cert.key_epoch != public.key_epoch || lease.key_epoch != public.key_epoch {
            return Err(Error::WrongEpoch);
        }
        match (&selected, &cert.origin) {
            (
                None,
                CertificateOrigin::Enrollment {
                    key_fingerprint, ..
                },
            ) if *key_fingerprint == public.key_fingerprint => (),
            (
                Some(selected),
                CertificateOrigin::Rotation {
                    previous_certificate,
                    previous_key_epoch: 1,
                },
            ) if *previous_certificate == selected.previous_certificate_hash
                && cert.digest()? == selected.certificate_hash =>
            {
                ()
            }
            _ => return Err(Error::WrongClaim),
        }
        if cert.capabilities.version != 1
            || cert.capabilities.scopes.0.as_slice() != [scope]
            || cert.scopes.0.as_slice() != [scope]
        {
            return Err(Error::CapabilityDenied);
        }
        let supported = match scope {
            Scope::TelemetryRead => matches!(binding.operation, Operation::TelemetryRead { .. }),
            Scope::TunnelOpen => matches!(binding.operation, Operation::TunnelOpenBound { .. }),
            Scope::MetricsCollect => matches!(binding.operation, Operation::MetricsCollect { .. }),
            Scope::ExportersManage => matches!(binding.operation, Operation::ExportersManage { .. }),
            _ => false,
        };
        if !supported || lease.operation != binding.operation {
            return Err(Error::ScopeDenied);
        }
        let body = AgentRequest {
            header: lease.header.clone(),
            agent_id,
            key_epoch: public.key_epoch,
            lease_hash: lease.digest()?,
            nonce: random_nonce()?,
            issued_at: now.0,
        };
        if selected.is_some() {
            return crate::node_prepare::sign_selected_request(&guard, &material.public, body);
        }
        let signature =
            SignatureBytes::from_bytes(material.signing.sign(&body.signing_bytes()?).to_bytes());
        Ok(Signed { body, signature })
    }

    pub(super) fn rotation(
        directory: &Path,
        agent_id: AgentId,
        rotation_id: &str,
        pin: &OwnerPin,
        input: PreparedRotationInput,
        now: Now,
    ) -> Result<PreparedRotationProofs> {
        rotation_for_scope(directory,agent_id,rotation_id,pin,input,now,Scope::TelemetryRead)
    }
    pub(super) fn tunnel_rotation(directory:&Path,agent_id:AgentId,rotation_id:&str,pin:&OwnerPin,
        input:PreparedRotationInput,now:Now)->Result<PreparedRotationProofs> {
        rotation_for_scope(directory,agent_id,rotation_id,pin,input,now,Scope::TunnelOpen)
    }
    fn rotation_for_scope(directory:&Path,agent_id:AgentId,rotation_id:&str,pin:&OwnerPin,
        input:PreparedRotationInput,now:Now,scope:Scope)->Result<PreparedRotationProofs> {
        if !matches!(scope,Scope::TelemetryRead|Scope::TunnelOpen) { return Err(Error::ScopeDenied); }
        let guard = crate::node_prepare::existing_read_lock(directory)?;
        crate::node_activation::deny_presence(&guard)?;
        let material = load_material(&guard.directory().join(KEY_FILE), agent_id)?;
        let prepared =
            crate::node_prepare::check(&guard, Some(&material.public))?.ok_or(Error::WrongState)?;
        if rotation_id != prepared.rotation_id {
            return Err(Error::WrongClaim);
        }
        State::bootstrap(pin.clone(), now)?;
        let next = &input.certificate.body;
        if next.header.context != pin.context {
            return Err(Error::WrongContext);
        }
        if next.header.owner_epoch != pin.epoch {
            return Err(Error::WrongEpoch);
        }
        if next.header.checkpoint
            != pin
                .checkpoint
                .checked_add(1)
                .ok_or(Error::WrongCheckpoint)?
        {
            return Err(Error::WrongCheckpoint);
        }
        input.certificate.verify(pin.key)?;
        let previous = &input.previous_certificate.body;
        let CertificateOrigin::Rotation {
            previous_certificate,
            previous_key_epoch,
        } = next.origin
        else {
            return Err(Error::WrongState);
        };
        if previous_certificate != previous.digest()? || previous_key_epoch != 1 {
            return Err(Error::WrongClaim);
        }
        if previous.header.context != pin.context {
            return Err(Error::WrongContext);
        }
        if previous.header.owner_epoch > pin.epoch {
            return Err(Error::WrongEpoch);
        }
        if previous.header.checkpoint > pin.checkpoint {
            return Err(Error::WrongCheckpoint);
        }
        if previous.header.owner_epoch == pin.epoch {
            input.previous_certificate.verify(pin.key)?;
        }
        if scope==Scope::TunnelOpen && previous.header.owner_epoch!=pin.epoch {return Err(Error::WrongEpoch);}
        // Older wrapper is not verified with a new key: current signed proposal
        // explicitly endorses the exact previous body. Actor retains authority.
        if previous.agent_id != agent_id
            || next.agent_id != agent_id
            || previous.agent_key != material.public.public_key
            || next.agent_key != prepared.new_public_key
        {
            return Err(Error::WrongClaim);
        }
        if previous.key_epoch != 1 || next.key_epoch != 2 {
            return Err(Error::WrongEpoch);
        }
        match previous.origin {
            CertificateOrigin::Enrollment {
                key_fingerprint, ..
            } if key_fingerprint == material.public.key_fingerprint => (),
            _ => return Err(Error::WrongClaim),
        }
        if previous.agent_key == pin.key || next.agent_key == pin.key {
            return Err(Error::KeyReuse);
        }
        if previous.capabilities.version != 1
            || previous.capabilities.scopes.0.as_slice() != [scope]
            || previous.scopes.0.as_slice() != [scope]
            || next.capabilities != previous.capabilities
            || next.scopes != previous.scopes
        {
            return Err(Error::CapabilityDenied);
        }
        crate::wire::live(previous.not_before, previous.expires_at, now)?;
        crate::wire::live(next.not_before, next.expires_at, now)?;
        if next.not_before < previous.not_before
            || next.expires_at > previous.expires_at
            || next.expires_at > pin.expires_at
        {
            return Err(Error::Expired);
        }
        crate::wire::recent(next.not_before, now)?;
        #[cfg(test)]
        crate::node_prepare::test_rotation_nonce()?;
        let body = RotationProof {
            header: next.header.clone(),
            agent_id,
            certificate_hash: next.digest()?,
            nonce: random_nonce()?,
            issued_at: now.0,
        };
        let old = Signed {
            signature: SignatureBytes::from_bytes(
                material.signing.sign(&body.signing_bytes()?).to_bytes(),
            ),
            body: body.clone(),
        };
        let new = crate::node_prepare::sign_new_proof(
            &guard,
            &material.public,
            &prepared,
            NewKeyProof(body),
        )?;
        Ok(PreparedRotationProofs {
            certificate: input.certificate,
            old,
            new,
        })
    }
}

#[cfg(target_os="linux")]
pub(crate) fn create_collector_service_claim(directory:&Path,agent:AgentId,pin:&OwnerPin,
    certificate:&Certificate,policy:&Signed<CollectorServiceDelegation>,now:Now)->Result<Signed<CollectorServiceClaim>> {
    linux::collector_service_claim(directory,agent,pin,certificate,policy,now)
}
#[cfg(target_os="linux")]
pub(crate) fn create_collector_service_receipt(directory:&Path,agent:AgentId,pin:&OwnerPin,
    policy:&Signed<CollectorServiceDelegation>,body:CollectorServiceReceipt,now:Now)->Result<Signed<CollectorServiceReceipt>> {
    linux::collector_service_receipt(directory,agent,pin,policy,body,now)
}

#[cfg(target_os = "linux")]
pub(crate) fn sign_collector_recipient_attestation<S: CompareAndSwap>(
    directory: &Path, prepared: crate::collector_credentials::PreparedRecipientAttestation<'_, S>,
    clock: &mut impl FnMut() -> Result<Now>,
) -> Result<Signed<CollectorRecipientAttestation>> {
    linux::sign_collector_recipient_attestation(directory, prepared, clock)
}

#[cfg(target_os="linux")]
pub(crate) fn sign_collector_service_lifetime_receipt(directory:&Path,prepared:crate::enrollment_control::metrics::service_lifetime_control::PreparedLifetimeReceipt<'_>)->Result<Signed<CollectorServiceCredentialSetReceipt>>{linux::sign_collector_service_lifetime_receipt(directory,prepared)}
