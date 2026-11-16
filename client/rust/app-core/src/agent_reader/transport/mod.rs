//! Staged private Reader SSH transport. No issuer, signer, shell or public API.
//! Root owns explicit approval and original context; the issuer signs locally.
mod http;
#[cfg(test)]
mod tests;

use super::scope::{
    HeldNativeReaderContext, ReaderContextSnapshot, ReaderOriginalContext, ReaderScopeIdentity,
};
use crate::AppCore;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_crypto_core::agent_reader::*;
use cc_models::credential::{Credential, CredentialKind};
use cc_models::known_host::KnownHost;
use cc_models::ObjectId;
use cc_ssh_core::{
    ConnectionPlan, CredentialResolver, HostKeyDecision, HostKeyInfo, HostKeyPolicy, HostKeyPrompt,
    KnownHostsError, KnownHostsStore, ResolveError, ResolvedCredential, SshBackend, SshConnector,
    SshSession,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt;
use std::future::Future;
use std::io::Write;
use std::net::IpAddr;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const SUBSYSTEM: &str = "consolecrypt-agent-reader-v1";
const CONTROL_FORMAT: &str = "cc-native-reader-control/v1";
const CONTROL_REQUEST_BYTES: usize = 16384;
const CONTROL_REPLY_BYTES: usize = 4096;
type Result<T> = std::result::Result<T, ReaderTransportError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReaderTransportError {
    Rejected,
    Malformed,
    ContextChanged,
    Expired,
    UnavailableBeforeDispatch,
    ControlOutcomeUnknown,
    DataOutcomeUnknown,
    AlreadyAttempted,
    BrokerRejected(ReaderBrokerError),
}
/// Cleanup observation only. Not authority or proof of all memory/socket
/// destruction. A requested disconnect alone is never an acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReaderCleanupObservation {
    ClosedObserved,
    Uncertain,
}
impl fmt::Display for ReaderTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReaderTransportError {}

// Closed broker codes: raw worker errors, paths and stderr cannot enter logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) enum ReaderBrokerError {
    InvalidWire,
    InvalidSignature,
    InvalidTime,
    Expired,
    WrongContext,
    WrongAgent,
    WrongEpoch,
    WrongTarget,
    WrongOperation,
    WrongIssuer,
    WrongHead,
    WrongAssociation,
    WrongDevice,
    ScopeDenied,
    Replay,
    Revoked,
    NotFound,
    Conflict,
    Quota,
    Entropy,
    ClockUnavailable,
    StoreUnavailable,
    StateUnknown,
    RoleDenied,
    ConfigUnavailable,
}

/// Public binding metadata, not authority by shape or deserialization.
/// Only the explicit Root/authority review path supplies the expected value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReaderPeerBinding {
    pub native_profile_id: AgentReaderNativeProfileId,
    pub agent_id: AgentReaderAgentId,
    pub audience_id: AgentReaderAudienceId,
    pub node_trust_origin_hash: AgentReaderHash,
    pub issuer_pin_genesis_hash: AgentReaderHash,
    pub transport_peer_hash: AgentReaderHash,
    pub agent_certificate_hash: AgentReaderHash,
    pub agent_key_epoch: u64,
    pub target_hashes: [AgentReaderHash; 2],
    pub ssh_server_key_digest: AgentReaderHash,
    pub http_port: u16,
}
impl fmt::Debug for ReaderPeerBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReaderPeerBinding([redacted])")
    }
}
impl ReaderPeerBinding {
    fn validate(&self) -> Result<()> {
        let zero = AgentReaderHash::from_bytes([0; 32]);
        if self.agent_key_epoch == 0
            || self.http_port == 0
            || self.target_hashes[0] == self.target_hashes[1]
            || self.transport_peer_hash != self.ssh_server_key_digest
            || [
                self.node_trust_origin_hash,
                self.issuer_pin_genesis_hash,
                self.transport_peer_hash,
                self.agent_certificate_hash,
                self.target_hashes[0],
                self.target_hashes[1],
                self.ssh_server_key_digest,
            ]
            .contains(&zero)
        {
            return Err(ReaderTransportError::Rejected);
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<AgentReaderHash> {
        use sha2::{Digest, Sha256};
        self.validate()?;
        // Explicit sort: this is a Native transport binding digest, not any
        // Agent ReaderStatement purpose or a new signed authority domain.
        let value = serde_json::to_value(self).map_err(|_| ReaderTransportError::Rejected)?;
        let sorted: std::collections::BTreeMap<_, _> = value
            .as_object()
            .ok_or(ReaderTransportError::Rejected)?
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let bytes = encode(&sorted, CONTROL_REPLY_BYTES)?;
        let mut hash = Sha256::new();
        hash.update(b"consolecrypt.native.reader.control-peer.v1\0");
        hash.update(bytes);
        Ok(AgentReaderHash::from_bytes(hash.finalize().into()))
    }
}

pub(super) struct ReaderSshSelection {
    original: ReaderScopeIdentity,
    plan: ConnectionPlan,
    key: KnownHost,
    credential: Credential,
    ssh_key_digest: AgentReaderHash,
    trust: ReaderSshTrust,
    resolver: Arc<dyn CredentialResolver>,
}
impl fmt::Debug for ReaderSshSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReaderSshSelection([redacted])")
    }
}
impl ReaderSshSelection {
    /// Existing strict SSH trust and original context permit observing an
    /// endpoint, not Reader approval or any root/key/association provisioning.
    pub fn from_original_context(
        held: &HeldNativeReaderContext<'_>,
        plan: ConnectionPlan,
        key: KnownHost,
    ) -> Result<Self> {
        use sha2::{Digest, Sha256};
        validate_plan(&plan)?;
        if held.snapshot.native_host_id != plan.host_id.to_string()
            || key.is_revoked()
            || key.is_cert_authority()
            || !cc_ssh_core::known_hosts::pattern_matches(
                &key.host_pattern,
                &plan.target.host,
                plan.target.port,
            )
        {
            return Err(ReaderTransportError::Rejected);
        }
        let blob = STANDARD
            .decode(&key.public_key)
            .map_err(|_| ReaderTransportError::Rejected)?;
        if blob.is_empty()
            || blob.len() > 16384
            || STANDARD.encode(&blob) != key.public_key
            || cc_ssh_core::keys::fingerprint_sha256(&blob) != key.fingerprint_sha256
        {
            return Err(ReaderTransportError::Rejected);
        }
        let credential = plan
            .credential
            .clone()
            .ok_or(ReaderTransportError::Rejected)?;
        let ssh_key_digest = AgentReaderHash::from_bytes(Sha256::digest(&blob).into());
        let trust = ReaderSshTrust::capture(held, &plan, &key, &credential)?;
        Ok(Self {
            original: held.original.scope_identity(),
            plan,
            key,
            credential,
            ssh_key_digest,
            trust,
            resolver: held.unlock.ssh.resolver.clone(),
        })
    }
}

