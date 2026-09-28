//! ADR-0106 profile transitions against the mock server: local-only mode,
//! enable sync (incl. interrupted upload + restart), reconnect merge,
//! disconnect.

mod common;

use cc_models::host::Host;
use cc_models::secret::{Secret, SecretKind, SecretValue};
use cc_models::{ObjectPayload, VaultObject};
use cc_protocol::{paths, Bytes, DeviceId, VaultId};
use cc_storage_core::{
    DatabaseKey, LocalMutationOutcome, LocalState, Profile, ProfileId, ProfileKind, Storage,
};
use cc_sync_core::mock::{FaultAction, MockServer};
use cc_sync_core::*;
use common::*;
use std::sync::Arc;

const DB_KEY: [u8; 32] = [0x42; 32];

fn p(h: Host) -> ObjectPayload {
    ObjectPayload::new(VaultObject::Host(h))
}

fn renamed(mut h: Host, name: &str) -> Host {
    h.name = name.into();
    h.updated_at = chrono::Utc::now();
    h
}

async fn local_storage(path: Option<&std::path::Path>, device_id: DeviceId) -> Storage {
    let storage = match path {
        Some(p) => Storage::open(p, DatabaseKey::from_bytes(DB_KEY))
            .await
            .unwrap(),
        None => Storage::open_in_memory(DatabaseKey::from_bytes(DB_KEY))
            .await
            .unwrap(),
    };
    storage
        .put_profile(Profile::new_local(
            ProfileId::new(),
            device_id,
            Some("Personal".into()),
        ))
        .await
        .unwrap();
    storage
}

async fn name_of(store: &ObjectStore, id: cc_protocol::ObjectId) -> Option<String> {
    match store.get(id).await.unwrap()?.payload.object {
        VaultObject::Host(h) => Some(h.name),
        _ => None,
    }
}

#[tokio::test]
async fn local_profile_works_without_server_or_outbox() {
    let server = MockServer::start().await;
    let device = DeviceId::new();
    let vault_id = VaultId::new();
    let storage = local_storage(None, device).await;
    let store = ObjectStore::new(vault_id, storage.clone(), TestCodec::new(vault_id));
    let a = Host::new("a", "10.0.0.1");
    let b = Host::new("b", "10.0.0.2");
    assert_eq!(
        store.put(p(a.clone())).await.unwrap(),
        LocalMutationOutcome::LocalOnly {
            revision: 1,
            deleted: false
        }
    );
    store.put(p(b.clone())).await.unwrap();
    assert_eq!(
        store.put(p(renamed(a.clone(), "a2"))).await.unwrap(),
        LocalMutationOutcome::LocalOnly {
            revision: 2,
            deleted: false
        }
    );
    assert_eq!(
        store.delete(b.id).await.unwrap(),
        LocalMutationOutcome::DroppedUnsent
    );
    let (objs, _) = store.list().await.unwrap();
    assert_eq!(objs.len(), 1);
    assert_eq!(objs[0].local_state, LocalState::LocalOnly);
    assert_eq!(store.outbox_counts().await.unwrap().total(), 0);
    let err = SyncEngine::new(store, api_for(&server), fast_config(device))
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::LocalProfile));
    assert_eq!(server.hits(paths::SYNC_PUSH), 0);
}

