use super::*;
struct Fixture {
    directory: tempfile::TempDir,
    app: AppCore,
    owner: Arc<Session>,
    scope: String,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned portable transfer".into(),
            "synthetic transfer profile password42".into(),
        )
        .await
        .unwrap();
        let (owner, _) = app.unlocked().await.unwrap();
        let scope = app.automation_portable_scope_json().await.unwrap();
        Self {
            directory,
            app,
            owner,
            scope,
        }
    }
    fn source(&self) -> String {
        self.directory
            .path()
            .join("not-opened-before-start.ccproject")
            .to_string_lossy()
            .into_owned()
    }
}
#[tokio::test]
async fn synchronous_transfer_rejects_invalid_utf8_and_exact_byte_budget_without_input_echo() {
    for value in [Vec::new(), vec![b'x'; 4097], vec![0xff, 0xfe, 0]] {
        let error = PortablePasswordTransfer::from_bytes(value).unwrap_err();
        assert!(matches!(error, AppError::InvalidInput { .. }));
    }
    let transfer = PortablePasswordTransfer::from_bytes("пароль🙂".as_bytes().to_vec()).unwrap();
    assert_eq!(transfer.take().unwrap().expose_secret(), "пароль🙂");
    assert!(transfer.take().is_err());
    assert_eq!(
        format!("{transfer:?}"),
        "PortablePasswordTransfer([REDACTED])"
    );
}
#[tokio::test]
async fn explicit_cancel_wipes_buffer_while_unpolled_async_frame_retains_arc() {
    let transfer =
        PortablePasswordTransfer::from_bytes(b"synthetic pending password".to_vec()).unwrap();
    let retained = transfer.clone();
    let unpolled = async move { retained.take() };
    transfer.cancel();
    assert!(transfer.value.lock().unwrap().is_none());
    assert!(unpolled.await.is_err());
}
#[tokio::test]
async fn actual_transfer_expiry_wipes_without_consumption_or_owner_creation() {
    let transfer = PortablePasswordTransfer::from_bytes_lifetime(
        b"synthetic expiry password".to_vec(),
        Duration::from_secs(1),
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), transfer.cancelled.cancelled())
        .await
        .unwrap();
    assert!(transfer.value.lock().unwrap().is_none());
    assert!(transfer.take().is_err());
}
#[tokio::test]
async fn cancel_while_actual_original_session_mutex_is_queued_prevents_secret_take_and_idle_insertion(
) {
    let f = Fixture::new().await;
    let transfer =
        PortablePasswordTransfer::from_bytes(b"synthetic deferred password".to_vec()).unwrap();
    let held = f.app.inner.active.lock().await;
    let mut pending = Box::pin(f.app.automation_portable_register_inspect_json(
        f.scope.clone(),
        f.source(),
        transfer.clone(),
    ));
    std::future::poll_fn(|cx| match pending.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("actual active-session mutex must queue registration"),
    })
    .await;
    assert!(transfer.value.lock().unwrap().is_some());
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    transfer.cancel();
    assert!(transfer.value.lock().unwrap().is_none());
    drop(held);
    assert!(tokio::time::timeout(Duration::from_secs(10), &mut pending)
        .await
        .unwrap()
        .is_err());
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
    drop(pending);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn cancel_after_actual_idle_insertion_before_transport_ack_wipes_original_operation_and_denies_start(
) {
    let f = Fixture::new().await;
    let transfer =
        PortablePasswordTransfer::from_bytes(b"synthetic late ACK password".to_vec()).unwrap();
    let (inserted_tx, inserted_rx) = oneshot::channel();
    let (ack_tx, ack_rx) = oneshot::channel();
    let pending = tokio::spawn({
        let app = f.app.clone();
        let scope = f.scope.clone();
        let source = f.source();
        let transfer = transfer.clone();
        async move {
            let operation = app
                .automation_portable_register_inspect_json(scope, source, transfer)
                .await?;
            inserted_tx.send(()).unwrap();
            ack_rx.await.map_err(|_| invalid())?;
            Ok::<_, AppError>(operation)
        }
    });
    tokio::time::timeout(Duration::from_secs(10), inserted_rx)
        .await
        .unwrap()
        .unwrap();
    assert!(
        transfer.value.lock().unwrap().is_none(),
        "password has really left transfer and entered Native idle body"
    );
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    transfer.cancel();
    assert_eq!(
        owned_setup::test_count(&f.owner),
        0,
        "cancel-only Weak must reach the exact idle owner before ACK is released"
    );
    assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
    ack_tx.send(()).unwrap();
    let operation = tokio::time::timeout(Duration::from_secs(10), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(operation.control.joined.is_cancelled());
    assert!(operation.start().await.is_err());
    assert_eq!(f.owner.automation.io_slots.available_permits(), 4);
    assert!(!Path::new(&f.source()).exists());
    drop(operation);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn cancellation_between_secret_take_and_weak_handoff_is_sticky_and_retires_idle_body_before_ack(
) {
    let f = Fixture::new().await;
    let transfer =
        PortablePasswordTransfer::from_bytes(b"synthetic handoff race password".to_vec()).unwrap();
    let password = transfer.take().unwrap();
    let scope = super::super::facade::scope(&f.scope).unwrap();
    let operation = f
        .app
        .portable_register(
            scope.clone(),
            Body::Inspect {
                scope,
                file: PathBuf::from(f.source()),
                password,
            },
            Duration::from_secs(300),
        )
        .await
        .unwrap();
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    transfer.cancel(); // cancellation happens BEFORE the control Weak is bound
    assert!(transfer.attach(&operation).is_err());
    assert!(operation.control.joined.is_cancelled());
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    assert!(operation.start().await.is_err());
    drop(operation);
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn successful_transfer_drop_never_cancels_legitimately_handed_off_request() {
    let f = Fixture::new().await;
    let transfer =
        PortablePasswordTransfer::from_bytes(b"synthetic successful handoff password".to_vec())
            .unwrap();
    let operation = f
        .app
        .automation_portable_register_inspect_json(f.scope.clone(), f.source(), transfer.clone())
        .await
        .unwrap();
    let weak = Arc::downgrade(&transfer);
    drop(transfer);
    assert!(
        weak.upgrade().is_none(),
        "Weak timer and control projection do not retain secret holder"
    );
    assert!(!operation.control.cancel.is_cancelled());
    assert!(matches!(
        *operation.control.phase.lock().unwrap(),
        Phase::Idle(_)
    ));
    assert_eq!(owned_setup::test_count(&f.owner), 1);
    operation.cancel();
    operation.joined().await;
    assert_eq!(owned_setup::test_count(&f.owner), 0);
    drop(operation);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn strict_scope_json_rejects_duplicate_escaped_unknown_and_oversized_frames_before_registration(
) {
    let f = Fixture::new().await;
    let raw: Value = serde_json::from_str(&f.scope).unwrap();
    let duplicate = f.scope.replacen('{', "{\"version\":1,", 1);
    let escaped = f.scope.replacen(
        '{',
        &format!("{{\"profile\\u0049d\":{},", raw["profileId"]),
        1,
    );
    let unknown = f.scope.replacen('{', "{\"approved\":true,", 1);
    let oversized = format!("{}{}", f.scope, " ".repeat(1025));
    for frame in [duplicate, escaped, unknown, oversized] {
        assert!(f
            .app
            .automation_portable_register_context_json(frame, Uuid::new_v4().to_string(), 1)
            .await
            .is_err());
        assert_eq!(owned_setup::test_count(&f.owner), 0);
        assert_eq!(f.owner.automation.prepare_slots.available_permits(), 2);
    }
    f.app.shutdown().await.unwrap();
}
