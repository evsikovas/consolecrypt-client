//! Actual encrypted stores plus fixed controlled metadata. The owned test-only
//! candidate is NOT a release, and the mock response is NOT server opt-in proof.
use super::*;
use crate::AccountMode;
use cc_sync_core::mock::{FaultAction, MockServer, MockServerConfig};
use serde_json::{json, Value};
const PASS: &str = "owned encrypted writer vault phrase42";
const ACCOUNT: &str = "owned encrypted writer account phrase42";
fn app(path: &std::path::Path, version: &str) -> AppCore {
    let mut cfg = crate::AppConfig::for_tests(path.to_string_lossy().into_owned());
    cfg.client_version = version.into();
    cfg.background_sync = false;
    AppCore::new(cfg).unwrap()
}
struct Fixture {
    _dir: tempfile::TempDir,
    server: MockServer,
    a: AppCore,
    email: String,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start_with(MockServerConfig {
            require_request_proof: true,
            ..Default::default()
        })
        .await;
        let a = app(&dir.path().join("a"), "2.0.0");
        let email = format!("{}@example.invalid", uuid::Uuid::new_v4());
        a.create_synced_profile(
            "Owned writer A".into(),
            server.url().to_string(),
            email.clone(),
            ACCOUNT.into(),
            AccountMode::Register,
        )
        .await
        .unwrap();
        a.create_vault(PASS.into()).await.unwrap();
        a.sync_now().await.unwrap();
        Self {
            _dir: dir,
            server,
            a,
            email,
        }
    }
    async fn login(&self, name: &str, version: &str) -> AppCore {
        let b = app(&self._dir.path().join(name), version);
        let (owner, u) = self.a.unlocked().await.unwrap();
        let vault = u.vault_id.to_string();
        drop(u);
        drop(owner);
        b.create_synced_profile(
            "Owned writer B".into(),
            self.server.url().to_string(),
            self.email.clone(),
            ACCOUNT.into(),
            AccountMode::Login,
        )
        .await
        .unwrap();
        b.join_vault_with_passphrase(vault, PASS.into())
            .await
            .unwrap();
        b.sync_now().await.unwrap();
        b
    }
    async fn enabled(&self, app: &AppCore) -> String {
        let (owner, u) = app.unlocked().await.unwrap();
        let profile = owner.profile().await.unwrap();
        let path = format!(
            "/v1/vaults/{}/compatibility/monitoring-settings",
            u.vault_id
        );
        self.server.inject(&path,1,FaultAction::RawResponse { status:200,body:json!({"version":1,"scope":"personalAccount","accountId":profile.user_id.unwrap(),"revision":1,"state":"enabled","minimumClientVersion":"2.0.0","offerMinimumClientVersion":"2.0.0","rosterDigest":"a".repeat(64),"checkpoint":{"vaultId":u.vault_id,"vaultEpoch":self.server.epoch(u.vault_id).unwrap(),"vaultSequence":self.server.latest_sequence(u.vault_id)}})});
        path
    }
}
fn ciphertext(server: &MockServer, vault: VaultId, id: cc_protocol::ObjectId, marker: &str) {
    let row = server.object(vault, id).unwrap();
    let body = row.body.unwrap();
    assert_eq!(body.format, 1);
    assert_eq!(body.nonce.len(), 24);
    assert_eq!(body.wrapped_dek.len(), 48);
    assert!(!body
        .ciphertext
        .as_slice()
        .windows(marker.len())
        .any(|v| v == marker.as_bytes()));
}
mod automation;
