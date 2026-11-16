//! DRAFT actual Synced AppCore/MockServer -> owned fixed SSH broker.
//! No fake provider, production approval, active profile or OS custody.
use super::*;
use cc_sync_core::mock::MockServer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ACCOUNT_PASSWORD: &str = "owned personal reader account password 42";

fn roots() -> (String, PathBuf, PathBuf) {
    let token = std::env::var("CC_READER_COLD_SSH_TOKEN").expect("owned UUID required");
    assert_eq!(Uuid::parse_str(&token).unwrap().to_string(), token);
    assert!(!Uuid::parse_str(&token).unwrap().is_nil());
    let public = PathBuf::from(std::env::var_os("CC_READER_COLD_SSH_ROOT").unwrap());
    let private = PathBuf::from(std::env::var_os("CC_READER_COLD_SSH_PRIVATE_ROOT").unwrap());
    for p in [&public, &private] {
        let m = std::fs::symlink_metadata(p).unwrap();
        assert!(p.is_absolute() && m.is_dir() && !m.file_type().is_symlink());
    }
    assert_eq!(
        public.file_name().unwrap().to_str().unwrap(),
        format!("owned-{token}")
    );
    assert_eq!(
        private.file_name().unwrap().to_str().unwrap(),
        format!("reader-cold-private-owned{token}")
    );
    assert_eq!(private.parent().unwrap().file_name().unwrap(), "Tasks");
    let a = public.canonicalize().unwrap();
    let b = private.canonicalize().unwrap();
    assert!(!a.starts_with(&b) && !b.starts_with(&a));
    (token, public, private)
}

// Real roundtrip on the SAME ordinary SSH session. Metadata probe only:
// no shell/exec/raw worker, Reader proof, freshness, grant or new session.
async fn metadata_probe(session: &cc_ssh_core::SshSession, user: cc_protocol::UserId) {
    let request_id = Uuid::new_v4().to_string();
    let mut raw = serde_json::to_vec(
        &json!({"format":"cc-native-reader-control/v1","requestId":request_id,
        "peerAudienceId":null,"expectedAgentId":null,"kind":"probe","payload":{}}),
    )
    .unwrap();
    raw.push(b'\n');
    assert!(raw.len() <= 16384);
    tokio::time::timeout(Duration::from_secs(5), async {
        assert!(!session.is_closed());
        let mut channel = session
            .open_subsystem("consolecrypt-agent-reader-v1")
            .await
            .unwrap();
        channel.write_all(&raw).await.unwrap();
        channel.shutdown().await.unwrap();
        let mut reply = Vec::new();
        channel.take(4097).read_to_end(&mut reply).await.unwrap();
        assert!(!reply.is_empty() && reply.len() <= 4096 && reply.last() == Some(&b'\n'));
        assert!(!reply[..reply.len() - 1]
            .iter()
            .any(|c| matches!(*c, b'\r' | b'\n')));
        let value: Value = strict_json(&reply, 4096).unwrap();
        let keys: std::collections::BTreeSet<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from(["format", "requestId", "ok", "metadata"])
        );
        assert_eq!(value["format"], "cc-native-reader-control/v1");
        assert_eq!(value["requestId"], request_id);
        assert_eq!(value["ok"], true);
        assert_eq!(value["metadata"]["controlRole"], "readerOperator");
        let context: AgentReaderContextV1 =
            serde_json::from_value(value["metadata"]["endpoint"]["nodeContext"].clone()).unwrap();
        match context.tenant {
            AgentReaderTenantV1::PersonalAccount { id } => {
                assert_eq!(id.as_uuid().to_string(), user.to_string())
            }
            _ => panic!("owned fixture relabeled Personal Node context"),
        }
        assert!(!session.is_closed());
    })
    .await
    .expect("same SSH fixed-probe roundtrip deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires exact owned PersonalAccount Agent+sshd fixture; missing inputs FAIL"]
