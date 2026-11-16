//! Saved routes dispatch to their original Native transport. Agent reads borrow
//! one approved channel; closing the read never stops or retains its owner.
use super::*;
use crate::session::{Session, Unlocked};
use crate::working_set::Entry;
use cc_models::ObjectId;
use cc_monitoring_core::{MonitoringConnect, MonitoringIo, MonitoringSourceConnector, Route};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Weak,
};

pub(super) enum SourceRoute {
    Ssh(Arc<ssh_route::SshSource>),
    Agent(AgentSource),
}
pub(super) struct AgentSource {
    source: SourceConfig,
    original: crate::agent_tunnel::MonitoringTunnelRoute,
    owner: Weak<Session>,
    app: Weak<crate::app::Inner>,
    unlock: Weak<Unlocked>,
    mutation: u64,
    cancel: CancellationToken,
    closed: AtomicBool,
}
impl std::fmt::Debug for SourceRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MonitoringSourceRoute([scoped])")
    }
}
fn failed() -> cc_monitoring_core::Error {
    cc_monitoring_core::Error {
        code: cc_monitoring_core::ErrorCode::Transport,
        message: "Original approved monitoring route unavailable or already used".into(),
        http_status: None,
    }
}
impl SourceRoute {
    pub(super) async fn open(
        app: &AppCore,
        owner: &Arc<Session>,
        unlock: &Arc<Unlocked>,
        source: &SourceConfig,
        saved_source: &SourceConfig,
        snapshot: &BTreeMap<ObjectId, Entry>,
        mutation: u64,
        cancel: &CancellationToken,
    ) -> AppResult<Option<Arc<Self>>> {
        if matches!(&source.route, Route::AgentTunnel { .. } | Route::AgentDestination { .. }) {
            let url = source
                .validate()
                .map_err(|_| invalid("invalid monitoring source"))?;
            // URL/Host/SNI/CA remain the original logical HTTPS origin. A
            // durable destination selects only the already signed physical TCP
            // target; it never rewrites the URL or triggers DNS/network fallback.
            let (ip,port)=match &saved_source.route {
                Route::AgentDestination { destination_ip, destination_port, .. } => {
                    let ip=destination_ip.parse::<std::net::IpAddr>()
                        .map_err(|_|invalid("invalid saved Agent destination"))?;
                    if !matches!(destination_ip.as_str(),"127.0.0.1"|"::1")
                        || ip.to_string()!=*destination_ip || *destination_port==0 {
                        return Err(invalid("invalid saved Agent destination"));
                    }
                    if matches!(&source.route,Route::AgentDestination{..}) && source.route!=saved_source.route {
                        return Err(invalid("saved Agent destination changed"));
                    }
                    (ip,*destination_port)
                }
                _ => {
                    let ip=match url.host() {
                        Some(url::Host::Ipv4(ip))=>std::net::IpAddr::V4(ip),
                        Some(url::Host::Ipv6(ip))=>std::net::IpAddr::V6(ip),
                        _=>return Err(invalid("Agent monitoring requires the approved numeric destination")),
                    };
                    let port=url.port_or_known_default().filter(|p|*p!=0)
                        .ok_or_else(||invalid("invalid monitoring destination"))?;
                    (ip,port)
                }
            };
            let route_id = match &source.route {
                Route::AgentTunnel { route_id } => Some(route_id.as_str()),
                Route::AgentDestination { .. } => None,
                _ => return Err(invalid("invalid Agent monitoring selection")),
            };
            // The persisted reference is DATA only. Even an explicit transient
            // route choice must retain its original association/Host/target.
            let (saved_association, saved_host) = match &saved_source.route {
                Route::AgentDestination { association_id, host_id, .. } => {
                    (Some(association_id.as_str()), Some(crate::dto::parse_id("monitoring_agent_host",host_id)?))
                }
                Route::SshHost { host_id } => (None, Some(crate::dto::parse_id("monitoring_ssh_host",host_id)?)),
                _ => (None,None),
            };
            let original = crate::agent_tunnel::MonitoringTunnelRoute::borrow(
                app, owner, unlock, route_id, saved_association,
                std::net::SocketAddr::new(ip, port), saved_host,
            )?;
            let route = Arc::new(Self::Agent(AgentSource {
                source: source.clone(),
                original,
                owner: Arc::downgrade(owner),
                app: Arc::downgrade(&app.inner),
                unlock: Arc::downgrade(unlock),
                mutation,
                cancel: cancel.child_token(),
                closed: AtomicBool::new(false),
            }));
            route.check()?;
            return Ok(Some(route));
        }
        Ok(
            ssh_route::SshSource::open(app, owner, unlock, source, snapshot, mutation, cancel)
                .await?
                .map(|route| Arc::new(Self::Ssh(route))),
        )
    }
    pub(super) fn check(&self) -> AppResult<()> {
        match self {
            Self::Ssh(route) => route.check(),
            Self::Agent(route) => {
                if route.cancel.is_cancelled()
                    || route.closed.load(Ordering::Acquire)
                    || !route.unlock.upgrade().is_some_and(|u| {
                        !*u.sharing_shutdown.borrow()
                            && u.working().mutation_stamp() == route.mutation
                    })
                {
                    return Err(invalid("original Agent monitoring route changed"));
                }
                route.original.check()
            }
        }
    }
    pub(super) async fn close(&self) {
        match self {
            Self::Ssh(route) => route.close().await,
            Self::Agent(route) => {
                route.original.release();
                route.closed.store(true, Ordering::Release);
                route.cancel.cancel();
            }
        }
    }
}
impl MonitoringSourceConnector for SourceRoute {
    fn source(&self) -> &SourceConfig {
        match self {
            Self::Ssh(route) => route.source(),
            Self::Agent(route) => &route.source,
        }
    }
    fn is_current(&self) -> bool {
        match self {
            Self::Ssh(route) => route.is_current(),
            Self::Agent(route) => {
                if self.check().is_err() {
                    return false;
                }
                let (Some(app), Some(owner)) = (route.app.upgrade(), route.owner.upgrade()) else {
                    return false;
                };
                let Ok(active) = app.active.try_lock() else {
                    return false;
                };
                active
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &owner))
            }
        }
    }
    fn connect(&self) -> MonitoringConnect<'_> {
        match self {
            Self::Ssh(route) => route.connect(),
            Self::Agent(route) => Box::pin(async move {
                if !self.is_current() {
                    return Err(failed());
                }
                let stream = tokio::select! {biased;_=route.cancel.cancelled()=>return Err(failed()),
                result=route.original.connect()=>result.map_err(|_|failed())?};
                if !self.is_current() {
                    return Err(failed());
                }
                Ok(Box::new(stream) as MonitoringIo)
            }),
        }
    }
}
impl Drop for AgentSource {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.closed.store(true, Ordering::Release);
    }
}
