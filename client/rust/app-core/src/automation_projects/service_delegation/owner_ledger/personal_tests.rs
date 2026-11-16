//! Real authenticated PERSONAL SQLCipher lifecycle. Record fixtures are DATA
//! only: no PreparedReview, Node ACK, signer or service job is fabricated.
use super::*;
use crate::service_credential_issuer::PersonalServiceFixture as Fixture;
use std::{future::Future, task::Poll, time::Duration};

fn metadata() -> ServiceOwnerRecord {
    ServiceOwnerRecord {
        service_id: Uuid::new_v4(),
        node_local_profile: "cc-service-local-v1-owned-test".into(),
        owner_epoch: 1,
        policy_revision: 1,
        revocation_epoch: 1,
        clock_floor: 100,
        signer_fingerprint: "a".repeat(64),
        recipient_fingerprint: "b".repeat(64),
        route_trust_digest: "c".repeat(64),
        policy_digest: "d".repeat(64),
        status_digest: "e".repeat(64),
        delivery: ServiceOwnerDelivery::DeliveryUnknown,
    }
}
async fn fresh(f: &Fixture) -> Arc<OwnerLedger> {
    let (owner, weak) = f.subject().await;
    Arc::new(
        OwnerLedger::capture_weak(&f.app, owner, weak)
            .await
            .unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn personal_ledger_pin_is_actual_account_owner_and_origin_with_exact_cas() {
    let f = Fixture::new().await;
    let ledger = fresh(&f).await;
    let me = ledger.owner.api().unwrap().me().await.unwrap();
    let pin = ledger.identity.personal.as_ref().unwrap();
    assert_eq!(pin.user_id, me.user_id);
    assert_ne!(
        pin.user_id.to_string(),
        ledger.identity.profile_id.to_string()
    );
    let before = ledger.read(&f.app).await.unwrap();
    let stale = ledger.read(&f.app).await.unwrap();
    let row = metadata();
    ledger
        .replace(&f.app, before, vec![row.clone()])
        .await
        .unwrap();
    assert!(ledger
        .replace(&f.app, stale, vec![metadata()])
        .await
        .is_err());
    let observed = ledger.read(&f.app).await.unwrap();
    assert_eq!(observed.records().len(), 1);
    assert!(observed.records()[0] == row);
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offline_expired_auth_can_only_block_existing_exact_metadata_not_export_or_create() {
    let f = Fixture::new().await;
    let ledger = fresh(&f).await;
    let row = metadata();
    let id = row.service_id;
    let absent = ledger.read(&f.app).await.unwrap();
    ledger.replace(&f.app, absent, vec![row]).await.unwrap();
    let review = f
        .app
        .service_credential_review(
            ledger.owner.profile_id.to_string(),
            f.credential.clone(),
            Fixture::recipient(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    ledger.owner.api().unwrap().clear_tokens().await.unwrap();
    f.server.set_offline(true);
    let before = f.server.hits(cc_protocol::paths::AUTH_ME);
    assert!(ledger.read(&f.app).await.is_err());
    assert!(review.approve_and_seal(&f.app).await.is_err());
    let stale = Arc::new(
        OwnerLedger::capture_stale(&f.app, ledger.owner.clone(), ledger.unlock.clone())
            .await
            .unwrap(),
    );
    assert!(stale.read(&f.app).await.is_err());
    let original = stale.read_stale_metadata(&f.app).await.unwrap();
    let blocked = stale.block_existing_local(&f.app, id).await.unwrap();
    let _: LocallyBlockedRemotelyUnknown = blocked;
    let after = stale.read_stale_metadata(&f.app).await.unwrap();
    assert!(after.records()[0].delivery == ServiceOwnerDelivery::RevokedLocally);
    assert_eq!(
        after.records()[0].revocation_epoch,
        original.records()[0].revocation_epoch + 1
    );
    assert_eq!(
        after.records()[0].policy_digest,
        original.records()[0].policy_digest
    );
    assert!(stale
        .block_existing_local(&f.app, Uuid::new_v4())
        .await
        .is_err());
    assert_eq!(
        f.server.hits(cc_protocol::paths::AUTH_ME),
        before,
        "denial does no network observation/sign/forward"
    );
    f.server.set_offline(false);
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_metadata_cannot_create_or_adopt_another_owner_record_or_survive_unlock_reopen() {
    let f = Fixture::new().await;
    let (owner, weak) = f.subject().await;
    owner.api().unwrap().clear_tokens().await.unwrap();
    let stale = Arc::new(
        OwnerLedger::capture_stale(&f.app, owner.clone(), weak.clone())
            .await
            .unwrap(),
    );
    assert!(stale
        .block_existing_local(&f.app, Uuid::new_v4())
        .await
        .is_err());
    assert!(stale
        .read_stale_metadata(&f.app)
        .await
        .unwrap()
        .revision()
        .is_none());
    f.app.lock().await.unwrap();
    f.app
        .unlock_with_passphrase("owned personal service vault42".into())
        .await
        .unwrap();
    assert!(stale.read_stale_metadata(&f.app).await.is_err());
    assert!(stale
        .block_existing_local(&f.app, Uuid::new_v4())
        .await
        .is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_blocked_logout_entry_revokes_queued_sql_and_selected_capsule_before_token_clear() {
    let f = Fixture::new().await;
    let ledger = fresh(&f).await;
    let absent = ledger.read(&f.app).await.unwrap();
    let review = f
        .app
        .service_credential_review(
            ledger.owner.profile_id.to_string(),
            f.credential.clone(),
            Fixture::recipient(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let storage = ledger.storage.clone();
    let (entered, wait_entered) = tokio::sync::oneshot::channel();
    let (release, wait_release) = std::sync::mpsc::channel();
    let barrier = tokio::spawn(async move {
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
        Poll::Ready(_) => panic!("must queue behind actual SQL barrier"),
    })
    .await;
    assert!(f.app.inner.active.try_lock().is_err());
    let mut logout = Box::pin(f.app.logout());
    std::future::poll_fn(|cx| match logout.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("logout must wait on original profile guard"),
    })
    .await;
    // Tokens are not cleared; the existing entry epoch itself must cancel.
    assert!(ledger
        .owner
        .api()
        .unwrap()
        .is_authenticated()
        .await
        .unwrap());
    drop(logout);
    release.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(10), &mut replace)
        .await
        .unwrap()
        .is_err());
    barrier.await.unwrap().unwrap();
    drop(replace);
    assert!(review.approve_and_seal(&f.app).await.is_err());
    let value: Option<serde_json::Value> = ledger
        .storage
        .setting_get("cc.native.service.owner.v1")
        .await
        .unwrap();
    assert!(
        value.is_none(),
        "no local consent metadata after logout entry"
    );
    f.app.shutdown().await.unwrap();
}