#[tokio::test]
async fn enable_sync_uploads_and_resumes_after_interruption_and_restart() {
    let server = MockServer::start().await;
    let ka = DeviceKeys::generate();
    let mail = format!("local-{}@example.test", uuid::Uuid::new_v4().simple());
    let (api, acct) = register(&server, &mail, &ka).await;
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("profiles").join("p1").join("vault.db");

    let vault_id = VaultId::new();
    let vak = Bytes::new(random_bytes(32));
    let storage = local_storage(Some(&db_path), ka.device_id).await;
    let store = ObjectStore::new(vault_id, storage.clone(), TestCodec::new(vault_id));
    let mut hosts = Vec::new();
    for i in 0..25 {
        let h = Host::new(format!("h{i}"), format!("10.0.2.{i}"));
        store.put(p(h.clone())).await.unwrap();
        hosts.push(h);
    }
    store
        .put(p(renamed(hosts[0].clone(), "h0-edited")))
        .await
        .unwrap(); // local revision 2
    store.delete(hosts[24].id).await.unwrap();
    let secret = Secret::new(SecretKind::Password, SecretValue::new("s3cret-local"));
    store
        .put(ObjectPayload::new(VaultObject::Secret(secret.clone())))
        .await
        .unwrap();

    let req = EnableSync {
        create_vault: create_vault_request(vault_id, &vak, ka.device_id),
        attest: None,
        profile: Profile::new_synced(
            ProfileId::new(),
            ka.device_id,
            server.url().as_str(),
            Some(acct.user_id),
            Some(mail.clone()),
        ),
    };
    let mut cfg = fast_config(ka.device_id);
    cfg.push_batch_size = 10;

    // The connection drops right after the server committed the first batch.
    server.inject(paths::SYNC_PUSH, 1, FaultAction::LoseResponse);
    let (engine, outcome) = enable_sync(&store, &api, req.clone(), cfg.clone())
        .await
        .unwrap();
    assert_eq!(outcome.path, EnableSyncPath::Created);
    let attach = outcome.attach.unwrap();
    assert_eq!((attach.queued, attach.dropped), (25, 0));
    assert!(outcome.upload.as_ref().is_err_and(|e| e.is_offline()));
    assert_eq!(server.latest_sequence(vault_id), 10);
    assert_eq!(
        storage.get_profile().await.unwrap().unwrap().kind,
        ProfileKind::Synced
    );

    // "Restart": drop everything and reopen the database file.
    drop((engine, store, storage));
    let storage = Storage::open(&db_path, DatabaseKey::from_bytes(DB_KEY))
        .await
        .unwrap();
    let store = ObjectStore::new(vault_id, storage.clone(), TestCodec::new(vault_id));
    assert_eq!(store.outbox_counts().await.unwrap().queued, 25);
    let (engine, outcome) = enable_sync(&store, &api, req, cfg).await.unwrap();
    assert_eq!(outcome.path, EnableSyncPath::Resumed);
    let report = outcome.upload.unwrap();
    assert_eq!(
        (report.pushed, report.replayed),
        (25, 10),
        "persisted mutation ids replayed"
    );
    assert_eq!(
        server.latest_sequence(vault_id),
        25,
        "every object uploaded exactly once"
    );
    assert_eq!(server.live_objects(vault_id).len(), 25);
    let st = engine.status();
    assert_eq!((st.pending, st.phase), (0, SyncPhase::Idle));
    let h0 = store.stored(hosts[0].id).await.unwrap().unwrap();
    assert_eq!(
        (h0.revision, h0.server_revision, h0.local_state),
        (1, 1, LocalState::Synced)
    );

    // A second device sees the uploaded vault.
    let kb = DeviceKeys::generate();
    let api_b = login(&server, &mail, &kb).await;
    let vault = VaultMaterial { vault_id, vak };
    attest(&api_b, &vault, kb.device_id).await;
    let b = client(&server, api_b, kb, vault_id).await;
    sync_ok(&b.engine).await;
    let (objs, failed) = b.store.list().await.unwrap();
    assert!(failed.is_empty());
    assert_eq!(objs.len(), 25);
    assert_eq!(
        name_of(&b.store, hosts[0].id).await.as_deref(),
        Some("h0-edited")
    );
    assert!(name_of(&b.store, hosts[24].id).await.is_none());
    let s = b.store.get(secret.id).await.unwrap().unwrap();
    assert!(
        matches!(s.payload.object, VaultObject::Secret(ref x) if x.value.expose_secret() == "s3cret-local")
    );
}

