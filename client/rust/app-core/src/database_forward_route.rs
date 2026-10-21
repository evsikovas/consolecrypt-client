//! Restorable database routes: a saved connection remembers the Kubernetes
//! workload, saved SSH tunnel or Docker container it was forwarded to. On
//! connect the device-local forward is restored (a running one is reused) and
//! the driver dials its loopback address. Forwards stay listed in their own
//! sections and keep running after the database session closes.
use crate::kubernetes_runtime::LiveSession;
use crate::session::Unlocked;
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::{ResourceError, ResourceRef};
use cc_models::database::DbKubeTargetKind;
use cc_models::tunnel::TunnelKind;
use cc_models::ObjectId;
use std::{net::SocketAddr, sync::Arc, time::Duration};

fn failure(code: &'static str) -> AppError {
    AppError::invalid("database", code)
}

/// Lock and cancellation keep their meaning; anything else is reported as
/// one database reason code (no Kubernetes response text crosses the bridge).
fn mapped(error: AppError, code: &'static str) -> AppError {
    match error {
        AppError::VaultLocked | AppError::Cancelled | AppError::NoActiveProfile => error,
        _ => failure(code),
    }
}

fn resource_failure(error: ResourceError) -> AppError {
    failure(match error {
        ResourceError::NotFound
        | ResourceError::UnknownResource
        | ResourceError::InvalidRequest => "kube_target_missing",
        ResourceError::Forbidden | ResourceError::Unauthorized => "kube_access_denied",
        _ => "kube_unreachable",
    })
}

/// The preferred local port: stable, so external tools keep working too.
pub(crate) fn stable_local_port(remote_port: u16) -> u16 {
    10_000 + remote_port % 55_000
}

fn api(kind: DbKubeTargetKind) -> (&'static str, &'static str) {
    match kind {
        DbKubeTargetKind::Pod => ("", "pods"),
        DbKubeTargetKind::Service => ("", "services"),
        DbKubeTargetKind::Deployment => ("apps", "deployments"),
        DbKubeTargetKind::StatefulSet => ("apps", "statefulsets"),
    }
}

fn running(
    sessions: &[Arc<LiveSession>],
    namespace: &str,
    pod: &str,
    port: u16,
) -> Option<SocketAddr> {
    sessions
        .iter()
        .filter(|live| !live.cancel.is_cancelled())
        .find_map(|live| live.forwards.running(namespace, pod, port))
        .and_then(|forward| forward.address.parse().ok())
}

impl AppCore {
    /// Returns the loopback address of a forward to `port` of the target,
    /// connecting the context and starting the forward when needed.
    pub(crate) async fn db_kube_forward(
        &self,
        unlocked: &Arc<Unlocked>,
        context_id: ObjectId,
        namespace: &str,
        kind: DbKubeTargetKind,
        name: &str,
        port: u16,
    ) -> AppResult<SocketAddr> {
        let context = context_id.to_string();
        let sessions: Vec<_> = unlocked
            .kube
            .connections()
            .into_iter()
            .filter(|live| live.info.context_id == context)
            .collect();
        if kind == DbKubeTargetKind::Pod {
            if let Some(address) = running(&sessions, namespace, name, port) {
                return Ok(address);
            }
        }
        let live = match sessions.first() {
            Some(live) => live.clone(),
            None => {
                let info = self
                    .kube_connect(context.clone())
                    .await
                    .map_err(|error| mapped(error, "kube_unreachable"))?;
                self.kube_live(&info.id).await?
            }
        };
        if live.info.production && !live.info.allow_port_forward_in_prod {
            return Err(failure("kube_forward_blocked"));
        }
        if live.resources.kinds().map_err(resource_failure)?.is_empty() {
            tokio::time::timeout(Duration::from_secs(15), live.resources.discover())
                .await
                .map_err(|_| failure("kube_unreachable"))?
                .map_err(|_| failure("kube_unreachable"))?;
        }
        let (group, resource) = api(kind);
        let reference = ResourceRef {
            api: live
                .resources
                .resolve(group, "v1", resource)
                .map_err(resource_failure)?,
            namespace: Some(namespace.to_owned()),
            name: name.to_owned(),
        };
        let targets = live
            .resources
            .forward_targets(&reference)
            .await
            .map_err(resource_failure)?;
        // Ready Pods first; a Pod target is used even while it is not ready.
        let pod = targets
            .items
            .iter()
            .find(|target| target.ready)
            .or(targets.items.first())
            .map(|target| target.pod.name.clone())
            .ok_or_else(|| failure("kube_target_missing"))?;
        if let Some(address) = running(&sessions, namespace, &pod, port) {
            return Ok(address);
        }
        let start = |local_port| {
            self.kube_forward_start(
                live.info.id.clone(),
                namespace.to_owned(),
                pod.clone(),
                port,
                local_port,
            )
        };
        let forward = match start(stable_local_port(port)).await {
            Ok(forward) => forward,
            Err(AppError::VaultLocked) => return Err(AppError::VaultLocked),
            // The stable port is taken: any free loopback port will do.
            Err(_) => start(0)
                .await
                .map_err(|error| mapped(error, "kube_forward_failed"))?,
        };
        forward
            .address
            .parse()
            .map_err(|_| failure("kube_forward_failed"))
    }

