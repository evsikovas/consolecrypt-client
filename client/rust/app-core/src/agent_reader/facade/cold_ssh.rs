//! DRAFT actual Local AppCore -> new strict SSH -> actual Agent broker/read.
//! Explicit owned coordinator only. No fake Peer/Actor, TOFU, production profile
//! or background endpoint is accepted. This test has not been compiled/run.
#[cfg(test)] #[path = "cold_ssh/personal.rs"] mod personal;
use super::*;
use cc_models::{host::HostKeyPolicy, VaultObject};
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use cc_ssh_core::keys::{generate_key, KeyGenAlgorithm};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const PASSWORD: &str = "owned reader cold SSH fixture passphrase 42";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Bootstrap {
    format: String,
    token: String,
    ssh_port: u16,
    host_public_key: String,
}
fn publish(root: &Path, name: &str, value: Value) {
    let mut raw = serde_json::to_vec(&value).unwrap();
    raw.push(b'\n');
    assert!(raw.len() <= 32768);
    let temp = root.join(format!(".{name}.prepared"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .unwrap();
    file.write_all(&raw).unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert!(std::fs::symlink_metadata(root.join(name))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound));
    std::fs::rename(temp, root.join(name)).unwrap();
}
async fn wait_value(root: &Path, name: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            match std::fs::symlink_metadata(root.join(name)) {
                Ok(before) => {
                    assert!(
                        before.is_file()
                            && !before.file_type().is_symlink()
                            && before.len() <= 32768
                    );
                    let mut file = std::fs::File::open(root.join(name)).unwrap();
                    let mut bytes = Vec::new();
                    std::io::Read::by_ref(&mut file)
                        .take(32769)
                        .read_to_end(&mut bytes)
                        .unwrap();
                    assert!(bytes.len() <= 32768 && bytes.len() as u64 == before.len());
                    return strict_json(&bytes, 32768).unwrap();
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => panic!("owned fixture public input unavailable"),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned fixture handshake deadline")
}
fn copy_selection(value: &NativeReaderSelection) -> NativeReaderSelection {
    NativeReaderSelection {
        original: value.original.clone(),
        source: value.source.clone(),
        known_host_id: value.known_host_id,
    }
}
fn copy_reference(value: &AssociationRef) -> AssociationRef {
    let (a, b) = value.locator();
    AssociationRef::from_public_locator(a, b)
}
async fn prepare_fixture(
    app: &AppCore,
    bootstrap: &Bootstrap,
    private: &str,
) -> NativeReaderSelection {
    let credential = app
        .import_ssh_key(
            "Cold stored credential".into(),
            Some("ccreader".into()),
            private.into(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    app.add_known_host(
        "127.0.0.1".into(),
        bootstrap.ssh_port,
        bootstrap.host_public_key.clone(),
    )
    .await
    .unwrap();
    let mut host = crate::HostDto::new("Owned strict Reader SSH", "127.0.0.1");
    host.port = Some(bootstrap.ssh_port);
    host.username = Some("ccreader".into());
    host.credential_id = Some(credential.id);
    host.host_key_policy = HostKeyPolicy::Strict;
    let host = app.save_host(host).await.unwrap();
    let view = app
        .monitoring_source_create(
            0,
            SourceConfig {
                id: String::new(),
                name: "Reader explicit metadata".into(),
                endpoint: "https://owned-reader.invalid/prometheus".into(),
                route: Route::Direct,
                tls: TlsConfig::default(),
                auth: AuthRef::None,
            },
        )
        .await
        .unwrap();
    let source = view.sources[0].id.clone();
    let view = app
        .monitoring_binding_create(
            view.revision.unwrap(),
            "Owned explicit source binding".into(),
            host.id.clone(),
            source.clone(),
            1,
            BTreeMap::from([("job".into(), "reader-owned-fixture".into())]),
        )
        .await
        .unwrap();
    let s = app.session().await.unwrap();
    let u = s.unlocked().await.unwrap();
    let key = u
        .working()
        .known_hosts()
        .into_iter()
        .find(|k| !k.is_revoked())
        .unwrap();
    NativeReaderSelection {
        original: None,
        source: ReaderSourceSelection {
            profile_id: s.profile_id.to_string(),
            host_id: host.id,
            source_id: source,
            source_revision: 1,
            binding_id: view.bindings[0].id.clone(),
            binding_revision: view.bindings[0].revision,
        },
        known_host_id: key.id,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires exact owned glibc sshd+Agent broker coordinator; missing inputs FAIL"]
async fn actual_local_reader_cold_strict_ssh_source_chain_and_original_fences() {
    let token = std::env::var("CC_READER_COLD_SSH_TOKEN").expect("owned UUID required");
    assert_eq!(Uuid::parse_str(&token).unwrap().to_string(), token);
    assert!(!Uuid::parse_str(&token).unwrap().is_nil());
    let root = PathBuf::from(
        std::env::var_os("CC_READER_COLD_SSH_ROOT").expect("owned control root required"),
    );
    assert!(
        root.is_absolute()
            && !std::fs::symlink_metadata(&root)
                .unwrap()
                .file_type()
                .is_symlink()
    );
    assert_eq!(
        root.file_name().unwrap().to_str().unwrap(),
        format!("owned-{token}")
    );
    let ssh_key = generate_key(KeyGenAlgorithm::Ed25519, "owned-reader-only", None).unwrap();
    publish(
        &root,
        "native-client-key.json",
        json!({"format":"cc-reader-cold-ssh-client/v1","token":token,"publicKey":ssh_key.public_openssh}),
    );
    let bootstrap: Bootstrap =
        serde_json::from_value(wait_value(&root, "ssh-bootstrap-ready.json").await).unwrap();
    assert_eq!(bootstrap.format, "cc-reader-cold-ssh-bootstrap/v1");
    assert_eq!(bootstrap.token, token);
    assert!(bootstrap.ssh_port > 0);
    assert!(
        bootstrap.host_public_key.starts_with("ssh-ed25519 ")
            && !bootstrap.host_public_key.chars().any(char::is_control)
    );
    let private_root = PathBuf::from(
        std::env::var_os("CC_READER_COLD_SSH_PRIVATE_ROOT")
            .expect("separate unmounted private Tasks root required"),
    );
    assert!(
        private_root.is_absolute()
            && !std::fs::symlink_metadata(&private_root)
                .unwrap()
                .file_type()
                .is_symlink()
    );
    assert_eq!(
        private_root.file_name().unwrap().to_str().unwrap(),
        format!("reader-cold-private-owned{token}")
    );
    assert_eq!(private_root.parent().unwrap().file_name().unwrap(), "Tasks");
    let private_canonical = private_root.canonicalize().unwrap();
    let public_canonical = root.canonicalize().unwrap();
    assert!(
        !private_canonical.starts_with(&public_canonical)
            && !public_canonical.starts_with(&private_canonical)
    );
    let profile = tempfile::Builder::new()
        .prefix("native-local-profile-")
        .tempdir_in(&private_root)
        .unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        profile.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Cold actual Reader".into(), PASSWORD.into())
        .await
        .unwrap();
    let selection =
        prepare_fixture(&app, &bootstrap, ssh_key.private_openssh.expose_secret()).await;
    drop(ssh_key); // Stored resolver is now the ONLY SSH credential source.
    let issuer_review = app
        .reader_prepare_issuer_initialization(copy_selection(&selection))
        .unwrap()
        .wait()
        .await
        .unwrap();
    let issuer = app
        .reader_confirm_issuer_initialization_once(issuer_review)
        .unwrap()
        .wait()
        .await
        .unwrap();
    let s = app.session().await.unwrap();
    let u = s.unlocked().await.unwrap();
    let cold_version = u.working().version();
    // First ever reader connection resolves the actual vault key and creates a
    // dedicated native SshSession. It MUST NOT mutate/rebase WorkingSet trust.
    let observed = app
        .reader_observe_endpoint_once(copy_selection(&selection))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(
        u.working().version(),
        cold_version,
        "cold credential resolution self-mutated trust"
    );
    let e = observed.endpoint();
    let now = chrono::Utc::now().timestamp() as u64;
    let pin:AgentReaderIssuerPinV1=serde_json::from_value(json!({"format":"cc-reader-issuer-pin/v1","context":e.node_context,
        "nativeProfileId":selection.source.profile_id,"issuerId":issuer.issuer_id,"issuerEpoch":1,"issuerKey":issuer.issuer_key,
        "audienceId":Uuid::new_v4(),"expiresAt":now+3600,"nodeTrustOriginHash":e.node_trust_origin_hash})).unwrap();
    let head = AgentReaderHeadV1 {
        context: pin.context.clone(),
        issuer_id: pin.issuer_id,
        issuer_epoch: 1,
        authority_revision: 0,
        authority_hash: pin.genesis_hash().unwrap(),
    };
    let target = e.targets.targets[1].clone();
    let binding = ReaderPeerBinding {
        native_profile_id: pin.native_profile_id,
        agent_id: e.agent_id,
        audience_id: pin.audience_id,
        node_trust_origin_hash: e.node_trust_origin_hash,
        issuer_pin_genesis_hash: pin.genesis_hash().unwrap(),
        transport_peer_hash: observed.ssh_key_digest(),
        agent_certificate_hash: e.agent_certificate_hash,
        agent_key_epoch: e.agent_key_epoch,
        target_hashes: [
            e.targets.targets[0].descriptor_hash().unwrap(),
            target.descriptor_hash().unwrap(),
        ],
        ssh_server_key_digest: observed.ssh_key_digest(),
        http_port: e.http_port,
    };
    binding.digest().unwrap();
    publish(
        &root,
        "native-provision.json",
        json!({"format":"cc-reader-cold-ssh-provision/v1","token":token,"pin":pin,"binding":binding}),
    );
    let provisioned = wait_value(&root, "agent-provisioned.json").await;
    assert_eq!(
        provisioned,
        json!({"format":"cc-reader-cold-ssh-provisioned/v1","token":token})
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
    // Product opaque handle: real read -> observed SSH close -> one consume.
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    let (peer, association) = reference.locator();
    let public_request = json!({"notificationScopeId":u.notices.scope_id().unwrap(),"selection":{"profileId":selection.source.profile_id,
        "hostId":selection.source.host_id,"sourceId":selection.source.source_id,
        "sourceRevision":selection.source.source_revision,"bindingId":selection.source.binding_id,
        "bindingRevision":selection.source.binding_revision,"knownHostId":selection.known_host_id.to_string()},
        "action":{"kind":"read","association":{"peerDigest":peer,"associationDigest":association}}});
    let handle = app
        .reader_prepare_action_json(&public_request.to_string())
        .await
        .unwrap();
    let ready: serde_json::Value =
        serde_json::from_str(&handle.confirm_json().await.unwrap()).unwrap();
    assert_eq!(ready["kind"], "readReady");
    assert_eq!(
        ready["targetHash"],
        serde_json::json!(target.descriptor_hash().unwrap())
    );
    // The real transport is already closed; publication context must remain live.
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (1, 0));
    let data: serde_json::Value =
        serde_json::from_str(&handle.consume_read_json().await.unwrap()).unwrap();
    let body: serde_json::Value = serde_json::from_str(data["bodyJson"].as_str().unwrap()).unwrap();
    assert_eq!(body["history_kind"], "fixed_os_snapshot");
    assert_eq!(body["points"].as_array().unwrap().len(), 2);
    assert!(handle.consume_read_json().await.is_err());
    assert_eq!(app.inner.reader_native.public_counts().unwrap(), (0, 0));
    drop(handle);
    assert_eq!(
        u.working().version(),
        cold_version,
        "strict normal read cannot self-rebase trust"
    );

    // One REAL subsystem channel is paused before pass-through broker dispatch;
    // add and remove a NEW matching revoked key. Final membership is restored,
    // but the original whole-WorkingSet version must suppress this attempt.
    let hosts_before: BTreeMap<_, _> = u
        .working()
        .known_hosts()
        .into_iter()
        .map(|k| (k.id, k))
        .collect();
    let selected_revision = u.working().revision_of(selection.known_host_id);
    assert!(selected_revision.is_some());
    let aba_version = u.working().version();
    let aba_user_cancel = CancellationToken::new();
    let aba_generation = s.automation.unlocked_generation().unwrap();
    let aba_ctx =
        ReaderOriginalContext::capture(&app, selection.source.clone(), &aba_user_cancel)
            .await
            .unwrap();
    aba_ctx.check(&app).await.unwrap();
    assert!(aba_ctx.scope_identity().is_live());
    assert!(!aba_user_cancel.is_cancelled());
    // The SAME actual planner/resolver/known-host constructor used by Base,
    // with no fake peer or caller-created ConnectionPlan.
    let (planner, key) = aba_ctx
        .with_held(&app, |held| {
            Ok((
                held.unlock.ssh.planner.clone(),
                held.unlock
                    .working()
                    .known_host(selection.known_host_id)
                    .ok_or_else(|| {
                        crate::AppError::invalid("reader_fixture", "missing_known_host")
                    })?,
            ))
        })
        .await
        .unwrap();
    let plan = planner
        .plan(crate::dto::parse_id("reader_fixture", &aba_ctx.snapshot().native_host_id).unwrap())
        .await
        .unwrap();
    let ssh = aba_ctx
        .with_held(&app, |held| {
            ReaderSshSelection::from_original_context(&held, plan, key)
                .map_err(|_| crate::AppError::invalid("reader_fixture", "actual_ssh_selection"))
        })
        .await
        .unwrap();
    publish(
        &root,
        "arm-probe.json",
        json!({"format":"cc-reader-cold-ssh-arm/v1","token":token}),
    );
    let probe_started = Instant::now();
    let attempt = ReaderTransportCore::probe_unapproved_once(&app, &aba_ctx, ssh);
    tokio::pin!(attempt);
    let paused = tokio::select! {
        result = &mut attempt => panic!("actual SSH probe ended before controlled pause: {:?}", result.err()),
        paused = wait_value(&root, "probe-paused.json") => paused,
    };
    assert_eq!(paused["token"], token);
    assert!(
        probe_started.elapsed() < Duration::from_secs(3),
        "probe too slow for causal trust oracle"
    );
    let (mut revoked, errors) = cc_ssh_core::known_hosts::import_known_hosts(&format!(
        "@revoked [127.0.0.1]:{} {}\n",
        bootstrap.ssh_port, bootstrap.host_public_key
    ));
    assert!(errors.is_empty());
    let revoked = revoked.pop().unwrap();
    assert!(revoked.is_revoked());
    let id = revoked.id;
    assert!(!hosts_before.contains_key(&id));
    u.writer.put(VaultObject::KnownHost(revoked)).await.unwrap();
    assert!(u.working().known_host(id).unwrap().is_revoked());
    u.writer.delete(id).await.unwrap();
    let hosts_after: BTreeMap<_, _> = u
        .working()
        .known_hosts()
        .into_iter()
        .map(|k| (k.id, k))
        .collect();
    assert_eq!(
        hosts_before, hosts_after,
        "ABA must restore exact membership/row bodies"
    );
    assert_eq!(
        u.working().revision_of(selection.known_host_id),
        selected_revision,
        "selected original key row must remain unchanged"
    );
    assert!(u.working().version() > aba_version);
    assert_eq!(
        aba_ctx.check(&app).await.unwrap_err(),
        crate::AppError::invalid("agent_reader", "reader_context_changed")
    );
    assert!(!aba_ctx.scope_identity().is_live());
    // The scope's metadata fence is dead; its real owner/unlock and caller's
    // cancellation token are independently unchanged, and expiry is not near.
    let current_owner = app.session().await.unwrap();
    assert!(Arc::ptr_eq(&s, &current_owner));
    assert!(Arc::ptr_eq(&u, &current_owner.unlocked_opt().await.unwrap()));
    assert_eq!(s.automation.unlocked_generation().unwrap(), aba_generation);
    assert!(!s.automation.stopped.is_cancelled());
    assert!(!aba_user_cancel.is_cancelled());
    assert!(aba_ctx.deadline().saturating_duration_since(Instant::now()) > Duration::from_secs(10));
    assert!(
        probe_started.elapsed() < Duration::from_secs(4),
        "ABA exhausted control budget before release"
    );
    publish(
        &root,
        "release-probe.json",
        json!({"format":"cc-reader-cold-ssh-release/v1","token":token}),
    );
    let denied = tokio::time::timeout(Duration::from_secs(1), &mut attempt)
        .await
        .expect("trust rejection did not arrive within still-live bound");
    assert!(
        matches!(denied, Err(ReaderTransportError::ContextChanged)),
        "ABA must reject trust, not pass on expiry/timeout/SSH failure"
    );
    assert_eq!(
        aba_ctx.check(&app).await.unwrap_err(),
        crate::AppError::invalid("agent_reader", "reader_context_changed")
    );
    assert!(!aba_ctx.scope_identity().is_live());
    // The scope's metadata fence is dead; its real owner/unlock and caller's
    // cancellation token are independently unchanged, and expiry is not near.
    let current_owner = app.session().await.unwrap();
    assert!(Arc::ptr_eq(&s, &current_owner));
    assert!(Arc::ptr_eq(&u, &current_owner.unlocked_opt().await.unwrap()));
    assert_eq!(s.automation.unlocked_generation().unwrap(), aba_generation);
    assert!(!s.automation.stopped.is_cancelled());
    assert!(!aba_user_cancel.is_cancelled());
    assert!(aba_ctx.deadline().saturating_duration_since(Instant::now()) > Duration::from_secs(10));
    aba_ctx.retire();

    old_original_cannot_dispatch_prepared_get(&app, &selection, &reference).await;
    publish(
        &root,
        "native-finished.json",
        json!({"format":"cc-reader-cold-ssh-native-result/v1","token":token,"passed":true,
        "checks":["actual_cold_credential_strict_knownhost_no_WS_change","actual_native_issuer_approval_broker_fixed_OS200","new_revoked_membership_ABA_denied","prepared_GET_original_context_substitution_denied"],
        "protectedOrigin":"owned_explicit_SSH_endpoint","osCustody":false,"syncedAccount":false}),
    );
    app.lock().await.unwrap();
}

async fn old_original_cannot_dispatch_prepared_get(
    app: &AppCore,
    selection: &NativeReaderSelection,
    reference: &AssociationRef,
) {
    let review = app
        .reader_prepare_read(copy_selection(selection), copy_reference(reference))
        .unwrap()
        .wait()
        .await
        .unwrap();
    let (mut base, mut payload) = review.take().unwrap();
    let Payload::Read {
        registered,
        transport,
    } = &mut payload
    else {
        panic!("actual read payload required")
    };
    let hash = registered.association.body.body_digest().unwrap();
    let operation = AgentReaderOperationId::new(&Uuid::new_v4().to_string()).unwrap();
    let mut action = base
        .authority
        .reserve_action_once(app, &base.ctx, registered.peer_digest, hash, operation)
        .await
        .unwrap();
    let request = base
        .authority
        .challenge_request_once(app, &base.ctx, &mut action, transport.peer())
        .await
        .unwrap();
    let challenge = transport
        .challenge_once(app, &base.ctx, &request)
        .await
        .unwrap();
    let issued = base
        .authority
        .issue_grant_once(app, &base.ctx, action, transport.peer(), &challenge)
        .await
        .unwrap();
    let packet = issued.packet;
    let raw = serde_json::to_vec(&packet).unwrap();
    let prepared = base
        .ctx
        .with_held(app, |held| {
            let expected = AgentReaderExpectedV1 {
                issuer_pin: registered.issuer_pin.clone(),
                current_head: packet.grant.body.head.clone(),
                association_body: registered.association.body.clone(),
                target: registered.target.clone(),
            };
            let input =
                AgentReaderValidatedInputsV1::from_packet(&raw, expected, base.ctx.now().unwrap())
                    .unwrap();
            let proof = held
                .identity
                .sign_agent_reader_proof_v1(input, base.ctx.now().unwrap())
                .unwrap();
            PreparedReaderGet::after_original_device_sign(
                &held,
                transport.peer(),
                &registered.target,
                AgentReaderBundleV1 {
                    association: packet.association,
                    grant: packet.grant,
                    proof,
                    freshness: packet.freshness,
                },
            )
            .map_err(|_| crate::AppError::invalid("reader_fixture", "prepared_get"))
        })
        .await
        .unwrap();
    let fresh =
        ReaderOriginalContext::capture(app, selection.source.clone(), &CancellationToken::new())
            .await
            .unwrap();
    assert!(base.ctx.snapshot() == fresh.snapshot());
    assert!(matches!(
        transport.dispatch_once(app, &fresh, prepared).await,
        Err(ReaderTransportError::ContextChanged)
    ));
    base.authority
        .retire_local(app, &base.ctx, issued.retirement)
        .await
        .unwrap();
    base.lease.closed(transport.close().await);
    assert!(!base.lease.poisoned);
    base.lease.ack();
    fresh.retire();
    base.ctx.retire();
}
