use super::*;
use cc_db_core::transport::DbDestination;
use cc_models::database::{DbConnection, DbEngine, DbKubeTargetKind, DbRoute};

fn route(context: &KubeContext, kind: DbKubeTargetKind, name: &str) -> DbRoute {
    DbRoute::Kubernetes {
        context_id: context.id,
        namespace: "default".into(),
        target_kind: kind,
        target_name: name.into(),
    }
}

fn reason(error: AppError) -> String {
    match error {
        AppError::InvalidInput { field, reason } if field == "database" => reason,
        other => panic!("unexpected error: {other:?}"),
    }
}

async fn echo(app: &AppCore, route: &DbRoute) -> AppResult<()> {
    let (owner, unlocked) = app.unlocked().await?;
    let cancel = CancellationToken::new();
    let connector = app
        .db_resolve_route(&owner, &unlocked, route, 80, &cancel)
        .await?;
    let destination = DbDestination::new("127.0.0.1", 80).unwrap();
    let mut socket = connector
        .connect(&destination, cancel.clone())
        .await
        .unwrap();
    socket.write_all(b"select").await.unwrap();
    let mut bytes = [0; 6];
    tokio::time::timeout(Duration::from_secs(3), socket.read_exact(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes, b"select");
    cancel.cancel();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_kubernetes_route_restores_and_reuses_its_forward_on_connect() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, mut context) = fixture.app().await;
    context.allow_port_forward_in_prod = true;
    app.save_kube_context(context.clone()).await.unwrap();

    // The connection keeps the route, not a local port.
    let mut connection = DbConnection::new("web", "127.0.0.1", DbEngine::Postgresql);
    connection.id = cc_models::ObjectId::NIL;
    connection.port = 80;
    connection.route = route(&context, DbKubeTargetKind::Pod, "web");
    let saved = app
        .db_save_connection(connection.clone(), None, None)
        .await
        .unwrap();
    assert_eq!(saved.route, connection.route);
    let mut dangling = connection.clone();
    dangling.route = DbRoute::Kubernetes {
        context_id: cc_models::ObjectId::new(),
        namespace: "default".into(),
        target_kind: DbKubeTargetKind::Pod,
        target_name: "web".into(),
    };
    assert!(app.db_save_connection(dangling, None, None).await.is_err());

    // After a restart nothing runs: connecting opens the context and forwards.
    assert!(app.kube_forward_list().await.unwrap().is_empty());
    echo(&app, &saved.route).await.unwrap();
    let forwards = app.kube_forward_list().await.unwrap();
    assert_eq!(forwards.len(), 1);
    assert_eq!(
        (
            forwards[0].namespace.as_str(),
            forwards[0].pod.as_str(),
            forwards[0].remote_port
        ),
        ("default", "web", 80)
    );
    assert!(forwards[0].address.starts_with("127.0.0.1:"));

    // A second connect reuses the running forward.
    echo(&app, &saved.route).await.unwrap();
    assert_eq!(app.kube_forward_list().await.unwrap().len(), 1);

    // A workload that cannot be resolved is a clear database error.
    let mut missing = saved.clone();
    missing.route = route(&context, DbKubeTargetKind::Service, "db");
    let error = app.db_test_connection(missing, None).await.unwrap_err();
    assert_eq!(reason(error), "kube_target_missing");

    // The context cannot be deleted while a connection forwards through it.
    assert!(matches!(
        app.delete_kube_context(context.id.to_string()).await,
        Err(AppError::InUse { .. })
    ));

    // Production policy still applies to a restored forward.
    context.allow_port_forward_in_prod = false;
    app.save_kube_context(context.clone()).await.unwrap();
    let mut blocked = saved.clone();
    blocked.route = route(&context, DbKubeTargetKind::Pod, "other");
    let error = app.db_test_connection(blocked, None).await.unwrap_err();
    assert_eq!(reason(error), "kube_forward_blocked");
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unreachable_cluster_is_reported_as_a_database_route_error() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    drop(fixture);
    let mut connection = DbConnection::new("web", "127.0.0.1", DbEngine::Postgresql);
    connection.port = 80;
    connection.route = route(&context, DbKubeTargetKind::Pod, "web");
    let error = app.db_test_connection(connection, None).await.unwrap_err();
    assert_eq!(reason(error), "kube_unreachable");
    app.shutdown().await.unwrap();
}
