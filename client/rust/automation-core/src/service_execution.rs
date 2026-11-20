//! Service-local SSH uses only owner-signed, exact delegation capsules.
//! No inventory resolver, OS agent, proxy password, or Desktop vault lookup.
use crate::{AutomationError, EnvironmentProbe, JobTargetScope, Result, VerifiedJob};
use cc_crypto_core::{
    DeviceSecretKeys, ServiceCredentialBinding, ServiceCredentialCapsule, ServiceCredentialKind,
    ServiceExecutionOrigin, VerifiedServiceDelegation,
};
use cc_models::{
    credential::CredentialKind,
    host::{HostKeyPolicy, SshBackend},
    known_host::KnownHost,
};
use cc_ssh_core::{
    ConnectionPlan, FixedHostKeyPrompt, HostKeyDecision, MemoryCredentialResolver,
    MemoryKnownHosts, ResolvedCredential, SshConnector, SshSession,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Same TCP transport; each IO poll is fenced to the ORIGINAL job lease.
/// Bytes already handed to the kernel before expiry cannot be retracted.
struct ServiceIo<T> {
    inner: T,
    wall_deadline: u64,
    deadline: std::pin::Pin<Box<tokio::time::Sleep>>,
    cancelled: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
}
impl<T> ServiceIo<T> {
    fn new(
        inner: T,
        deadline: std::time::Instant,
        wall_deadline: u64,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            inner,
            wall_deadline,
            deadline: Box::pin(tokio::time::sleep_until(tokio::time::Instant::from_std(
                deadline,
            ))),
            cancelled: Box::pin(cancel.cancelled_owned()),
        }
    }
    fn check(&mut self, cx: &mut std::task::Context<'_>) -> std::io::Result<()> {
        let wall_expired = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(true, |now| now.as_secs() >= self.wall_deadline);
        if wall_expired
            || std::future::Future::poll(self.cancelled.as_mut(), cx).is_ready()
            || std::future::Future::poll(self.deadline.as_mut(), cx).is_ready()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "service transport lease ended",
            ));
        }
        Ok(())
    }
}
impl<T: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for ServiceIo<T> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = this.check(cx) {
            return std::task::Poll::Ready(Err(error));
        }
        std::pin::Pin::new(&mut this.inner).poll_read(cx, buffer)
    }
}
impl<T: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for ServiceIo<T> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if let Err(error) = this.check(cx) {
            return std::task::Poll::Ready(Err(error));
        }
        std::pin::Pin::new(&mut this.inner).poll_write(cx, bytes)
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = this.check(cx) {
            return std::task::Poll::Ready(Err(error));
        }
        std::pin::Pin::new(&mut this.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

fn denied() -> AutomationError {
    AutomationError::Invalid("service execution context unavailable")
}
pub fn service_known_hosts_digest(entries: &[KnownHost]) -> Result<String> {
    if entries.is_empty() || entries.len() > 512 {
        return Err(denied());
    }
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or(std::io::ErrorKind::InvalidData)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Budget(256 * 1024), entries).map_err(|_| denied())?;
    let mut rows = entries.iter().collect::<Vec<_>>();
    rows.sort_by_key(|r| r.id);
    if rows.windows(2).any(|p| p[0].id == p[1].id) {
        return Err(denied());
    }
    let bytes = serde_json::to_vec(&crate::project::canonical(
        serde_json::to_value(rows).map_err(|_| denied())?,
    ))
    .map_err(|_| denied())?;
    if bytes.len() > 256 * 1024 {
        return Err(denied());
    }
    Ok(hex::encode(Sha256::digest(bytes)))
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceCapsuleInput {
    pub binding: ServiceCredentialBinding,
    pub capsule: ServiceCredentialCapsule,
}

pub(crate) fn validate_service_capsules(
    delegation: &VerifiedServiceDelegation,
    actual_origin: &ServiceExecutionOrigin,
    recipient: &DeviceSecretKeys,
    capsules: &[ServiceCapsuleInput],
    required_deadline: u64,
    now: u64,
) -> Result<BTreeSet<String>> {
    let policy = delegation.body();
    let owner_signing = delegation.owner_signing_key();
    if capsules.len() != policy.credentials.len() || capsules.len() > 64 || required_deadline <= now
    {
        return Err(denied());
    }
    let mut capsule_ids = BTreeSet::new();
    for input in capsules {
        let b = &input.binding;
        if !capsule_ids.insert(b.credential_id.clone())
            || b.kind != ServiceCredentialKind::Password
            || b.service_id != policy.service_id
            || b.tenant_id != policy.tenant.id
            || b.tenant_kind != policy.tenant.kind
            || b.owner_device_id != policy.owner_id
            || b.owner_signing_key_sha256 != hex::encode(Sha256::digest(owner_signing))
            || b.delegation_digest != delegation.digest().map_err(|_| denied())?
            || b.policy_revision != policy.policy_revision
            || b.revocation_epoch != policy.revocation_epoch
            || b.recipient_key_epoch != policy.recipient.key_epoch
            || b.recipient_certificate_epoch != policy.recipient.certificate_epoch
            || b.delegation_expires_at != policy.expires_at
            || b.recipient_key_sha256 != actual_origin.recipient_key_sha256
            || !policy
                .credentials
                .iter()
                .any(|c| c.id == b.credential_id && c.revision == b.credential_revision)
            || required_deadline > input.capsule.access_expires_at().map_err(|_| denied())?
        {
            return Err(denied());
        }
        let opened = cc_crypto_core::open_service_credential(
            &input.capsule,
            b,
            owner_signing,
            recipient,
            now,
        )
        .map_err(|_| denied())?;
        if !opened
            .with_secret(b, now, |raw| std::str::from_utf8(raw).is_ok())
            .map_err(|_| denied())?
        {
            return Err(denied());
        }
    }
    Ok(capsule_ids)
}

pub(crate) fn validate_service_plans(
    delegation: &VerifiedServiceDelegation,
    actual_origin: &ServiceExecutionOrigin,
    plans: &[ConnectionPlan],
    known_hosts: &[KnownHost],
    expected: &[cc_models::automation::ApprovedTarget],
    capsule_ids: &BTreeSet<String>,
) -> Result<()> {
    let policy = delegation.body();
    if expected.is_empty()
        || expected.len() > 32
        || plans.len() != expected.len()
        || policy.hosts.len() != expected.len()
        || service_known_hosts_digest(known_hosts)? != actual_origin.known_hosts_digest
    {
        return Err(denied());
    }
    // Check every plan/reference before the first connection. No implicit agent
    // authentication or trusted-host additions are available in this mode.
    let mut ids = BTreeSet::new();
    let mut used = BTreeSet::new();
    let mut hops = 0usize;
    for plan in plans {
        let id = Uuid::parse_str(&plan.host_id.to_string()).map_err(|_| denied())?;
        if !ids.insert(id)
            || !expected.iter().any(|t| t.host_id == id)
            || plan.backend != SshBackend::Native
            || plan.proxy.is_some()
            || !plan.forwards.is_empty()
            || plan.agent_forwarding
        {
            return Err(denied());
        }
        let approved = expected
            .iter()
            .find(|t| t.host_id == id)
            .ok_or_else(denied)?;
        let first = plan.all_hops().into_iter().next().ok_or_else(denied)?;
        let origin = cc_ssh_core::client::NetworkOrigin::ScopedStream {
            scope_digest: actual_origin
                .stream_scope(&first.endpoint.host, first.endpoint.port)
                .map_err(|_| denied())?,
        };
        let proposed = crate::scope::target_from_connection(approved.alias.clone(), plan, &origin)?;
        if serde_json::to_value(&proposed).ok() != serde_json::to_value(approved).ok()
            || !policy.hosts.iter().any(|h| {
                cc_crypto_core::ServicePolicyHostRef::from_target(&proposed, h.revision)
                    .is_ok_and(|a| a.id == h.id && a.target_digest == h.target_digest)
            })
        {
            return Err(denied());
        }
        for hop in plan.all_hops() {
            hops += 1;
            let c = hop.credential.ok_or_else(denied)?;
            if hops > 128
                || hop.host_key_policy != HostKeyPolicy::Strict
                || c.kind != CredentialKind::Password
                || !capsule_ids.contains(&c.id.to_string())
            {
                return Err(denied());
            }
            used.insert(c.id.to_string());
        }
    }
    if &used != capsule_ids {
        return Err(denied());
    }
    Ok(())
}

/// Caller must hold current custody/scheduler/revocation authority independently.
/// The original signed policy + actual local runtime are checked here, before
/// any capsule is opened or endpoint dialed. Plans are DATA, never grants.
pub async fn bind_service_job_scope(
    delegation: &VerifiedServiceDelegation,
    actual_origin: &ServiceExecutionOrigin,
    actual_probe: &EnvironmentProbe,
    actual_helper_digest: &str,
    approval: VerifiedJob,
    plans: Vec<ConnectionPlan>,
    known_hosts: Vec<KnownHost>,
    capsules: &[ServiceCapsuleInput],
    recipient: &DeviceSecretKeys,
    cancelled: CancellationToken,
) -> Result<Arc<JobTargetScope>> {
    // A Windows/Desktop process cannot pretend that its direct socket is the
    // selected Linux service. Probe reports remain compatibility, not attestation.
    if !cfg!(target_os = "linux") {
        return Err(denied());
    }
    let policy = delegation.body();
    let manifest = approval.manifest();
    let owner_signing = delegation.owner_signing_key();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| denied())?
        .as_secs();
    actual_origin.validate().map_err(|_| denied())?;
    if cancelled.is_cancelled()
        || approval.is_access_expired()
        || approval.is_submission_expired()
        || policy.execution_origin.as_ref() != Some(actual_origin)
        || actual_probe.system != "Linux"
        || !actual_probe.non_root
        || !actual_probe.dependencies_ready
        || !actual_probe.sandbox.filesystem
        || !actual_probe.sandbox.network
        || !actual_probe.sandbox.pid
        || actual_probe.require_trusted_worker_bootstrap().is_err()
        || actual_probe.environment_digest != actual_origin.environment_digest
        || actual_helper_digest != actual_origin.helper_digest
        || hex::encode(Sha256::digest(recipient.public_keys().encryption))
            != actual_origin.recipient_key_sha256
        || hex::encode(Sha256::digest(recipient.public_keys().signing))
            != actual_origin.signer_key_sha256
        || service_known_hosts_digest(&known_hosts)? != actual_origin.known_hosts_digest
        || manifest.runner_id.to_string() != policy.service_id
        || manifest.tenant_id.to_string() != policy.tenant.id
        || manifest.project_id.to_string() != policy.project_id
        || manifest.revision_digest != policy.template_digest
        || manifest.playbook != policy.playbook
        || manifest.inventory_digest != policy.inventory_digest
        || manifest.parameters_digest != policy.parameters_digest
        || manifest.environment_digest != policy.environment_digest
        || manifest.access_expires_at as u64 > policy.expires_at
        || manifest.expires_at as u64 > policy.expires_at
        || plans.len() != manifest.targets.len()
        || plans.is_empty()
        || plans.len() > 32
        || policy.hosts.len() != plans.len()
        || capsules.len() != policy.credentials.len()
        || capsules.len() > 512
        || now < policy.issued_at
        || now >= policy.expires_at
    {
        return Err(denied());
    }
    let capsule_ids = validate_service_capsules(
        delegation,
        actual_origin,
        recipient,
        capsules,
        manifest.access_expires_at as u64,
        now,
    )?;
    validate_service_plans(
        delegation,
        actual_origin,
        &plans,
        &known_hosts,
        &manifest.targets,
        &capsule_ids,
    )?;
    let access_deadline = approval.access_deadline();
    let connect_started = std::time::Instant::now();
    let connect_budget = approval.dispatch_remaining().min(Duration::from_secs(120));
    let connect_deadline = tokio::time::Instant::from_std(connect_started + connect_budget);
    if connect_budget.is_zero() {
        return Err(denied());
    }
    let mut sessions = HashMap::<Uuid, Arc<SshSession>>::new();
    let diagnostic_capture=Arc::new(cc_ssh_core::DiagnosticCapture::new());
    let connect = async {
        for plan in plans {
            if cancelled.is_cancelled()
                || approval.is_access_expired()
                || approval.is_submission_expired()
            {
                return Err(denied());
            }
            let resolver = Arc::new(MemoryCredentialResolver::new());
            for input in capsules {
                let opened = cc_crypto_core::open_service_credential(
                    &input.capsule,
                    &input.binding,
                    owner_signing,
                    recipient,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| denied())?
                        .as_secs(),
                )
                .map_err(|_| denied())?;
                let secret = opened
                    .with_secret(
                        &input.binding,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(|_| denied())?
                            .as_secs(),
                        |raw| {
                            std::str::from_utf8(raw).map(|text| {
                                cc_ssh_core::secrecy::SecretString::from(text.to_owned())
                            })
                        },
                    )
                    .map_err(|_| denied())?
                    .map_err(|_| denied())?;
                let id = input.binding.credential_id.parse().map_err(|_| denied())?;
                resolver.insert(id, ResolvedCredential::Password(secret));
            }
            let connector = SshConnector::new(
                resolver,
                Arc::new(MemoryKnownHosts::with_entries(known_hosts.clone())),
                Arc::new(FixedHostKeyPrompt::new(HostKeyDecision::Reject)),
            ).with_diagnostic_capture(diagnostic_capture.clone());
            let first = plan.all_hops().into_iter().next().ok_or_else(denied)?;
            let stream =
                tokio::net::TcpStream::connect((first.endpoint.host.as_str(), first.endpoint.port))
                    .await
                    .map_err(|_| denied())?;
            if cancelled.is_cancelled()
                || approval.is_access_expired()
                || approval.is_submission_expired()
            {
                return Err(denied());
            }
            let stream = ServiceIo::new(
                stream,
                access_deadline,
                manifest.access_expires_at as u64,
                cancelled.clone(),
            );
            let origin = actual_origin
                .stream_scope(&first.endpoint.host, first.endpoint.port)
                .map_err(|_| denied())?;
            let session = Arc::new(
                connector
                    .connect_over_stream(&plan, stream, origin)
                    .await
                    .map_err(|_| denied())?,
            );
            let id = plan.host_id.to_string().parse().map_err(|_| denied())?;
            let expected = manifest
                .targets
                .iter()
                .find(|t| t.host_id == id)
                .ok_or_else(denied)?;
            let actual = crate::target_from_session(expected.alias.clone(), &session)?;
            sessions.insert(id, session);
            if serde_json::to_value(&actual).ok() != serde_json::to_value(expected).ok()
                || !policy.hosts.iter().any(|h| {
                    cc_crypto_core::ServicePolicyHostRef::from_target(&actual, h.revision)
                        .is_ok_and(|a| a.id == h.id && a.target_digest == h.target_digest)
                })
            {
                return Err(denied());
            }
        }
        Ok(())
    };
    let result = tokio::select! {biased;
        _=cancelled.cancelled()=>Err(denied()),
        _=tokio::time::sleep_until(connect_deadline)=>Err(denied()),
        value=connect=>value,
    };
    if let Err(error) = result {
        for session in sessions.values() {
            let _ = tokio::time::timeout(Duration::from_secs(5), session.disconnect()).await;
        }
        return Err(error);
    }
    let cleanup = sessions.values().cloned().collect::<Vec<_>>();
    match JobTargetScope::bind_with_cancellation(approval, sessions, cancelled) {
        Ok(scope) => Ok(scope),
        Err(error) => {
            for session in cleanup {
                let _ = tokio::time::timeout(Duration::from_secs(5), session.disconnect()).await;
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod service_io_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn blocked_write_wakes_at_original_deadline_without_forwarding_new_bytes() {
        let (inner, mut peer) = tokio::io::duplex(1);
        let mut stream = ServiceIo::new(
            inner,
            std::time::Instant::now() + Duration::from_millis(100),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 60,
            CancellationToken::new(),
        );
        stream.write_all(&[1]).await.unwrap();
        let error = tokio::time::timeout(Duration::from_secs(2), stream.write_all(&[2]))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        let mut byte = [0];
        peer.read_exact(&mut byte).await.unwrap();
        assert_eq!(byte, [1]);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), peer.read(&mut byte))
                .await
                .is_err()
        );
        assert_eq!(
            stream.flush().await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
    #[tokio::test]
    async fn original_cancel_denies_read_write_flush_but_allows_shutdown() {
        let (inner, mut peer) = tokio::io::duplex(8);
        let token = CancellationToken::new();
        let mut stream = ServiceIo::new(
            inner,
            std::time::Instant::now() + Duration::from_secs(60),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 60,
            token.clone(),
        );
        peer.write_all(&[3]).await.unwrap();
        token.cancel();
        let mut byte = [0];
        assert_eq!(
            stream.read(&mut byte).await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            stream.write_all(&[4]).await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            stream.flush().await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        stream.shutdown().await.unwrap();
        assert_eq!(peer.read(&mut byte).await.unwrap(), 0);
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "service_execution/owner_revocation_tests.rs"]
mod owner_revocation_tests;
