//! A single read borrows an already approved channel. No approval, reconnect,
//! custody transfer or listener ownership is acquired by this adapter.
use super::*;
use crate::session::{Session, Unlocked};
use std::{net::SocketAddr, sync::Weak};
use tokio::net::{TcpSocket, TcpStream};

#[derive(Default)]
pub(super) struct AcceptedSource {
    state: Mutex<Acceptance>,
    released: CancellationToken,
}
#[derive(Default)]
struct Acceptance {
    claimed: bool,
    used: bool,
    accepted: bool,
    pending: Option<(SocketAddr, oneshot::Sender<()>)>,
}
impl AcceptedSource {
    pub(super) async fn wait_for_read_release(&self) -> bool {
        let claimed=self.state.lock().map(|state|state.claimed).unwrap_or(true);
        if claimed {self.released.cancelled().await;}
        claimed
    }
    fn reserve(&self, peer: SocketAddr) -> AppResult<oneshot::Receiver<()>> {
        let mut state = self.state.lock().map_err(|_| unknown())?;
        if state.used || state.accepted {
            return Err(unavailable());
        }
        let (tx, rx) = oneshot::channel();
        state.used = true;
        state.pending = Some((peer, tx));
        Ok(rx)
    }
    pub(super) fn close_pending(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.take();
        }
    }
    pub(super) fn accepted(&self, peer: SocketAddr) {
        if let Ok(mut state) = self.state.lock() {
            state.accepted = true;
            if let Some((expected, tx)) = state.pending.take() {
                if peer == expected {
                    let _ = tx.send(());
                }
            }
        }
    }
}
struct PendingRead(Weak<Shared>);
impl Drop for PendingRead {
    fn drop(&mut self) {
        if let Some(shared) = self.0.upgrade() {
            shared.monitoring.close_pending();
        }
    }
}
pub(crate) struct MonitoringTunnelRoute {
    shared: Weak<Shared>,
    id: ResourceId,
    target: SocketAddr,
    released: CancellationToken,
}
impl Drop for MonitoringTunnelRoute {
    fn drop(&mut self){self.release();}
}
impl MonitoringTunnelRoute {
    pub(crate) fn release(&self){self.released.cancel();}
    pub(crate) fn borrow(
        app: &AppCore,
        owner: &Arc<Session>,
        unlock: &Arc<Unlocked>,
        route_id: Option<&str>,
        saved_association: Option<&str>,
        target: SocketAddr,
        saved_host: Option<ObjectId>,
    ) -> AppResult<Self> {
        let expected_id = route_id.map(ResourceId::new).transpose().map_err(|_|unavailable())?;
        let association = saved_association.map(|raw| {
            let id=Uuid::parse_str(raw).map_err(|_|unavailable())?;
            if id.is_nil() || id.to_string()!=raw {return Err(unavailable())}
            Ok(id)
        }).transpose()?;
        if expected_id.is_none() && association.is_none() || association.is_some() && saved_host.is_none() {
            return Err(unavailable());
        }
        let mut found = None;
        for weak in automatic::monitoring_candidates()? {
            let Some(shared) = weak.upgrade() else {
                continue;
            };
            if !Weak::ptr_eq(&shared.context.core, &Arc::downgrade(&app.inner)) {
                continue;
            }
            // An explicit transport choice cannot retarget a source whose
            // saved SSH route already identifies a different remote Host.
            if saved_host.is_some_and(|id|id!=shared.automatic.saved_host()) {
                continue;
            }
            if association.is_some_and(|id|id!=shared.automatic.association_id()) {
                continue;
            }
            if !shared
                .context
                .scope
                .matches_monitoring_owner(owner, unlock)
                .unwrap_or(false)
            {
                continue;
            }
            // Select only an already issued, live, signed original. Reading a
            // stable reference never prepares, signs, reconnects or renews it.
            let id = {
                let state=shared.state.lock().map_err(|_|unknown())?;
                let Some(route)=state.route.as_ref() else {continue};
                let Ok((id,actual_target))=route.monitoring_target() else {continue};
                if actual_target!=target || expected_id.as_ref().is_some_and(|expected|expected!=&id) {continue}
                id
            };
            let candidate = Self {
                shared: weak,
                id,
                target,
                released: CancellationToken::new(),
            };
            if candidate.check().is_err() {
                continue;
            }
            {
                let state = shared.monitoring.state.lock().map_err(|_| unknown())?;
                if state.claimed || state.used || state.accepted {
                    return Err(unavailable());
                }
            }
            if found.is_some() {
                return Err(unknown());
            }
            found = Some(candidate);
        }
        let mut found=found.ok_or_else(unavailable)?;
        let shared=found.shared.upgrade().ok_or_else(unavailable)?;
        {
            let mut state=shared.monitoring.state.lock().map_err(|_|unknown())?;
            if state.claimed||state.used||state.accepted {return Err(unavailable())}
            state.claimed=true;
            found.released=shared.monitoring.released.clone();
        }
        Ok(found)
    }
    pub(crate) fn check(&self) -> AppResult<()> {
        let shared = self.shared.upgrade().ok_or_else(unavailable)?;
        shared.context.scope.check_current()?;
        let state = shared.state.lock().map_err(|_| unknown())?;
        if !state.busy
            || !state.consumed
            || state.orphaned
            || state.joined
            || !matches!(
                state.view["phase"].as_str(),
                Some("listening" | "connecting" | "connected")
            )
        {
            return Err(unavailable());
        }
        let actual = state
            .route
            .as_ref()
            .ok_or_else(unavailable)?
            .monitoring_target()?;
        if actual != (self.id.clone(), self.target) {
            return Err(unavailable());
        }
        Ok(())
    }
    pub(crate) async fn connect(&self) -> AppResult<TcpStream> {
        self.check()?;
        let shared = self.shared.upgrade().ok_or_else(unavailable)?;
        let address = {
            let state = shared.state.lock().map_err(|_| unknown())?;
            if state.view["phase"] != "listening" {
                return Err(unavailable());
            }
            state.view["boundAddress"]
                .as_str()
                .ok_or_else(unavailable)?
                .parse::<SocketAddr>()
                .map_err(|_| unavailable())?
        };
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(unavailable());
        }
        let socket = if address.is_ipv4() {
            TcpSocket::new_v4()
        } else {
            TcpSocket::new_v6()
        }
        .map_err(|_| unavailable())?;
        socket
            .bind(SocketAddr::new(address.ip(), 0))
            .map_err(|_| unavailable())?;
        let peer = socket.local_addr().map_err(|_| unavailable())?;
        let rx = shared.monitoring.reserve(peer)?;
        let _pending = PendingRead(self.shared.clone());
        // No application byte is returned until the ORIGINAL typed listener
        // accepts this exact kernel socket. A rebound local port cannot pass.
        let result = bounded_original(&shared.context.scope, async {
            let stream = socket.connect(address).await.map_err(|_| unavailable())?;
            rx.await.map_err(|_| unavailable())?;
            self.check()?;
            Ok(stream)
        })
        .await;
        // No retry, even after a failed dial or a lost original generation.
        result
    }
}

