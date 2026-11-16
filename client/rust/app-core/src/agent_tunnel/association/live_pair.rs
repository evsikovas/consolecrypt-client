//! Actual Native -> maintained pinned SSH -> actual fixed Agent enrollment worker.
//! Opt-in coordinator must provision real Node/Actor from PUBLIC bootstrap before
//! opening SSH. No metadata fixture or synthetic JSON success bridge is accepted.
use super::*;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const LEDGER: &str = "cc.native.agent.node-associations.v1";
const PASSWORD: &str = "owned Native enrollment profile password42";
const CASES: [&str; 5] = [
    "wrongHostKey",
    "staleRevision",
    "cancelled",
    "lostStageAck",
    "positive",
];
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseBootstrap {
    #[serde(skip_serializing)]
    case: &'static str,
    agent_id: AgentId,
    #[serde(rename = "genesisOwnerPin")]
    owner_pin: OwnerPin,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ready {
    format: String,
    subsystem: String,
    token: Uuid,
    case: String,
    host: String,
    port: u16,
    user: String,
    host_public_key: String,
}
fn publish(root: &Path, name: &str, value: &Value) {
    let bytes = serde_json::to_vec(value).unwrap();
    assert!(bytes.len() <= 32768);
    let mut file = tempfile::Builder::new()
        .prefix(".native-public-")
        .tempfile_in(root)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.as_file().sync_all().unwrap();
    file.persist_noclobber(root.join(name)).unwrap();
}
async fn ready(root: &Path, name: &str, token: Uuid) -> Ready {
    loop {
        let path = root.join(name);
        if let Ok(m) = std::fs::symlink_metadata(&path) {
            assert!(m.is_file() && !m.file_type().is_symlink() && m.len() <= 16384);
            let mut file = File::open(path).unwrap();
            let mut raw = Vec::new();
            Read::by_ref(&mut file)
                .take(16385)
                .read_to_end(&mut raw)
                .unwrap();
            assert!(raw.len() <= 16384);
            let r: Ready = serde_json::from_slice(&raw).unwrap();
            assert!(
                r.format == "cc-owned-enrollment-ssh-endpoint/v1"
                    && r.subsystem == "consolecrypt-agent-enroll-v1"
                    && r.token == token
                    && r.host == "127.0.0.1"
                    && r.port > 0
                    && r.user == "ccenroll"
            );
            assert!(
                r.host_public_key.starts_with("ssh-ed25519 ")
                    && r.host_public_key.len() < 8192
                    && !r.host_public_key.chars().any(char::is_control)
            );
            return r;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
async fn row(app: &AppCore, agent: AgentId) -> Option<Value> {
    let owner = app.session().await.unwrap();
    let storage = owner.storage().unwrap();
    drop(owner);
    let value = storage.setting_get::<Value>(LEDGER).await.unwrap();
    let agent = agent_text(agent).unwrap();
    value.and_then(|v| {
        v["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["public"]["agent_id"] == agent)
            .cloned()
    })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires exact owned actual Agent enrollment worker over fixed original SSH subsystem"]
async fn actual_native_owner_association_four_frames_and_ambiguous_commit_custody() {
    tokio::time::timeout(Duration::from_secs(300), run_owned())
        .await
        .expect("owned enrollment total deadline");
}
async fn run_owned() {
    let raw_token = std::env::var("CC_NODE_ENROLL_PROOF_TOKEN").unwrap();
    let token = Uuid::parse_str(&raw_token).unwrap();
    assert!(!token.is_nil() && token.to_string() == raw_token);
    let root = PathBuf::from(std::env::var_os("CC_NODE_ENROLL_PROOF_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let allowed = workspace
        .join("Tasks/node-enroll/control")
        .canonicalize()
        .unwrap();
    assert_eq!(
        root.canonicalize().unwrap().parent(),
        Some(allowed.as_path())
    );
    assert_eq!(
        root.file_name().unwrap().to_str().unwrap(),
        format!("owned-{token}")
    );
    assert!(
        !std::fs::symlink_metadata(&root)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let profile = tempfile::Builder::new()
        .prefix("native-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned actual Node enrollment".into(), PASSWORD.into())
        .await
        .unwrap();
    let ssh = cc_ssh_core::keys::generate_key(
        cc_ssh_core::keys::KeyGenAlgorithm::Ed25519,
        "owned enrollment Native",
        None,
    )
    .unwrap();
    let public_ssh = ssh
        .public_openssh
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let credential = app
        .import_ssh_key(
            "Owned enrollment Native key".into(),
            Some("ccenroll".into()),
            ssh.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    drop(ssh);
    let scope = Arc::new(
        OriginalScope::capture(&app, &CancellationToken::new(), Duration::from_secs(300))
            .await
            .unwrap(),
    );
    let (owner, u) = app.unlocked().await.unwrap();
    let identity_frame = json!({"version":1,"profileId":owner.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap()});
    drop(u);
    drop(owner);
    let identity_review = app
        .agent_node_client_identity_prepare_json(&identity_frame.to_string())
        .await
        .unwrap();
    let identity_stored = identity_review.confirm_creation().await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&identity_stored).unwrap()["phase"],
        "stored"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&identity_review.joined_json().await.unwrap()).unwrap()["cleanupState"],
        "closedObserved"
    );
    let identity = ManagedTlsCustody::capture(&app, scope.clone())
        .await
        .unwrap()
        .load(&app)
        .await
        .unwrap();
    let tls = identity.public_identity().unwrap().clone();
    drop(identity);
    println!("CC_ASSOCIATION_PAIRED_TLS_STORED={identity_stored}");
    let key = scope
        .with_loaded(&app, |id| {
            Ok(PublicKey::from_bytes(id.public_keys().signing))
        })
        .await
        .unwrap();
    let now = scope.now().unwrap();
    let native_profile = app.session().await.unwrap().profile_id;
    let mut cases = Vec::new();
    for name in CASES {
        let local_id = Uuid::new_v4();
        assert_ne!(local_id.to_string(), native_profile.to_string());
        cases.push(CaseBootstrap {
            case: name,
            agent_id: AgentId::new(&Uuid::new_v4().to_string()).unwrap(),
            owner_pin: OwnerPin {
                context: cc_agent_node_wire::Context {
                    tenant: Tenant::LocalProfile {
                        id: LocalProfileId::new(&local_id.to_string()).unwrap(),
                    },
                    deployment_id: DeploymentId::new(&Uuid::new_v4().to_string()).unwrap(),
                },
                key,
                epoch: 1,
                checkpoint: 0,
                expires_at: now.0.checked_add(600).unwrap(),
            },
        });
    }
    let public_cases: serde_json::Map<String, Value> = cases
        .iter()
        .map(|case| (case.case.to_owned(), serde_json::to_value(case).unwrap()))
        .collect();
    // PUBLIC ONLY. Coordinator must not start SSH or synthesize replies before
    // real provisioning of these exact original pins/Agent IDs has completed.
    publish(
        &root,
        "bootstrap.json",
        &json!({"version":1,"token":token,"sshClientPublicKey":public_ssh,"clientTlsPublic":tls,"cases":public_cases}),
    );
    drop(scope);
    let mut ports = BTreeSet::new();
    let mut outcomes = Vec::new();
    for case in &cases {
        let r = ready(&root, &format!("ssh-ready-{}.json", case.case), token).await;
        assert_eq!(r.case, case.case);
        // Distinct actual listeners make wrong-host-key negative independent
        // of a previously accepted matching known_hosts record.
        assert!(ports.insert(r.port));
        let host_key = if case.case == "wrongHostKey" {
            cc_ssh_core::keys::generate_key(
                cc_ssh_core::keys::KeyGenAlgorithm::Ed25519,
                "owned wrong SSH key",
                None,
            )
            .unwrap()
            .public_openssh
        } else {
            r.host_public_key
        };
        app.import_known_hosts(format!("[127.0.0.1]:{} {host_key}\n", r.port))
            .await
            .unwrap();
        let mut h = crate::HostDto::new("Owned actual enrollment role", "127.0.0.1");
        h.port = Some(r.port);
        h.username = Some(r.user);
        h.credential_id = Some(credential.id.clone());
        h.host_key_policy = cc_ssh_core::HostKeyPolicy::Strict;
        let h = app.save_host(h).await.unwrap();
        let host_id = crate::dto::parse_id("host", &h.id).unwrap();
        let revision = app
            .unlocked()
            .await
            .unwrap()
            .1
            .working()
            .revision_of(host_id)
            .unwrap();
        let cancel = CancellationToken::new();
        if case.case == "cancelled" {
            cancel.cancel();
        }
        let expected_revision = if case.case == "staleRevision" {
            revision + 1
        } else {
            revision
        };
        if case.case == "positive" {
            let (owner, u) = app.unlocked().await.unwrap();
            let request = json!({"version":1,"profileId":owner.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap(),"hostId":host_id,"expectedHostRevision":revision.to_string()});
            drop(u);
            drop(owner);
            let frame = json!({"version":1,"profileId":request["profileId"],"notificationScopeId":request["notificationScopeId"]});
            let status_raw = app
                .agent_node_association_status_json(&frame.to_string())
                .await
                .unwrap();
            let status: Value = serde_json::from_str(&status_raw).unwrap();
            assert_eq!(status["clientIdentity"]["state"], "storedMetadata");
            assert!(
                status["hosts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|h| h["hostId"] == json!(host_id)
                        && h["hostRevision"] == revision.to_string())
            );
            println!("CC_ASSOCIATION_PAIRED_STATUS={status_raw}");
            let operation = app
                .agent_node_association_register_json(&request.to_string())
                .await
                .unwrap();
            let offer_raw = operation.probe().await.unwrap();
            let offer: Value = serde_json::from_str(&offer_raw).unwrap();
            assert_eq!(offer["phase"], "offerReview");
            assert_eq!(offer["node"]["agentId"], json!(case.agent_id));
            assert_eq!(offer["actorRevision"], "0");
            assert_eq!(offer["checkpoint"], "0");
            let claim_raw = operation.approve_offer().await.unwrap();
            let claim: Value = serde_json::from_str(&claim_raw).unwrap();
            assert_eq!(claim["phase"], "claimReview");
            assert_eq!(claim["actorRevision"], "2");
            assert_eq!(claim["checkpoint"], "1");
            let lifetime = 120u32.min(
                claim["maxCertificateLifetimeSeconds"]
                    .as_u64()
                    .unwrap()
                    .try_into()
                    .unwrap(),
            );
            assert!(lifetime > 0);
            let associated_raw = operation.approve_certificate(lifetime).await.unwrap();
            let details: Value = serde_json::from_str(&associated_raw).unwrap();
            assert_eq!(details["phase"], "associated");
            assert_eq!(details["checkpoint"], "2");
            assert_eq!(details["actorRevision"], "3");
            assert_eq!(details["actorAdmission"], false);
            assert!(operation.approve_offer().await.is_err());
            assert!(operation.approve_certificate(lifetime).await.is_err());
            let joined: Value =
                serde_json::from_str(&operation.joined_json().await.unwrap()).unwrap();
            assert_eq!(joined["cleanupState"], "closedObserved");
            let stored = row(&app, case.agent_id).await.unwrap();
            assert_eq!(stored["phase"], "associated");
            assert_eq!(
                stored["public"]["client_tls_identity_id"],
                json!(tls.identity_id)
            );
            assert_eq!(
                stored["public"]["certificate"]["expected_actor_revision"],
                3
            );
            println!("CC_ASSOCIATION_PAIRED_OFFER={offer_raw}");
            println!("CC_ASSOCIATION_PAIRED_CLAIM={claim_raw}");
            println!("CC_ASSOCIATION_PAIRED_ASSOCIATED={associated_raw}");
            outcomes.push(json!({"case":case.case,"associated":true,"details":details,"offerReview":offer,"claimReview":claim,"cleanup":joined,"publicApi":true}));
        } else {
            let captured =
                NodeAssociationReview::capture(&app, host_id, expected_revision, &cancel).await;
            if matches!(case.case, "wrongHostKey" | "staleRevision" | "cancelled") {
                assert!(captured.is_err());
                assert!(row(&app, case.agent_id).await.is_none());
                outcomes.push(json!({"case":case.case,"denied":true,"localRecord":false}));
            } else {
                let review = captured.unwrap();
                assert_eq!(review.probe.node.agent_id, case.agent_id);
                assert_eq!(review.probe.owner_pin, case.owner_pin);
                assert_eq!(review.probe.actor_revision, 0);
                assert_eq!(review.details().unwrap()["scope"], "tunnel.open");
                let claimed = review.approve_offer_once(&app).await;
                if case.case == "lostStageAck" {
                    // Fixture may only DROP actual worker bytes after its genuine
                    // second commit/readback, never replace any success payload.
                    assert!(claimed.is_err());
                    let before = row(&app, case.agent_id).await.unwrap();
                    assert_eq!(before["phase"], "unknown");
                    assert!(before["public"]["certificate"].is_null());
                    app.lock().await.unwrap();
                    app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
                    let after = row(&app, case.agent_id).await.unwrap();
                    assert_eq!(before, after);
                    // Explicit new attempt cannot replace the existing Unknown;
                    // this is not automatic reconnect/recovery of the old operation.
                    assert!(
                        NodeAssociationReview::capture(
                            &app,
                            host_id,
                            revision,
                            &CancellationToken::new()
                        )
                        .await
                        .is_err()
                    );
                    assert_eq!(row(&app, case.agent_id).await.unwrap(), after);
                    outcomes.push(
                        json!({"case":case.case,"unknownSurvivedReopen":true,"retryDenied":true}),
                    );
                } else {
                    panic!("unexpected owned fixture case");
                }
            }
        }

        publish(
            &root,
            &format!("native-case-{}.json", case.case),
            &json!({"version":1,"token":token,"case":case.case,"outcome":outcomes.last().unwrap()}),
        );
    }
    app.shutdown().await.unwrap();
    publish(
        &root,
        "native-complete.json",
        &json!({"version":1,"token":token,"cases":outcomes,"shutdownObserved":true}),
    );
}

// Separate opt-in PUBLIC revocation fixture. The original five-case entry above
// and its output/ABI remain unchanged. Root must provide a newly reviewed
// coordinator/real worker, including test-only armable opaque ACK-byte loss.
const REVOKE_CASES: [&str; 2] = ["revokePositive", "revokeLostAck"];
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires real original public association+revocation over fixed SSH and owned reply-loss coordinator"]
async fn actual_public_node_revocation_and_lost_ack_custody() {
    assert_eq!(std::env::var("CC_NODE_REVOKE_PROOF").unwrap(), "1");
    tokio::time::timeout(Duration::from_secs(300), run_owned_revocation())
        .await.expect("owned public revocation total deadline");
}
async fn run_owned_revocation() {
    let raw_token = std::env::var("CC_NODE_ENROLL_PROOF_TOKEN").unwrap();
    let token = Uuid::parse_str(&raw_token).unwrap();
    assert!(!token.is_nil() && token.to_string() == raw_token);
    let root = PathBuf::from(std::env::var_os("CC_NODE_ENROLL_PROOF_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let allowed = workspace
        .join("Tasks/node-enroll/control")
        .canonicalize()
        .unwrap();
    assert_eq!(
        root.canonicalize().unwrap().parent(),
        Some(allowed.as_path())
    );
    assert_eq!(
        root.file_name().unwrap().to_str().unwrap(),
        format!("owned-{token}")
    );
    assert!(
        !std::fs::symlink_metadata(&root)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let profile = tempfile::Builder::new()
        .prefix("native-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned actual Node enrollment".into(), PASSWORD.into())
        .await
        .unwrap();
    let ssh = cc_ssh_core::keys::generate_key(
        cc_ssh_core::keys::KeyGenAlgorithm::Ed25519,
        "owned enrollment Native",
        None,
    )
    .unwrap();
    let public_ssh = ssh
        .public_openssh
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let credential = app
        .import_ssh_key(
            "Owned enrollment Native key".into(),
            Some("ccenroll".into()),
            ssh.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    drop(ssh);
    let scope = Arc::new(
        OriginalScope::capture(&app, &CancellationToken::new(), Duration::from_secs(300))
            .await
            .unwrap(),
    );
    let (owner, u) = app.unlocked().await.unwrap();
    let identity_frame = json!({"version":1,"profileId":owner.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap()});
    drop(u);
    drop(owner);
    let identity_review = app
        .agent_node_client_identity_prepare_json(&identity_frame.to_string())
        .await
        .unwrap();
    let identity_stored = identity_review.confirm_creation().await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&identity_stored).unwrap()["phase"],
        "stored"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&identity_review.joined_json().await.unwrap()).unwrap()["cleanupState"],
        "closedObserved"
    );
    let identity = ManagedTlsCustody::capture(&app, scope.clone())
        .await
        .unwrap()
        .load(&app)
        .await
        .unwrap();
    let tls = identity.public_identity().unwrap().clone();
    drop(identity);
    println!("CC_REVOCATION_PAIRED_TLS_STORED={identity_stored}");
    let key = scope
        .with_loaded(&app, |id| {
            Ok(PublicKey::from_bytes(id.public_keys().signing))
        })
        .await
        .unwrap();
    let now = scope.now().unwrap();
    let native_profile = app.session().await.unwrap().profile_id;
    let mut cases = Vec::new();
    for name in REVOKE_CASES {
        let local_id = Uuid::new_v4();
        assert_ne!(local_id.to_string(), native_profile.to_string());
        cases.push(CaseBootstrap {
            case: name,
            agent_id: AgentId::new(&Uuid::new_v4().to_string()).unwrap(),
            owner_pin: OwnerPin {
                context: cc_agent_node_wire::Context {
                    tenant: Tenant::LocalProfile {
                        id: LocalProfileId::new(&local_id.to_string()).unwrap(),
                    },
                    deployment_id: DeploymentId::new(&Uuid::new_v4().to_string()).unwrap(),
                },
                key,
                epoch: 1,
                checkpoint: 0,
                expires_at: now.0.checked_add(600).unwrap(),
            },
        });
    }
    let public_cases: serde_json::Map<String, Value> = cases
        .iter()
        .map(|case| (case.case.to_owned(), serde_json::to_value(case).unwrap()))
        .collect();
    // PUBLIC ONLY. Coordinator must not start SSH or synthesize replies before
    // real provisioning of these exact original pins/Agent IDs has completed.
    publish(
        &root,
        "revoke-bootstrap.json",
        &json!({"version":1,"token":token,"sshClientPublicKey":public_ssh,"clientTlsPublic":tls,"cases":public_cases}),
    );
    drop(scope);

    let mut ports = BTreeSet::new();
    let mut outcomes = Vec::new();
    for case in &cases {
        let r = ready(&root, &format!("ssh-ready-{}.json", case.case), token).await;
        assert_eq!(r.case, case.case); assert!(ports.insert(r.port));
        app.import_known_hosts(format!("[127.0.0.1]:{} {}\n", r.port, r.host_public_key)).await.unwrap();
        let mut host = crate::HostDto::new("Owned actual public revoke role", "127.0.0.1");
        host.port = Some(r.port); host.username = Some(r.user.clone());
        host.credential_id = Some(credential.id.clone()); host.host_key_policy = cc_ssh_core::HostKeyPolicy::Strict;
        let host = app.save_host(host).await.unwrap();
        let host_id = crate::dto::parse_id("host", &host.id).unwrap();
        let host_revision = app.unlocked().await.unwrap().1.working().revision_of(host_id).unwrap();
        let (owner, unlocked) = app.unlocked().await.unwrap();
        let frame = json!({"version":1,"profileId":owner.profile_id.to_string(),
            "notificationScopeId":unlocked.notices.scope_id().unwrap()});
        drop(unlocked); drop(owner);
        let mut request = frame.clone(); request["hostId"] = json!(host_id);
        request["expectedHostRevision"] = json!(host_revision.to_string());
        let association = app.agent_node_association_register_json(&request.to_string()).await.unwrap();
        let offer: Value = serde_json::from_str(&association.probe().await.unwrap()).unwrap();
        assert_eq!(offer["phase"], "offerReview"); assert_eq!(offer["node"]["agentId"],json!(case.agent_id));
        let claim: Value = serde_json::from_str(&association.approve_offer().await.unwrap()).unwrap();
        assert_eq!(claim["phase"],"claimReview"); assert_eq!(claim["actorRevision"],"2");
        let lifetime = 120u32.min(claim["maxCertificateLifetimeSeconds"].as_u64().unwrap().try_into().unwrap());
        assert!(lifetime > 0);
        let associated: Value = serde_json::from_str(&association.approve_certificate(lifetime).await.unwrap()).unwrap();
        assert_eq!(associated["phase"],"associated"); assert_eq!(associated["actorRevision"],"3");
        assert_eq!(associated["checkpoint"],"2"); assert_eq!(associated["actorAdmission"],false);
        let closed: Value = serde_json::from_str(&association.joined_json().await.unwrap()).unwrap();
        assert_eq!(closed["cleanupState"],"closedObserved");
        let before = row(&app,case.agent_id).await.unwrap(); assert_eq!(before["phase"],"associated");
        let certificate_before = before["public"]["certificate"].clone();
        publish(&root,&format!("native-revoke-associated-{}.json",case.case),&json!({
            "version":1,"token":token,"case":case.case,"associationId":associated["associationId"],
            "node":offer["node"],"certificateDigest":associated["certificateDigest"],
            "actorRevision":"3","checkpoint":"2","publicApi":true}));
        // Scheduling only: Root arms opaque reply2 loss for the NEXT connection
        // in revokeLostAck. This marker is not an ACK, approval or trust input.
        let scheduled = ready(&root,&format!("revoke-ready-{}.json",case.case),token).await;
        assert_eq!(scheduled.case,case.case); assert_eq!(scheduled.host,r.host);
        assert_eq!(scheduled.port,r.port); assert_eq!(scheduled.user,r.user);
        assert_eq!(scheduled.host_public_key,r.host_public_key);
        assert_eq!(app.unlocked().await.unwrap().1.working().revision_of(host_id),Some(host_revision));
        let mut revoke_request = frame.clone(); revoke_request["associationId"] = associated["associationId"].clone();
        let review = app.agent_node_association_revoke_prepare_json(&revoke_request.to_string()).await.unwrap();
        let details: Value = serde_json::from_str(&review.details_json().await.unwrap()).unwrap();
        assert_eq!(details["phase"],"revocationReview"); assert_eq!(details["node"],offer["node"]);
        assert_eq!(details["certificateDigest"],associated["certificateDigest"]);
        assert_eq!(details["hostId"],json!(host_id)); assert_eq!(details["hostRevision"],host_revision.to_string());
        assert_eq!(details["actorRevision"],"3"); assert_eq!(details["checkpoint"],"2");
        assert_eq!(details["remoteRevocationObserved"],false);
        let result = review.confirm_once().await;
        let joined: Value = serde_json::from_str(&review.joined_json().await.unwrap()).unwrap();
        assert_eq!(joined["cleanupState"],"closedObserved");
        let stored = row(&app,case.agent_id).await.unwrap();
        assert_eq!(stored["public"]["certificate"],certificate_before);
        assert_eq!(stored["public"]["node_key_fingerprint"],offer["node"]["keyFingerprint"]);
        assert_eq!(stored["public"]["key_epoch"],1);
        assert_eq!(stored["revocation"]["expected_actor_revision"],4);
        let raw: Vec<u8> = serde_json::from_value(stored["revocation"]["signed_revocation_json"].clone()).unwrap();
        let revocation: Signed<Revocation> = serde_json::from_slice(&raw).unwrap();
        verify_statement(&revocation,case.owner_pin.key).unwrap();
        assert_eq!(revocation.body.agent_id,case.agent_id); assert_eq!(revocation.body.key_epoch,1);
        assert_eq!(revocation.body.header.context,case.owner_pin.context);
        assert_eq!(revocation.body.header.checkpoint,3);
        assert_eq!(json!(revocation.body.certificate_hash),associated["certificateDigest"]);
        let outcome = if case.case == "revokePositive" {
            let confirmed: Value = serde_json::from_str(&result.unwrap()).unwrap();
            assert_eq!(confirmed["phase"],"revoked"); assert_eq!(confirmed["remoteRevocationObserved"],true);
            assert_eq!(confirmed["actorRevision"],"4"); assert_eq!(confirmed["checkpoint"],"3");
            assert_eq!(confirmed["localBlocked"],true); assert_eq!(confirmed["node"],offer["node"]);
            assert_eq!(confirmed["certificateDigest"],associated["certificateDigest"]);
            assert_eq!(stored["phase"],"revoked"); assert_eq!(joined["remoteRevocationObserved"],true);
            assert!(review.confirm_once().await.is_err());
            let status: Value = serde_json::from_str(&app.agent_node_association_status_json(&frame.to_string()).await.unwrap()).unwrap();
            let choice = status["associations"].as_array().unwrap().iter()
                .find(|r|r["associationId"]==associated["associationId"]).unwrap();
            assert_eq!(choice["state"],"revoked"); assert_eq!(choice["remoteRevocationObserved"],true);
            assert_eq!(choice["revocationState"],"observedRevoked");
            assert!(app.agent_node_association_revoke_prepare_json(&revoke_request.to_string()).await.is_err());
            json!({"case":case.case,"publicApi":true,"associatedBeforeRevoke":true,
                "remoteRevocationObserved":true,"nativeState":"revoked","sameNodeKey":true,
                "retryDenied":true,"details":confirmed,"cleanup":joined})
        } else {
            assert_eq!(case.case,"revokeLostAck"); assert!(result.is_err());
            let unknown: Value = serde_json::from_str(&review.details_json().await.unwrap()).unwrap();
            assert_eq!(unknown["phase"],"unknown"); assert_eq!(unknown["remoteRevocationObserved"],false);
            assert_eq!(unknown["localBlocked"],true); assert_eq!(stored["phase"],"blocked");
            assert_eq!(joined["remoteRevocationObserved"],false); assert!(review.confirm_once().await.is_err());
            app.lock().await.unwrap(); app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
            assert_eq!(row(&app,case.agent_id).await.unwrap(),stored);
            let (owner,unlocked) = app.unlocked().await.unwrap();
            let fresh_frame = json!({"version":1,"profileId":owner.profile_id.to_string(),
                "notificationScopeId":unlocked.notices.scope_id().unwrap()});
            drop(unlocked);drop(owner);
            // Fresh nonce/owner admission succeeds. Retry denial is the durable
            // blocked row, not the stale pre-lock notification nonce.
            let status: Value = serde_json::from_str(&app.agent_node_association_status_json(&fresh_frame.to_string()).await.unwrap()).unwrap();
            let choice = status["associations"].as_array().unwrap().iter()
                .find(|r|r["associationId"]==associated["associationId"]).unwrap();
            assert_eq!(choice["state"],"blocked"); assert_eq!(choice["revocationState"],"unknown");
            assert_eq!(choice["remoteRevocationObserved"],false);
            let mut fresh_request=fresh_frame;fresh_request["associationId"]=associated["associationId"].clone();
            assert!(app.agent_node_association_revoke_prepare_json(&fresh_request.to_string()).await.is_err());
            assert_eq!(row(&app,case.agent_id).await.unwrap(),stored);
            json!({"case":case.case,"publicApi":true,"associatedBeforeRevoke":true,
                "remoteRevocationObserved":false,"nativeState":"blocked","sameNodeKey":true,
                "unknownSurvivedReopen":true,"retryDenied":true,"expectedActorRevision":"4",
                "certificateDigest":associated["certificateDigest"],"node":offer["node"],"cleanup":joined})
        };
        println!("CC_REVOCATION_PAIRED_CASE {}",serde_json::to_string(&outcome).unwrap());
        publish(&root,&format!("native-revoke-case-{}.json",case.case),&json!({
            "version":1,"token":token,"case":case.case,"outcome":outcome}));
        outcomes.push(outcome);
    }
    app.shutdown().await.unwrap();
    publish(&root,"native-revoke-complete.json",&json!({"version":1,"token":token,
        "cases":outcomes,"shutdownObserved":true}));
}

// Standalone future public rotation fixture; both accepted earlier entry bodies
// remain exact prefixes. No caller metadata is accepted as Actor/selector ACK.
const ROTATION_CASES:[&str;2]=["rotationPositive","rotationLostAck"];
#[tokio::test(flavor="multi_thread",worker_threads=4)]
#[ignore="requires new reviewed owned rotation worker/Native ELF and opaque reply4-loss coordinator"]
async fn actual_public_tunnel_rotation_exact_reconcile_and_epoch2_revoke() {
    assert_eq!(std::env::var("CC_NODE_ROTATION_PROOF").unwrap(),"1");
    tokio::time::timeout(Duration::from_secs(300),Box::pin(run_owned_rotation())).await.expect("owned rotation total deadline");
}
async fn run_owned_rotation() {
    let raw_token = std::env::var("CC_NODE_ENROLL_PROOF_TOKEN").unwrap();
    let token = Uuid::parse_str(&raw_token).unwrap();
    assert!(!token.is_nil() && token.to_string() == raw_token);
    let root = PathBuf::from(std::env::var_os("CC_NODE_ENROLL_PROOF_ROOT").unwrap());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let allowed = workspace
        .join("Tasks/node-enroll/control")
        .canonicalize()
        .unwrap();
    assert_eq!(
        root.canonicalize().unwrap().parent(),
        Some(allowed.as_path())
    );
    assert_eq!(
        root.file_name().unwrap().to_str().unwrap(),
        format!("owned-{token}")
    );
    assert!(
        !std::fs::symlink_metadata(&root)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let profile = tempfile::Builder::new()
        .prefix("native-")
        .tempdir_in(&root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned actual Node enrollment".into(), PASSWORD.into())
        .await
        .unwrap();
    let ssh = cc_ssh_core::keys::generate_key(
        cc_ssh_core::keys::KeyGenAlgorithm::Ed25519,
        "owned enrollment Native",
        None,
    )
    .unwrap();
    let public_ssh = ssh
        .public_openssh
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let credential = app
        .import_ssh_key(
            "Owned enrollment Native key".into(),
            Some("ccenroll".into()),
            ssh.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    drop(ssh);
    let scope = Arc::new(
        OriginalScope::capture(&app, &CancellationToken::new(), Duration::from_secs(300))
            .await
            .unwrap(),
    );
    let (owner, u) = app.unlocked().await.unwrap();
    let identity_frame = json!({"version":1,"profileId":owner.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap()});
    drop(u);
    drop(owner);
    let identity_review = app
        .agent_node_client_identity_prepare_json(&identity_frame.to_string())
        .await
        .unwrap();
    let identity_stored = identity_review.confirm_creation().await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&identity_stored).unwrap()["phase"],
        "stored"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&identity_review.joined_json().await.unwrap()).unwrap()["cleanupState"],
        "closedObserved"
    );
    let identity = ManagedTlsCustody::capture(&app, scope.clone())
        .await
        .unwrap()
        .load(&app)
        .await
        .unwrap();
    let tls = identity.public_identity().unwrap().clone();
    drop(identity);
    println!("CC_ROTATION_PAIRED_TLS_STORED={identity_stored}");
    let key = scope
        .with_loaded(&app, |id| {
            Ok(PublicKey::from_bytes(id.public_keys().signing))
        })
        .await
        .unwrap();
    let now = scope.now().unwrap();
    let native_profile = app.session().await.unwrap().profile_id;
    let mut cases = Vec::new();
    for name in ROTATION_CASES {
        let local_id = Uuid::new_v4();
        assert_ne!(local_id.to_string(), native_profile.to_string());
        cases.push(CaseBootstrap {
            case: name,
            agent_id: AgentId::new(&Uuid::new_v4().to_string()).unwrap(),
            owner_pin: OwnerPin {
                context: cc_agent_node_wire::Context {
                    tenant: Tenant::LocalProfile {
                        id: LocalProfileId::new(&local_id.to_string()).unwrap(),
                    },
                    deployment_id: DeploymentId::new(&Uuid::new_v4().to_string()).unwrap(),
                },
                key,
                epoch: 1,
                checkpoint: 0,
                expires_at: now.0.checked_add(600).unwrap(),
            },
        });
    }
    let public_cases: serde_json::Map<String, Value> = cases
        .iter()
        .map(|case| (case.case.to_owned(), serde_json::to_value(case).unwrap()))
        .collect();
    // PUBLIC ONLY. Coordinator must not start SSH or synthesize replies before
    // real provisioning of these exact original pins/Agent IDs has completed.
    publish(
        &root,
        "rotation-bootstrap.json",
        &json!({"version":1,"token":token,"sshClientPublicKey":public_ssh,"clientTlsPublic":tls,"cases":public_cases}),
    );
    drop(scope);

    let mut ports=BTreeSet::new();let mut outcomes=Vec::new();
    for case in &cases {
        let endpoint=ready(&root,&format!("ssh-ready-{}.json",case.case),token).await;
        assert_eq!(endpoint.case,case.case);assert!(ports.insert(endpoint.port));
        app.import_known_hosts(format!("[127.0.0.1]:{} {}\n",endpoint.port,endpoint.host_public_key)).await.unwrap();
        let mut host=crate::HostDto::new("Owned actual rotation role","127.0.0.1");host.port=Some(endpoint.port);
        host.username=Some(endpoint.user.clone());host.credential_id=Some(credential.id.clone());host.host_key_policy=cc_ssh_core::HostKeyPolicy::Strict;
        let host=app.save_host(host).await.unwrap();let host_id=crate::dto::parse_id("host",&host.id).unwrap();
        let host_revision=app.unlocked().await.unwrap().1.working().revision_of(host_id).unwrap();
        let(owner,u)=app.unlocked().await.unwrap();let mut frame=json!({"version":1,"profileId":owner.profile_id.to_string(),
            "notificationScopeId":u.notices.scope_id().unwrap()});drop(u);drop(owner);
        let mut request=frame.clone();request["hostId"]=json!(host_id);request["expectedHostRevision"]=json!(host_revision.to_string());
        let association=app.agent_node_association_register_json(&request.to_string()).await.unwrap();
        let old_node:Value=serde_json::from_str(&association.probe().await.unwrap()).unwrap();
        let claim:Value=serde_json::from_str(&association.approve_offer().await.unwrap()).unwrap();
        let lifetime=120u32.min(claim["maxCertificateLifetimeSeconds"].as_u64().unwrap().try_into().unwrap());assert!(lifetime>0);
        let associated:Value=serde_json::from_str(&association.approve_certificate(lifetime).await.unwrap()).unwrap();
        assert_eq!(associated["actorRevision"],"3");assert_eq!(associated["checkpoint"],"2");
        let joined:Value=serde_json::from_str(&association.joined_json().await.unwrap()).unwrap();assert_eq!(joined["cleanupState"],"closedObserved");
        let initial=row(&app,case.agent_id).await.unwrap();let initial_public=initial["public"].clone();
        let old_raw:Vec<u8>=serde_json::from_value(initial_public["certificate"]["signed_certificate_json"].clone()).unwrap();
        let old_cert:Signed<Certificate>=serde_json::from_slice(&old_raw).unwrap();verify_statement(&old_cert,case.owner_pin.key).unwrap();
        publish(&root,&format!("native-rotation-associated-{}.json",case.case),&json!({"version":1,"token":token,"case":case.case,
            "associationId":associated["associationId"],"node":old_node["node"],"actorRevision":"3","checkpoint":"2"}));
        // Root arms reply4 loss ONLY for the next rotation SSH connection. No
        // Host/pin/SSHD/worker reconfiguration and no frame parsing/synthesis.
        let scheduled=ready(&root,&format!("rotation-ready-{}.json",case.case),token).await;
        assert_eq!(scheduled.case,case.case);assert_eq!(scheduled.host,endpoint.host);assert_eq!(scheduled.port,endpoint.port);
        assert_eq!(scheduled.user,endpoint.user);assert_eq!(scheduled.host_public_key,endpoint.host_public_key);
        assert_eq!(app.unlocked().await.unwrap().1.working().revision_of(host_id),Some(host_revision));
        let mut action=frame.clone();action["associationId"]=associated["associationId"].clone();
        let review=app.agent_node_association_rotation_prepare_json(&action.to_string()).await.unwrap();
        let before:Value=serde_json::from_str(&review.details_json().await.unwrap()).unwrap();assert_eq!(before["phase"],"rotationPreparationReview");
        assert_eq!(before["node"],old_node["node"]);assert_eq!(before["selectionObserved"],false);
        let candidate:Value=serde_json::from_str(&review.prepare_candidate_once().await.unwrap()).unwrap();
        assert_eq!(candidate["phase"],"rotationCertificateReview");assert_eq!(candidate["proposedNode"]["keyEpoch"],"2");
        assert_ne!(candidate["proposedNode"]["publicKey"],old_node["node"]["publicKey"]);
        assert!(review.prepare_candidate_once().await.is_err());
        let result=review.confirm_once().await;
        let closed:Value=serde_json::from_str(&review.joined_json().await.unwrap()).unwrap();assert_eq!(closed["cleanupState"],"closedObserved");
        let current=if case.case=="rotationPositive" {
            let value:Value=serde_json::from_str(&result.unwrap()).unwrap();assert_eq!(value["selectionObserved"],true);
            assert_eq!(value["phase"],"rotated");assert!(review.confirm_once().await.is_err());value
        }else{
            assert_eq!(case.case,"rotationLostAck");assert!(result.is_err());
            let unknown:Value=serde_json::from_str(&review.details_json().await.unwrap()).unwrap();
            assert_eq!(unknown["phase"],"unknown");assert_eq!(unknown["selectionObserved"],false);
            let blocked=row(&app,case.agent_id).await.unwrap();assert_eq!(blocked["phase"],"blocked");
            assert_eq!(blocked["rotation"]["selected_observed"],false);assert!(!blocked["rotation"]["old_proof_json"].is_null());
            assert!(review.confirm_once().await.is_err());
            app.lock().await.unwrap();app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
            assert_eq!(row(&app,case.agent_id).await.unwrap(),blocked);
            let(owner,u)=app.unlocked().await.unwrap();frame=json!({"version":1,"profileId":owner.profile_id.to_string(),
                "notificationScopeId":u.notices.scope_id().unwrap()});drop(u);drop(owner);
            let status:Value=serde_json::from_str(&app.agent_node_association_status_json(&frame.to_string()).await.unwrap()).unwrap();
            let choice=status["associations"].as_array().unwrap().iter().find(|r|r["associationId"]==associated["associationId"]).unwrap();
            assert_eq!(choice["rotationState"],"unknown");assert_eq!(choice["state"],"blocked");
            let mut fresh=frame.clone();fresh["associationId"]=associated["associationId"].clone();
            assert!(app.agent_node_association_rotation_prepare_json(&fresh.to_string()).await.is_err());
            publish(&root,&format!("native-rotation-unknown-{}.json",case.case),&json!({"version":1,"token":token,"case":case.case,
                "nativeState":"blocked","selectionObserved":false,"unknownSurvivedReopen":true,"retryDenied":true}));
            // Scheduling only; the future coordinator independently observes
            // real ActorR4C3/selected2 and disarms the opaque test loss adapter.
            let scheduled=ready(&root,&format!("rotation-reconcile-ready-{}.json",case.case),token).await;
            assert_eq!(scheduled.case,case.case);assert_eq!(scheduled.port,endpoint.port);assert_eq!(scheduled.host,endpoint.host);
            assert_eq!(scheduled.user,endpoint.user);assert_eq!(scheduled.host_public_key,endpoint.host_public_key);
            let reconcile=app.agent_node_association_rotation_reconcile_prepare_json(&fresh.to_string()).await.unwrap();
            let details:Value=serde_json::from_str(&reconcile.details_json().await.unwrap()).unwrap();
            assert_eq!(details["phase"],"rotationReconcileReview");assert_eq!(details["rotationId"],candidate["rotationId"]);
            assert!(reconcile.prepare_candidate_once().await.is_err());
            let done:Value=serde_json::from_str(&reconcile.confirm_once().await.unwrap()).unwrap();
            let closed:Value=serde_json::from_str(&reconcile.joined_json().await.unwrap()).unwrap();assert_eq!(closed["cleanupState"],"closedObserved");
            assert!(reconcile.confirm_once().await.is_err());
            let after=row(&app,case.agent_id).await.unwrap();
            for field in ["candidate_json","signed_certificate_json","old_proof_json","new_proof_json","committed_owner_pin_json","expected_actor_revision"] {
                assert_eq!(after["rotation"][field],blocked["rotation"][field]);
            }
            assert_eq!(after["rotation"]["selected_observed"],true);done
        };
        assert_eq!(current["actorRevision"],"4");assert_eq!(current["checkpoint"],"3");assert_eq!(current["node"]["keyEpoch"],"2");
        assert_eq!(current["node"],candidate["proposedNode"]);assert_eq!(current["selectionObserved"],true);
        let rotated=row(&app,case.agent_id).await.unwrap();assert_eq!(rotated["public"],initial_public);
        assert_eq!(rotated["phase"],"associated");assert_eq!(rotated["rotation"]["selected_observed"],true);
        let raw:Vec<u8>=serde_json::from_value(rotated["rotation"]["signed_certificate_json"].clone()).unwrap();
        let cert:Signed<Certificate>=serde_json::from_slice(&raw).unwrap();verify_statement(&cert,case.owner_pin.key).unwrap();
        assert_eq!(cert.body.key_epoch,2);assert_eq!(cert.body.agent_id,case.agent_id);assert_eq!(cert.body.scopes,old_cert.body.scopes);
        assert_eq!(cert.body.capabilities,old_cert.body.capabilities);assert!(cert.body.expires_at<=old_cert.body.expires_at);
        assert_eq!(cert.body.header.context,old_cert.body.header.context);assert_eq!(cert.body.header.owner_epoch,old_cert.body.header.owner_epoch);
        assert_eq!(cert.body.origin,CertificateOrigin::Rotation{previous_certificate:old_cert.body.digest().unwrap(),previous_key_epoch:1});
        let rotation_value=json!({"version":1,"token":token,"case":case.case,"publicApi":true,"details":current,
            "actorRevision":"4","checkpoint":"3","selectedKeyEpoch":"2","capsUnchanged":true,"expiryNotExtended":true});
        publish(&root,&format!("native-rotation-selected-{}.json",case.case),&rotation_value);
        // Independent future Agent readback R4C3/selected2 precedes a SEPARATE
        // explicit public revoke action; it never grants authority via marker.
        let scheduled=ready(&root,&format!("rotation-revoke-ready-{}.json",case.case),token).await;
        assert_eq!(scheduled.case,case.case);assert_eq!(scheduled.port,endpoint.port);assert_eq!(scheduled.host,endpoint.host);
        assert_eq!(scheduled.user,endpoint.user);assert_eq!(scheduled.host_public_key,endpoint.host_public_key);
        assert_eq!(app.unlocked().await.unwrap().1.working().revision_of(host_id),Some(host_revision));
        let mut revoke_request=frame.clone();revoke_request["associationId"]=associated["associationId"].clone();
        let revoke=app.agent_node_association_revoke_prepare_json(&revoke_request.to_string()).await.unwrap();
        let target:Value=serde_json::from_str(&revoke.details_json().await.unwrap()).unwrap();assert_eq!(target["node"]["keyEpoch"],"2");
        assert_eq!(target["certificateDigest"],json!(cert.body.digest().unwrap()));
        let revoked:Value=serde_json::from_str(&revoke.confirm_once().await.unwrap()).unwrap();
        assert_eq!(revoked["remoteRevocationObserved"],true);assert_eq!(revoked["actorRevision"],"5");assert_eq!(revoked["checkpoint"],"4");
        let closed:Value=serde_json::from_str(&revoke.joined_json().await.unwrap()).unwrap();assert_eq!(closed["cleanupState"],"closedObserved");
        let final_row=row(&app,case.agent_id).await.unwrap();assert_eq!(final_row["phase"],"revoked");
        assert_eq!(final_row["public"],initial_public);assert_eq!(final_row["rotation"],rotated["rotation"]);
        let outcome=json!({"case":case.case,"publicApi":true,"rotation":rotation_value,"epoch2Revocation":revoked,"cleanup":closed,
            "reconcileUsed":case.case=="rotationLostAck","newCertificateHash":cert.body.digest().unwrap(),"noNewGrant":true});
        println!("CC_ROTATION_PAIRED_CASE {}",serde_json::to_string(&outcome).unwrap());
        publish(&root,&format!("native-rotation-case-{}.json",case.case),&json!({"version":1,"token":token,"case":case.case,"outcome":outcome}));
        outcomes.push(outcome);
    }
    app.shutdown().await.unwrap();publish(&root,"native-rotation-complete.json",&json!({"version":1,"token":token,"cases":outcomes,"shutdownObserved":true}));
}


// The existing helper body and actual ignored fixture are unchanged. This
// diagnostic obtains only its future TYPE size; it does not call the helper.
#[test]
fn owned_rotation_fixture_future_type_layout_without_construction() {
    fn bytes<F: std::future::Future>(_: impl FnOnce() -> F) -> usize {
        std::mem::size_of::<F>()
    }
    println!("CC_ROTATION_FIXTURE_FUTURE_LAYOUT bytes={}", bytes(run_owned_rotation));
}