async fn actual_personal_reader_get_then_logout_denies_old_read_with_same_ssh_alive() {
    let (token, root, private_root) = roots();
    let profile = tempfile::Builder::new()
        .prefix("native-personal-profile-")
        .tempdir_in(&private_root)
        .unwrap();
    let account = MockServer::start().await;
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_synced_profile(
        "Owned Personal Reader".into(),
        account.url().to_string(),
        "owned-personal-reader@example.org".into(),
        ACCOUNT_PASSWORD.into(),
        crate::AccountMode::Register,
    )
    .await
    .unwrap();
    app.create_vault(PASSWORD.into()).await.unwrap();
    let owner = app.session().await.unwrap();
    let api = owner.api().unwrap();
    let authenticated = api.me().await.unwrap();
    let user = authenticated.user_id;
    assert_ne!(user.to_string(), owner.profile_id.to_string());
    assert_eq!(authenticated.current_device_id, owner.device_id());
    // Test-only public metadata, never an account credential/Node approval.
    // MUST precede client-key marker and Agent Node provisioning.
    publish(
        &root,
        "native-personal-subject.json",
        json!({"format":"cc-reader-cold-personal-subject/v1",
        "token":token,"userId":user,"nativeProfileId":owner.profile_id}),
    );
    let ssh_key =
        generate_key(KeyGenAlgorithm::Ed25519, "owned-personal-reader-only", None).unwrap();
    publish(
        &root,
        "native-client-key.json",
        json!({"format":"cc-reader-cold-ssh-client/v1",
        "token":token,"publicKey":ssh_key.public_openssh}),
    );
    let bootstrap: Bootstrap =
        serde_json::from_value(wait_value(&root, "ssh-bootstrap-ready.json").await).unwrap();
    assert_eq!(bootstrap.format, "cc-reader-cold-ssh-bootstrap/v1");
    assert_eq!(bootstrap.token, token);
    assert!(bootstrap.ssh_port > 0 && bootstrap.host_public_key.starts_with("ssh-ed25519 "));
    assert!(!bootstrap.host_public_key.chars().any(char::is_control));
    let selection =
        prepare_fixture(&app, &bootstrap, ssh_key.private_openssh.expose_secret()).await;
    drop(ssh_key);
    // Quiesce background sync BEFORE Reader baseline; auth remains real.
    app.sync_now().await.unwrap();
    let unlocked = owner.unlocked_opt().await.unwrap();
    unlocked.stop_engine().await;
    assert!(api.me().await.is_ok());
    let version = unlocked.working().version();
    let review = app
        .reader_prepare_issuer_initialization(copy_selection(&selection))
        .unwrap()
        .wait()
        .await
        .unwrap();
    let issuer = app
        .reader_confirm_issuer_initialization_once(review)
        .unwrap()
        .wait()
        .await
        .unwrap();
    let observed = app
        .reader_observe_endpoint_once(copy_selection(&selection))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(
        unlocked.working().version(),
        version,
        "Reader setup self-mutated trust"
    );
    let endpoint = observed.endpoint();
    match &endpoint.node_context.tenant {
        AgentReaderTenantV1::PersonalAccount { id } => {
            assert_eq!(id.as_uuid().to_string(), user.to_string())
        }
        _ => panic!("independently provisioned PersonalAccount Node context required"),
    }
    let pin:AgentReaderIssuerPinV1=serde_json::from_value(json!({"format":"cc-reader-issuer-pin/v1",
        "context":endpoint.node_context,"nativeProfileId":owner.profile_id,"issuerId":issuer.issuer_id,
        "issuerEpoch":1,"issuerKey":issuer.issuer_key,"audienceId":Uuid::new_v4(),
        "expiresAt":chrono::Utc::now().timestamp() as u64+3600,"nodeTrustOriginHash":endpoint.node_trust_origin_hash})).unwrap();
    let head = AgentReaderHeadV1 {
        context: pin.context.clone(),
        issuer_id: pin.issuer_id,
        issuer_epoch: 1,
        authority_revision: 0,
        authority_hash: pin.genesis_hash().unwrap(),
    };
    let target = endpoint.targets.targets[1].clone();
    let binding = ReaderPeerBinding {
        native_profile_id: pin.native_profile_id,
        agent_id: endpoint.agent_id,
        audience_id: pin.audience_id,
        node_trust_origin_hash: endpoint.node_trust_origin_hash,
        issuer_pin_genesis_hash: pin.genesis_hash().unwrap(),
        transport_peer_hash: observed.ssh_key_digest(),
        agent_certificate_hash: endpoint.agent_certificate_hash,
        agent_key_epoch: endpoint.agent_key_epoch,
        target_hashes: [
            endpoint.targets.targets[0].descriptor_hash().unwrap(),
            target.descriptor_hash().unwrap(),
        ],
        ssh_server_key_digest: observed.ssh_key_digest(),
        http_port: endpoint.http_port,
    };
    binding.digest().unwrap();
    publish(
        &root,
        "native-provision.json",
        json!({"format":"cc-reader-cold-ssh-provision/v1",
        "token":token,"pin":pin,"binding":binding}),
    );
    assert_eq!(
        wait_value(&root, "agent-provisioned.json").await,
        json!({
        "format":"cc-reader-cold-ssh-provisioned/v1","token":token})
    );
    let review = app
        .reader_prepare_association(
            copy_selection(&selection),
            ProvisionedAssociationSelection {
                pin,
                target: target.clone(),
                binding,
                installed_head: head,
            },
        )
        .unwrap()
        .wait()
        .await
        .unwrap();
    let reference = app
        .reader_approve_association_once(review)
        .unwrap()
        .wait()
        .await
        .unwrap();
    let read = app
        .reader_prepare_read(copy_selection(&selection), copy_reference(&reference))
        .unwrap()
        .wait()
        .await
        .unwrap();
    let data = app.reader_read_once(read).unwrap().wait().await.unwrap();
    assert_eq!(data.target_hash(), target.descriptor_hash().unwrap());
    assert_eq!(data.private_dataset()["history_kind"], "fixed_os_snapshot");
    assert_eq!(
        data.private_dataset()["points"].as_array().unwrap().len(),
        2
    );
    drop(data);
    assert_eq!(
        unlocked.working().version(),
        version,
        "Reader positive self-mutated trust"
    );
    logout_phase(
        &app, &owner, &unlocked, &selection, &reference, &root, &token, user,
    )
    .await;
    app.shutdown().await.unwrap();
    publish(
        &root,
        "native-finished.json",
        json!({"format":"cc-reader-cold-personal-native-result/v1",
        "token":token,"passed":true,"checks":["actual_personal_native_issuer_signer_fixed_OS200",
        "logout_old_reader_denied_live_agent_finalize","same_unlocked_vault_and_same_existing_SSH_roundtrip"],
        "protectedOrigin":"owned_explicit_SSH_endpoint","osCustody":false,"humanApproval":false,
        "productionAccountApproval":false}),
    );
}

