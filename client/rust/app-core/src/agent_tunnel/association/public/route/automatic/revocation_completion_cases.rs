//! Shared actual original completion assertions; no Node/Caller authority.
use super::*;

const PASSWORD:&str="owned retained original tunnel test password 42";
async fn local()->(tempfile::TempDir,AppCore,Frame) {
    let root=tempfile::tempdir().unwrap();
    let app=AppCore::new(crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned())).unwrap();
    app.create_local_profile("Retained original test".into(),PASSWORD.into()).await.unwrap();
    let(session,unlocked)=app.unlocked().await.unwrap();
    let frame=Frame{version:1,profile_id:session.profile_id.to_string(),
        notification_scope_id:unlocked.notices.scope_id().unwrap()};
    (root,app,frame)
}


pub(super) async fn blocked_then_transport_eof_waits_for_verified_original_revocation_completion() {
    let(_root,app,frame)=local().await;
    let context=Context::capture(&app,frame).await.unwrap();
    let(tx,rx)=watch::channel(RevocationCompletion::Pending);
    let original=OriginalRevocation{context:context.clone(),sender:Some(tx)};
    // This is the actual terminal cause resolver invoked AFTER the original
    // transport ends. Pending/Blocked/EOF must never manufacture Revoked.
    let wait=completion_event(rx,&context.scope,context.scope.deadline());
    tokio::pin!(wait);
    assert!(futures::poll!(wait.as_mut()).is_pending());
    original.verified(&context).unwrap();
    assert!(matches!(wait.await,RouteAuditEnd::Revoked));
    app.shutdown().await.unwrap();
}

pub(super) async fn failed_or_lost_original_revoke_completion_after_eof_stays_unknown() {
    let(_root,app,frame)=local().await;
    let context=Context::capture(&app,frame).await.unwrap();
    let(tx,rx)=watch::channel(RevocationCompletion::Pending);
    let original=OriginalRevocation{context:context.clone(),sender:Some(tx)};
    let wait=completion_event(rx,&context.scope,context.scope.deadline());
    tokio::pin!(wait);assert!(futures::poll!(wait.as_mut()).is_pending());
    drop(original); // Existing confirm error/cancel/unwind drops this guard.
    assert!(matches!(wait.await,RouteAuditEnd::Unknown));
    let(tx,rx)=watch::channel(RevocationCompletion::Pending);drop(tx);
    assert!(matches!(completion_event(rx,&context.scope,context.scope.deadline()).await,RouteAuditEnd::Unknown));
    app.shutdown().await.unwrap();
}

pub(super) async fn original_revocation_cause_cannot_extend_deadline_or_cross_unlock_generation() {
    let(_root,app,frame)=local().await;
    let context=Context::capture(&app,frame.clone()).await.unwrap();
    let(tx,rx)=watch::channel(RevocationCompletion::Pending);
    let original=OriginalRevocation{context:context.clone(),sender:Some(tx)};
    original.verified(&context).unwrap();
    assert!(matches!(completion_event(rx,&context.scope,Instant::now()).await,RouteAuditEnd::Unknown));
    let(tx,rx)=watch::channel(RevocationCompletion::Pending);
    let original=OriginalRevocation{context:context.clone(),sender:Some(tx)};
    app.lock().await.unwrap();app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let(session,unlocked)=app.unlocked().await.unwrap();
    let new_frame=Frame{version:1,profile_id:session.profile_id.to_string(),notification_scope_id:unlocked.notices.scope_id().unwrap()};
    let replacement=Context::capture(&app,new_frame).await.unwrap();
    assert!(original.verified(&replacement).is_err());
    assert!(matches!(completion_event(rx,&context.scope,context.scope.deadline()).await,RouteAuditEnd::Unknown));
    drop(unlocked);drop(session);app.shutdown().await.unwrap();
}
