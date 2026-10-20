//! Every database socket uses one frozen native SSH/jump/proxy route.
//! The driver supplies the original destination and keeps TLS identity separate.
use crate::session::{Session, Unlocked};
use crate::{AppCore, AppError, AppResult};
use async_trait::async_trait;
use cc_db_core::transport::{DbConnector, DbDestination, DbIo, DirectConnector};
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
        DbDestination::new(destination.host.clone(), destination.port)?;
        let permit = self.slots.clone().try_acquire_owned().map_err(|_| DbError::ConnectionFailed)?;
        let connect = async {
            match &self.transport {
                Transport::Direct => DirectConnector.connect(destination, cancel.clone()).await,
                Transport::Ssh(ssh) => ssh
                    .open_direct_tcpip(&destination.host, destination.port, None)
                    .await
                    .map(|stream| Box::new(stream) as DbIo)
                    .map_err(|_| DbError::ConnectionFailed),
                Transport::Proxy(proxy, password) => {
                    cc_ssh_core::proxy::connect_via_proxy(
                        proxy,
                        password.as_ref(),
                        &destination.host,
                        destination.port,
                    )
                    .await
                    .map(|stream| Box::new(stream) as DbIo)
                    .map_err(|_| DbError::ConnectionFailed)
                }
            }
        };
        let mut remote = tokio::select! {
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
        Ok(Box::new(local))
    }
}

impl AppCore {
    pub(crate) async fn db_resolve_route(
        &self,
        owner: &Arc<Session>,
        unlocked: &Arc<Unlocked>,
        route: &DbRoute,
        cancel: &CancellationToken,
    ) -> AppResult<Arc<dyn DbConnector>> {
        // Establish dependency guards before resolving credentials or prompting.
        if let Some(id) = route.reference() {
            let snapshot = unlocked.working().snapshot();
            let revisions = crate::kubernetes_runtime::dependencies(&snapshot, id)?;
            if !unlocked.working().watch_revisions(revisions, cancel.clone()) {
                return Err(AppError::VaultLocked);
            }
        }
        let transport = match route {
            DbRoute::Direct => Transport::Direct,
            DbRoute::Ssh { host_id } => {
                let prepared = self.sharing_prepare_connection(&host_id.to_string()).await?;
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
                let jump = unlocked.working().jump_profile(*jump_profile_id)
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
                let mut last = plans.pop()
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
                let proxy = unlocked.working().proxy(*proxy_id)
                    .ok_or_else(|| AppError::invalid("database", "route_unavailable"))?;
                let password = match proxy.password_secret_id {
                    Some(id) => Some(SecretString::from(
                        unlocked.writer.read_secret(id).await?.value.expose_secret(),
                    )),
                    None => None,
                };
                Transport::Proxy(proxy, password)
            }
        };
        // Trust changes (including a host key replaced after a prompt) revoke
        // the route in the same way as a changed connection or credential.
        let trust = unlocked.working().snapshot().into_iter()
            .filter(|(_, entry)| entry.kind() == ObjectKind::KnownHost)
            .map(|(id, entry)| (id, entry.revision)).collect();
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
