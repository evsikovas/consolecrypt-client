use super::*;
use crate::working_set::Readback;
use crate::{HostDto, config::AppConfig};
use cc_models::{ObjectPayload, VaultObject, host::HostKeyPolicy};
use std::{
    future::Future,
    task::{Context, Poll},
};
use tokio::{io::AsyncReadExt, net::TcpListener};

async fn fixture() -> (tempfile::TempDir, AppCore, String) {
    let dir = tempfile::tempdir().unwrap();
    let app = AppCore::new(AppConfig::for_tests(
        dir.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile(
        "Owned SSH admission".into(),
        "owned SSH admission password42".into(),
    )
    .await
    .unwrap();
    let credential = app
        .add_password_credential(
            "Own target".into(),
            Some("owned".into()),
            "owned SSH target42".into(),
        )
        .await
        .unwrap();
    let mut host = HostDto::new("Original selected host", "127.0.0.1");
    host.port = Some(1);
    host.username = Some("owned".into());
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    let host = app.save_host(host).await.unwrap();
    (dir, app, host.id)
}

#[tokio::test(flavor = "current_thread")]
async fn queued_planning_same_revision_host_change_is_denied_before_any_tcp_dial() {
    let (_dir, app, host_id) = fixture().await;
    let (owner, u) = app.unlocked().await.unwrap();
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let active = app.inner.active.lock().await;
    let snapshot = u.working().snapshot();
    let mutation = u.working().version();
    let host_object_id = crate::dto::parse_id("host", &host_id).unwrap();
    let original = snapshot.get(&host_object_id).unwrap();
    let crate::working_set::Item::Object(object) = &original.item else {
        panic!("host");
    };
    let VaultObject::Host(host) = object.as_ref() else {
        panic!("host");
    };
    let mut changed = host.clone();
    changed.port = Some(target.local_addr().unwrap().port());
    let source = SourceConfig {
        id: Uuid::new_v4().to_string(),
        name: "Own source".into(),
        endpoint: "http://127.0.0.1:9090".into(),
        route: Route::SshHost { host_id },
        tls: cc_monitoring_core::TlsConfig::default(),
        auth: cc_monitoring_core::AuthRef::None,
    };
    let cancel = CancellationToken::new();
    // Real global lock blocks sharing_prepare_connection's first original
    // capture. Mutation is deliberately SAME revision, so revision-only
    // watchers cannot explain the required pre-dial rejection.
    let open = SshSource::open(&app, &owner, &u, &source, &snapshot, mutation, &cancel);
    tokio::pin!(open);
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    assert!(matches!(open.as_mut().poll(&mut cx), Poll::Pending));
    u.working().apply(
        host_object_id,
        Readback::Live(Box::new(cc_sync_core::DecryptedObject {
            object_id: host_object_id,
            revision: original.revision,
            local_state: cc_storage_core::LocalState::LocalOnly,
            conflict_origin: None,
            payload: ObjectPayload::new(VaultObject::Host(changed)),
        })),
    );
    assert_eq!(
        u.working().revision_of(host_object_id),
        Some(original.revision)
    );
    assert_ne!(u.working().mutation_stamp(), mutation);
    drop(active);
    let result = tokio::time::timeout(Duration::from_secs(2), open)
        .await
        .expect("admission must reject before handshake");
    assert!(result.is_err());
    // A synchronous poll observes the already-listening real socket; no sleep
    // or retry can hide a queued-plan connection.
    assert!(matches!(target.poll_accept(&mut cx), Poll::Pending));
}

async fn pending_handshake(expire: bool) {
    let (_dir, app, host_id) = fixture().await;
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut host = app
        .list_hosts()
        .await
        .unwrap()
        .into_iter()
        .find(|h| h.id == host_id)
        .unwrap();
    host.port = Some(target.local_addr().unwrap().port());
    app.save_host(host).await.unwrap();
    let (_owner, u) = app.unlocked().await.unwrap();
    let weak = Arc::downgrade(&u);
    drop(u);
    let cancel = CancellationToken::new();
    let end = Instant::now()
        + if expire {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(5)
        };
    let open =
        app.open_private_runner_session_fenced(&host_id, &weak, &cancel, Some(end), || Ok(()));
    tokio::pin!(open);
    let accepted = async { target.accept().await.unwrap().0 };
    tokio::pin!(accepted);
    // Poll the actual helper and actual peer concurrently until TCP exists,
    // then deliberately withhold the SSH banner (no password can be sent).
    let mut peer = tokio::select! { biased;
        result=&mut open=>panic!("unexpected completed handshake: {}", result.is_ok()),
        peer=&mut accepted=>peer,
    };
    let mut first = [0; 128];
    let n = tokio::select! {biased;
        result=&mut open=>panic!("handshake completed before original deadline/cancel: {}", result.is_ok()),
        value=tokio::time::timeout(Duration::from_secs(1),peer.read(&mut first))=>value.unwrap().unwrap(),
    };
    assert!(n > 0 && first[..n].starts_with(b"SSH-"));
    if !expire {
        cancel.cancel();
    }
    assert!(
        tokio::time::timeout(Duration::from_secs(3), open)
            .await
            .unwrap()
            .is_err()
    );
}
#[tokio::test]
async fn explicit_cancel_during_actual_ssh_handshake_cannot_return_a_session() {
    pending_handshake(false).await;
}
#[tokio::test]
async fn original_deadline_during_actual_ssh_handshake_cannot_return_a_session() {
    pending_handshake(true).await;
}
