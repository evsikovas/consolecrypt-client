use super::*;

#[tokio::test]
async fn gitops_overview_coalesces_native_watches_marks_rbac_partial_and_stops() {
    let fixture = Fixture::start_gitops().await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let watch = app
        .kube_insights_watch_start(session.id.clone(), Some("default".into()))
        .await
        .unwrap();
    assert!(app
        .kube_insights_watch_start(session.id.clone(), None)
        .await
        .is_err());
    let mut dirty = false;
    let mut partial = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !dirty || !partial {
            let event = app
                .kube_insights_watch_poll(session.id.clone(), watch.id.clone())
                .await
                .unwrap();
            dirty |= event.dirty;
            partial |= event.partial;
            assert!(!event.closed);
            let text = serde_json::to_string(&event).unwrap();
            assert!(!text.contains("private"));
            assert!(text.len() < 100);
        }
    })
    .await
    .unwrap();
    let requests = fixture.requests.lock().unwrap().clone();
    assert!(requests
        .iter()
        .any(|line| line.contains("/v1/namespaces/default/gitrepositories?")));
    assert!(!requests
        .iter()
        .any(|line| line.contains("/v1beta2/namespaces/")));
    assert!(!requests
        .iter()
        .any(|line| line.contains("/pods?") && line.contains("watch=true")));
    app.kube_insights_watch_stop(session.id.clone(), watch.id.clone())
        .await
        .unwrap();
    assert!(app
        .kube_insights_watch_poll(session.id.clone(), watch.id)
        .await
        .is_err());
    let replacement = app
        .kube_insights_watch_start(session.id.clone(), None)
        .await
        .unwrap();
    app.lock().await.unwrap();
    assert!(app
        .kube_insights_watch_poll(session.id, replacement.id)
        .await
        .is_err());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn cluster_without_gitops_closes_invalidation_handle_without_spinning() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let watch = app
        .kube_insights_watch_start(session.id.clone(), None)
        .await
        .unwrap();
    let event = app
        .kube_insights_watch_poll(session.id, watch.id)
        .await
        .unwrap();
    assert!(event.closed);
    assert!(!event.partial);
    assert!(!event.dirty);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn denied_gitops_kinds_close_with_partial_instead_of_an_endless_subscription() {
    let fixture = Fixture::start_gitops().await;
    let (_dir, app, context) = fixture.app().await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let watch = app
        .kube_insights_watch_start(session.id.clone(), Some("blocked".into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = app
                .kube_insights_watch_poll(session.id.clone(), watch.id.clone())
                .await
                .unwrap();
            if event.closed {
                assert!(event.partial);
                break;
            }
        }
    })
    .await
    .unwrap();
    app.shutdown().await.unwrap();
}