/// No secret content is read. Actual known-host, credential and referenced
/// secret revisions are fenced independently of the copied SSH verifier.
struct ReaderSshTrust {
    host: String,
    port: u16,
    key: KnownHost,
    credential: Credential,
    matching_digest: [u8; 32],
    revisions: std::collections::BTreeMap<ObjectId, i64>,
    cancelled: CancellationToken,
    planner: cc_ssh_core::ConnectionPlanner,
    plan: ConnectionPlan,
    working_version: u64,
}
fn matching_trust(
    held: &HeldNativeReaderContext<'_>,
    host: &str,
    port: u16,
) -> Result<(Vec<KnownHost>, [u8; 32])> {
    use sha2::{Digest, Sha256};
    let mut rows: Vec<_> = held
        .unlock
        .working()
        .known_hosts()
        .into_iter()
        .filter(|k| cc_ssh_core::known_hosts::pattern_matches(&k.host_pattern, host, port))
        .collect();
    if rows.is_empty() || rows.len() > 1024 {
        return Err(ReaderTransportError::Rejected);
    }
    rows.sort_by_key(|k| k.id);
    let bytes = encode(&rows, 262144)?;
    Ok((rows, Sha256::digest(bytes).into()))
}
impl ReaderSshTrust {
    fn capture(
        held: &HeldNativeReaderContext<'_>,
        plan: &ConnectionPlan,
        key: &KnownHost,
        credential: &Credential,
    ) -> Result<Self> {
        let w = held.unlock.working();
        let working_version = w.version();
        let host = w.host(plan.host_id).ok_or(ReaderTransportError::Rejected)?;
        held.require_personal_object(plan.host_id)
            .map_err(|_| ReaderTransportError::Rejected)?;
        if host.proxy_command.is_some() {
            return Err(ReaderTransportError::Rejected);
        }
        let (rows, matching_digest) = matching_trust(held, &plan.target.host, plan.target.port)?;
        if w.known_host(key.id).as_ref() != Some(key)
            || w.credential(credential.id).as_ref() != Some(credential)
            || rows
                .iter()
                .any(|r| r.is_revoked() && r.public_key == key.public_key)
        {
            return Err(ReaderTransportError::Rejected);
        }
        let mut revisions = std::collections::BTreeMap::new();
        for id in rows
            .iter()
            .map(|v| v.id)
            .chain(std::iter::once(credential.id))
            .chain(credential.secret_id)
            .chain(credential.passphrase_secret_id)
        {
            let revision = w
                .revision_of(id)
                .filter(|v| *v > 0)
                .ok_or(ReaderTransportError::Rejected)?;
            held.require_personal_object(id)
                .map_err(|_| ReaderTransportError::Rejected)?;
            revisions.insert(id, revision);
        }
        // Group inheritance is part of the actual Native plan; do not let a
        // caller-supplied ConnectionPlan bypass it or miss a change/back.
        let mut next = host.group_id;
        let mut groups = std::collections::BTreeSet::new();
        while let Some(id) = next {
            if groups.len() >= 64 || !groups.insert(id) {
                return Err(ReaderTransportError::Rejected);
            }
            let group = w.group(id).ok_or(ReaderTransportError::Rejected)?;
            held.require_personal_object(id)
                .map_err(|_| ReaderTransportError::Rejected)?;
            revisions.insert(
                id,
                w.revision_of(id)
                    .filter(|v| *v > 0)
                    .ok_or(ReaderTransportError::Rejected)?,
            );
            next = group.parent_id;
        }
        let cancelled = CancellationToken::new();
        if !w.watch_revisions(revisions.clone(), cancelled.clone()) {
            return Err(ReaderTransportError::Rejected);
        }
        if w.version() != working_version {
            cancelled.cancel();
            return Err(ReaderTransportError::Rejected);
        }
        Ok(Self {
            host: plan.target.host.clone(),
            port: plan.target.port,
            key: key.clone(),
            credential: credential.clone(),
            matching_digest,
            revisions,
            cancelled,
            planner: held.unlock.ssh.planner.clone(),
            plan: plan.clone(),
            working_version,
        })
    }
    fn check_held(&self, held: &HeldNativeReaderContext<'_>) -> crate::AppResult<()> {
        let invalid = || crate::AppError::invalid("agent_reader", "reader_ssh_trust_changed");
        let w = held.unlock.working();
        held.require_personal_object(self.plan.host_id)?;
        for id in self.revisions.keys() {
            held.require_personal_object(*id)?;
        }
        if self.cancelled.is_cancelled()
            || w.version() != self.working_version
            || w.known_host(self.key.id).as_ref() != Some(&self.key)
            || w.credential(self.credential.id).as_ref() != Some(&self.credential)
            || self
                .revisions
                .iter()
                .any(|(id, r)| w.revision_of(*id) != Some(*r))
            || matching_trust(held, &self.host, self.port)
                .map_err(|_| invalid())?
                .1
                != self.matching_digest
            || w.version() != self.working_version
        {
            return Err(invalid());
        }
        Ok(())
    }
    async fn check(&self, app: &AppCore, context: &ReaderOriginalContext) -> Result<()> {
        context
            .with_held(app, |held| self.check_held(&held))
            .await
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        // Real Native planner, captured from the original unlocked runtime.
        // No profile/unlock guard is held across its async lookup.
        let actual = self
            .planner
            .plan(self.plan.host_id)
            .await
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        if actual != self.plan {
            return Err(ReaderTransportError::ContextChanged);
        }
        context
            .with_held(app, |held| self.check_held(&held))
            .await
            .map_err(|_| ReaderTransportError::ContextChanged)
    }
}
impl Drop for ReaderSshTrust {
    fn drop(&mut self) {
        self.cancelled.cancel();
    }
}

