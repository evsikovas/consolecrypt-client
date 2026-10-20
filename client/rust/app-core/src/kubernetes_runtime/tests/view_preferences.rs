use super::*;

#[tokio::test]
async fn view_preferences_merge_without_syncing_or_accepting_unbounded_values() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let before = serde_json::to_value(app.kube_inventory().await.unwrap()).unwrap();
    let id = context.id.to_string();
    for patch in [
        json!({"kind":"workspace","height":340.0,"open":true,"tab":"logs"}),
        json!({"kind":"table","resource_kind":"/v1/pods","preferences":{
            "widths":{"name":300.0},"hidden":["age"],"sort":"name","ascending":false,
        }}),
    ] {
        app.kube_view_preferences_update(id.clone(), serde_json::from_value(patch).unwrap())
            .await
            .unwrap();
    }
    let preferences = app.kube_view_preferences(id.clone()).await.unwrap();
    assert_eq!(preferences.workspace.unwrap().height, 340.0);
    assert_eq!(preferences.tables["/v1/pods"].widths["name"], 300.0);
    assert_eq!(
        before,
        serde_json::to_value(app.kube_inventory().await.unwrap()).unwrap()
    );
    let invalid =
        serde_json::from_value(json!({"kind":"workspace","height":1.0,"open":true,"tab":"logs"}))
            .unwrap();
    assert!(app
        .kube_view_preferences_update(id.clone(), invalid)
        .await
        .is_err());
    let invalid = serde_json::from_value(
        json!({"kind":"workspace","height":300.0,"open":true,"tab":"arbitrary"}),
    )
    .unwrap();
    assert!(app
        .kube_view_preferences_update(id.clone(), invalid)
        .await
        .is_err());
    app.lock().await.unwrap();
    assert!(app.kube_view_preferences(id.clone()).await.is_err());
    app.unlock_with_passphrase("synthetic-runtime-test-passphrase".into())
        .await
        .unwrap();
    assert_eq!(
        app.kube_view_preferences(id).await.unwrap().tables["/v1/pods"].hidden,
        ["age"]
    );
    app.shutdown().await.unwrap();
}