async fn logout_phase(
    app: &AppCore,
    owner: &Arc<crate::session::Session>,
    unlocked: &Arc<crate::session::Unlocked>,
    selection: &NativeReaderSelection,
    reference: &AssociationRef,
    root: &Path,
    token: &str,
    user: cc_protocol::UserId,
) {
    // Ordinary OWN connection BEFORE new Reader baseline. Keep THIS exact Arc;
    // fetching the pool after logout could conceal an automatic reconnect.
    let host = crate::dto::parse_id("owned_personal_ssh", &selection.source.host_id).unwrap();
    let plan = unlocked.ssh.plan(host).await.unwrap();
    assert!(matches!(plan.backend, cc_ssh_core::SshBackend::Native));
    assert!(matches!(
        plan.host_key_policy,
        cc_ssh_core::HostKeyPolicy::Strict
    ));
    assert!(
        plan.route.is_empty()
            && plan.proxy.is_none()
            && plan.forwards.is_empty()
            && !plan.agent_forwarding
    );
    let alive = unlocked.ssh.shared_session_plan(&plan).await.unwrap();
    metadata_probe(&alive, user).await;
    let version = unlocked.working().version();
    let review = app
        .reader_prepare_read(copy_selection(selection), copy_reference(reference))
        .unwrap()
        .wait()
        .await
        .unwrap();
    let original = review.base.as_ref().unwrap().ctx.scope_identity();
    let deadline = review.base.as_ref().unwrap().ctx.deadline();
    assert!(original.is_live());
    publish(
        root,
        "arm-personal-query.json",
        json!({
        "format":"cc-reader-cold-personal-query-arm/v1","token":token}),
    );
    let started = Instant::now();
    let read = app.reader_read_once(review).unwrap();
    let paused = tokio::time::timeout(
        Duration::from_secs(3),
        wait_value(root, "personal-query-paused.json"),
    )
    .await
    .expect("real admitted query did not reach barrier");
    assert_eq!(
        paused
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["format", "token", "stage", "grantExpiresAt"])
    );
    assert_eq!(paused["format"], "cc-reader-cold-personal-query-paused/v1");
    assert_eq!(paused["token"], token);
    assert_eq!(paused["stage"], "history_query_after_reader_check");
    let expires = paused["grantExpiresAt"].as_u64().unwrap();
    assert!(expires <= 253402300799 && (chrono::Utc::now().timestamp() as u64) < expires);
    assert!(
        original.is_live(),
        "old scope must still be live immediately BEFORE logout"
    );
    let logout = tokio::time::timeout(Duration::from_secs(2), app.logout()).await;
    // Release even if bounded logout returns an error/timeout; never await
    // Native read while the Agent query is blocked. No EOF=cancel assumption.
    publish(
        root,
        "release-personal-query.json",
        json!({
        "format":"cc-reader-cold-personal-query-release/v1","token":token}),
    );
    logout
        .expect("logout exceeded causal fixture window")
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5) && Instant::now() < deadline);
    assert!((chrono::Utc::now().timestamp() as u64) < expires);
    assert!(!original.is_live());
    assert!(Arc::ptr_eq(unlocked, &owner.unlocked_opt().await.unwrap()));
    assert!(!owner.api().unwrap().is_authenticated().await.unwrap()); // Observation only.
    let outcome = tokio::time::timeout(Duration::from_secs(2), read.wait())
        .await
        .expect("owned Reader cleanup deadline");
    assert!(
        matches!(outcome, Err(Failure::Cancelled | Failure::OutcomeUnknown)),
        "old Reader must return NoData due auth lifetime, never success/expiry"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "NoData must precede actual5s custody expiry"
    );
    assert!((chrono::Utc::now().timestamp() as u64) < expires && Instant::now() < deadline);
    assert!(!alive.is_closed());
    metadata_probe(&alive, user).await;
    assert_eq!(unlocked.working().version(), version);
    let finished = wait_value(root, "personal-query-finished.json").await;
    assert_eq!(
        finished,
        json!({"format":"cc-reader-cold-personal-query-finished/v1","token":token,
        "readerBegin":"ok","readerCheck":"ok","readerFinalize":"ok","retireCompletion":true}),
        "actual Agent permit must stay live through finalize, not timeout"
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        alive.disconnect().await.unwrap();
        alive.closed().await;
        assert!(alive.is_closed());
    })
    .await
    .expect("owned ordinary SSH closure observation");
}