fn validate_plan(plan: &ConnectionPlan) -> Result<()> {
    if plan.backend != SshBackend::Native
        || plan.host_key_policy != HostKeyPolicy::Strict
        || plan.proxy.is_some()
        || !plan.route.is_empty()
        || !plan.forwards.is_empty()
        || plan.agent_forwarding
        || plan.target.port == 0
        || plan.target.host.parse::<IpAddr>().is_err()
        || plan.username.is_empty()
        || plan.username.len() > 256
        || plan.username.bytes().any(|b| b < 33 || b == 127)
    {
        return Err(ReaderTransportError::Rejected);
    }
    let c = plan
        .credential
        .as_ref()
        .ok_or(ReaderTransportError::Rejected)?;
    if !matches!(
        c.kind,
        CredentialKind::Password | CredentialKind::SshPrivateKey | CredentialKind::SshCertificate
    ) || c.secret_id.is_none()
        || c.agent_path.is_some()
    {
        return Err(ReaderTransportError::Rejected);
    }
    Ok(())
}

struct PinnedHosts {
    host: String,
    port: u16,
    key: KnownHost,
}
#[async_trait]
impl KnownHostsStore for PinnedHosts {
    async fn lookup(
        &self,
        h: &str,
        p: u16,
    ) -> std::result::Result<Vec<KnownHost>, KnownHostsError> {
        if h != self.host || p != self.port {
            return Err(KnownHostsError("reader peer mismatch".into()));
        }
        Ok(vec![self.key.clone()])
    }
    async fn add(&self, _: KnownHost) -> std::result::Result<(), KnownHostsError> {
        Err(KnownHostsError("reader trust is immutable".into()))
    }
    async fn revoke(&self, _: &str) -> std::result::Result<(), KnownHostsError> {
        Err(KnownHostsError("reader trust is immutable".into()))
    }
    async fn remove(&self, _: ObjectId) -> std::result::Result<(), KnownHostsError> {
        Err(KnownHostsError("reader trust is immutable".into()))
    }
}
struct NoPrompt;
#[async_trait]
impl HostKeyPrompt for NoPrompt {
    async fn confirm_unknown(&self, _: HostKeyInfo) -> HostKeyDecision {
        HostKeyDecision::Reject
    }
}
struct FixedCredential {
    expected: Credential,
    inner: Arc<dyn CredentialResolver>,
}
#[async_trait]
impl CredentialResolver for FixedCredential {
    async fn resolve(
        &self,
        c: &Credential,
    ) -> std::result::Result<ResolvedCredential, ResolveError> {
        if c != &self.expected {
            return Err(ResolveError::Cancelled);
        }
        let value = self.inner.resolve(c).await?;
        match (&value, c.kind) {
            (ResolvedCredential::Password(_), CredentialKind::Password)
            | (
                ResolvedCredential::PrivateKey { .. },
                CredentialKind::SshPrivateKey | CredentialKind::SshCertificate,
            ) => Ok(value),
            _ => Err(ResolveError::Cancelled),
        }
    }
    // No ambient OS agent, passphrase prompt, proxy credential or fallback.
}