#[cfg(any(test,all(feature="test-harness",target_os="linux")))]
async fn saved_destination_data_cannot_restore_a_pre_save_scope_or_mint_a_route() {
    let directory=tempfile::tempdir().unwrap();
    let app=AppCore::new(crate::AppConfig::for_tests(directory.path().to_string_lossy().into_owned())).unwrap();
    app.create_local_profile("Saved Agent destination scope".into(),"owned saved source scope test password 42".into()).await.unwrap();
    let mut host=crate::HostDto::new("Saved Agent destination host","127.0.0.1");
    host.port=Some(22);host.username=Some("owned".into());host.host_key_policy=cc_ssh_core::HostKeyPolicy::Strict;
    let mut host_events=app.subscribe_events();
    let saved=app.save_host(host).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            if let crate::AppEvent::ObjectsChanged{kind:cc_models::ObjectKind::Host,
                origin:crate::ChangeOriginDto::Local,ids}=host_events.recv().await.unwrap() {
                if ids.iter().any(|id|id==&saved.id) {break}
            }
        }
    }).await.unwrap();drop(host_events);
    let host=crate::dto::parse_id("monitoring_agent_host",&saved.id).unwrap();
    let mut credential_events=app.subscribe_events();
    let credential=app.add_password_credential("Saved source token".into(),None,"owned-source-token-42".into()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            if let crate::AppEvent::ObjectsChanged{kind:cc_models::ObjectKind::Credential,
                origin:crate::ChangeOriginDto::Local,ids}=credential_events.recv().await.unwrap() {
                if ids.iter().any(|id|id==&credential.id) {break}
            }
        }
    }).await.unwrap();drop(credential_events);
    let ca=rcgen::generate_simple_self_signed(vec!["owned-source.invalid".into()]).unwrap().cert.pem();
    let original=OriginalScope::capture(&app,&CancellationToken::new(),Duration::from_secs(300)).await.unwrap();
    let association=Uuid::new_v4().to_string();
    let config=cc_monitoring_core::SourceConfig{id:String::new(),name:"Saved DATA only".into(),
        endpoint:"https://owned-source.invalid/".into(),
        route:cc_monitoring_core::Route::AgentDestination{association_id:association.clone(),host_id:host.to_string(),
            destination_ip:"127.0.0.1".into(),destination_port:9090},
        tls:cc_monitoring_core::TlsConfig{custom_ca_pem:Some(ca.clone())},
        auth:cc_monitoring_core::AuthRef::BearerRef{credential_id:credential.id.clone()}};
    let mut events=app.subscribe_events();
    let catalog=app.monitoring_source_create(0,config).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            if let crate::AppEvent::ObjectsChanged{kind:cc_models::ObjectKind::VaultSettings,
                origin:crate::ChangeOriginDto::Local,..}=events.recv().await.unwrap() {break}
        }
    }).await.unwrap();drop(events);
    assert!(original.check_current().is_err());
    let(owner,unlock)=app.unlocked().await.unwrap();
    assert!(!original.matches_monitoring_owner(&owner,&unlock).unwrap_or(false));
    assert!(MonitoringTunnelRoute::borrow(&app,&owner,&unlock,None,Some(&association),
        "127.0.0.1:9090".parse().unwrap(),Some(host)).is_err());
    drop(unlock);drop(owner);
    let source=&catalog.sources[0];
    assert!(app.monitoring_read(json!({"operationId":Uuid::new_v4(),"sourceId":source.id,
        "sourceRevision":1,"action":{"kind":"query","expression":"up","time":null}}).to_string()).await.is_err());
    let expected=source.configuration.clone();let source_id=source.id.clone();
    assert!(!serde_json::to_string(&catalog).unwrap().contains("owned-source-token-42"));
    app.lock().await.unwrap();
    app.unlock_with_passphrase("owned saved source scope test password 42".into()).await.unwrap();
    let restored=app.monitoring_sources().await.unwrap();assert_eq!(restored.sources.len(),1);
    assert_eq!(restored.sources[0].id,source_id);assert_eq!(restored.sources[0].revision,Some(1));
    assert_eq!(restored.sources[0].configuration,expected);
    assert!(original.check_current().is_err());
    let(owner,unlock)=app.unlocked().await.unwrap();
    assert!(!original.matches_monitoring_owner(&owner,&unlock).unwrap_or(false));
    assert!(MonitoringTunnelRoute::borrow(&app,&owner,&unlock,None,Some(&association),
        "127.0.0.1:9090".parse().unwrap(),Some(host)).is_err());
    drop(unlock);drop(owner);
    assert!(app.monitoring_read(json!({"operationId":Uuid::new_v4(),"sourceId":source_id,
        "sourceRevision":1,"action":{"kind":"query","expression":"up","time":null}}).to_string()).await.is_err());
    app.shutdown().await.unwrap();
}
#[cfg(all(feature="test-harness",target_os="linux"))]
impl AppCore {
    pub(crate) async fn __owned_saved_destination_source_scope() {
        saved_destination_data_cannot_restore_a_pre_save_scope_or_mint_a_route().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn saved_destination_save_retires_old_scope_without_recreating_a_grant() {
        saved_destination_data_cannot_restore_a_pre_save_scope_or_mint_a_route().await;
    }
    #[tokio::test]
    async fn only_the_actual_accepted_kernel_peer_releases_the_read_once() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let peer = socket.local_addr().unwrap();
        let accepted = AcceptedSource::default();
        let pending = accepted.reserve(peer).unwrap();
        assert!(accepted.reserve(peer).is_err());
        let (client, server) = tokio::join!(
            socket.connect(listener.local_addr().unwrap()),
            listener.accept()
        );
        let client = client.unwrap();
        let (server, observed) = server.unwrap();
        assert_eq!(observed, peer);
        accepted.accepted(observed);
        pending.await.unwrap();
        drop(client);
        drop(server);
        assert!(accepted.reserve(peer).is_err());
    }
    #[tokio::test]
    async fn other_accept_or_original_close_cannot_release_or_retry_a_pending_read() {
        let peer = "127.0.0.1:49151".parse().unwrap();
        let original = AcceptedSource::default();
        let pending = original.reserve(peer).unwrap();
        original.accepted("127.0.0.1:49152".parse().unwrap());
        assert!(pending.await.is_err());
        assert!(original.reserve(peer).is_err());
        let closed = AcceptedSource::default();
        let pending = closed.reserve(peer).unwrap();
        closed.close_pending();
        assert!(pending.await.is_err());
        assert!(closed.reserve(peer).is_err());
    }
}