    /// Returns the loopback address of a saved local-forward tunnel, starting
    /// it when it is not running.
    pub(crate) async fn db_tunnel_forward(
        &self,
        unlocked: &Arc<Unlocked>,
        tunnel_id: ObjectId,
    ) -> AppResult<SocketAddr> {
        let tunnel = unlocked
            .working()
            .tunnel(tunnel_id)
            .ok_or_else(|| failure("route_unavailable"))?;
        if tunnel.kind != TunnelKind::Local {
            return Err(failure("tunnel_unsupported"));
        }
        let running = || {
            unlocked
                .ssh
                .tunnels
                .status(tunnel_id)
                .filter(|status| status.state == cc_tunnel_core::TunnelState::Running)
                .and_then(|status| loopback(&status.listen))
        };
        if let Some(address) = running() {
            return Ok(address);
        }
        match self.start_tunnel(tunnel_id.to_string()).await {
            Ok(status) => loopback(&status.listen).ok_or_else(|| failure("tunnel_failed")),
            Err(AppError::VaultLocked) => Err(AppError::VaultLocked),
            // Started concurrently (e.g. by the Tunnels section).
            Err(_) => running().ok_or_else(|| failure("tunnel_failed")),
        }
    }

    /// Returns the loopback address of a Docker forward to `port` of the
    /// named container, connecting the engine and starting it when needed.
    pub(crate) async fn db_docker_forward(
        &self,
        unlocked: &Arc<Unlocked>,
        host_id: Option<ObjectId>,
        container: &str,
        port: u16,
    ) -> AppResult<SocketAddr> {
        let mut sessions = unlocked.docker.session_ids(host_id);
        if let Some(address) = unlocked.docker.running_forward(&sessions, container, port) {
            return address
                .parse()
                .map_err(|_| failure("docker_forward_failed"));
        }
        if sessions.is_empty() {
            let host = host_id.map_or_else(
                || crate::docker_runtime::LOCAL_HOST.to_owned(),
                |id| id.to_string(),
            );
            match self.docker_connect(host).await {
                Ok(crate::docker_runtime::DockerConnectOutcome::Connected(session)) => {
                    sessions.push(session.id)
                }
                Ok(_) => return Err(failure("docker_unreachable")),
                Err(error) => return Err(mapped(error, "docker_unreachable")),
            }
        }
        let docker_failure = |error: AppError| match &error {
            AppError::InvalidInput { reason, .. }
                if reason == "container_not_found" || reason == "container_unreachable" =>
            {
                failure("docker_container_missing")
            }
            _ => mapped(error, "docker_forward_failed"),
        };
        let start = |local_port| {
            self.docker_forward_start(sessions[0].clone(), container.to_owned(), port, local_port)
        };
        let forward = match start(stable_local_port(port)).await {
            Ok(forward) => forward,
            Err(AppError::InvalidInput { reason, .. }) if reason == "local_port_busy" => {
                start(0).await.map_err(docker_failure)?
            }
            Err(error) => return Err(docker_failure(error)),
        };
        forward
            .address
            .parse()
            .map_err(|_| failure("docker_forward_failed"))
    }
}