/// Only actual dedicated SSH authentication PLUS the exact fixed broker probe
/// and explicit reviewed binding can construct this value. No JSON constructor.
pub(super) struct VerifiedReaderControlPeer {
    original: ReaderScopeIdentity,
    binding: ReaderPeerBinding,
    session: Arc<SshSession>,
    trust_cancelled: CancellationToken,
}
impl fmt::Debug for VerifiedReaderControlPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedReaderControlPeer([redacted])")
    }
}
impl VerifiedReaderControlPeer {
    pub fn binding(&self) -> &ReaderPeerBinding {
        &self.binding
    }
    pub fn binding_digest(&self) -> Result<AgentReaderHash> {
        self.binding.digest()
    }
    pub fn is_live(&self) -> bool {
        self.original.is_live() && !self.session.is_closed() && !self.trust_cancelled.is_cancelled()
    }
    pub fn is_live_for(&self, context: &ReaderOriginalContext) -> bool {
        self.original.check(context).is_ok() && self.is_live()
    }
}
/// Authenticated origin attachment, not cryptographic grant/head acceptance.
pub(super) struct BoundReaderControlReply<T> {
    metadata: T,
    session: Weak<SshSession>,
    binding: ReaderPeerBinding,
}
impl<T> fmt::Debug for BoundReaderControlReply<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BoundReaderControlReply([redacted])")
    }
}
impl<T> BoundReaderControlReply<T> {
    pub fn metadata_for(&self, peer: &VerifiedReaderControlPeer) -> Result<&T> {
        let session = self
            .session
            .upgrade()
            .ok_or(ReaderTransportError::Rejected)?;
        if !Arc::ptr_eq(&session, &peer.session) || self.binding != peer.binding || !peer.is_live()
        {
            return Err(ReaderTransportError::Rejected);
        }
        Ok(&self.metadata)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ControlRequest<'a, T> {
    format: &'static str,
    request_id: Uuid,
    peer_audience_id: Option<AgentReaderAudienceId>,
    expected_agent_id: Option<AgentReaderAgentId>,
    kind: &'static str,
    payload: &'a T,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ControlReply<T> {
    format: String,
    request_id: Uuid,
    ok: bool,
    metadata: Option<T>,
    code: Option<ReaderBrokerError>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Probe {
    control_role: String,
    endpoint: ObservedReaderEndpointInfo,
    provisioned_reader_info: Option<ProvisionedReaderInfo>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObservedReaderEndpointInfo {
    pub node_context: AgentReaderContextV1,
    pub node_trust_origin_hash: AgentReaderHash,
    pub original_owner_key: AgentReaderPublicKey,
    pub agent_id: AgentReaderAgentId,
    pub agent_certificate_hash: AgentReaderHash,
    pub agent_key_epoch: u64,
    pub node_key: AgentReaderPublicKey,
    pub targets: AgentReaderTargetSetV1,
    pub http_port: u16,
}
impl fmt::Debug for ObservedReaderEndpointInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ObservedReaderEndpointInfo([redacted])")
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProvisionedReaderInfo {
    binding: ReaderPeerBinding,
    head: AgentReaderHeadV1,
}

/// Authenticated SSH observation only. Reader root may be absent. This is NOT
/// Native approval and has no admin/challenge/data methods. The explicit
/// approval/privileged-provisioning path must run before verified promotion.
pub(super) struct ObservedAgentEndpoint {
    original: ReaderScopeIdentity,
    session: Arc<SshSession>,
    context: ReaderContextSnapshot,
    ssh_key_digest: AgentReaderHash,
    endpoint: ObservedReaderEndpointInfo,
    provisioned: Option<ProvisionedReaderInfo>,
    promoted: bool,
    trust: Option<ReaderSshTrust>,
    cleanup: Option<ReaderCleanupObservation>,
}
impl fmt::Debug for ObservedAgentEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ObservedAgentEndpoint([redacted])")
    }
}
impl ObservedAgentEndpoint {
    pub async fn close(&mut self) -> ReaderCleanupObservation {
        if let Some(v) = self.cleanup {
            return v;
        }
        if let Some(t) = &self.trust {
            t.cancelled.cancel();
        }
        // Arm the terminal observation before awaiting: cancellation must not
        // let Drop retry a disconnect whose outcome is already uncertain.
        self.cleanup = Some(ReaderCleanupObservation::Uncertain);
        let result = close_observed(&self.session).await;
        self.cleanup = Some(result);
        result
    }
    pub fn endpoint(&self) -> &ObservedReaderEndpointInfo {
        &self.endpoint
    }
    pub fn ssh_server_key_digest(&self) -> AgentReaderHash {
        self.ssh_key_digest
    }
    pub fn context_snapshot(&self) -> &ReaderContextSnapshot {
        &self.context
    }
    /// Optional metadata from the actual broker, never an installed-head ack.
    pub fn observed_reader_binding(&self) -> Option<&ReaderPeerBinding> {
        self.provisioned.as_ref().map(|v| &v.binding)
    }
    pub fn observed_reader_head(&self) -> Option<&AgentReaderHeadV1> {
        self.provisioned.as_ref().map(|v| &v.head)
    }
    pub async fn probe_verified_once(
        mut self,
        app: &AppCore,
        context: &ReaderOriginalContext,
        expected: ReaderPeerBinding,
        expected_head: &AgentReaderHeadV1,
    ) -> Result<ReaderTransportCore> {
        self.original
            .check(context)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        if self.cleanup.is_some() {
            return Err(ReaderTransportError::Rejected);
        }
        expected.validate()?;
        expected_head
            .validate()
            .map_err(|_| ReaderTransportError::Rejected)?;
        if context.snapshot() != &self.context
            || self.context.native_profile_id != expected.native_profile_id.as_uuid().to_string()
            || expected.ssh_server_key_digest != self.ssh_key_digest
            || expected.transport_peer_hash != self.ssh_key_digest
        {
            return Err(ReaderTransportError::Rejected);
        }
        let next: Probe = fenced(
            app,
            context,
            self.trust.as_ref().ok_or(ReaderTransportError::Rejected)?,
            Instant::now() + Duration::from_secs(10),
            exchange(
                &self.session,
                Some(&expected),
                "probe",
                &serde_json::json!({}),
            ),
        )
        .await?;
        validate_probe(&next)?;
        context.check_agent_context(&next.endpoint.node_context)
            .map_err(|_| ReaderTransportError::Rejected)?;
        let provisioned = next
            .provisioned_reader_info
            .as_ref()
            .ok_or(ReaderTransportError::UnavailableBeforeDispatch)?;
        let a = serde_json::to_value(&next.endpoint.targets)
            .map_err(|_| ReaderTransportError::Malformed)?;
        let b = serde_json::to_value(&self.endpoint.targets)
            .map_err(|_| ReaderTransportError::Malformed)?;
        if next.endpoint.agent_id != self.endpoint.agent_id
            || next.endpoint.node_context != self.endpoint.node_context
            || next.endpoint.node_trust_origin_hash != self.endpoint.node_trust_origin_hash
            || next.endpoint.original_owner_key != self.endpoint.original_owner_key
            || next.endpoint.node_key != self.endpoint.node_key
            || next.endpoint.agent_certificate_hash != self.endpoint.agent_certificate_hash
            || next.endpoint.agent_key_epoch != self.endpoint.agent_key_epoch
            || next.endpoint.http_port != self.endpoint.http_port
            || a != b
            || provisioned.binding != expected
            || &provisioned.head != expected_head
            || expected_head.context != next.endpoint.node_context
            || expected.agent_id != next.endpoint.agent_id
            || expected.node_trust_origin_hash != next.endpoint.node_trust_origin_hash
            || expected.agent_certificate_hash != next.endpoint.agent_certificate_hash
            || expected.agent_key_epoch != next.endpoint.agent_key_epoch
            || expected.http_port != next.endpoint.http_port
            || expected.target_hashes
                != [
                    next.endpoint.targets.targets[0]
                        .descriptor_hash()
                        .map_err(|_| ReaderTransportError::Malformed)?,
                    next.endpoint.targets.targets[1]
                        .descriptor_hash()
                        .map_err(|_| ReaderTransportError::Malformed)?,
                ]
        {
            return Err(ReaderTransportError::Rejected);
        }
        self.promoted = true;
        Ok(ReaderTransportCore {
            peer: VerifiedReaderControlPeer {
                original: self.original.clone(),
                binding: expected,
                session: self.session.clone(),
                trust_cancelled: self
                    .trust
                    .as_ref()
                    .ok_or(ReaderTransportError::Rejected)?
                    .cancelled
                    .clone(),
            },
            targets: next.endpoint.targets,
            context: self.context.clone(),
            trust: self.trust.take().ok_or(ReaderTransportError::Rejected)?,
            admin_attempted: false,
            challenge_attempted: false,
            head_attempted: false,
            data_attempted: false,
            closed: false,
            cleanup: ReaderCleanupObservation::Uncertain,
        })
    }
}
impl Drop for ObservedAgentEndpoint {
    fn drop(&mut self) {
        if !self.promoted && self.cleanup.is_none() {
            schedule_close(self.session.clone());
        }
    }
}
fn validate_probe(probe: &Probe) -> Result<()> {
    let e = &probe.endpoint;
    if probe.control_role != "readerOperator"
        || e.http_port == 0
        || e.agent_key_epoch == 0
        || e.agent_certificate_hash == AgentReaderHash::from_bytes([0; 32])
        || e.node_trust_origin_hash == AgentReaderHash::from_bytes([0; 32])
    {
        return Err(ReaderTransportError::Malformed);
    }
    e.original_owner_key
        .validate()
        .map_err(|_| ReaderTransportError::Malformed)?;
    e.node_key
        .validate()
        .map_err(|_| ReaderTransportError::Malformed)?;
    e.targets
        .validate()
        .map_err(|_| ReaderTransportError::Malformed)?;
    if e.targets.targets[0].agent_id != e.agent_id {
        return Err(ReaderTransportError::Malformed);
    }
    if let Some(p) = &probe.provisioned_reader_info {
        p.binding.validate()?;
        p.head
            .validate()
            .map_err(|_| ReaderTransportError::Malformed)?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum SignedReaderAdmin {
    InstallAssociation {
        signed: AgentReaderSigned<AgentReaderAssociationV1>,
    },
    Revoke {
        signed: AgentReaderSigned<AgentReaderRevocationV1>,
    },
}
impl fmt::Debug for SignedReaderAdmin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SignedReaderAdmin([redacted])")
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReaderChallengeRequest {
    pub association_hash: AgentReaderHash,
    pub operation_id: AgentReaderOperationId,
    pub expected_agent_id: AgentReaderAgentId,
    pub operation: AgentReaderOperationV1,
    pub target_descriptor_hash: AgentReaderHash,
    pub freshness: AgentReaderSigned<AgentReaderFreshnessV1>,
}
impl fmt::Debug for ReaderChallengeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReaderChallengeRequest([redacted])")
    }
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("reader frame budget"));
        }
        self.bytes.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    let mut w = LimitedWriter {
        bytes: Vec::with_capacity(limit.min(1024)),
        limit,
    };
    serde_json::to_writer(&mut w, value).map_err(|_| ReaderTransportError::Rejected)?;
    Ok(w.bytes)
}
fn parse_reply<T: DeserializeOwned>(bytes: &[u8], id: Uuid) -> Result<T> {
    if bytes.len() > CONTROL_REPLY_BYTES
        || !bytes.ends_with(b"\n")
        || bytes[..bytes.len() - 1].contains(&b'\n')
        || bytes.contains(&b'\r')
    {
        return Err(ReaderTransportError::Malformed);
    }
    let raw: serde_json::Value = strict_json(&bytes[..bytes.len() - 1], CONTROL_REPLY_BYTES)
        .map_err(|_| ReaderTransportError::Malformed)?;
    let map = raw.as_object().ok_or(ReaderTransportError::Malformed)?;
    let success = map
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .ok_or(ReaderTransportError::Malformed)?;
    let fields = if success {
        ["format", "requestId", "ok", "metadata"]
    } else {
        ["format", "requestId", "ok", "code"]
    };
    if map.len() != 4
        || fields.iter().any(|k| !map.contains_key(*k))
        || map.get("requestId").and_then(serde_json::Value::as_str)
            != Some(id.hyphenated().to_string().as_str())
        || (success && map.get("metadata").is_some_and(serde_json::Value::is_null))
    {
        return Err(ReaderTransportError::Malformed);
    }
    let r: ControlReply<T> =
        serde_json::from_value(raw).map_err(|_| ReaderTransportError::Malformed)?;
    if r.format != CONTROL_FORMAT || r.request_id != id {
        return Err(ReaderTransportError::Malformed);
    }
    match (r.ok, r.metadata, r.code) {
        (true, Some(metadata), None) => Ok(metadata),
        (false, None, Some(code)) => Err(ReaderTransportError::BrokerRejected(code)),
        _ => Err(ReaderTransportError::Malformed),
    }
}

