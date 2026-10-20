use super::*;

#[tokio::test]
async fn lock_during_watch_poll_cannot_return_queued_objects() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let live = app.kube_live(&session.id).await.unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let task = tokio::spawn(std::future::pending::<()>());
    live.watches.lock().unwrap().insert(
        "controlled".into(),
        Arc::new(StreamHandle {
            receiver: tokio::sync::Mutex::new(receiver),
            failure: Arc::new(Mutex::new(None)),
            cancel: live.cancel.child_token(),
            task: task.abort_handle(),
        }),
    );
    let poll = app.kube_watch_poll(session.id, "controlled".into());
    tokio::pin!(poll);
    assert!(futures::poll!(&mut poll).is_pending());
    app.lock().await.unwrap();
    // An already buffered frame must not escape through the old poll future.
    let _ = sender.try_send(ResourceChange::Bookmark {
        resource_version: "private-revision".into(),
    });
    assert!(poll.await.is_err());
    task.abort();
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn completed_response_is_discarded_when_session_closes_before_delivery() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    // Completion and cancellation can occur in the same executor poll. The
    // operation is ready, so checking only select!'s entry branch is too early.
    let result = app
        .with_kube_session(&session.id, |live| async move {
            live.cancel.cancel();
            Ok(zeroize::Zeroizing::new(
                b"synthetic-private-response".to_vec(),
            ))
        })
        .await;
    assert!(result.is_err());
    app.shutdown().await.unwrap();
}
