//! DRAFT, not compiled/executed while active Native Cargo is frozen.
//! Pure framing/selection tests are not protected-SSH or approval acceptance.
use super::*;

fn plan() -> ConnectionPlan {
    let mut credential = Credential::new("reader-control", CredentialKind::SshPrivateKey);
    credential.secret_id = Some(ObjectId::new());
    ConnectionPlan {
        host_id: ObjectId::new(),
        name: "reader".into(),
        target: cc_ssh_core::Endpoint::new("127.0.0.1", 2222),
        username: "reader-control".into(),
        credential: Some(credential),
        host_key_policy: HostKeyPolicy::Strict,
        route: vec![],
        proxy: None,
        forwards: vec![],
        backend: SshBackend::Native,
        keepalive_secs: None,
        agent_forwarding: false,
        warnings: vec![],
    }
}
#[test]
fn direct_native_strict_explicit_credential_only() {
    let p = plan();
    assert!(validate_plan(&p).is_ok());
    let mut bad = p.clone();
    bad.host_key_policy = HostKeyPolicy::AcceptNew;
    assert!(validate_plan(&bad).is_err());
    let mut bad = p.clone();
    bad.backend = SshBackend::OpenSsh;
    assert!(validate_plan(&bad).is_err());
    let mut bad = p.clone();
    bad.agent_forwarding = true;
    assert!(validate_plan(&bad).is_err());
    let mut bad = p.clone();
    bad.route.push(p.target_hop());
    assert!(validate_plan(&bad).is_err());
    let mut bad = p.clone();
    bad.target.host = "localhost".into();
    assert!(validate_plan(&bad).is_err());
    let mut bad = p.clone();
    bad.credential = None;
    assert!(validate_plan(&bad).is_err());
    for kind in [
        CredentialKind::OsSshAgent,
        CredentialKind::ExternalAgent,
        CredentialKind::Fido2,
    ] {
        let mut bad = p.clone();
        bad.credential.as_mut().unwrap().kind = kind;
        assert!(validate_plan(&bad).is_err());
    }
}

#[test]
fn control_reply_has_exact_id_frame_and_closed_failure() {
    let id = Uuid::new_v4();
    let good=format!("{{\"format\":\"{CONTROL_FORMAT}\",\"requestId\":\"{id}\",\"ok\":true,\"metadata\":{{\"a\":1}}}}\n");
    let value: serde_json::Value = parse_reply(good.as_bytes(), id).unwrap();
    assert_eq!(value["a"], 1);
    let missing_lf = &good.as_bytes()[..good.len() - 1];
    assert!(parse_reply::<serde_json::Value>(missing_lf, id).is_err());
    assert!(parse_reply::<serde_json::Value>(format!("{good}\n").as_bytes(), id).is_err());
    assert!(parse_reply::<serde_json::Value>(good.as_bytes(), Uuid::new_v4()).is_err());
    for tail in [
        "\"ok\":true,\"ok\":false,\"metadata\":{}",
        "\"ok\":true,\"metadata\":{},\"code\":\"Replay\"",
        "\"ok\":false,\"metadata\":{},\"code\":\"Replay\"",
        "\"ok\":false,\"code\":\"CANARY_URL_OR_PRIVATE_ERROR\"",
        "\"ok\":true,\"metadata\":{\"n\":1.5}",
        "\"ok\":true,\"metadata\":{},\"unknown\":1",
        "\"ok\":false,\"metadata\":null,\"code\":\"Replay\"",
        "\"ok\":true,\"metadata\":{},\"code\":null",
        "\"ok\":true,\"metadata\":null",
    ] {
        let frame = format!("{{\"format\":\"{CONTROL_FORMAT}\",\"requestId\":\"{id}\",{tail}}}\n");
        assert!(parse_reply::<serde_json::Value>(frame.as_bytes(), id).is_err());
    }
    let deny=format!("{{\"format\":\"{CONTROL_FORMAT}\",\"requestId\":\"{id}\",\"ok\":false,\"code\":\"Replay\"}}\n");
    assert!(matches!(
        parse_reply::<serde_json::Value>(deny.as_bytes(), id),
        Err(ReaderTransportError::BrokerRejected(
            ReaderBrokerError::Replay
        ))
    ));
}