async fn fenced<T>(
    app: &AppCore,
    context: &ReaderOriginalContext,
    trust: &ReaderSshTrust,
    deadline: Instant,
    io: impl Future<Output = Result<T>>,
) -> Result<T> {
    let deadline = deadline.min(context.deadline());
    let operation = async {
        trust.check(app, context).await?;
        let value = io.await?;
        trust.check(app, context).await?;
        Ok(value)
    };
    tokio::select! {biased;
        _=context.cancelled()=>Err(ReaderTransportError::ContextChanged),
        _=trust.cancelled.cancelled()=>Err(ReaderTransportError::ContextChanged),
        _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>Err(ReaderTransportError::Expired),
        result=operation=>{if Instant::now()>=deadline{Err(ReaderTransportError::Expired)}else{result}},
    }
}
async fn exchange<T: DeserializeOwned>(
    session: &SshSession,
    binding: Option<&ReaderPeerBinding>,
    kind: &'static str,
    payload: &impl Serialize,
) -> Result<T> {
    let id = Uuid::new_v4();
    if kind != "probe" && binding.is_none() {
        return Err(ReaderTransportError::Rejected);
    }
    let request = ControlRequest {
        format: CONTROL_FORMAT,
        request_id: id,
        peer_audience_id: binding.map(|v| v.audience_id),
        expected_agent_id: binding.map(|v| v.agent_id),
        kind,
        payload,
    };
    let mut frame = Zeroizing::new(encode(&request, CONTROL_REQUEST_BYTES - 1)?);
    frame.push(b'\n');
    let mut stream = session
        .open_subsystem(SUBSYSTEM)
        .await
        // No typed control request has been sent. The fixed broker must never
        // initialize or mutate state merely because its subsystem was opened.
        .map_err(|_| ReaderTransportError::UnavailableBeforeDispatch)?;
    stream
        .write_all(&frame)
        .await
        .map_err(|_| ReaderTransportError::ControlOutcomeUnknown)?;
    stream
        .shutdown()
        .await
        .map_err(|_| ReaderTransportError::ControlOutcomeUnknown)?;
    let mut reply = Zeroizing::new(Vec::with_capacity(1024));
    let mut block = [0; 1024];
    loop {
        let remaining = (CONTROL_REPLY_BYTES + 1).saturating_sub(reply.len());
        if remaining == 0 {
            return Err(ReaderTransportError::Malformed);
        }
        let n = stream
            .read(&mut block[..remaining.min(1024)])
            .await
            .map_err(|_| ReaderTransportError::ControlOutcomeUnknown)?;
        if n == 0 {
            break;
        }
        reply.extend_from_slice(&block[..n]);
        if reply.len() > CONTROL_REPLY_BYTES {
            return Err(ReaderTransportError::Malformed);
        }
    }
    parse_reply(&reply, id)
}

