use super::*;
use std::{future::Future, task::Poll, time::Duration};

struct Fixture {
    _directory: tempfile::TempDir,
    app: AppCore,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned service ledger fixture".into(),
            "synthetic ledger owner42".into(),
        )
        .await
        .unwrap();
        Self {
            _directory: directory,
            app,
        }
    }
    async fn ledger(&self) -> Arc<OwnerLedger> {
        let (owner, u) = self.app.unlocked().await.unwrap();
        Arc::new(OwnerLedger::capture(&self.app, owner, &u).await.unwrap())
    }
}
// Metadata fixtures exercise the storage adapter only. They do not construct
// PreparedReview, PendingDelivery, signer, service ACK or execution authority.
fn metadata() -> ServiceOwnerRecord {
    ServiceOwnerRecord {
        service_id: Uuid::new_v4(),
        node_local_profile: "cc-service-local-v1-test-only".into(),
        owner_epoch: 1,
        policy_revision: 1,
        revocation_epoch: 1,
        clock_floor: 100,
        signer_fingerprint: "b".repeat(64),
        recipient_fingerprint: "c".repeat(64),
        route_trust_digest: "d".repeat(64),
        policy_digest: "e".repeat(64),
        status_digest: "f".repeat(64),
        delivery: ServiceOwnerDelivery::DeliveryUnknown,
    }
}

#[tokio::test]
async fn actual_original_session_derives_owner_and_unknown_metadata_survives_new_capture() {
    let f = Fixture::new().await;
    let ledger = f.ledger().await;
    assert!(ledger.owner.dir.is_some());
    assert_eq!(ledger.identity.profile_id, ledger.owner.profile_id);
    assert_eq!(ledger.identity.device_id, ledger.owner.device_id());
    assert_eq!(
        ledger.identity.signing_fingerprint,
        digest(&ledger.owner.identity().public_keys().signing)
    );
    let snapshot = ledger.read(&f.app).await.unwrap();
    assert!(snapshot.revision().is_none());
    let row = metadata();
    ledger
        .replace(&f.app, snapshot, vec![row.clone()])
        .await
        .unwrap();
    drop(ledger);
    let fresh = f.ledger().await;
    let observed = fresh.read(&f.app).await.unwrap();
    assert_eq!(observed.revision(), Some(1));
    assert!(observed.records()[0] == row);
    assert!(observed.records()[0].delivery == ServiceOwnerDelivery::DeliveryUnknown);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn exact_snapshot_cas_cannot_overwrite_another_transition_or_drop_tombstone() {
    let f = Fixture::new().await;
    let ledger = f.ledger().await;
    let first = ledger.read(&f.app).await.unwrap();
    let stale = ledger.read(&f.app).await.unwrap();
    let row = metadata();
    ledger
        .replace(&f.app, first, vec![row.clone()])
        .await
        .unwrap();
    assert!(ledger
        .replace(&f.app, stale, vec![metadata()])
        .await
        .is_err());
    ledger.revoke_local(&f.app, row.service_id).await.unwrap();
    let snapshot = ledger.read(&f.app).await.unwrap();
    let retained = &snapshot.records()[0];
    assert!(retained.delivery == ServiceOwnerDelivery::RevokedLocally);
    assert_eq!(retained.revocation_epoch, 2);
    assert!(retained.clock_floor >= now().unwrap() - 1);
    assert!(ledger.replace(&f.app, snapshot, vec![row]).await.is_err());
    assert!(
        ledger.read(&f.app).await.unwrap().records()[0].delivery
            == ServiceOwnerDelivery::RevokedLocally
    );
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_profile_lock_reunlock_does_not_recapture_an_old_ledger_subject() {
    let f = Fixture::new().await;
    let ledger = f.ledger().await;
    let original_scope = ledger.scope.clone();
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("synthetic ledger owner42".into())
        .await
        .unwrap();
    assert!(ledger.read(&f.app).await.is_err());
    assert!(ledger.revoke_local(&f.app, Uuid::new_v4()).await.is_err());
    let new = f.ledger().await;
    assert_ne!(original_scope, new.scope);
    assert!(new.read(&f.app).await.unwrap().revision().is_none());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn actual_queued_sql_rechecks_identity_before_any_ledger_mutation() {
    let f = Fixture::new().await;
    let ledger = f.ledger().await;
    let absent = ledger.read(&f.app).await.unwrap();
    let storage = ledger.storage.clone();
    let (entered, wait_entered) = tokio::sync::oneshot::channel();
    let (release, wait_release) = std::sync::mpsc::channel();
    let blocker = tokio::spawn(async move {
        storage
            .write(move |_tx| -> AppResult<()> {
                entered.send(()).unwrap();
                wait_release
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|_| invalid())?;
                Ok(())
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), wait_entered)
        .await
        .unwrap()
        .unwrap();
    let replace = ledger.replace(&f.app, absent, vec![metadata()]);
    tokio::pin!(replace);
    std::future::poll_fn(|cx| match replace.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("must wait for the actual blocked SQL worker"),
    })
    .await;
    assert!(
        f.app.inner.active.try_lock().is_err(),
        "bounded original profile guard spans SQL"
    );
    ledger
        .owner
        .set_identity(Arc::new(cc_vault_core::DeviceIdentity::generate().unwrap()));
    release.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(10), &mut replace)
        .await
        .unwrap()
        .is_err());
    blocker.await.unwrap().unwrap();
    let raw: Option<serde_json::Value> = ledger
        .storage
        .setting_get("cc.native.service.owner.v1")
        .await
        .unwrap();
    assert!(
        raw.is_none(),
        "original identity changed before queued closure: no row write"
    );
    drop(replace);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_reader_role_and_synced_profile_sql_rows_cannot_use_owner_ledger() {
    for change in ["reader", "synced"] {
        let f = Fixture::new().await;
        let ledger = f.ledger().await;
        let vault = ledger.identity.vault_id;
        ledger
            .storage
            .write(move |tx| -> AppResult<()> {
                if change == "reader" {
                    let mut row = tx.get_vault(vault)?.unwrap();
                    row.role = cc_protocol::vaults::VaultRole::Viewer;
                    tx.put_vault(&row)?;
                } else {
                    let mut row = tx.get_profile()?.unwrap();
                    row.kind = ProfileKind::Synced;
                    row.server_url = Some("https://owned-invalid.example".into());
                    tx.put_profile(&row)?;
                }
                Ok(())
            })
            .await
            .unwrap();
        assert!(ledger.read(&f.app).await.is_err());
        let raw: Option<serde_json::Value> = ledger
            .storage
            .setting_get("cc.native.service.owner.v1")
            .await
            .unwrap();
        assert!(raw.is_none());
        f.app.shutdown().await.unwrap();
    }
}

#[test]
fn persistent_namespace_requires_both_actual_holder_epochs_and_legacy_none_is_unchanged() {
    let namespace=Some(cc_automation_core::ServiceLocalNamespace{version:1,identity_id:Uuid::new_v4(),key_epoch:1,namespace:"metadata-only-test".into()});
    assert!(super::super::namespace_epochs(&namespace,1,1).is_ok());
    assert!(super::super::namespace_epochs(&namespace,2,1).is_err());
    assert!(super::super::namespace_epochs(&namespace,1,2).is_err());
    assert!(super::super::namespace_epochs(&None,2,3).is_ok());
}
