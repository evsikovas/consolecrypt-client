//! Every database socket uses one frozen native SSH/jump/proxy route, or the
//! loopback address of a restored Kubernetes port-forward.
//! The driver supplies the original destination and keeps TLS identity separate.
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult};
use async_trait::async_trait;
use cc_db_core::transport::{DbConnector, DbDestination, DbIo, DbPath, DirectConnector};
use cc_db_core::DbError;
use cc_models::database::DbRoute;
use cc_models::host::Proxy;
use cc_models::ObjectKind;
use secrecy::SecretString;
use std::{collections::BTreeSet, fmt, sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

enum Transport {
    Direct,
    Ssh(Arc<cc_ssh_core::SshSession>),
    Proxy(Proxy, Option<SecretString>),
    /// A Kubernetes port-forward: every socket dials its loopback listener.
    Loopback(std::net::SocketAddr),
}

struct NativeConnector {
    transport: Transport,
    scope: CancellationToken,
    slots: Arc<Semaphore>,
}

impl fmt::Debug for NativeConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeDatabaseConnector([REDACTED])")
    }
}

#[async_trait]
impl DbConnector for NativeConnector {
    async fn connect(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<DbIo, DbError> {
        Ok(self.connect_path(destination, cancel).await?.0)
    }

    /// SSH and local forwards (Kubernetes, Docker, saved tunnels) protect the
    /// socket; a proxy does not, whatever address it is asked for.
    fn tunnelled(&self) -> bool {
        matches!(self.transport, Transport::Ssh(_) | Transport::Loopback(_))
    }

    async fn connect_path(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<(DbIo, DbPath), DbError> {
        DbDestination::new(destination.host.clone(), destination.port)?;
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| DbError::ConnectionFailed)?;
        let connect = async {
            match &self.transport {
                // The resolved peer decides whether a direct socket is loopback.
                Transport::Direct => {
                    DirectConnector
                        .connect_path(destination, cancel.clone())
                        .await
                }
                Transport::Ssh(ssh) => ssh
                    .open_direct_tcpip(&destination.host, destination.port, None)
                    .await
                    .map(|stream| (Box::new(stream) as DbIo, DbPath::Tunnel))
                    .map_err(|_| DbError::ConnectionFailed),
                Transport::Proxy(proxy, password) => cc_ssh_core::proxy::connect_via_proxy(
                    proxy,
                    password.as_ref(),
                    &destination.host,
                    destination.port,
                )
                .await
                .map(|stream| (Box::new(stream) as DbIo, DbPath::Network))
                .map_err(|_| DbError::ConnectionFailed),
                Transport::Loopback(address) => {
                    let forward = DbDestination::new(address.ip().to_string(), address.port())?;
                    let stream = DirectConnector.connect(&forward, cancel.clone()).await?;
                    Ok((stream, DbPath::Tunnel))
                }
            }
        };
        let (mut remote, path) = tokio::select! {
            biased;
            _ = self.scope.cancelled() => return Err(DbError::Cancelled),
            _ = cancel.cancelled() => return Err(DbError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(10), connect) =>
                result.map_err(|_| DbError::Timeout)??,
        };
        // A bounded in-process stream lets revocation close even an idle driver
        // socket without relying on another read/write from the driver.
        let (local, mut relay) = tokio::io::duplex(64 * 1024);
        let scope = self.scope.clone();
        tokio::spawn(async move {
            let _permit = permit;
            tokio::select! {
                biased;
                _ = scope.cancelled() => {},
                _ = cancel.cancelled() => {},
                _ = tokio::io::copy_bidirectional(&mut relay, &mut remote) => {},
            }
        });
        Ok((Box::new(local), path))
    }
}

impl AppCore {
    pub(crate) async fn db_resolve_route(
        &self,
        owner: &Arc<Session>,
        unlocked: &Arc<Unlocked>,
        route: &DbRoute,
        port: u16,
        cancel: &CancellationToken,
    ) -> AppResult<Arc<dyn DbConnector>> {
        // Establish dependency guards before resolving credentials or prompting.
        if let Some(id) = route.reference() {
            let snapshot = unlocked.working().snapshot();
            let revisions = crate::kubernetes_runtime::dependencies(&snapshot, id)?;
            if !unlocked
                .working()
                .watch_revisions(revisions, cancel.clone())
            {
                return Err(AppError::VaultLocked);
            }
        }
        let transport = match route {
            DbRoute::Direct => Transport::Direct,
            DbRoute::Ssh { host_id } => {
                let prepared = self
                    .sharing_prepare_connection(&host_id.to_string())
                    .await?;
                if !Arc::ptr_eq(unlocked, &prepared.unlocked) {
                    return Err(AppError::VaultLocked);
                }
                let session = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(AppError::VaultLocked),
                    result = prepared.run(async {
                        Ok(unlocked.ssh.connector.connect(&prepared.plan).await?)
                    }) => result?,
                };
                Transport::Ssh(Arc::new(session))
            }
            DbRoute::Jump { jump_profile_id } => {
                let jump = unlocked
                    .working()
                    .jump_profile(*jump_profile_id)
                    .ok_or_else(|| AppError::invalid("database", "route_unavailable"))?;
                if jump.chain.is_empty() || jump.chain.len() > cc_ssh_core::planner::MAX_HOPS {
                    return Err(AppError::invalid("database", "invalid_jump_chain"));
                }
                let mut visited = BTreeSet::new();
                let mut plans = Vec::new();
                for id in jump.chain {
                    if !visited.insert(id) {
                        return Err(AppError::invalid("database", "invalid_jump_chain"));
                    }
                    let prepared = self.sharing_prepare_connection(&id.to_string()).await?;
                    if !Arc::ptr_eq(unlocked, &prepared.unlocked) || cancel.is_cancelled() {
                        return Err(AppError::VaultLocked);
                    }
                    plans.push(prepared.plan);
                }
                let first_proxy = plans[0].proxy.clone();
                let mut last = plans
                    .pop()
                    .ok_or_else(|| AppError::invalid("database", "invalid_jump_chain"))?;
                last.route = plans.into_iter().map(|plan| plan.target_hop()).collect();
                last.proxy = first_proxy;
                let session = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(AppError::VaultLocked),
                    result = unlocked.ssh.connector.connect(&last) => result?,
                };
                Transport::Ssh(Arc::new(session))
            }
            DbRoute::Proxy { proxy_id } => {
                let proxy = unlocked
                    .working()
                    .proxy(*proxy_id)
                    .ok_or_else(|| AppError::invalid("database", "route_unavailable"))?;
                let password = match proxy.password_secret_id {
                    Some(id) => Some(SecretString::from(
                        unlocked.writer.read_secret(id).await?.value.expose_secret(),
                    )),
                    None => None,
                };
                Transport::Proxy(proxy, password)
            }
            DbRoute::Kubernetes {
                context_id,
                namespace,
                target_kind,
                target_name,
            } => {
                let forward = self.db_kube_forward(
                    unlocked,
                    *context_id,
                    namespace,
                    *target_kind,
                    target_name,
                    port,
                );
                let address = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(AppError::VaultLocked),
                    result = forward => result?,
                };
                Transport::Loopback(address)
            }
            DbRoute::Tunnel { tunnel_id } => {
                let forward = self.db_tunnel_forward(unlocked, *tunnel_id);
                let address = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(AppError::VaultLocked),
                    result = forward => result?,
                };
                Transport::Loopback(address)
            }
            DbRoute::Docker { host_id, container } => {
                let forward = self.db_docker_forward(unlocked, *host_id, container, port);
                let address = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(AppError::VaultLocked),
                    result = forward => result?,
                };
                Transport::Loopback(address)
            }
        };
        // Trust changes (including a host key replaced after a prompt) revoke
        // the route in the same way as a changed connection or credential.
        let trust = unlocked
            .working()
            .snapshot()
            .into_iter()
            .filter(|(_, entry)| entry.kind() == ObjectKind::KnownHost)
            .map(|(id, entry)| (id, entry.revision))
            .collect();
        if !unlocked.working().watch_revisions(trust, cancel.clone()) {
            return Err(AppError::VaultLocked);
        }
        let _profile = self.hold_session(owner).await?;
        let _generation = owner.hold_unlocked(unlocked).await?;
        if cancel.is_cancelled() {
            return Err(AppError::VaultLocked);
        }
        if let Transport::Ssh(ssh) = &transport {
            let ssh = ssh.clone();
            let scope = cancel.clone();
            tokio::spawn(async move {
                scope.cancelled().await;
                let _ = ssh.disconnect().await;
            });
        }
        Ok(Arc::new(NativeConnector {
            transport,
            scope: cancel.clone(),
            slots: Arc::new(Semaphore::new(32)),
        }))
    }
}