/// One Root attempt, one dedicated authenticated SSH session; no reconnection.
pub(super) struct ReaderTransportCore {
    peer: VerifiedReaderControlPeer,
    targets: AgentReaderTargetSetV1,
    context: ReaderContextSnapshot,
    trust: ReaderSshTrust,
    admin_attempted: bool,
    challenge_attempted: bool,
    head_attempted: bool,
    data_attempted: bool,
    closed: bool,
    cleanup: ReaderCleanupObservation,
}
impl fmt::Debug for ReaderTransportCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReaderTransportCore([redacted])")
    }
}
impl ReaderTransportCore {
    pub async fn probe_unapproved_once(
        app: &AppCore,
        context: &ReaderOriginalContext,
        selection: ReaderSshSelection,
    ) -> Result<ObservedAgentEndpoint> {
        selection
            .original
            .check(context)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        validate_plan(&selection.plan)?;
        let hosts = Arc::new(PinnedHosts {
            host: selection.plan.target.host.clone(),
            port: selection.plan.target.port,
            key: selection.key,
        });
        let resolver = Arc::new(FixedCredential {
            expected: selection.credential,
            inner: selection.resolver,
        });
        let mut options = cc_ssh_core::ConnectOptions::default();
        options.connect_timeout = Duration::from_secs(10);
        options.prompt_timeout = Duration::from_secs(10);
        options.auth_timeout = Duration::from_secs(10);
        options.default_keepalive = None;
        let connector =
            SshConnector::new(resolver, hosts, Arc::new(NoPrompt)).with_options(options);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut pending = fenced(app, context, &selection.trust, deadline, async {
            let session = connector
                .connect(&selection.plan)
                .await
                .map_err(|_| ReaderTransportError::UnavailableBeforeDispatch)?;
            // Arm cleanup before the post-I/O original-context/trust
            // checks, which can themselves fail after SSH authenticated.
            Ok(PendingSession(Some(Arc::new(session))))
        })
        .await?;
        let session = pending
            .0
            .as_ref()
            .ok_or(ReaderTransportError::UnavailableBeforeDispatch)?
            .clone();
        let probe: Probe = fenced(
            app,
            context,
            &selection.trust,
            deadline,
            exchange(&session, None, "probe", &serde_json::json!({})),
        )
        .await?;
        validate_probe(&probe)?;
        context.check_agent_context(&probe.endpoint.node_context)
            .map_err(|_| ReaderTransportError::Rejected)?;
        if session.network_origin() != &cc_ssh_core::client::NetworkOrigin::Native {
            return Err(ReaderTransportError::Rejected);
        }
        // Probe supplies metadata only; no root initialization, admin or grant.
        pending.0.take();
        Ok(ObservedAgentEndpoint {
            original: selection.original,
            session,
            context: context.snapshot().clone(),
            ssh_key_digest: selection.ssh_key_digest,
            endpoint: probe.endpoint,
            provisioned: probe.provisioned_reader_info,
            promoted: false,
            trust: Some(selection.trust),
            cleanup: None,
        })
    }
    pub fn peer(&self) -> &VerifiedReaderControlPeer {
        &self.peer
    }
    fn ready(&self) -> Result<()> {
        if self.closed || !self.peer.is_live() {
            Err(ReaderTransportError::UnavailableBeforeDispatch)
        } else {
            Ok(())
        }
    }
    async fn control<T: DeserializeOwned>(
        &mut self,
        app: &AppCore,
        context: &ReaderOriginalContext,
        kind: &'static str,
        payload: &impl Serialize,
    ) -> Result<BoundReaderControlReply<T>> {
        self.peer
            .original
            .check(context)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        self.ready()?;
        if context.snapshot() != &self.context {
            return Err(ReaderTransportError::ContextChanged);
        }
        let result = fenced(
            app,
            context,
            &self.trust,
            Instant::now() + Duration::from_secs(10),
            exchange(&self.peer.session, Some(&self.peer.binding), kind, payload),
        )
        .await;
        match result {
            Ok(metadata) => Ok(BoundReaderControlReply {
                metadata,
                session: Arc::downgrade(&self.peer.session),
                binding: self.peer.binding.clone(),
            }),
            Err(error) => {
                self.close().await;
                Err(error)
            }
        }
    }
    pub async fn install_signed_admin_once(
        &mut self,
        app: &AppCore,
        context: &ReaderOriginalContext,
        signed: &SignedReaderAdmin,
    ) -> Result<BoundReaderControlReply<AgentReaderHeadV1>> {
        self.peer
            .original
            .check(context)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        if std::mem::replace(&mut self.admin_attempted, true) {
            return Err(ReaderTransportError::AlreadyAttempted);
        }
        self.control(app, context, "installSignedAdmin", signed)
            .await
    }
    pub async fn challenge_once(
        &mut self,
        app: &AppCore,
        context: &ReaderOriginalContext,
        request: &ReaderChallengeRequest,
    ) -> Result<BoundReaderControlReply<AgentReaderChallengeV1>> {
        self.peer
            .original
            .check(context)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        if std::mem::replace(&mut self.challenge_attempted, true) {
            return Err(ReaderTransportError::AlreadyAttempted);
        }
        if request.expected_agent_id != self.peer.binding.agent_id
            || !self
                .peer
                .binding
                .target_hashes
                .contains(&request.target_descriptor_hash)
            || self
                .targets
                .target(&request.operation)
                .map_err(|_| ReaderTransportError::Rejected)?
                .descriptor_hash()
                .map_err(|_| ReaderTransportError::Rejected)?
                != request.target_descriptor_hash
        {
            return Err(ReaderTransportError::Rejected);
        }
        self.control(app, context, "challenge", request).await
    }
    /// Root invokes only for an explicit operator reconciliation. No implicit
    /// retry/adoption of a pending administration or fallback to an older head.
    pub async fn observe_head_once(
        &mut self,
        app: &AppCore,
        context: &ReaderOriginalContext,
    ) -> Result<BoundReaderControlReply<AgentReaderHeadV1>> {
        self.peer
            .original
            .check(context)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        if std::mem::replace(&mut self.head_attempted, true) {
            return Err(ReaderTransportError::AlreadyAttempted);
        }
        self.control(app, context, "observeHead", &serde_json::json!({}))
            .await
    }
    pub async fn dispatch_once(
        &mut self,
        app: &AppCore,
        context: &ReaderOriginalContext,
        request: PreparedReaderGet,
    ) -> Result<ProtectedReaderResponse> {
        self.peer
            .original
            .check(context)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        request
            .original
            .check(context)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        if std::mem::replace(&mut self.data_attempted, true) {
            return Err(ReaderTransportError::AlreadyAttempted);
        }
        self.ready()?;
        if context.snapshot() != &self.context {
            return Err(ReaderTransportError::ContextChanged);
        }
        if request.binding != self.peer.binding {
            return Err(ReaderTransportError::Rejected);
        }
        let result = self.dispatch_inner(app, context, request).await;
        // One data action only, success or uncertainty. No pooled reader route.
        self.close().await;
        result
    }
    async fn dispatch_inner(
        &self,
        app: &AppCore,
        context: &ReaderOriginalContext,
        request: PreparedReaderGet,
    ) -> Result<ProtectedReaderResponse> {
        let mut clock = ReadWall::new(request.expires_at)?;
        let remaining = clock.remaining()?;
        let deadline = (Instant::now() + Duration::from_secs(5)).min(Instant::now() + remaining);
        let frame = http::request(request.path, self.peer.binding.http_port, &request.bundle)?;
        let response = fenced(app, context, &self.trust, deadline, async {
            clock.check()?;
            let mut stream = self
                .peer
                .session
                .open_direct_tcpip("127.0.0.1", self.peer.binding.http_port, None)
                .await
                .map_err(|_| ReaderTransportError::UnavailableBeforeDispatch)?;
            clock.check()?;
            stream
                .write_all(&frame)
                .await
                .map_err(|_| ReaderTransportError::DataOutcomeUnknown)?;
            let response = http::response(&mut stream, request.max_bytes).await?;
            clock.check()?;
            // Still private. Root validates data and original context again
            // before publication; a request proof is not response attestation.
            Ok(ProtectedReaderResponse {
                status: response.status,
                body: response.body,
                target_hash: request.target_hash,
                expires_at: request.expires_at,
            })
        })
        .await?;
        // The post-I/O original-context recheck can itself do storage work.
        // Preserve a fresh wall observation after that last callback too.
        clock.check()?;
        Ok(response)
    }
    pub fn cleanup_observation(&self) -> ReaderCleanupObservation {
        self.cleanup
    }
    pub async fn close(&mut self) -> ReaderCleanupObservation {
        if !self.closed {
            self.closed = true;
            self.trust.cancelled.cancel();
            self.cleanup = close_observed(&self.peer.session).await;
        }
        self.cleanup
    }
}
async fn close_observed(session: &SshSession) -> ReaderCleanupObservation {
    let deadline = Instant::now() + Duration::from_secs(1);
    if session.is_closed() {
        return ReaderCleanupObservation::ClosedObserved;
    }
    let observed = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
        let _ = session.disconnect().await;
        session.closed().await;
        session.is_closed()
    })
    .await
    .unwrap_or(false);
    if observed && Instant::now() < deadline {
        ReaderCleanupObservation::ClosedObserved
    } else {
        ReaderCleanupObservation::Uncertain
    }
}
struct PendingSession(Option<Arc<SshSession>>);
impl Drop for PendingSession {
    fn drop(&mut self) {
        if let Some(session) = self.0.take() {
            schedule_close(session);
        }
    }
}
fn schedule_close(session: Arc<SshSession>) {
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            let _ = close_observed(&session).await;
        });
    }
}
impl Drop for ReaderTransportCore {
    fn drop(&mut self) {
        if !self.closed {
            self.closed = true;
            schedule_close(self.peer.session.clone());
        }
    }
}

