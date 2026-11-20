use super::*;
struct OwnedTestRuntime(Option<tokio::runtime::Runtime>);
impl Drop for OwnedTestRuntime {fn drop(&mut self){if let Some(runtime)=self.0.take(){runtime.shutdown_background();}}}
static REGISTRY_TEST_SERIAL:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_process_session_ceiling_cancelled_caller_and_joined_cleanup() {
    let _serial=REGISTRY_TEST_SERIAL.lock().await;
    let registry = Arc::new(VaultValidationOperations::default());
    let external = CancellationToken::new();
    let marker = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut receivers = vec![];
    for _ in 0..2 {
        let seen = marker.clone();
        let (receiver, _) = registry
            .launch(
                "owned-fixture-runner".into(),
                &external,
                move |_, cancel, mut resources, send| async move {
                    resources.staging = Some(tempfile::tempdir().unwrap());
                    cancel.cancelled().await;
                    let _retained=resources.finish().await;
                    seen.fetch_add(1, Ordering::AcqRel);
                    let _ = send.send(Err(invalid()));
                },
            )
            .unwrap();
        receivers.push(receiver);
    }
    assert!(registry
        .launch("third".into(), &external, |_, _, _, _| async {})
        .is_err());
    // Dropping caller receivers does not abort registered workers or free
    // permits; cancellation and close join their actual staged cleanup.
    drop(receivers);
    assert_eq!(registry.state.lock().unwrap().entries.len(), 2);
    registry.close_and_wait().await;
    assert_eq!(marker.load(Ordering::Acquire), 2);
    assert!(registry.state.lock().unwrap().entries.is_empty());
    assert_eq!(ACTIVE.get().unwrap().available_permits(), 2);
    assert!(registry
        .launch("closed".into(), &external, |_, _, _, _| async {})
        .is_err());
}

#[path = "tests/live_remote.rs"]
mod live_remote;

#[tokio::test]
async fn private_owner_admission_future_fits_desktop_thread_budget() {
    let root=tempfile::tempdir().unwrap();
    let app=AppCore::new(crate::AppConfig::for_tests(root.path().to_string_lossy().into_owned())).unwrap();
    let input=VaultValidationInput {
        project_id:Uuid::new_v4().to_string(),project_revision:1,revision_digest:"a".repeat(64),credential_id:Uuid::new_v4().to_string(),
        runner_id:Uuid::new_v4().to_string(),runner_revision:1,playbook:"playbook.yml".into(),
        parameters:RunParameters{extra_vars:BTreeMap::new(),tags:vec![],skip_tags:vec![],diff:false,vault:None},inventory:json!({}),targets:vec![],
    };
    let cancel=CancellationToken::new();let future=app.automation_vault_validate_private(input,&cancel);
    let bytes=std::mem::size_of_val(&future);
    assert!(bytes<=64*1024,"private admission future uses {bytes} bytes; nested transport work must stay off desktop stack");
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn real_cleanup_retains_registration_until_result_and_only_external_cancel_denies() {
    let _serial=REGISTRY_TEST_SERIAL.lock().await;
    for mode in ["healthy","during_cleanup","after_cleanup"] {
        let runtime=OwnedTestRuntime(Some(tokio::runtime::Builder::new_multi_thread().worker_threads(2).max_blocking_threads(1).enable_all().build().unwrap()));
        let handle=runtime.0.as_ref().unwrap().handle().clone();
        let (blocking_entered,entered)=std::sync::mpsc::channel();let (release,blocking_release)=std::sync::mpsc::channel();
        let blocker=handle.spawn_blocking(move||{blocking_entered.send(()).unwrap();let _=blocking_release.recv();});
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let staging=tempfile::Builder::new().prefix("owned-retained-cleanup-").tempdir().unwrap();let path=staging.path().to_owned();
        let registry=Arc::new(VaultValidationOperations::default());let owned=registry.clone();
        let gate=CancellationToken::new();let hold=gate.clone();let (observe,seen)=oneshot::channel();let (commit,committed)=oneshot::channel();
        let launched=handle.spawn(async move{
            owned.launch("fixture".into(),&CancellationToken::new(),move|_,cancel,mut resources,send|async move{
                resources.staging=Some(staging);
                let retained=resources.finish().await;
                assert!(retained.is_some());
                let _=observe.send(completed::<()>(Ok(()),&cancel,&retained).is_ok());
                hold.cancelled().await;
                let _=commit.send(completed::<()>(Ok(()),&cancel,&retained).is_ok());
                let _=send.send(Err(invalid())); // no fabricated validation witness
                drop(retained);
            }).unwrap()
        }).await.unwrap();
        let (receive,operation_cancel)=launched;
        tokio::time::timeout(Duration::from_secs(2),async{
            while registry.state.lock().unwrap().cleanups.len()!=1{tokio::task::yield_now().await;}
        }).await.unwrap();
        assert!(path.exists());assert_eq!(registry.state.lock().unwrap().entries.len(),1);
        assert_eq!(ACTIVE.get().unwrap().available_permits(),1);
        if mode=="during_cleanup" {registry.cancel_all();assert!(operation_cancel.is_cancelled());}
        else {assert!(!operation_cancel.is_cancelled());}
        release.send(()).unwrap();blocker.await.unwrap();
        assert_eq!(seen.await.unwrap(),mode!="during_cleanup","normal cleanup must not manufacture cancellation");
        assert!(!path.exists());
        // Cleanup completed, but RESULT has not been handed off yet: retain
        // both admission and registration so late profile/Runner cancellation
        // is still routed, rather than disappearing during the cleanup await.
        assert_eq!(registry.state.lock().unwrap().entries.len(),1);
        assert_eq!(ACTIVE.get().unwrap().available_permits(),1);
        if mode=="after_cleanup" {registry.revoke_runner("fixture");assert!(operation_cancel.is_cancelled());}
        gate.cancel();assert_eq!(committed.await.unwrap(),mode=="healthy");
        assert!(receive.await.unwrap().is_err());registry.close_and_wait().await;
        assert!(registry.state.lock().unwrap().entries.is_empty());assert_eq!(ACTIVE.get().unwrap().available_permits(),2);
        drop(runtime);
    }
}

#[test]
fn cleanup_channel_without_retained_owner_cannot_publish_success() {
    let cancel=CancellationToken::new();
    assert!(completed::<()>(Ok(()),&cancel,&None).is_err());
    assert!(completed::<()>(Err(invalid()),&cancel,&None).is_err());
    // Some→Ok/cancel→Err is tested with real registered Reservation and joined
    // TempDir cleanup above, not a forged/fake validated result or owner.
}
