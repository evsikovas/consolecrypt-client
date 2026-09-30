//! Real SQLCipher/AppCore route guards. No sharing server is configured in a
//! local profile: a bound endpoint anywhere in the SSH route must fail before
//! credentials or even a TCP connection are sent. Private keys are generated
//! by AppCore at runtime, and the public binding IDs are generated here.
mod common;

use cc_app_core::*;
use common::app;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use uuid::Uuid;

const INSTANCE: &str = "cc.shared.instance";
const SHARE: &str = "cc.shared.share";

struct Fixture {
    _directory: tempfile::TempDir,
    app: AppCore,
    port: u16,
    attempts: Arc<AtomicUsize>,
    probe: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.probe.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        app.create_local_profile(
            "Private local profile".into(),
            format!("runtime-{}-{}", Uuid::new_v4(), Uuid::new_v4()),
        )
        .await
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let attempts = Arc::new(AtomicUsize::new(0));
        let seen = attempts.clone();
        let probe = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                seen.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        Self {
            _directory: directory,
            app,
            port,
            attempts,
            probe,
        }
    }
    async fn host(&self, name: &str, bound: bool) -> HostDto {
        let mut host = HostDto::new(name, "127.0.0.1");
        host.port = Some(self.port);
        host.username = Some("operator".into());
        if bound {
            host.metadata
                .insert(INSTANCE.into(), Uuid::new_v4().to_string());
            host.metadata
                .insert(SHARE.into(), Uuid::new_v4().to_string());
        }
        self.app.save_host(host).await.unwrap()
    }
    async fn tunnel(&self, host: &HostDto, auto_start: bool) -> TunnelDto {
        self.app
            .save_tunnel(TunnelDto {
                id: String::new(),
                name: "Private port forward".into(),
                kind: TunnelKind::Local,
                host_id: host.id.clone(),
                bind_host: "127.0.0.1".into(),
                bind_port: 32179,
                target_host: Some("127.0.0.1".into()),
                target_port: Some(5432),
                auto_start,
                binds_publicly: false,
                created_at_ms: 0,
                updated_at_ms: 0,
            })
            .await
            .unwrap()
    }
    async fn no_network(&self) {
        tokio::task::yield_now().await;
        assert_eq!(
            self.attempts.load(Ordering::SeqCst),
            0,
            "sharing verification must precede every TCP/SSH connection"
        );
    }
}

async fn blocked_local<T>(operation: impl Future<Output = AppResult<T>>) {
    let result = tokio::time::timeout(Duration::from_secs(3), operation)
        .await
        .expect("the route guard must finish before any SSH wait");
    assert!(
        matches!(result, Err(AppError::LocalProfile)),
        "a bound route requires authenticated online sharing, even for a private target"
    );
}