/// Constructed from an actual purpose-signed bundle under Root's held review.
/// Structural checks here do not replace identity's crypto validation or the
/// authority ledger; no frontend raw envelope or URL constructor exists.
pub(super) struct PreparedReaderGet {
    original: ReaderScopeIdentity,
    binding: ReaderPeerBinding,
    bundle: Zeroizing<Vec<u8>>,
    target_hash: AgentReaderHash,
    path: &'static str,
    max_bytes: usize,
    expires_at: u64,
}
impl fmt::Debug for PreparedReaderGet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreparedReaderGet([redacted])")
    }
}
impl PreparedReaderGet {
    pub fn after_original_device_sign(
        held: &HeldNativeReaderContext<'_>,
        peer: &VerifiedReaderControlPeer,
        target: &AgentReaderTargetDescriptorV1,
        bundle: AgentReaderBundleV1,
    ) -> Result<Self> {
        peer.original
            .check(held.original)
            .map_err(|_| ReaderTransportError::ContextChanged)?;
        if !peer.is_live() {
            return Err(ReaderTransportError::Rejected);
        }
        target
            .validate()
            .map_err(|_| ReaderTransportError::Rejected)?;
        let hash = target
            .descriptor_hash()
            .map_err(|_| ReaderTransportError::Rejected)?;
        held.original.check_agent_context(&bundle.association.body.context)
            .map_err(|_| ReaderTransportError::Rejected)?;
        let a = &bundle.association.body;
        let g = &bundle.grant.body;
        let p = &bundle.proof.body;
        if held.snapshot.native_profile_id != peer.binding.native_profile_id.as_uuid().to_string()
            || held.snapshot.device_id != p.device_id.as_uuid().to_string()
            || held.snapshot.device_signing_key != *a.device_key.as_bytes()
            || target.agent_id != peer.binding.agent_id
            || !peer.binding.target_hashes.contains(&hash)
            || a.agent_id != target.agent_id
            || a.target_descriptor_hash != hash
            || g.target_descriptor_hash != hash
            || a.agent_certificate_hash != peer.binding.agent_certificate_hash
            || a.agent_key_epoch != peer.binding.agent_key_epoch
            || a.transport_peer_hash != peer.binding.transport_peer_hash
            || a.operation != target.operation()
            || g.operation != target.operation()
            || p.head != g.head
            || g.head != bundle.freshness.body.head
            || p.operation_id != g.operation_id
            || p.device_id != a.device_id
            || p.device_epoch != a.device_epoch
            || g.device_id != a.device_id
            || g.device_epoch != a.device_epoch
            || p.association_hash
                != a.body_digest()
                    .map_err(|_| ReaderTransportError::Rejected)?
            || g.association_hash != p.association_hash
            || p.grant_hash
                != g.body_digest()
                    .map_err(|_| ReaderTransportError::Rejected)?
            || p.challenge_hash != g.challenge_hash
        {
            return Err(ReaderTransportError::Rejected);
        }
        bundle
            .proof
            .verify(a.device_key)
            .map_err(|_| ReaderTransportError::Rejected)?;
        let bytes = Zeroizing::new(encode(&bundle, 8192)?);
        let path = match target.action {
            AgentReaderActionV1::HeartbeatRead => "/v1/heartbeat",
            AgentReaderActionV1::HistoryRead => "/v1/history",
        };
        Ok(Self {
            original: held.original.scope_identity(),
            binding: peer.binding.clone(),
            bundle: bytes,
            target_hash: hash,
            path,
            max_bytes: target.max_response_bytes as usize,
            expires_at: g.expires_at.seconds(),
        })
    }
}
pub(super) struct ProtectedReaderResponse {
    status: u16,
    body: Zeroizing<Vec<u8>>,
    target_hash: AgentReaderHash,
    expires_at: u64,
}
impl fmt::Debug for ProtectedReaderResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProtectedReaderResponse([redacted])")
    }
}
impl ProtectedReaderResponse {
    pub fn status(&self) -> u16 {
        self.status
    }
    pub fn target_hash(&self) -> AgentReaderHash {
        self.target_hash
    }
    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }
    /// Root must recheck original context, exact target and expiry before data
    /// publication. This borrowed body cannot be accepted from the wire alone.
    pub fn private_body(&self) -> &[u8] {
        &self.body
    }
}
struct ReadWall {
    last: f64,
    expires: u64,
}
impl ReadWall {
    fn now() -> Result<f64> {
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ReaderTransportError::Expired)?
            .as_secs_f64();
        if !t.is_finite() || !(0.0..=253402300799.0).contains(&t) {
            return Err(ReaderTransportError::Expired);
        }
        Ok(t)
    }
    fn new(expires: u64) -> Result<Self> {
        if expires > 253402300799 {
            return Err(ReaderTransportError::Expired);
        }
        let mut v = Self { last: 0.0, expires };
        v.check()?;
        Ok(v)
    }
    fn check(&mut self) -> Result<()> {
        let now = Self::now()?;
        if now < self.last || now >= self.expires as f64 {
            return Err(ReaderTransportError::Expired);
        }
        self.last = now;
        Ok(())
    }
    fn remaining(&mut self) -> Result<Duration> {
        self.check()?;
        Ok(Duration::from_secs_f64(self.expires as f64 - self.last))
    }
}
