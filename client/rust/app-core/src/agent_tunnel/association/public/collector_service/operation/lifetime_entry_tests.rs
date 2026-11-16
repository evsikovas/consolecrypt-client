//! DRAFT/UNRUN actual public-entry/Context capture causals. No fake Node or Peer.
//! The hook observes the real capture before any association/SSH work; it never
//! supplies successful preparation. Full paired prepare/confirm remains separate.
use super::*;
use cc_platform_core::{InMemorySecureStore, UnsupportedDeviceKeys};
use std::sync::{OnceLock, Weak};
const PASSWORD: &str = "owned finite lifetime entry passphrase42";
struct CaptureHook {
    core: Weak<crate::app::Inner>,
    observed: oneshot::Sender<Context>,
    release: oneshot::Receiver<()>,
}
fn hooks() -> &'static Mutex<Option<CaptureHook>> {
    static HOOK: OnceLock<Mutex<Option<CaptureHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}
pub(super) async fn observe_original_capture(context: &Context) -> AppResult<()> {
    let hook = {
        let mut slot = hooks().lock().map_err(|_| unavailable())?;
        if slot.as_ref().is_some_and(|h| Weak::ptr_eq(&h.core,&context.core)) { slot.take() } else { None }
    };
    if let Some(hook) = hook {
        hook.observed.send(context.clone()).map_err(|_| unavailable())?;
        hook.release.await.map_err(|_| unavailable())?;
    }
    Ok(())
}
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| tokio::sync::Mutex::new(())).lock().await
}
struct Fixture { _root: tempfile::TempDir, app: AppCore, request: Value }
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let app = AppCore::with_platform(crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned()),
            Arc::new(InMemorySecureStore::new()), Arc::new(UnsupportedDeviceKeys)).unwrap();
        app.create_local_profile("Original public lifetime entry".into(), PASSWORD.into()).await.unwrap();
        let (owner,unlock) = app.unlocked().await.unwrap();
        let request = json!({"version":1,"profileId":owner.profile_id.to_string(),
            "notificationScopeId":unlock.notices.scope_id().unwrap(),"associationId":Uuid::new_v4().to_string(),
            "collectorId":Uuid::new_v4().to_string(),"manifestSha256":"a".repeat(64),
            "credentials":[{"target":{"kind":"receiver"},"credentialId":Uuid::new_v4().to_string(),"expectedCredentialRevision":"1"}]});
        drop(unlock); drop(owner);
        Self {_root:root,app,request}
    }
    async fn capture(&self) -> (Context, oneshot::Sender<()>, JoinHandle<AppResult<AgentCollectorServiceReview>>) {
        let (observed,receiver)=oneshot::channel();let (release,hold)=oneshot::channel();
        let mut hook=hooks().lock().unwrap();assert!(hook.is_none());
        *hook=Some(CaptureHook {core:Arc::downgrade(&self.app.inner),observed,release:hold});drop(hook);
        let app=self.app.clone();let raw=self.request.to_string();
        let call=tokio::spawn(async move{app.agent_collector_service_lifetime_prepare_json(&raw).await});
        let context=tokio::time::timeout(Duration::from_secs(5),receiver).await.unwrap().unwrap();
        (context,release,call)
    }
}
#[tokio::test]
async fn public_entry_captures_original_once_and_lock_aborts_before_association_or_ssh() {
    let _serial=serial().await;let f=Fixture::new().await;let (context,_release,call)=f.capture().await;
    assert!(context.check().await.is_ok());let original_until=context.scope.deadline();
    f.app.lock().await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2),call).await.unwrap().unwrap().is_err());
    assert_eq!(context.scope.deadline(),original_until);assert!(context.check().await.is_err());
    assert!(hooks().lock().unwrap().is_none());f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn original_public_prepare_cancel_ends_pending_capture_without_new_context() {
    let _serial=serial().await;let f=Fixture::new().await;let (context,_release,call)=f.capture().await;
    assert!(context.check().await.is_ok());context.cancel.cancel();
    assert!(tokio::time::timeout(Duration::from_secs(2),call).await.unwrap().unwrap().is_err());
    assert!(context.check().await.is_err());assert!(hooks().lock().unwrap().is_none());f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn same_profile_reunlock_cannot_resume_original_public_prepare() {
    let _serial=serial().await;let f=Fixture::new().await;let (context,_release,call)=f.capture().await;
    f.app.lock().await.unwrap();f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert_eq!(f.app.session().await.unwrap().profile_id.to_string(),context.frame.profile_id);
    assert!(context.check().await.is_err());
    assert!(tokio::time::timeout(Duration::from_secs(2),call).await.unwrap().unwrap().is_err());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn public_lifetime_intent_rejects_implicit_empty_or_mode_substitution_before_capture() {
    let _serial=serial().await;let f=Fixture::new().await;
    let mut empty=f.request.clone();empty["credentials"]=json!([]);
    reject_without_capture(&f,empty).await;
    let mut absent=f.request.clone();absent.as_object_mut().unwrap().remove("credentials");
    reject_without_capture(&f,absent).await;
    let mut excess=f.request.clone();excess["credentials"]=json!([f.request["credentials"][0],f.request["credentials"][0],f.request["credentials"][0]]);
    reject_without_capture(&f,excess).await;
    for field in ["credentialProtocol","purpose","approved","reuseReview","deadline","route","manifest"] {
        let mut request=f.request.clone();request[field]=json!("caller replacement");
        reject_without_capture(&f,request).await;
    }
    assert!(hooks().lock().unwrap().is_none());f.app.shutdown().await.unwrap();
}
async fn reject_without_capture(f:&Fixture,request:Value) {
    let (observed,mut receiver)=oneshot::channel();let (_release,hold)=oneshot::channel();
    let mut hook=hooks().lock().unwrap();assert!(hook.is_none());
    *hook=Some(CaptureHook {core:Arc::downgrade(&f.app.inner),observed,release:hold});drop(hook);
    assert!(tokio::time::timeout(Duration::from_secs(2),f.app.clone()
        .agent_collector_service_lifetime_prepare_json(&request.to_string())).await
        .expect("invalid intent must reject before real capture hook").is_err());
    assert!(matches!(receiver.try_recv(),Err(tokio::sync::oneshot::error::TryRecvError::Empty)));
    assert!(hooks().lock().unwrap().take().is_some(),"capture observer was not consumed");
}

// Private identity metadata only, constructed in cfg(test) on real captured
// Contexts. This cannot create Prepared/Peer, sign, launch or return a review.
fn identity(context:&Context,protocol:CredentialProtocol)->PreparedReviewIdentity {
    PreparedReviewIdentity {scope:context.scope.clone(),until:context.scope.deadline(),review_id:Uuid::new_v4(),
        protocol,host:ObjectId::new(),host_revision:1,route:"original pinned route".into(),
        delegation_hash:Hash::from_bytes([1;32]),manifest_sha256:Hash::from_bytes([2;32]),
        credentials:vec![json!({"target":{"kind":"receiver"},"credentialId":Uuid::new_v4(),"credentialRevision":"1"})]}
}
async fn actual_context(f:&Fixture)->Context {
    Context::capture(&f.app,Frame {version:1,profile_id:f.request["profileId"].as_str().unwrap().to_owned(),
        notification_scope_id:f.request["notificationScopeId"].as_str().unwrap().to_owned()}).await.unwrap()
}
#[tokio::test]
async fn confirmation_fence_denies_another_original_context_even_with_same_profile_frame() {
    let _serial=serial().await;let f=Fixture::new().await;let a=actual_context(&f).await;let b=actual_context(&f).await;
    let original=identity(&a,CredentialProtocol::ServiceLifetime);assert!(original.require_identity(&original.clone()).is_ok());
    let mut swapped=original.clone();swapped.scope=b.scope.clone();
    assert_eq!(a.frame.profile_id,b.frame.profile_id);assert!(original.require_identity(&swapped).is_err());
    let mut another=original.clone();another.review_id=Uuid::new_v4();assert!(original.require_identity(&another).is_err());
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn confirmation_fence_cannot_upgrade_v1_or_replace_refs_route_policy_or_original_deadline() {
    let _serial=serial().await;let f=Fixture::new().await;let context=actual_context(&f).await;
    let original=identity(&context,CredentialProtocol::SingleStart);
    for kind in 0..6 {
        let mut changed=original.clone();
        match kind {0=>changed.protocol=CredentialProtocol::ServiceLifetime,1=>changed.route="replacement".into(),
            2=>changed.credentials[0]["credentialRevision"]=json!("2"),3=>changed.delegation_hash=Hash::from_bytes([3;32]),
            4=>changed.until+=Duration::from_secs(60),_=>changed.host_revision+=1}
        assert!(original.require_identity(&changed).is_err());
    }
    assert!(original.require_identity(&original.clone()).is_ok());f.app.shutdown().await.unwrap();
}
