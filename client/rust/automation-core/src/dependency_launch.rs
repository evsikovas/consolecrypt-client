//! Operation-owned dependency stdio. Never wraps a borrowed/shared job peer.
use crate::{AutomationError, HelperPeer, HelperProcess, Result};
use cc_ssh_core::SshSession;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const NAMESPACE: &str = ".consolecrypt-dependency-operations";
fn invalid() -> AutomationError {
    AutomationError::Protocol("invalid dependency helper identity")
}

#[derive(Debug, Clone)]
pub(crate) struct DependencyExpectation {
    pub operation: Uuid,
    validation: bool,
    pub family: String,
    pub operation_root: String,
}
impl DependencyExpectation {
    pub fn new(configured_root: &str, operation: Uuid) -> Result<Self> {
        Self::for_mode(configured_root, operation, false)
    }
    pub(crate) fn validation(configured_root: &str, operation: Uuid) -> Result<Self> {
        Self::for_mode(configured_root, operation, true)
    }
    fn for_mode(configured_root: &str, operation: Uuid, validation: bool) -> Result<Self> {
        if operation.is_nil()
            || configured_root.len() > 4096
            || !configured_root.starts_with('/')
            || configured_root.chars().any(char::is_control)
            || configured_root[1..]
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(AutomationError::Invalid(
                "invalid dependency operation root",
            ));
        }
        let (parent, name) = configured_root.rsplit_once('/').ok_or_else(invalid)?;
        let family = hex::encode(Sha256::digest(name.as_bytes()));
        let namespace = if validation {
            ".consolecrypt-validation-operations"
        } else {
            NAMESPACE
        };
        Ok(Self {
            operation,
            validation,
            operation_root: format!("{parent}/{namespace}/{family}/{operation}"),
            family,
        })
    }
    fn validate(&self, value: serde_json::Value) -> Result<()> {
        let report: ModeProbe = serde_json::from_value(value).map_err(|_| invalid())?;
        if report.version != 1
            || report.mode
                != if self.validation {
                    "vault_validation"
                } else {
                    "dependency_prepare"
                }
            || report.mode_version != 1
            || report.helper != "0.1.0"
            || report.protocol != 1
            || report.root_namespace
                != if self.validation {
                    ".consolecrypt-validation-operations"
                } else {
                    NAMESPACE
                }
            || report.root_family != self.family
            || report.operation_id != self.operation.to_string()
            || report.execution_enabled
            || report.target_dispatch_enabled
            || report.network_policy
                != if self.validation {
                    "none"
                } else {
                    "reviewed_exact_https_worker"
                }
        {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeProbe {
    version: u32,
    mode: String,
    mode_version: u32,
    helper: String,
    protocol: u32,
    root_namespace: String,
    root_family: String,
    operation_id: String,
    execution_enabled: bool,
    target_dispatch_enabled: bool,
    network_policy: String,
}

#[derive(Debug)]
pub(crate) struct OwnedDependencySsh(Option<SshSession>);
impl OwnedDependencySsh {
    pub fn new(session: SshSession) -> Self {
        Self(Some(session))
    }
    pub fn session(&self) -> &SshSession {
        self.0.as_ref().expect("owned dependency session")
    }
    pub(crate) async fn close(&mut self) {
        if let Some(session) = self.0.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), session.disconnect()).await;
        }
    }
}
impl Drop for OwnedDependencySsh {
    fn drop(&mut self) {
        if let Some(session) = self.0.take() {
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _ =
                        tokio::time::timeout(Duration::from_secs(5), session.disconnect()).await;
                });
            }
        }
    }
}
enum Owner {
    Local(HelperProcess),
    Remote(OwnedDependencySsh),
}