#[tokio::test]
async fn reconnect_merges_local_changes_with_the_server_copy() {
    let server = MockServer::start().await;
    let (a, b, vault) = two_devices(&server).await;
    let x = Host::new("x", "10.0.3.1");
    let y = Host::new("y", "10.0.3.2");
    let z = Host::new("z", "10.0.3.3");
    let w = Host::new("w", "10.0.3.4");
    for h in [&x, &y, &z, &w] {
        a.engine.put(p(h.clone())).await.unwrap();
    }
    sync_ok(&a.engine).await;
    sync_ok(&b.engine).await;

    // A disconnects (logout, device kept) and keeps working locally.
    let device = a.keys.device_id;
    let out = disconnect(
        &a.store,
        Some(&a.engine),
        Some(&a.api),
        Profile::new_local(ProfileId::new(), device, None),
        DisconnectOptions::default(),
    )
    .await
    .unwrap();
    assert!(out.server_error.is_none());
    a.store.put(p(renamed(y.clone(), "y-local"))).await.unwrap();
    a.store.put(p(renamed(z.clone(), "z-A"))).await.unwrap();
    let n = Host::new("n", "10.0.3.5");
    a.store.put(p(n.clone())).await.unwrap();

    // Meanwhile B changes the server copy.
    b.engine.put(p(renamed(z.clone(), "z-B"))).await.unwrap();
    b.engine.delete(w.id).await.unwrap();
    sync_ok(&b.engine).await;

    // A reconnects: the vault exists → attest, snapshot, merge.
    let email = b.api.me().await.unwrap().email;
    let api_a = login(&server, &email, &a.keys).await;
    let req = EnableSync {
        create_vault: create_vault_request(vault.vault_id, &vault.vak, device),
        attest: Some(attest_request(&vault, device)),
        profile: Profile::new_synced(
            ProfileId::new(),
            device,
            server.url().as_str(),
            None,
            Some(email),
        ),
    };
    let (engine, outcome) = enable_sync(&a.store, &api_a, req, fast_config(device))
        .await
        .unwrap();
    assert_eq!(outcome.path, EnableSyncPath::Reconnected);
    let attach = outcome.attach.unwrap();
    assert_eq!(
        (attach.queued, attach.unchanged),
        (3, 2),
        "y, z, n queued; x, w unchanged"
    );
    let report = outcome.upload.unwrap();
    assert_eq!((report.conflicts, report.resolved), (1, 1));
    assert_eq!(engine.status().pending, 0);

    let srv = |id| server.object(vault.vault_id, id).unwrap();
    assert_eq!(srv(x.id).revision, 1, "untouched object not re-uploaded");
    assert_eq!(srv(y.id).revision, 2);
    assert!(srv(w.id).deleted);
    assert!(!srv(n.id).deleted);
    assert_eq!(
        name_of(&a.store, z.id).await.as_deref(),
        Some("z-B"),
        "remote wins in place"
    );
    assert!(
        name_of(&a.store, w.id).await.is_none(),
        "remote deletion applied"
    );
    let (objs, _) = a.store.list().await.unwrap();
    let copy = objs
        .iter()
        .find(|o| o.conflict_origin == Some(z.id))
        .expect("conflict copy");
    assert!(
        matches!(&copy.payload.object, VaultObject::Host(h) if h.name == format!("z-A{CONFLICT_COPY_SUFFIX}"))
    );
    assert_eq!(objs.len(), 5); // x, y, z, n, copy

    sync_ok(&b.engine).await;
    assert_eq!(name_of(&b.store, y.id).await.as_deref(), Some("y-local"));
    assert_eq!(name_of(&b.store, n.id).await.as_deref(), Some("n"));
    assert_eq!(b.store.list().await.unwrap().0.len(), 5);
}

#[tokio::test]
async fn disconnect_revokes_device_and_keeps_local_data() {
    let server = MockServer::start().await;
    let (a, _b, vault) = two_devices(&server).await;
    let h1 = Host::new("h1", "10.0.4.1");
    let h2 = Host::new("h2", "10.0.4.2");
    a.engine.put(p(h1.clone())).await.unwrap();
    sync_ok(&a.engine).await;
    a.engine.put(p(h2.clone())).await.unwrap(); // never pushed
    a.engine.start();

    let out = disconnect(
        &a.store,
        Some(&a.engine),
        Some(&a.api),
        Profile::new_local(ProfileId::new(), a.keys.device_id, Some("Offline".into())),
        DisconnectOptions {
            revoke_device: true,
            reason: Some("going local".into()),
        },
    )
    .await
    .unwrap();
    assert!(out.server_error.is_none());
    assert_eq!(out.detach.objects, 2);
    assert!(!server.device_trusted(a.keys.device_id, vault.vault_id));
    assert!(!a.api.is_authenticated().await.unwrap());
    assert!(matches!(
        a.engine.sync_now().await,
        Err(SyncError::Stopped(StopReason::Shutdown))
    ));

    let profile = a.storage.get_profile().await.unwrap().unwrap();
    assert_eq!(profile.kind, ProfileKind::Local);
    assert!(profile.server_url.is_none());
    assert_eq!(a.store.outbox_counts().await.unwrap().total(), 0);
    let (objs, _) = a.store.list().await.unwrap();
    assert_eq!(objs.len(), 2);
    assert!(objs.iter().all(|o| o.local_state == LocalState::LocalOnly));
    // Keeps working offline, no outbox.
    let h3 = Host::new("h3", "10.0.4.3");
    assert!(matches!(
        a.store.put(p(h3)).await.unwrap(),
        LocalMutationOutcome::LocalOnly { .. }
    ));
    assert_eq!(a.store.outbox_counts().await.unwrap().total(), 0);
    let _ = Arc::strong_count(&a.codec);
}
