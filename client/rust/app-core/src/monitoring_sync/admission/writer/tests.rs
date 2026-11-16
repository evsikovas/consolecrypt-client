//! Actual encrypted stores plus fixed controlled metadata. The owned test-only
//! candidate is NOT a release, and the mock response is NOT server opt-in proof.
use super::*;
use crate::AccountMode;
use cc_models::ObjectPayload;
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use cc_sync_core::mock::{FaultAction, MockServer, MockServerConfig};
use serde_json::{json, Value};
use std::{
    future::{poll_fn, Future},
    task::Poll,
};
const PASS: &str = "owned encrypted writer vault phrase42";
const ACCOUNT: &str = "owned encrypted writer account phrase42";
const QUERY: &str = "sum(owned_private_panel_marker{job=\"explicit\"})";
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
    source: String,
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
        let result = a
            .monitoring_source_create(
                0,
                SourceConfig {
                    id: String::new(),
                    name: "Owned source".into(),
                    endpoint: "https://fixture.invalid/prometheus".into(),
                    route: Route::Direct,
                    tls: TlsConfig::default(),
                    auth: AuthRef::None,
                },
            )
            .await
            .unwrap();
        a.sync_now().await.unwrap();
        Self {
            _dir: dir,
            server,
            a,
            email,
            source: result.sources[0].id.clone(),
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
    fn definition(&self, query: &str) -> crate::monitoring_sources::MonitoringPanelDefinition {
        crate::monitoring_sources::MonitoringPanelDefinition {
            title: "Owned panel".into(),
            source_id: self.source.clone(),
            source_revision: 1,
            query: query.into(),
            unit_hint: crate::monitoring_sources::MonitoringUnitHint::Unknown,
        }
    }
    async fn admission(&self) -> PanelWriteAdmission {
        self.enabled(&self.a).await;
        let (owner, u) = self.a.unlocked().await.unwrap();
        let revision = catalog_checkpoint(&u).unwrap().revision;
        PanelWriteAdmission::prepare(
            &self.a,
            owner,
            u,
            revision,
            PanelWritePolicy::ReviewedCandidate,
        )
        .await
        .unwrap()
    }
}
async fn raw(app: &AppCore) -> Value {
    let (_, u) = app.unlocked().await.unwrap();
    u.working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .unwrap()
        .monitoring
        .unwrap()
}
async fn object(app: &AppCore, name: &str) -> VaultObject {
    let mut settings = app.get_vault_settings().await.unwrap();
    settings.vault_name = name.into();
    // DTO get_vault_settings differs from the encrypted model: retain the real
    // existing object, changing one harmless field rather than forging payloads.
    let (_, u) = app.unlocked().await.unwrap();
    let mut model = u
        .working()
        .all_vault_settings()
        .into_iter()
        .find(|s| s.id.to_string() == settings.id)
        .unwrap();
    model.vault_name = settings.vault_name;
    VaultObject::VaultSettings(model)
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
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn admitted_create_and_update_use_real_encryption_and_roundtrip_between_two_appcores() {
    let f = Fixture::new().await;
    f.enabled(&f.a).await;
    let created =
        f.a.monitoring_panel_create_with_policy(
            1,
            f.definition(QUERY),
            PanelWritePolicy::ReviewedCandidate,
        )
        .await
        .unwrap();
    let panel = created.panels.unwrap()[0].id.clone();
    f.a.sync_now().await.unwrap();
    let (owner, u) = f.a.unlocked().await.unwrap();
    let vault = u.vault_id;
    let id = catalog_checkpoint(&u).unwrap().settings_id;
    drop(u);
    drop(owner);
    ciphertext(&f.server, vault, id, QUERY);
    let b = f.login("b", "2.0.0").await;
    assert_eq!(
        b.monitoring_panel_definition(panel.clone(), 1)
            .await
            .unwrap()
            .query,
        QUERY
    );
    assert_eq!(raw(&f.a).await, raw(&b).await);
    f.enabled(&b).await;
    let next = "sum(owned_private_updated_marker)";
    b.monitoring_panel_update_with_policy(
        panel.clone(),
        2,
        1,
        f.definition(next),
        PanelWritePolicy::ReviewedCandidate,
    )
    .await
    .unwrap();
    b.sync_now().await.unwrap();
    f.a.sync_now().await.unwrap();
    assert_eq!(
        f.a.monitoring_panel_definition(panel.clone(), 2)
            .await
            .unwrap()
            .query,
        next
    );
    assert_eq!(raw(&f.a).await, raw(&b).await);
    ciphertext(&f.server, vault, id, next);
    // Same encrypted v3 survives an ordinary settings edit on the other client.
    let mut generic = f.a.get_vault_settings().await.unwrap();
    generic.vault_name = "Keep admitted panels".into();
    f.a.save_vault_settings(generic).await.unwrap();
    f.a.sync_now().await.unwrap();
    b.sync_now().await.unwrap();
    assert_eq!(
        b.monitoring_panel_definition(panel, 2).await.unwrap().query,
        next
    );
    assert!(f.server.request_proof_stats().valid > 0);
    assert_eq!(f.server.request_proof_stats().rejected, 0);
    assert_eq!(RELEASED_PRESERVATION_FLOOR, Some("1.2.0"));
    b.shutdown().await.unwrap();
    f.a.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn public_operator_off_old_client_and_real_dirty_outbox_cannot_admit_synced_panels() {
    let f = Fixture::new().await;
    let before = raw(&f.a).await;
    let (owner, u) = f.a.unlocked().await.unwrap();
    let path = format!(
        "/v1/vaults/{}/compatibility/monitoring-settings",
        u.vault_id
    );
    drop(u);
    drop(owner);
    let hits = f.server.hits(&path);
    assert!(f
        .a
        .monitoring_panel_create(1, f.definition(QUERY))
        .await
        .is_err());
    assert_eq!(f.server.hits(&path), hits + 1);
    assert_eq!(raw(&f.a).await, before);
    let old = f.login("old", "1.9.9").await;
    assert!(old
        .monitoring_panel_create_with_policy(
            1,
            f.definition(QUERY),
            PanelWritePolicy::ReviewedCandidate
        )
        .await
        .is_err());
    assert_eq!(f.server.hits(&path), hits + 1);
    old.shutdown().await.unwrap();
    let mut generic = f.a.get_vault_settings().await.unwrap();
    generic.vault_name = "Actual unsent mutation".into();
    f.a.save_vault_settings(generic).await.unwrap();
    let (owner, u) = f.a.unlocked().await.unwrap();
    let vault = u.vault_id;
    assert!(
        owner
            .storage()
            .unwrap()
            .read(move |tx| tx.outbox_counts(vault))
            .await
            .unwrap()
            .total()
            > 0
    );
    drop(u);
    drop(owner);
    f.enabled(&f.a).await;
    assert!(f
        .a
        .monitoring_panel_create_with_policy(
            1,
            f.definition(QUERY),
            PanelWritePolicy::ReviewedCandidate
        )
        .await
        .is_err());
    assert_eq!(raw(&f.a).await, before);
    assert_eq!(
        f.a.get_vault_settings().await.unwrap().vault_name,
        "Actual unsent mutation"
    );
    f.a.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_sql_queue_epoch_race_and_held_auth_or_removed_unlock_fail_closed_without_deadlock()
{
    let f = Fixture::new().await;
    let (owner, u) = f.a.unlocked().await.unwrap();
    let storage = owner.storage().unwrap();
    let store = u.store().clone();
    let vault = u.vault_id;
    let id = catalog_checkpoint(&u).unwrap().settings_id;
    drop(u);
    drop(owner);
    for dirty in [false, true] {
        let admission = f.admission().await;
        let before = storage
            .read(move |tx| tx.get_object(vault, id))
            .await
            .unwrap()
            .unwrap();
        let cursor = storage
            .read(move |tx| tx.get_sync_cursor(vault))
            .await
            .unwrap();
        let (_, u) = f.a.unlocked().await.unwrap();
        let version = u.working().version();
        drop(u);
        let payload = object(&f.a, "Must not be written").await;
        let predecessor = ObjectPayload::new(object(&f.a, "Actual queued predecessor").await);
        let codec = store.codec().clone();
        let (entered, seen) = tokio::sync::oneshot::channel();
        let (release, receive) = std::sync::mpsc::sync_channel(1);
        let queued = storage.clone();
        let blocked = tokio::spawn(async move {
            queued
                .call(move |db| {
                    entered.send(()).unwrap();
                    receive.recv_timeout(Duration::from_secs(5)).unwrap();
                    db.write(|tx| {
                        if dirty {
                            // Real encrypted/outbox mutation on the predecessor SQL
                            // turn, deliberately without a WorkingSet/event refresh.
                            tx.record_local_put::<SyncError, _>(
                                vault,
                                id,
                                Some(predecessor.object.kind().kek_class()),
                                |rev| {
                                    codec
                                        .encrypt(id, rev, &predecessor)
                                        .map_err(SyncError::from)
                                },
                            )?;
                        } else {
                            let mut c = tx.get_sync_cursor(vault)?;
                            c.epoch = Some(uuid::Uuid::new_v4());
                            tx.put_sync_cursor(&c)?;
                        }
                        Ok::<_, SyncError>(())
                    })
                })
                .await
        });
        seen.await.unwrap();
        let mut write = Box::pin(admission.commit(&f.a, payload, |_| Ok(())));
        // All original async locks are free; first Pending is the blocked
        // SQL worker. The predecessor changes SQL, not WorkingSet/version.
        poll_fn(|cx| {
            assert!(write.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        release.send(()).unwrap();
        blocked.await.unwrap().unwrap();
        // The pending caller has not performed readback yet. The queued CAS
        // must reject SQL changes even with the same in-memory draft version.
        let (_, u) = f.a.unlocked().await.unwrap();
        assert_eq!(u.working().version(), version);
        drop(u);
        assert!(tokio::time::timeout(Duration::from_secs(3), write)
            .await
            .unwrap()
            .is_err());
        let after = storage
            .read(move |tx| tx.get_object(vault, id))
            .await
            .unwrap()
            .unwrap();
        if dirty {
            assert_eq!(after.revision, before.revision + 1);
            assert_eq!(after.local_state, LocalState::Pending);
            assert!(
                storage
                    .read(move |tx| tx.outbox_counts(vault))
                    .await
                    .unwrap()
                    .total()
                    > 0
            );
            let actual = store.get(id).await.unwrap().unwrap();
            let VaultObject::VaultSettings(s) = actual.payload.object else {
                panic!("wrong object")
            };
            assert_eq!(s.vault_name, "Actual queued predecessor");
            f.a.sync_now().await.unwrap();
            let (_, u) = f.a.unlocked().await.unwrap();
            u.writer.reload_all().await.unwrap();
        } else {
            assert_eq!(before, after);
            // Only undo this owned injected cursor fault with the exact
            // original cursor. No policy/auth witness is constructed.
            storage
                .write(move |tx| tx.put_sync_cursor(&cursor))
                .await
                .unwrap();
        }
    }
    let admission = f.admission().await;
    for provenance in [false, true] {
        let original = admission.0.clone();
        let held = original.clone();
        let (entered, seen) = tokio::sync::oneshot::channel();
        let (release, receive) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let wait = || {
                entered.send(()).unwrap();
                receive.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            };
            if provenance {
                let owner = held.owner.upgrade().unwrap();
                held.provenance.with_current(&owner, wait)
            } else {
                held.auth.with_current(wait)
            }
        });
        seen.await.unwrap();
        let sql = storage.clone();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            sql.call(move |_| {
                let mut invoked = false;
                let result = original.run(&mut || {
                    invoked = true;
                    Ok(())
                });
                assert!(!invoked);
                Ok::<_, SyncError>(result.is_err())
            }),
        )
        .await
        .expect("SQL must not wait on held auth/provenance")
        .unwrap();
        assert!(result);
        release.send(()).unwrap();
        thread.join().unwrap().unwrap();
    }
    // A real async profile/slot writer can be awaiting the SQL worker too.
    // Holding either exclusive lock must fail without waiting on that lock.
    let active = f.a.inner.active.lock().await;
    let original = admission.0.clone();
    assert!(tokio::time::timeout(
        Duration::from_secs(1),
        storage.call(move |_| {
            let mut invoked = false;
            let result = original.run(&mut || {
                invoked = true;
                Ok(())
            });
            assert!(!invoked);
            Ok::<_, SyncError>(result.is_err())
        })
    )
    .await
    .unwrap()
    .unwrap());
    drop(active);
    // Exact original slot has been taken by real lock, even while SQL is held.
    let payload = object(&f.a, "Must not cross lock").await;
    let (entered, seen) = tokio::sync::oneshot::channel();
    let (release, receive) = std::sync::mpsc::sync_channel(1);
    let sql = storage.clone();
    let blocked = tokio::spawn(async move {
        sql.call(move |_| {
            entered.send(()).unwrap();
            receive.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok::<_, SyncError>(())
        })
        .await
    });
    seen.await.unwrap();
    let mut write = Box::pin(admission.commit(&f.a, payload, |_| Ok(())));
    poll_fn(|cx| {
        assert!(write.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let lock_app = f.a.clone();
    let lock = tokio::spawn(async move { lock_app.lock().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.a.is_unlocked().await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    release.send(()).unwrap();
    blocked.await.unwrap().unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(3), write)
        .await
        .unwrap()
        .is_err());
    lock.await.unwrap().unwrap();
    f.a.unlock_with_passphrase(PASS.into()).await.unwrap();
    assert_ne!(
        f.a.get_vault_settings().await.unwrap().vault_name,
        "Must not cross lock"
    );
    f.a.shutdown().await.unwrap();
}
#[tokio::test]
async fn checked_write_cannot_skip_repeat_or_hide_a_committed_mutation_event() {
    let dir = tempfile::tempdir().unwrap();
    let a = app(dir.path(), "2.0.0");
    a.create_local_profile("Owned checked kernel".into(), PASS.into())
        .await
        .unwrap();
    let (_, u) = a.unlocked().await.unwrap();
    let store = u.store().clone();
    let settings = u.working().all_vault_settings().into_iter().next().unwrap();
    let id = settings.id;
    let before = store.get(id).await.unwrap().unwrap().revision;
    let mut events = store.subscribe();
    let payload = ObjectPayload::new(VaultObject::VaultSettings(settings.clone()));
    assert!(store
        .put_checked(payload, |_| Ok(()), |_| Ok(()))
        .await
        .is_err());
    assert_eq!(store.get(id).await.unwrap().unwrap().revision, before);
    assert!(events.try_recv().is_err());
    let payload = ObjectPayload::new(VaultObject::VaultSettings(settings.clone()));
    assert!(store
        .put_checked(payload, |write| write(), |_| Err(failed()))
        .await
        .is_err());
    assert_eq!(store.get(id).await.unwrap().unwrap().revision, before);
    assert!(events.try_recv().is_err());
    let payload = ObjectPayload::new(VaultObject::VaultSettings(settings.clone()));
    assert!(store
        .put_checked(
            payload,
            |write| {
                write()?;
                let _ = write();
                Ok(())
            },
            |_| Ok(())
        )
        .await
        .is_err());
    assert_eq!(store.get(id).await.unwrap().unwrap().revision, before + 1);
    assert!(
        matches!(events.try_recv().unwrap(),cc_sync_core::SyncEvent::ObjectChanged{object_id,..} if object_id==id)
    );
    assert!(events.try_recv().is_err());
    let payload = ObjectPayload::new(VaultObject::VaultSettings(settings));
    assert!(store
        .put_checked(
            payload,
            |write| {
                write()?;
                Err(failed())
            },
            |_| Ok(())
        )
        .await
        .is_err());
    assert_eq!(store.get(id).await.unwrap().unwrap().revision, before + 2);
    assert!(
        matches!(events.try_recv().unwrap(),cc_sync_core::SyncEvent::ObjectChanged{object_id,..} if object_id==id)
    );
    assert!(events.try_recv().is_err());
    drop(u);
    a.shutdown().await.unwrap();
}

mod automation;
