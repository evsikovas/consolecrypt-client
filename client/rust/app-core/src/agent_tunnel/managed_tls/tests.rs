use super::*;
use cc_platform_core::{InMemorySecureStore, SecureStoreError};
use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
const PASSWORD: &str = "managed client TLS owned test passphrase 42";
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}
async fn fixture() -> (tempfile::TempDir, AppCore, ManagedTlsCustody) {
    let root = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Managed TLS fixture".into(), PASSWORD.into())
        .await
        .unwrap();
    let scope = Arc::new(
        OriginalSessionDeviceIdentity::capture(
            &app,
            &CancellationToken::new(),
            Duration::from_secs(60),
        )
        .await
        .unwrap(),
    );
    let custody = ManagedTlsCustody::capture(&app, scope).await.unwrap();
    (root, app, custody)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_custody_after_last_core_drop_does_not_retain_real_sqlcipher_worker() {
    let _serial = serial().await;
    let (_root, app, custody) = fixture().await;
    let public = custody.create_explicit(&app).await.unwrap();
    let (owner, u) = app.unlocked().await.unwrap();
    let original_owner = Arc::downgrade(&owner);
    let original_unlock = Arc::downgrade(&u);
    // Explicit positive-control sender: this one must keep the real SQLCipher
    // connection alive after bare Core drop. The idle custody must not do so.
    let control = owner.storage().unwrap();
    let db = control.path().unwrap().to_path_buf();
    let sidecar = |suffix: &str| {
        let mut path = db.as_os_str().to_os_string();
        path.push(suffix);
        PathBuf::from(path)
    };
    let wal = sidecar("-wal");
    let shm = sidecar("-shm");
    assert!(
        wal.is_file() && shm.is_file(),
        "owned SQLCipher is actually in WAL mode before retirement"
    );
    let profile = owner.profile_id;
    drop(u);
    drop(owner);
    drop(app); // Deliberately NO shutdown()/lock()/close().
    assert!(original_owner.upgrade().is_none());
    assert!(original_unlock.upgrade().is_none());
    assert!(custody.scope.check_current().is_err());
    assert_eq!(
        control.get_profile().await.unwrap().unwrap().profile_id,
        profile
    );
    assert!(
        wal.is_file() && shm.is_file(),
        "explicit sender keeps positive-control worker open"
    );
    drop(control);
    // The existing Storage worker exits when its final sender drops. SQLite's
    // actual final connection close removes owned WAL/SHM; no close/reset or
    // delete is performed by this test. Old custody.storage prevents this.
    tokio::time::timeout(Duration::from_secs(5), async {
        while wal.exists() || shm.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("idle custody must not retain the real SQLCipher connection");
    assert!(db.is_file());
    assert!(
        custody
            .secure
            .get(&custody.secret_name(public.identity_id))
            .unwrap()
            .is_some(),
        "device-level stored TLS identity is retained; this does not claim OS secret deletion"
    );
    drop(custody);
}
#[test]
fn actual_generated_p256_certificate_matches_bounded_private_key_and_rejects_substitution() {
    let generated = generate().unwrap();
    assert!(generated.key.len() <= 2048);
    let (_, cert) = x509_parser::parse_x509_certificate(&generated.public.certificate_der).unwrap();
    assert_eq!(
        cert.public_key().algorithm.algorithm.to_id_string(),
        "1.2.840.10045.2.1"
    );
    assert_eq!(
        cert.public_key()
            .algorithm
            .parameters
            .as_ref()
            .unwrap()
            .as_oid()
            .unwrap()
            .to_id_string(),
        "1.2.840.10045.3.1.7"
    );
    validate_pair(&generated.public, &generated.key).unwrap();
    let other = generate().unwrap();
    let mut forged = generated.public.clone();
    forged.private_key_sha256 = hash(&other.key);
    assert!(
        validate_pair(&forged, &other.key).is_err(),
        "digest equality alone cannot replace actual certificate/key match"
    );
}

#[test]
fn genuine_expired_p256_certificate_returns_closed_expiry_reason_without_replacement() {
    let pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let key = Zeroizing::new(pair.serialize_der());
    let mut params = CertificateParams::new(vec!["cc-tunnel-client.invalid".into()]).unwrap();
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(2020, 2, 1);
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    let cert = params.self_signed(&pair).unwrap().der().to_vec();
    let public = TunnelTlsPublic {
        identity_id: Uuid::new_v4(),
        certificate_sha256: hash(&cert),
        private_key_sha256: hash(&key),
        certificate_der: cert,
    };
    let before = public.clone();
    assert!(
        matches!(validate_pair(&public,&key),Err(AppError::InvalidInput{field,reason})if field=="agent_tunnel"&&reason=="tls_identity_expired")
    );
    assert_eq!(
        public, before,
        "expiry only refuses use; it cannot rotate or rewrite the frozen identity"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_create_load_reunlock_uses_same_identity_and_original_scope_retires() {
    let _serial = serial().await;
    let (_root, app, custody) = fixture().await;
    let public = custody.create_explicit(&app).await.unwrap();
    assert_eq!(custody.load(&app).await.unwrap().public, public);
    assert!(
        custody.create_explicit(&app).await.is_err(),
        "no overwrite/rotation implicit in creation"
    );
    app.lock().await.unwrap();
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(custody.load(&app).await.is_err());
    let fresh = Arc::new(
        OriginalSessionDeviceIdentity::capture(
            &app,
            &CancellationToken::new(),
            Duration::from_secs(60),
        )
        .await
        .unwrap(),
    );
    let reopened = ManagedTlsCustody::capture(&app, fresh).await.unwrap();
    assert_eq!(reopened.load(&app).await.unwrap().public, public);
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_or_changed_secure_key_and_changed_head_never_recreate() {
    let _serial = serial().await;
    let (_root, app, custody) = fixture().await;
    let public = custody.create_explicit(&app).await.unwrap();
    let name = custody.secret_name(public.identity_id);
    let original = custody.secure.get(&name).unwrap().unwrap();
    custody.secure.delete(&name).unwrap();
    assert!(custody.load(&app).await.is_err());
    assert!(custody.create_explicit(&app).await.is_err());
    assert!(custody.secure.get(&name).unwrap().is_none());
    let other = generate().unwrap();
    custody.secure.set(&name, &other.key).unwrap();
    assert!(custody.load(&app).await.is_err());
    custody.secure.set(&name, original.expose_secret()).unwrap();
    custody
        .secure
        .set(&custody.witness_name(), b"changed")
        .unwrap();
    assert!(custody.load(&app).await.is_err());
    assert!(custody.create_explicit(&app).await.is_err());
    app.shutdown().await.unwrap();
}
#[derive(Debug, Default)]
struct FailHead {
    inner: InMemorySecureStore,
    failed: AtomicBool,
}
impl SecureStore for FailHead {
    fn get(&self, name: &str) -> Result<Option<secrecy::SecretSlice<u8>>, SecureStoreError> {
        self.inner.get(name)
    }
    fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecureStoreError> {
        if name.ends_with("head-v1") {
            self.failed.store(true, Ordering::SeqCst);
            return Err(SecureStoreError::Corrupted);
        }
        self.inner.set(name, secret)
    }
    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        self.inner.delete(name)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_sql_reservation_survives_os_failure_and_no_retry_repairs_it() {
    let _serial = serial().await;
    let (_root, app, mut custody) = fixture().await;
    let failure = Arc::new(FailHead::default());
    custody.secure = failure.clone();
    assert!(custody.create_explicit(&app).await.is_err());
    assert!(failure.failed.load(Ordering::SeqCst));
    let owner = custody.owner.clone();
    let row = custody
        .storage(&app)
        .await
        .unwrap()
        .read(move |tx| tx.tunnel_tls_snapshot(&owner))
        .await
        .unwrap();
    assert_eq!(row.phase(), Some(TunnelTlsPhase::Pending));
    let id = row.public().unwrap().identity_id;
    let before = failure.get(&custody.secret_name(id)).unwrap().unwrap();
    assert!(custody.load(&app).await.is_err());
    assert!(custody.create_explicit(&app).await.is_err());
    assert_eq!(
        failure
            .get(&custody.secret_name(id))
            .unwrap()
            .unwrap()
            .expose_secret(),
        before.expose_secret()
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_certificate_export_is_no_clobber_and_has_no_private_pem() {
    let _serial = serial().await;
    let (root, app, custody) = fixture().await;
    let public = custody.create_explicit(&app).await.unwrap();
    let destination = root.path().join("client.pem");
    custody
        .export_public(&app, destination.clone())
        .await
        .unwrap();
    let raw = std::fs::read(&destination).unwrap();
    let parsed = pem::parse(&raw).unwrap();
    assert_eq!(parsed.tag(), "CERTIFICATE");
    assert_eq!(parsed.contents(), public.certificate_der.as_slice());
    assert!(!String::from_utf8_lossy(&raw).contains("PRIVATE"));
    assert!(
        custody
            .export_public(&app, destination.clone())
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&destination).unwrap(), raw);
    app.shutdown().await.unwrap();
}

#[derive(Debug)]
struct HeldKeyWrite {
    inner: InMemorySecureStore,
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}
impl SecureStore for HeldKeyWrite {
    fn get(&self, name: &str) -> Result<Option<secrecy::SecretSlice<u8>>, SecureStoreError> {
        self.inner.get(name)
    }
    fn set(&self, name: &str, secret: &[u8]) -> Result<(), SecureStoreError> {
        if name.contains("/tunnel-tls/") {
            self.entered
                .send(())
                .map_err(|_| SecureStoreError::Corrupted)?;
            self.release
                .lock()
                .map_err(|_| SecureStoreError::Corrupted)?
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| SecureStoreError::Corrupted)?;
        }
        self.inner.set(name, secret)
    }
    fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        self.inner.delete(name)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_real_os_job_after_lock_cannot_complete_or_be_repaired_after_unlock() {
    let _serial = serial().await;
    let (_root, app, mut custody) = fixture().await;
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (release, held) = std::sync::mpsc::sync_channel(1);
    let gate = Arc::new(HeldKeyWrite {
        inner: InMemorySecureStore::new(),
        entered,
        release: std::sync::Mutex::new(held),
    });
    custody.secure = gate.clone();
    let original_storage = custody.storage(&app).await.unwrap();
    let custody = Arc::new(custody);
    let task = {
        let custody = custody.clone();
        let app = app.clone();
        tokio::spawn(async move { custody.create_explicit(&app).await })
    };
    tokio::task::spawn_blocking(move || observed.recv_timeout(Duration::from_secs(10)).unwrap())
        .await
        .unwrap();
    app.lock().await.unwrap();
    assert!(
        !task.is_finished(),
        "the real OS write remains owned until it actually returns"
    );
    release.send(()).unwrap();
    assert!(task.await.unwrap().is_err());
    assert!(
        gate.get(&custody.witness_name()).unwrap().is_none(),
        "no head publication after stale OS return"
    );
    let owner = custody.owner.clone();
    let closed = original_storage
        .read(move |tx| tx.tunnel_tls_snapshot(&owner))
        .await;
    assert!(
        matches!(closed, Err(cc_storage_core::StorageError::Closed)),
        "real lock closes the original SQL worker; never reuse it after unlock"
    );
    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let scope = Arc::new(
        OriginalSessionDeviceIdentity::capture(
            &app,
            &CancellationToken::new(),
            Duration::from_secs(60),
        )
        .await
        .unwrap(),
    );
    let mut fresh = ManagedTlsCustody::capture(&app, scope).await.unwrap();
    fresh.secure = gate;
    let owner = fresh.owner.clone();
    let row = fresh
        .storage(&app)
        .await
        .unwrap()
        .read(move |tx| tx.tunnel_tls_snapshot(&owner))
        .await
        .unwrap();
    assert_eq!(
        row.phase(),
        Some(TunnelTlsPhase::Pending),
        "the durable pending reservation survives lock and is read through the new worker"
    );
    assert!(fresh.load(&app).await.is_err());
    assert!(fresh.create_explicit(&app).await.is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_account_context_is_distinct_and_sql_account_transition_cannot_adopt_tls_identity() {
    use crate::agent_tunnel::scope::OriginalOwnerContext;
    use cc_sync_core::mock::MockServer;
    let _serial = serial().await;
    let server = MockServer::start().await;
    let a_root = tempfile::tempdir().unwrap();
    let b_root = tempfile::tempdir().unwrap();
    let a = AppCore::new(crate::AppConfig::for_tests(
        a_root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    let b = AppCore::new(crate::AppConfig::for_tests(
        b_root.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    for (app, email) in [(&a, "managed-a@example.org"), (&b, "managed-b@example.org")] {
        app.create_synced_profile(
            "Managed TLS actual account".into(),
            server.url().to_string(),
            email.into(),
            "owned account password42".into(),
            crate::AccountMode::Register,
        )
        .await
        .unwrap();
    }
    a.create_vault(PASSWORD.into()).await.unwrap();
    a.sync_now().await.unwrap();
    let (owner, u) = a.unlocked().await.unwrap();
    u.stop_engine().await;
    drop(u);
    let actual_a = owner.api().unwrap().me().await.unwrap().user_id;
    let actual_b = b
        .session()
        .await
        .unwrap()
        .api()
        .unwrap()
        .me()
        .await
        .unwrap()
        .user_id;
    assert_ne!(actual_a, actual_b);
    let scope = Arc::new(
        OriginalSessionDeviceIdentity::capture(
            &a,
            &CancellationToken::new(),
            Duration::from_secs(60),
        )
        .await
        .unwrap(),
    );
    assert!(
        matches!(scope.owner_context().unwrap(),OriginalOwnerContext::PersonalAccount{user_id}if user_id==actual_a)
    );
    let custody = ManagedTlsCustody::capture(&a, scope).await.unwrap();
    let public = custody.create_explicit(&a).await.unwrap();
    let before = custody.owner.context_fingerprint.clone();
    let original = owner.profile().await.unwrap();
    let mut changed = original.clone();
    changed.user_id = Some(actual_b);
    owner.put_profile(changed).await.unwrap();
    // SAME origin/profile/vault/device/signer; actual authenticated A cannot
    // become B merely because a SQL account row was replaced/imported.
    assert!(custody.load(&a).await.is_err());
    assert!(
        OriginalSessionDeviceIdentity::capture(
            &a,
            &CancellationToken::new(),
            Duration::from_secs(60)
        )
        .await
        .is_err()
    );
    owner.put_profile(original).await.unwrap();
    let fresh = Arc::new(
        OriginalSessionDeviceIdentity::capture(
            &a,
            &CancellationToken::new(),
            Duration::from_secs(60),
        )
        .await
        .unwrap(),
    );
    let recovered = ManagedTlsCustody::capture(&a, fresh).await.unwrap();
    assert_eq!(recovered.owner.context_fingerprint, before);
    assert_eq!(
        recovered.load(&a).await.unwrap().public_identity().unwrap(),
        &public
    );
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}