/// Fresh process/root and, remotely, a consumed independently owned SSH session.
/// Private fields prevent constructing this owner around a shared Runner peer.
/// Native profile/Runner/source approval and lifecycle registration are still
/// required before exposing any AppCore action; a probe is not authorization.
pub struct DependencyHelperConnection {
    peer: Arc<HelperPeer>,
    owner: Option<Owner>,
    expected: DependencyExpectation,
    mode_verified: bool,
}
impl std::fmt::Debug for DependencyHelperConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DependencyHelperConnection")
            .field("operation_id", &self.expected.operation)
            .finish_non_exhaustive()
    }
}
impl DependencyHelperConnection {
    #[cfg(test)]
    pub(crate) async fn for_protocol_fixture(
        peer: Arc<HelperPeer>,
        operation: Uuid,
        configured_root: &str,
    ) -> Result<Self> {
        let expected = DependencyExpectation::new(configured_root, operation)?;
        // Test-owned framed transport only, still passing strict mode probing.
        // No production constructor can omit its process/SSH owner.
        Self {
            peer,
            owner: None,
            expected,
            mode_verified: false,
        }
        .verify(&CancellationToken::new())
        .await
    }
    pub(crate) async fn local(
        local: HelperProcess,
        expected: DependencyExpectation,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        let connection = Self {
            peer: local.peer.clone(),
            owner: Some(Owner::Local(local)),
            expected,
            mode_verified: false,
        };
        connection.verify(cancel).await
    }
    pub(crate) async fn remote(
        peer: Arc<HelperPeer>,
        remote: OwnedDependencySsh,
        expected: DependencyExpectation,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        Self {
            peer,
            owner: Some(Owner::Remote(remote)),
            expected,
            mode_verified: false,
        }
        .verify(cancel)
        .await
    }
    async fn verify(mut self, cancel: &CancellationToken) -> Result<Self> {
        let checked = async {
            let probe = tokio::select! {biased;
                _=cancel.cancelled()=>return Err(AutomationError::Closed),
                result=tokio::time::timeout(Duration::from_secs(10),self.peer.request_dependency_bounded(
                    "probe",json!({}),Duration::from_secs(10),4096))=>result.map_err(|_|AutomationError::Timeout)??,
            };
            self.expected.validate(probe)
        }.await;
        match checked {
            Ok(()) => {
                self.mode_verified = true;
                Ok(self)
            }
            Err(error) => {
                // Token cancellation/failed identity must not return while the
                // owned local child still exits, or its SSH cleanup is merely
                // scheduled. Caller retains admission while awaiting us.
                self.close().await;
                Err(error)
            }
        }
    }
    pub fn operation_id(&self) -> Uuid {
        self.expected.operation
    }
    /// Expected infrastructure path derived by Native, never a Helper/UI path.
    pub fn operation_root(&self) -> &str {
        &self.expected.operation_root
    }
    /// Trusted Rust control-plane integration only. Never expose generic RPC
    /// through the bridge. This peer has no target handler/executor/job mode.
    /// Keeping a clone does not retain authority after this owner is dropped.
    pub fn private_control_peer(&self) -> &Arc<HelperPeer> {
        &self.peer
    }
    /// Returns only whether the exact private data-dispose ACK was received.
    /// This is NOT proof of remote process exit; failed/closed peers are never
    /// retried or reopened, and no ambient path deletion fallback exists.
    pub async fn close(mut self) -> bool {
        let disposed = if self.mode_verified && self.owner.is_some() && !self.peer.is_closed() {
            let result = tokio::time::timeout(Duration::from_secs(10), self.peer.request_dependency_bounded(
                if self.expected.validation {"validation_dispose"}else{"dependency_dispose"}, json!({"operation_id":self.expected.operation,"root_family":self.expected.family}),
                Duration::from_secs(10),4096)).await;
            match result {
                Ok(Ok(value)) => disposal_ack(value, self.expected.operation),
                _ => false,
            }
        } else {
            false
        };
        self.peer.close().await;
        if let Some(mut owner) = self.owner.take() {
            match &mut owner {
                Owner::Local(local) => local.close().await,
                Owner::Remote(remote) => remote.close().await,
            }
        }
        disposed
    }
}
fn disposal_ack(value: serde_json::Value, operation: Uuid) -> bool {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Ack {
        version: u32,
        operation_id: Uuid,
        data_disposed: bool,
        operation_removed: bool,
    }
    serde_json::from_value::<Ack>(value).is_ok_and(|ack| {
        ack.version == 1
            && ack.operation_id == operation
            && ack.data_disposed
            && ack.operation_removed
    })
}
impl Drop for DependencyHelperConnection {
    fn drop(&mut self) {
        // Synchronous fence before owned process/session cleanup. A retained
        // clone or late response cannot reuse this operation's control stream.
        self.peer.abort_dependency_exchange();
        // Local child's existing kill_on_drop and remote's bounded disconnect
        // guard affect this owned helper/route only. No directory is removed.
        let _ = self.owner.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(expected: &DependencyExpectation) -> serde_json::Value {
        json!({"version":1,"mode":"dependency_prepare","mode_version":1,"helper":"0.1.0","protocol":1,
            "root_namespace":NAMESPACE,"root_family":expected.family,"operation_id":expected.operation,
            "execution_enabled":false,"target_dispatch_enabled":false,"network_policy":"reviewed_exact_https_worker"})
    }

    #[test]
    fn disposal_ack_is_exact_data_only_and_never_remote_process_authority() {
        let id = Uuid::new_v4();
        let good =
            json!({"version":1,"operation_id":id,"data_disposed":true,"operation_removed":true});
        assert!(disposal_ack(good.clone(), id));
        for (key, value) in [
            ("version", json!(2)),
            ("operation_id", json!(Uuid::new_v4())),
            ("data_disposed", json!(false)),
            ("operation_removed", json!(false)),
            ("remote_process_joined", json!(true)),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(!disposal_ack(bad, id));
        }
    }
    #[test]
    fn root_derivation_matches_python_and_rejects_noncanonical_roots() {
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let expected = DependencyExpectation::new("/home/runner/jobs", id).unwrap();
        assert_eq!(expected.family, hex::encode(Sha256::digest(b"jobs")));
        assert_eq!(
            expected.operation_root,
            format!("/home/runner/{NAMESPACE}/{}/{id}", expected.family)
        );
        assert!(DependencyExpectation::new("/jobs", id)
            .unwrap()
            .operation_root
            .starts_with("/.consolecrypt-dependency-operations/"));
        for root in [
            "", "relative", "/", "/a/", "/a//b", "/a/../b", "/a/./b", "/a\nb",
        ] {
            assert!(DependencyExpectation::new(root, id).is_err());
        }
        assert!(DependencyExpectation::new("/jobs", Uuid::nil()).is_err());
    }
    #[test]
    fn normal_helper_and_wrong_mode_family_operation_execution_or_future_probe_are_denied() {
        let expected = DependencyExpectation::new("/home/runner/jobs", Uuid::new_v4()).unwrap();
        assert!(expected.validate(report(&expected)).is_ok());
        for (key, value) in [
            ("version", json!(2)),
            ("mode", json!("stdio")),
            ("mode_version", json!(2)),
            ("helper", json!("future")),
            ("protocol", json!(2)),
            ("root_family", json!("0".repeat(64))),
            ("root_namespace", json!("jobs")),
            ("operation_id", json!(Uuid::new_v4())),
            ("execution_enabled", json!(true)),
            ("target_dispatch_enabled", json!(true)),
            ("network_policy", json!("all")),
            ("future", json!(true)),
        ] {
            let mut bad = report(&expected);
            bad[key] = value;
            assert!(expected.validate(bad).is_err());
        }
        assert!(expected
            .validate(json!({"helper":"0.1.0","protocol":1,"capabilities":{}}))
            .is_err());
    }
    #[tokio::test]
    async fn dropping_owner_synchronously_revokes_retained_peer_clone() {
        let (reader, _server) = tokio::io::duplex(4096);
        let peer = HelperPeer::connect(reader, tokio::io::sink());
        let expected = DependencyExpectation::new("/home/runner/jobs", Uuid::new_v4()).unwrap();
        // Constructor bypass is local to this unit test; production factories
        // always own a fresh process or consumed SSH session before probing.
        let owner = DependencyHelperConnection {
            peer: peer.clone(),
            owner: None,
            expected,
            mode_verified: false,
        };
        assert!(!peer.is_closed());
        drop(owner);
        assert!(peer.is_closed());
        assert!(matches!(
            peer.request("probe", json!({}), Duration::from_secs(1))
                .await,
            Err(AutomationError::Closed)
        ));
    }
}