/// A listen address as a dialable loopback address (`0.0.0.0` → `127.0.0.1`).
fn loopback(listen: &str) -> Option<SocketAddr> {
    let mut address: SocketAddr = listen.parse().ok()?;
    if address.ip().is_unspecified() {
        address.set_ip(match address {
            SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
            SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
        });
    }
    Some(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::database::{DbConnection, DbEngine, DbRoute};

    #[test]
    fn tunnel_listen_addresses_become_dialable_loopback_addresses() {
        assert_eq!(
            loopback("0.0.0.0:15432"),
            Some("127.0.0.1:15432".parse().unwrap())
        );
        assert_eq!(loopback("[::]:6379"), Some("[::1]:6379".parse().unwrap()));
        assert_eq!(
            loopback("127.0.0.1:3306"),
            Some("127.0.0.1:3306".parse().unwrap())
        );
        assert_eq!(loopback("localhost:3306"), None);
    }

    #[test]
    fn stable_ports_match_the_flutter_preference_and_stay_in_range() {
        assert_eq!(stable_local_port(5432), 15432);
        assert_eq!(stable_local_port(27017), 37017);
        assert_eq!(stable_local_port(65535), 20535);
        assert_eq!(stable_local_port(1), 10001);
    }

    #[test]
    fn kubernetes_failures_become_database_reason_codes() {
        let code = |error: AppError| match error {
            AppError::InvalidInput { field, reason } => (field, reason),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            code(resource_failure(ResourceError::NotFound)),
            ("database".into(), "kube_target_missing".into())
        );
        assert_eq!(
            code(resource_failure(ResourceError::Timeout)).1,
            "kube_unreachable"
        );
        assert_eq!(
            code(resource_failure(ResourceError::Forbidden)).1,
            "kube_access_denied"
        );
        assert!(matches!(
            mapped(AppError::VaultLocked, "kube_unreachable"),
            AppError::VaultLocked
        ));
        assert_eq!(
            code(mapped(
                AppError::invalid("kubernetes", "timeout"),
                "kube_unreachable"
            ))
            .1,
            "kube_unreachable"
        );
    }

    async fn app() -> (tempfile::TempDir, AppCore, crate::dto::HostDto) {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("routes".into(), "synthetic-route-test-passphrase".into())
            .await
            .unwrap();
        // A loopback port nothing listens on: SSH and Docker cannot connect.
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut host = crate::dto::HostDto::new("db-bastion", "127.0.0.1");
        host.port = Some(closed);
        let host = app.save_host(host).await.unwrap();
        (dir, app, host)
    }

    fn reason(error: AppError) -> String {
        match error {
            AppError::InvalidInput { field, reason } if field == "database" => reason,
            other => panic!("unexpected error: {other:?}"),
        }
    }

    fn connection(route: DbRoute) -> DbConnection {
        let mut connection = DbConnection::new("orders", "db.internal", DbEngine::Postgresql);
        connection.id = ObjectId::NIL;
        connection.port = 5432;
        connection.route = route;
        connection
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saved_tunnel_route_is_checked_and_started_on_connect() {
        let (_dir, app, host) = app().await;
        let tunnel = |kind| crate::dto::TunnelDto {
            id: String::new(),
            name: "orders-db".into(),
            kind,
            host_id: host.id.clone(),
            bind_host: "127.0.0.1".into(),
            bind_port: 0,
            target_host: (kind != TunnelKind::Dynamic).then(|| "db.internal".into()),
            target_port: (kind != TunnelKind::Dynamic).then_some(5432),
            auto_start: false,
            binds_publicly: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let local = app.save_tunnel(tunnel(TunnelKind::Local)).await.unwrap();
        let socks = app.save_tunnel(tunnel(TunnelKind::Dynamic)).await.unwrap();
        let route = |tunnel: &crate::dto::TunnelDto| DbRoute::Tunnel {
            tunnel_id: tunnel.id.parse().unwrap(),
        };
        let saved = app
            .db_save_connection(connection(route(&local)), None, None)
            .await
            .unwrap();
        assert_eq!(saved.route, route(&local));
        // Only local forwards reach a database; dangling tunnels are refused.
        assert!(app
            .db_save_connection(connection(route(&socks)), None, None)
            .await
            .is_err());
        assert!(app
            .db_save_connection(
                connection(DbRoute::Tunnel {
                    tunnel_id: ObjectId::new()
                }),
                None,
                None
            )
            .await
            .is_err());
        assert!(matches!(
            app.delete_tunnel(local.id.clone()).await,
            Err(AppError::InUse { .. })
        ));
        // Connecting starts the stopped tunnel; its SSH host is unreachable.
        let error = tokio::time::timeout(
            Duration::from_secs(30),
            app.db_test_connection(saved.clone(), None),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(reason(error), "tunnel_failed");
        let mut dynamic = saved;
        dynamic.route = route(&socks);
        let error = app.db_test_connection(dynamic, None).await.unwrap_err();
        assert_eq!(reason(error), "tunnel_unsupported");
        app.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn docker_route_connects_the_engine_and_reports_it_unreachable() {
        let (_dir, app, host) = app().await;
        let route = DbRoute::Docker {
            host_id: Some(host.id.parse().unwrap()),
            container: "shop-postgres-1".into(),
        };
        let saved = app
            .db_save_connection(connection(route), None, None)
            .await
            .unwrap();
        assert!(app
            .db_save_connection(
                connection(DbRoute::Docker {
                    host_id: Some(ObjectId::new()),
                    container: "pg".into()
                }),
                None,
                None
            )
            .await
            .is_err());
        assert!(matches!(
            app.delete_host(host.id.clone()).await,
            Err(AppError::InUse { .. })
        ));
        let error =
            tokio::time::timeout(Duration::from_secs(30), app.db_test_connection(saved, None))
                .await
                .unwrap()
                .unwrap_err();
        assert_eq!(reason(error), "docker_unreachable");
        app.shutdown().await.unwrap();
    }
}
