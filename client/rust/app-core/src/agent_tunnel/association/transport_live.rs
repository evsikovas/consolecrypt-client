//! Actual maintained SSH/password/host-key/subsystem framing. Metadata-only
//! server deliberately cannot simulate an Actor commit or association ACK.
use super::*;
use cc_ssh_core::russh::server::Server as _;
use cc_ssh_core::russh::{self, Channel, ChannelId, server};
use std::collections::HashMap;
#[derive(Clone)]
struct OwnedServer {
    payload: Arc<serde_json::Value>,
    names: Arc<std::sync::Mutex<Vec<String>>>,
    tasks: Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}
struct OwnedHandler {
    server: OwnedServer,
    channels: HashMap<ChannelId, Channel<server::Msg>>,
}
impl server::Server for OwnedServer {
    type Handler = OwnedHandler;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> OwnedHandler {
        OwnedHandler {
            server: self.clone(),
            channels: HashMap::new(),
        }
    }
}
impl server::Handler for OwnedHandler {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        Ok(
            if user == "enrollment" && password == "owned enrollment password42" {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.server.names.lock().unwrap().push(name.into());
        if name != SUBSYSTEM {
            session.channel_failure(id)?;
            return Ok(());
        }
        session.channel_success(id)?;
        let mut stream = self.channels.remove(&id).unwrap().into_stream();
        let payload = self.server.payload.clone();
        let task = tokio::spawn(async move {
            let mut request = Vec::new();
            loop {
                let b = stream.read_u8().await.unwrap();
                request.push(b);
                assert!(request.len() <= protocol::MAX_FRAME);
                if b == b'\n' {
                    break;
                }
            }
            let v: serde_json::Value = serde_json::from_slice(&request).unwrap();
            assert_eq!(
                protocol::canonical(&v).unwrap().as_slice(),
                request.as_slice()
            );
            assert_eq!(v["kind"], "probe");
            assert_eq!(v["payload"], serde_json::json!({}));
            let raw=protocol::canonical(&serde_json::json!({"format":protocol::FORMAT,"requestId":v["requestId"],"kind":"probe","payload":*payload})).unwrap();
            stream.write_all(&raw).await.unwrap();
            stream.flush().await.unwrap();
            stream.shutdown().await.unwrap();
        });
        self.server.tasks.lock().await.push(task);
        Ok(())
    }
    async fn exec_request(
        &mut self,
        _: ChannelId,
        _: &[u8],
        _: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        panic!("no generic exec in enrollment fixture")
    }
    async fn shell_request(
        &mut self,
        _: ChannelId,
        _: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        panic!("no shell in enrollment fixture")
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_original_saved_host_uses_pinned_ssh_fixed_subsystem_no_exec() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let key = cc_ssh_core::keys::generate_key(
        cc_ssh_core::keys::KeyGenAlgorithm::Ed25519,
        "owned enrollment SSH",
        None,
    )
    .unwrap();
    let private = cc_ssh_core::keys::load_private_key(&key.private_openssh, None).unwrap();
    let (root, app, old_scope) = super::super::tests::local().await;
    drop(old_scope);
    let credential = app
        .add_password_credential(
            "Owned enrollment".into(),
            Some("enrollment".into()),
            "owned enrollment password42".into(),
        )
        .await
        .unwrap();
    app.import_known_hosts(format!("[127.0.0.1]:{port} {}\n", key.public_openssh))
        .await
        .unwrap();
    let mut host = crate::HostDto::new("Owned enrollment node", "127.0.0.1");
    host.port = Some(port);
    host.username = Some("enrollment".into());
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    let host = app.save_host(host).await.unwrap();
    let host_id = crate::dto::parse_id("host", &host.id).unwrap();
    let revision = app
        .unlocked()
        .await
        .unwrap()
        .1
        .working()
        .revision_of(host_id)
        .unwrap();
    let scope = Arc::new(
        OriginalScope::capture(&app, &CancellationToken::new(), Duration::from_secs(30))
            .await
            .unwrap(),
    );
    let identity = KeyFixture::new();
    let mut policy: TunnelRoutePolicy = serde_json::from_str(include_str!(
        "../../../../agent-node-wire/src/tunnel_policy_vector.json"
    ))
    .unwrap();
    policy.context.tenant = Tenant::LocalProfile {
        id: LocalProfileId::new(&Uuid::new_v4().to_string()).unwrap(),
    };
    let signing = scope
        .with_loaded(&app, |id| Ok(id.public_keys().signing))
        .await
        .unwrap();
    let pin = OwnerPin {
        context: policy.context,
        key: PublicKey::from_bytes(signing),
        epoch: 1,
        checkpoint: 0,
        expires_at: scope.now().unwrap().0.checked_add(60).unwrap(),
    };
    let payload = Arc::new(
        serde_json::json!({"node":{"format":1,"agentId":policy.agent_id,"keyEpoch":1,"publicKey":identity.0,"keyFingerprint":identity.0.fingerprint()},
        "ownerPin":pin,"actorRevision":0,"certificate":null,"revoked":false}),
    );
    let names = Arc::new(std::sync::Mutex::new(Vec::new()));
    let tasks = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let mut server = OwnedServer {
        payload,
        names: names.clone(),
        tasks: tasks.clone(),
    };
    let config = Arc::new(server::Config {
        keys: vec![private],
        inactivity_timeout: Some(Duration::from_secs(30)),
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let running = server.run_on_socket(config, &listener);
        assert!(tx.send(running.handle()).is_ok());
        running.await.unwrap();
    });
    let handle = rx.await.unwrap();
    let mut peer = Peer::open(&app, scope.clone(), host_id, revision)
        .await
        .unwrap();
    let probe: protocol::Probe = peer
        .exchange(&app, "probe", serde_json::json!({}), None)
        .await
        .unwrap();
    protocol::initial(&scope, &probe).unwrap();
    assert_eq!(probe.actor_revision, 0);
    assert_eq!(names.lock().unwrap().as_slice(), [SUBSYSTEM]);
    // No actual Claim/cert/Actor endpoint exists here; no positive association.
    std::thread::spawn(move || {
        assert!(tokio::runtime::Handle::try_current().is_err());
        drop(peer);
    })
    .join()
    .unwrap();
    app.shutdown().await.unwrap();
    handle.shutdown("owned enrollment SSH complete".into());
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    for task in tasks.lock().await.drain(..) {
        task.await.unwrap();
    }
    drop(root);
}
struct KeyFixture(PublicKey);
impl KeyFixture {
    fn new() -> Self {
        let key = cc_crypto_core::DeviceSecretKeys::generate().unwrap();
        Self(PublicKey::from_bytes(key.public_keys().signing))
    }
}