#[tokio::test]
async fn explicit_shared_jump_guards_all_connection_entrypoints_before_tcp() {
    let fixture = Fixture::new().await;
    let shared_jump = fixture.host("Shared jump", true).await;
    let mut target = fixture.host("Private target", false).await;
    target.jump_chain = vec![shared_jump.id];
    let target = fixture.app.save_host(target).await.unwrap();
    let tunnel = fixture.tunnel(&target, false).await;
    let app = &fixture.app;
    blocked_local(app.describe_connection(target.id.clone())).await;
    blocked_local(app.exec(target.id.clone(), "printf explicit".into())).await;
    blocked_local(app.open_terminal(target.id.clone(), 80, 24)).await;
    blocked_local(app.sftp_open(target.id.clone())).await;
    blocked_local(app.prepare_openssh_session(target.id.clone(), None, true)).await;
    blocked_local(app.start_tunnel(tunnel.id)).await;
    fixture.no_network().await;
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn inherited_and_recursive_shared_jumps_require_verification() {
    let fixture = Fixture::new().await;
    let shared_jump = fixture.host("Shared outer jump", true).await;
    let mut private_jump = fixture.host("Private inner jump", false).await;
    private_jump.jump_chain = vec![shared_jump.id];
    let private_jump = fixture.app.save_host(private_jump).await.unwrap();
    let jump_profile = fixture
        .app
        .save_jump_profile(JumpProfileDto {
            id: String::new(),
            name: "Inherited private route".into(),
            chain: vec![private_jump.id],
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .unwrap();
    let mut group = GroupDto::new("Inherited route group");
    group.inherited_jump_profile_id = Some(jump_profile.id);
    let group = fixture.app.save_group(group).await.unwrap();
    let mut target = fixture.host("Private grouped target", false).await;
    target.group_id = Some(group.id);
    let target = fixture.app.save_host(target).await.unwrap();
    blocked_local(fixture.app.describe_connection(target.id.clone())).await;
    let tunnel = fixture.tunnel(&target, true).await;
    let failures = fixture.app.start_auto_tunnels().await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].tunnel_id, tunnel.id);
    assert!(failures[0]
        .message
        .contains("shared_host_requires_verification"));
    assert!(fixture.app.tunnel_statuses().await.unwrap().is_empty());
    fixture.no_network().await;
    fixture.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn direct_bound_target_and_its_auto_tunnel_fail_closed() {
    let fixture = Fixture::new().await;
    let target = fixture.host("Shared target", true).await;
    blocked_local(fixture.app.describe_connection(target.id.clone())).await;
    let tunnel = fixture.tunnel(&target, true).await;
    let failures = fixture.app.start_auto_tunnels().await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].tunnel_id, tunnel.id);
    assert!(failures[0]
        .message
        .contains("shared_host_requires_verification"));
    fixture.no_network().await;
    fixture.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn partial_malformed_and_nil_bindings_never_downgrade_to_private_host() {
    let fixture = Fixture::new().await;
    let valid_instance = Uuid::new_v4().to_string();
    let valid_share = Uuid::new_v4().to_string();
    let cases = [
        (None, Some(valid_share.clone())),
        (Some(valid_instance.clone()), None),
        (Some("invalid".into()), Some(valid_share.clone())),
        (Some(valid_instance.clone()), Some("invalid".into())),
        (Some(Uuid::nil().to_string()), Some(valid_share.clone())),
        (Some(valid_instance.clone()), Some(Uuid::nil().to_string())),
        (Some(format!("{{{valid_instance}}}")), Some(valid_share)),
    ];
    for (index, (instance, share)) in cases.into_iter().enumerate() {
        let mut host = fixture
            .host(&format!("Malformed endpoint {index}"), false)
            .await;
        if let Some(instance) = instance {
            host.metadata.insert(INSTANCE.into(), instance);
        }
        if let Some(share) = share {
            host.metadata.insert(SHARE.into(), share);
        }
        let host = fixture.app.save_host(host).await.unwrap();
        for result in [
            fixture
                .app
                .describe_connection(host.id.clone())
                .await
                .map(|_| ()),
            fixture
                .app
                .sharing_refresh_bound_host(host.id.clone(), false)
                .await
                .map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(AppError::InvalidInput { ref field, ref reason })
                if field == "shared_host" && reason == "binding_metadata_invalid")
            );
        }
        let detached = fixture.app.sharing_detach_host(host.id).await.unwrap();
        assert!(!detached.metadata.contains_key(INSTANCE));
        assert!(!detached.metadata.contains_key(SHARE));
        assert!(fixture
            .app
            .describe_connection(detached.id)
            .await
            .unwrap()
            .route
            .contains("operator@127.0.0.1"));
    }
    fixture.no_network().await;
    fixture.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_shared_jump_also_blocks_automatic_tunnels() {
    let fixture = Fixture::new().await;
    let mut jump = fixture.host("Partial shared jump", false).await;
    jump.metadata
        .insert(INSTANCE.into(), Uuid::new_v4().to_string());
    let jump = fixture.app.save_host(jump).await.unwrap();
    let mut target = fixture.host("Private target", false).await;
    target.jump_chain = vec![jump.id];
    let target = fixture.app.save_host(target).await.unwrap();
    let tunnel = fixture.tunnel(&target, true).await;
    let failures = fixture.app.start_auto_tunnels().await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].tunnel_id, tunnel.id);
    assert!(failures[0].message.contains("binding_metadata_invalid"));
    fixture.no_network().await;
    fixture.app.shutdown().await.unwrap();
}
