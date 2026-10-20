use super::*;

#[tokio::test]
async fn view_preferences_merge_without_syncing_or_accepting_unbounded_values() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let before = serde_json::to_value(app.kube_inventory().await.unwrap()).unwrap();
    let id = context.id.to_string();
    let reminder = json!({
        "resource": {
            "api": {
                "group": "kustomize.toolkit.fluxcd.io", "version": "v1",
                "resource": "kustomizations", "kind": "Kustomization",
                "namespaced": true, "verbs": ["get", "list", "watch"]
            },
            "namespace": "flux-system", "name": "apps"
        },
        "due_at_ms": 1_800_000_000_000_i64
    });
    for patch in [
        json!({"kind":"workspace","height":340.0,"open":true,"tab":"logs"}),
        json!({"kind":"table","resource_kind":"/v1/pods","preferences":{
            "widths":{"name":300.0},"hidden":["age"],"sort":"name","ascending":false,
        }}),
        json!({"kind":"gitops","preferences":{"notifications":true,"reminders":[reminder]}}),
    ] {
        app.kube_view_preferences_update(id.clone(), serde_json::from_value(patch).unwrap())
            .await
            .unwrap();
    }
    let preferences = app.kube_view_preferences(id.clone()).await.unwrap();
    assert_eq!(preferences.workspace.unwrap().height, 340.0);
    assert_eq!(preferences.tables["/v1/pods"].widths["name"], 300.0);
    let gitops = preferences.gitops.unwrap();
    assert!(gitops.notifications);
    assert_eq!(gitops.reminders[0].resource.name, "apps");
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
    for reminders in [
        vec![reminder.clone(); 33],
        vec![reminder.clone(); 2],
        vec![{
            let mut malformed = reminder.clone();
            malformed["resource"]["name"] = json!("apps\n");
            malformed
        }],
        vec![{
            let mut malformed = reminder.clone();
            malformed["due_at_ms"] = json!(-1);
            malformed
        }],
    ] {
        let invalid = serde_json::from_value(json!({
            "kind":"gitops", "preferences":{"notifications":false,"reminders":reminders}
        }))
        .unwrap();
        assert!(app
            .kube_view_preferences_update(id.clone(), invalid)
            .await
            .is_err());
    }
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
    let persisted = app.kube_view_preferences(id).await.unwrap();
    assert_eq!(persisted.tables["/v1/pods"].hidden, ["age"]);
    assert!(persisted.gitops.unwrap().notifications);
    app.shutdown().await.unwrap();
}
