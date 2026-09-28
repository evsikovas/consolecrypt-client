//! No secret in logs (CLIENT_ARCHITECTURE §5): capture everything at TRACE
//! (with the `log` → tracing bridge and a user directive that tries to
//! re-enable tungstenite) through app-core's logging helper while running
//! full flows — local vault, credentials, backups, recovery, a synced
//! account with a live WebSocket event stream, device approval,
//! revocation and re-authentication — then grep for every secret.

mod common;

use cc_app_core::platform::SecureStore;
use cc_app_core::*;
use cc_sync_core::mock::MockServer;
use common::*;
use secrecy::ExposeSecret;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        CaptureWriter(self.0.clone())
    }
}

impl Capture {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

const CRED_PASSWORD: &str = "credential-password-canary-7731";
const KEY_PASSPHRASE: &str = "key-passphrase-canary-5521";
const NEW_PASSPHRASE: &str = "new-vault-passphrase-canary-9932";
const HEADER_CANARY: &str = "header-canary-4411";

fn tokens(store: &Arc<dyn SecureStore>, profile_id: &str) -> Vec<String> {
    let name = format!("profiles/{profile_id}/session-tokens");
    let Some(v) = store.get(&name).unwrap() else {
        return Vec::new();
    };
    let json: serde_json::Value = serde_json::from_slice(v.expose_secret()).unwrap();
    ["access_token", "refresh_token"]
        .iter()
        .filter_map(|k| json[k].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_flows_at_trace_leak_no_secrets() {
    let capture = Capture::default();
    logging::init_with_writer(
        Some("trace,tungstenite=trace,tungstenite::handshake=trace,reqwest=trace"),
        capture.clone(),
    )
    .unwrap();
    // The caps must hold for tracing events and bridged `log` records.
    tracing::trace!(target: "tungstenite::handshake::client", "Authorization: Bearer {HEADER_CANARY}");
    tracing::debug!(target: "reqwest::connect", "authorization: bearer {HEADER_CANARY}");
    log::debug!(target: "tungstenite::handshake", "Authorization: Bearer {HEADER_CANARY}");
    log::warn!(target: "hyper::proto", "hyper warn passes through");
    tracing::trace!(target: "log_hygiene", "trace level marker");
    log::debug!(target: "log_hygiene", "bridged debug marker");

    let tmp = tempfile::tempdir().unwrap();
    let mut secrets: Vec<String> = vec![
        PASSPHRASE.into(),
        NEW_PASSPHRASE.into(),
        ACCOUNT_PASSWORD.into(),
        CRED_PASSWORD.into(),
        KEY_PASSPHRASE.into(),
        HEADER_CANARY.into(),
    ];

    // ── local profile flows ─────────────────────────────────────────────
    let local = common::app(&tmp.path().join("local"));
    let created = local
        .create_local_profile("Personal".into(), PASSPHRASE.into())
        .await
        .unwrap();
    let kit = created.recovery_kit.clone().unwrap();
    secrets.push(kit.phrase.clone());
    secrets.push(kit.qr_payload.clone());
    local
        .add_password_credential("pw".into(), Some("root".into()), CRED_PASSWORD.into())
        .await
        .unwrap();
    let key = local
        .generate_ssh_key(
            "k".into(),
            None,
            KeyGenAlgorithm::Ed25519,
            Some(KEY_PASSPHRASE.into()),
            true,
        )
        .await
        .unwrap();
    let ext = cc_ssh_core::keys::generate_key(KeyGenAlgorithm::Ed25519, "ext", None).unwrap();
    let ext_text = ext.private_openssh.expose_secret().to_owned();
    secrets.push(ext_text.lines().nth(1).unwrap().to_owned());
    local
        .import_ssh_key("ext".into(), None, ext_text, None, false, None)
        .await
        .unwrap();
    local
        .save_host(host("h", "192.0.2.50", Some(&key.id)))
        .await
        .unwrap();
    let backup = tmp.path().join("b.ccbackup").to_string_lossy().to_string();
    local.export_backup(backup.clone()).await.unwrap();
    local.lock().await.unwrap();
    let _ = local
        .unlock_with_passphrase("wrong wrong wrong".into())
        .await;
    local
        .unlock_with_recovery_key(kit.phrase.clone())
        .await
        .unwrap();
    local
        .change_passphrase(PASSPHRASE.into(), NEW_PASSPHRASE.into())
        .await
        .unwrap();
    let kit2 = local.regenerate_recovery_kit().await.unwrap();
    secrets.push(kit2.phrase.clone());
    let restored = common::app(&tmp.path().join("restored"));
    restored
        .import_backup(
            backup,
            "R".into(),
            BackupUnlock::Passphrase(PASSPHRASE.into()),
            None,
        )
        .await
        .unwrap();

    // ── synced flows with a live WebSocket ──────────────────────────────
    let server = MockServer::start().await;
    let url = server.url().to_string();
    let store_a = memory_store();
    let a = app_with(&tmp.path().join("a"), store_a.clone(), false);
    let acct = a
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            "log@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Register,
        )
        .await
        .unwrap();
    let a_pid = acct.profile.id.clone();
    secrets.extend(tokens(&store_a, &a_pid));
    let vault_kit = a.create_vault(PASSPHRASE.into()).await.unwrap();
    secrets.push(vault_kit.phrase.clone());
    a.add_password_credential("srv".into(), None, CRED_PASSWORD.into())
        .await
        .unwrap();
    a.sync_now().await.unwrap();
    wait_until!(
        10,
        "WebSocket connected",
        server.hits(cc_protocol::paths::EVENTS_WS) >= 1
    );
    server.expire_access_tokens();
    a.sync_now().await.unwrap();
    secrets.extend(tokens(&store_a, &a_pid));

    let store_b = memory_store();
    let b = app_with(&tmp.path().join("b"), store_b.clone(), false);
    let acct_b = b
        .create_synced_profile(
            "Work".into(),
            url.clone(),
            "log@example.org".into(),
            ACCOUNT_PASSWORD.into(),
            AccountMode::Login,
        )
        .await
        .unwrap();
    let req = b
        .request_device_approval(Some(vault_kit.vault_id.clone()))
        .await
        .unwrap();
    a.start_device_approval(req.request_id.clone())
        .await
        .unwrap();
    a.confirm_device_approval(req.request_id).await.unwrap();
    assert!(b.finish_device_approval(None).await.unwrap());
    b.sync_now().await.unwrap();
    secrets.extend(tokens(&store_b, &acct_b.profile.id));
    a.revoke_device(acct_b.device_id.clone(), None)
        .await
        .unwrap();
    wait_until!(10, format!("B stopped ({:?})", b.sync_status().await), {
        b.sync_status().await.unwrap().phase == SyncPhaseDto::Stopped
    });
    b.reauthenticate(ACCOUNT_PASSWORD.into(), true)
        .await
        .unwrap();
    b.sync_now().await.unwrap();
    secrets.extend(tokens(&store_b, &acct_b.profile.id));
    secrets.extend(tokens(&store_a, &a_pid));
    for x in [&local, &restored, &a, &b] {
        x.shutdown().await.unwrap();
    }

    // ── verify ──────────────────────────────────────────────────────────
    let logs = capture.text();
    assert!(logs.len() > 1000, "capture works ({} bytes)", logs.len());
    assert!(logs.contains("device approved"), "app-core events captured");
    assert!(
        logs.contains("hyper warn passes through"),
        "log bridge active"
    );
    assert!(logs.contains("trace level marker"), "TRACE level captured");
    assert!(logs.contains("bridged debug marker"), "log records bridged");
    let lower = logs.to_lowercase();
    for needle in ["authorization", "bearer ", "private key"] {
        assert!(!lower.contains(needle), "logs contain {needle:?}");
    }
    let mut checked = 0;
    for s in &secrets {
        assert!(!s.is_empty());
        assert!(!logs.contains(s.as_str()), "a secret leaked into the logs");
        checked += 1;
    }
    assert!(
        checked >= 14,
        "tokens were collected ({checked} secrets checked)"
    );
}