#[test]
fn control_framing_bound_applies_before_complete_serialization() {
    assert_eq!(encode(&"a", 3).unwrap(), b"\"a\"");
    assert!(encode(&"a", 2).is_err());
    let huge = "x".repeat(CONTROL_REQUEST_BYTES);
    assert!(encode(
        &serde_json::json!({"payload":huge}),
        CONTROL_REQUEST_BYTES - 1
    )
    .is_err());
    let id = Uuid::new_v4();
    let mut reply = vec![b' '; CONTROL_REPLY_BYTES];
    reply.push(b'\n');
    assert!(parse_reply::<serde_json::Value>(&reply, id).is_err());
}

fn binding() -> ReaderPeerBinding {
    ReaderPeerBinding {
        native_profile_id: AgentReaderNativeProfileId::new("77777777-7777-4777-8777-777777777777")
            .unwrap(),
        agent_id: AgentReaderAgentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
        audience_id: AgentReaderAudienceId::new("33333333-3333-4333-8333-333333333333").unwrap(),
        node_trust_origin_hash: AgentReaderHash::from_bytes([1; 32]),
        issuer_pin_genesis_hash: AgentReaderHash::from_bytes([2; 32]),
        transport_peer_hash: AgentReaderHash::from_bytes([3; 32]),
        agent_certificate_hash: AgentReaderHash::from_bytes([4; 32]),
        agent_key_epoch: 1,
        target_hashes: [
            AgentReaderHash::from_bytes([5; 32]),
            AgentReaderHash::from_bytes([6; 32]),
        ],
        ssh_server_key_digest: AgentReaderHash::from_bytes([3; 32]),
        http_port: 8765,
    }
}
#[test]
fn full_peer_digest_changes_for_namespace_target_epoch_endpoint() {
    let v = binding();
    let digest = v.digest().unwrap();
    assert_eq!(digest, v.clone().digest().unwrap());
    let mut cases = Vec::new();
    let mut x = v.clone();
    x.agent_key_epoch = 2;
    cases.push(x);
    let mut x = v.clone();
    x.http_port += 1;
    cases.push(x);
    let mut x = v.clone();
    x.target_hashes.swap(0, 1);
    cases.push(x);
    let mut x = v.clone();
    x.issuer_pin_genesis_hash = AgentReaderHash::from_bytes([7; 32]);
    cases.push(x);
    let mut x = v.clone();
    x.audience_id = AgentReaderAudienceId::new("44444444-4444-4444-8444-444444444444").unwrap();
    cases.push(x);
    for changed in cases {
        assert_ne!(digest, changed.digest().unwrap());
    }
    let mut invalid = v;
    invalid.http_port = 0;
    assert!(invalid.digest().is_err());
}

#[tokio::test]
async fn pinned_store_is_exact_and_immutable_without_network() {
    let key = cc_ssh_core::known_hosts::new_known_host(
        "127.0.0.1",
        2222,
        "ssh-ed25519",
        b"public-test-blob",
        cc_models::known_host::KnownHostSource::Manual,
    );
    let store = PinnedHosts {
        host: "127.0.0.1".into(),
        port: 2222,
        key: key.clone(),
    };
    assert_eq!(
        store.lookup("127.0.0.1", 2222).await.unwrap(),
        vec![key.clone()]
    );
    assert!(store.lookup("localhost", 2222).await.is_err());
    assert!(store.lookup("127.0.0.1", 22).await.is_err());
    assert!(store.add(key.clone()).await.is_err());
    assert!(store.revoke(&key.fingerprint_sha256).await.is_err());
    assert!(store.remove(key.id).await.is_err());
}

#[test]
fn transport_public_diagnostics_do_not_contain_raw_metadata() {
    let b = binding();
    assert_eq!(format!("{b:?}"), "ReaderPeerBinding([redacted])");
    assert!(!format!(
        "{:?}",
        ReaderTransportError::BrokerRejected(ReaderBrokerError::RoleDenied)
    )
    .contains("77777777"));
    assert!(ReadWall::new(0).is_err());
    assert!(ReadWall::new(253402300800).is_err());
}

#[test]
fn bootstrap_probe_metadata_needs_no_reader_root_but_creates_no_verified_peer() {
    let a = "22222222-2222-4222-8222-222222222222";
    let h = "33333333-3333-4333-8333-333333333333";
    let heartbeat = "44444444-4444-4444-8444-444444444444";
    let history = "55555555-5555-4555-8555-555555555555";
    let endpoint = serde_json::json!({
        "controlRole":"readerOperator",
        "endpoint":{
            "nodeContext":{"tenant":{"kind":"localProfile","id":"77777777-7777-4777-8777-777777777777"},"deploymentId":"88888888-8888-4888-8888-888888888888"},
            "nodeTrustOriginHash":"11".repeat(32),"originalOwnerKey":format!("58{}","66".repeat(31)),
            "agentId":a,"agentCertificateHash":"22".repeat(32),"agentKeyEpoch":1,
            "nodeKey":format!("58{}e6","66".repeat(30)),"httpPort":8765,
            "targets":{"format":"cc-reader-target-set/v1","targets":[
                {"format":"cc-reader-target/v1","agentId":a,"configuredHostId":h,"action":"heartbeatRead","resourceId":heartbeat,
                 "dataset":{"kind":"heartbeatV1","schemaVersion":1,"descriptor":{"agent_id":a,"host_id":h,"heartbeat_resource_id":heartbeat,"history_kind":"heartbeat","schema_version":1}},
                 "maxResponseBytes":65536,"maxQueryPoints":0},
                {"format":"cc-reader-target/v1","agentId":a,"configuredHostId":h,"action":"historyRead","resourceId":history,
                 "dataset":{"kind":"localAgentHistoryV1","schemaVersion":1,"descriptor":{"agent_id":a,"host_id":h,"history_resource_id":history,"sample_interval_seconds":15,"max_bytes":1024,"max_points":100,"max_age_seconds":3600,"history_kind":"agent_gauge_history","schema_version":1}},
                 "maxResponseBytes":262144,"maxQueryPoints":1000}
            ]}
        },"provisionedReaderInfo":null
    });
    let p: Probe = serde_json::from_value(endpoint.clone()).unwrap();
    assert!(validate_probe(&p).is_ok());
    assert!(p.provisioned_reader_info.is_none());
    let mut wrong = endpoint.clone();
    wrong["controlRole"] = serde_json::json!("dataReader");
    assert!(validate_probe(&serde_json::from_value(wrong).unwrap()).is_err());
    let mut wrong = endpoint;
    wrong["endpoint"]["targets"]["targets"][1]["maxQueryPoints"] = serde_json::json!(100);
    assert!(validate_probe(&serde_json::from_value(wrong).unwrap()).is_err());
    // The test parses public metadata only. It has no SSH session, explicit
    // review, root or constructor for VerifiedReaderControlPeer.
}

#[test]
fn new_matching_revoked_membership_aba_is_visible_to_whole_attempt_version() {
    use crate::working_set::{Readback, WorkingSet};
    use cc_models::{ObjectPayload, VaultObject};
    use cc_storage_core::LocalState;
    let ws = WorkingSet::new();
    let key = cc_ssh_core::known_hosts::new_known_host(
        "127.0.0.1",
        2222,
        "ssh-ed25519",
        b"public-test-blob",
        cc_models::known_host::KnownHostSource::Manual,
    );
    let put = |key: &KnownHost, revision| {
        Readback::Live(Box::new(cc_sync_core::DecryptedObject {
            object_id: key.id,
            revision,
            local_state: LocalState::LocalOnly,
            conflict_origin: None,
            payload: ObjectPayload::new(VaultObject::KnownHost(key.clone())),
        }))
    };
    ws.apply(key.id, put(&key, 1));
    let baseline = ws.version();
    let before = ws.known_hosts();
    let token = CancellationToken::new();
    assert!(ws.watch_revisions(
        std::collections::BTreeMap::from([(key.id, 1)]),
        token.clone()
    ));
    let mut revoked = key.clone();
    revoked.id = ObjectId::new();
    revoked.revoked = true;
    ws.apply(revoked.id, put(&revoked, 1));
    ws.apply(revoked.id, Readback::Deleted { revision: 2 });
    assert_eq!(ws.known_hosts(), before);
    assert!(
        !token.is_cancelled(),
        "an existing-ID watch alone misses membership ABA"
    );
    assert_ne!(
        ws.version(),
        baseline,
        "the transport's whole-attempt baseline must now deny"
    );
}
