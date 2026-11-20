//! Normal Native prepare/admission, with an actual owned loopback SSH target.
//! The duplex Runner is metadata-only: no command or playbook executes here.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_ssh_core::russh::server::Server as _;
use cc_ssh_core::russh::{self, server, Channel, ChannelId};

#[derive(Clone)]
struct OwnedSsh {
    port: u16,
    forwards: Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}
impl server::Server for OwnedSsh {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        self.clone()
    }
}
impl server::Handler for OwnedSsh {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        Ok(
            if user == "runner" && password == "owned loopback fixture password" {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        // This fixture cannot forward to anything except its own listener.
        if host != "127.0.0.1" || port != u32::from(self.port) {
            return Ok(());
        }
        let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", self.port)).await?;
        reply.accept().await;
        let task = tokio::spawn(async move {
            let mut channel = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut channel, &mut socket).await;
        });
        self.forwards.lock().await.push(task);
        Ok(())
    }
    async fn channel_open_session(
        &mut self,
        _: Channel<server::Msg>,
        _: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        panic!("the metadata-only fixture must never execute a target command")
    }
    async fn exec_request(
        &mut self,
        _: ChannelId,
        _: &[u8],
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        panic!("no target execution is permitted")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn correct_password_normal_prepare_submits_exact_frozen_manifest_once() {
    let generated = cc_ssh_core::keys::generate_key(
        cc_ssh_core::keys::KeyGenAlgorithm::Ed25519,
        "owned RUN10 loopback",
        None,
    )
    .unwrap();
    let private = cc_ssh_core::keys::load_private_key(&generated.private_openssh, None).unwrap();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let forwards = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let mut server = OwnedSsh {
        port,
        forwards: forwards.clone(),
    };
    let config = Arc::new(server::Config {
        keys: vec![private],
        inactivity_timeout: Some(Duration::from_secs(30)),
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let (ready, server_ready) = tokio::sync::oneshot::channel();
    let ssh_task = tokio::spawn(async move {
        let running = server.run_on_socket(config, &listener);
        ready
            .send(running.handle())
            .unwrap_or_else(|_| panic!("fixture receiver retired"));
        running.await.unwrap();
    });
    let ssh_handle = server_ready.await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let app = AppCore::new(crate::AppConfig::for_tests(
        directory.path().to_string_lossy().into_owned(),
    ))
    .unwrap();
    app.create_local_profile("Owned positive production review".into(), PASSWORD.into())
        .await
        .unwrap();
    let (owner, unlocked) = app.unlocked().await.unwrap();
    let credential = app
        .add_password_credential(
            "Owned SSH fixture".into(),
            Some("runner".into()),
            "owned loopback fixture password".into(),
        )
        .await
        .unwrap();
    app.import_known_hosts(format!("[127.0.0.1]:{port} {}\n", generated.public_openssh))
        .await
        .unwrap();
    let mut host = crate::HostDto::new("Owned production target", "127.0.0.1");
    host.port = Some(port);
    host.username = Some("runner".into());
    host.credential_id = Some(credential.id);
    host.tags = vec!["production".into()];
    host.host_key_policy = cc_models::host::HostKeyPolicy::Strict;
    host.backend = cc_models::host::SshBackend::Native;
    let host = app.save_host(host).await.unwrap();
    let project = app
        .automation_project_create("Owned production source".into(), None)
        .await
        .unwrap();
    app.automation_project_save(
        project.id.clone(),
        "site.yml".into(),
        None,
        "---\n- hosts: all\n  tasks: []\n".into(),
    )
    .await
    .unwrap();
    app.automation_project_save_inventory(
        project.id.clone(),
        1,
        json!({"version":2,"groups":{"owned":[host.id]},"selected_hosts":[host.id]}).to_string(),
    )
    .await
    .unwrap();
    let remote = Arc::new(
        app.open_private_runner_session(
            &host.id,
            &Arc::downgrade(&unlocked),
            &CancellationToken::new(),
        )
        .await
        .unwrap(),
    );
    let runner_id = Uuid::new_v4();
    let expected = Arc::new(std::sync::Mutex::new(None::<Value>));
    let submitted = Arc::new(std::sync::Mutex::new(None::<Value>));
    let count = Arc::new(AtomicUsize::new(0));
    let (client, helper) = tokio::io::duplex(262144);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let expected_worker = expected.clone();
    let submitted_worker = submitted.clone();
    let calls = count.clone();
    let public = owner.identity().public_keys().signing;
    let original_signer: Uuid = owner.device_id().to_string().parse().unwrap();
    let helper_task = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(helper);
        let mut reader = BufReader::new(reader);
        let probe: Value = serde_json::from_str(include_str!(
            "../../../../automation-core/src/vault_validation/runtime-pin-v1.json"
        ))
        .unwrap();
        let environment = probe["environment_digest"].as_str().unwrap().to_owned();
        let upload = Uuid::new_v4();
        let revision = Uuid::new_v4();
        let mut revision_digest = String::new();
        let mut entries = Vec::<Value>::new();
        let mut contents = Vec::<Vec<u8>>::new();
        let mut grant = None::<Value>;
        let mut job_state = Value::Null;
        while let Ok(Some(raw)) = cc_automation_core::read_frame(&mut reader).await {
            let request: Value = serde_json::from_slice(&raw).unwrap();
            let body = &request["body"];
            let result = match request["operation"].as_str().unwrap() {
                "probe" => probe.clone(),
                "upload_begin" => {
                    revision_digest = body["digest"].as_str().unwrap().into();
                    json!({"upload_id":upload})
                }
                "upload_entry" => {
                    assert_eq!(body["upload_id"], json!(upload));
                    entries.push(body["entry"].clone());
                    contents.push(vec![]);
                    json!({})
                }
                "upload_chunk" => {
                    assert_eq!(body["upload_id"], json!(upload));
                    let data = contents.last_mut().unwrap();
                    assert_eq!(body["offset"], json!(data.len()));
                    data.extend(STANDARD.decode(body["data"].as_str().unwrap()).unwrap());
                    json!({"offset":data.len()})
                }
                "upload_commit" => {
                    let mut files = 0;
                    let mut bytes = 0;
                    for (entry, data) in entries.iter().zip(&contents) {
                        if let Some(size) = entry["bytes"].as_u64() {
                            files += 1;
                            bytes += size;
                            assert_eq!(size, data.len() as u64);
                            assert_eq!(entry["sha256"], json!(hex::encode(Sha256::digest(data))));
                        }
                    }
                    assert_eq!(
                        witness(json!({"version":1,"entries":entries}))["digest"],
                        json!(revision_digest)
                    );
                    json!({"revision_id":revision,"digest":revision_digest,"file_count":files,"entry_count":entries.len(),"bytes":bytes})
                }
                "prepare_run" => {
                    assert_eq!(body["revision_id"], json!(revision));
                    let parameters = witness(body["parameters"].clone());
                    let inventory = witness(body["inventory"].clone());
                    let targets = witness(body["targets"].clone());
                    let checks=["syntax","lint"].map(|name|json!({"name":name,"exit_code":0,"output":"metadata fixture, no Ansible process","truncated":false,"cancelled":false,"timed_out":false,"elapsed_ms":1}));
                    json!({"revision_digest":revision_digest,"environment_digest":environment,"playbook":body["playbook"],
                        "parameters_digest":parameters["digest"],"inventory_digest":inventory["digest"],"targets_digest":targets["digest"],
                        "valid":true,"checks":checks,"input_witnesses":{"parameters":parameters,"inventory":inventory}})
                }
                "configure_interactive" => {
                    assert!(grant.is_none());
                    grant = Some(body.clone());
                    json!({"configured":true,"persistent":false})
                }
                "configure_manual_owner" => {
                    assert_eq!(grant.as_ref(), Some(body));
                    json!({"configured":true,"persistent":false,"owner_policy":"ownerConfirmationVaultPassphraseV1"})
                }
                "accept_job" => {
                    assert_eq!(calls.fetch_add(1, Ordering::SeqCst), 0);
                    let original = expected_worker.lock().unwrap().clone().unwrap();
                    assert_eq!(
                        body["approval"]["manifest"], original,
                        "no job/nonce/time/digest may be reminted after password"
                    );
                    let manifest: cc_models::automation::JobManifest =
                        serde_json::from_value(original).unwrap();
                    let proof: cc_crypto_core::OwnerConfirmation =
                        serde_json::from_value(body["approval"]["owner_confirmation"].clone())
                            .unwrap();
                    let signer =
                        serde_json::from_value(body["approval"]["signer_id"].clone()).unwrap();
                    assert_eq!(signer, original_signer);
                    proof.verify(&manifest, signer, &public).unwrap();
                    let signature: [u8; 64] = STANDARD
                        .decode(body["approval"]["signature"].as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap();
                    cc_crypto_core::verify_automation_manifest(&public, &manifest, &signature)
                        .unwrap();
                    *submitted_worker.lock().unwrap() = Some(body.clone());
                    job_state = json!({"job_id":manifest.job_id,"manifest_digest":proof.manifest_digest(),"state":"succeeded"});
                    json!({"accepted":true,"job_id":manifest.job_id})
                }
                "job_get" => job_state.clone(),
                "vault_state" => {
                    assert_eq!(body.as_object().unwrap().len(), 2);
                    assert_eq!(
                        body["locked"], true,
                        "only actual shutdown reaches this fixture"
                    );
                    assert!(body["generation"].as_u64().is_some_and(|value| value > 0));
                    json!({"locked":true})
                }
                "revision_release" | "job_cancel" => json!({}),
                other => panic!("unexpected positive fixture operation {other}"),
            };
            let mut response=serde_json::to_vec(&json!({"version":1,"type":"response","request_id":request["request_id"],"result":result,"error":null})).unwrap();
            response.push(b'\n');
            if writer.write_all(&response).await.is_err() {
                break;
            }
        }
    });
    owner.automation.connections.lock().unwrap().insert(
        runner_id.to_string(),
        Arc::new(RunnerConnection {
            id: runner_id.to_string(),
            revision: 1,
            configuration: AutomationRunnerConfig::Remote {
                host_id: host.id.clone(),
                helper: cc_automation_core::RemoteHelperConfig {
                    python: "/owned/metadata/python".into(),
                    module_dir: "/owned/metadata/module".into(),
                    root: "/owned/metadata/state".into(),
                },
            },
            peer: peer.clone(),
            probe: Value::Null,
            remote: Some(remote),
            local: None,
            authorization: tokio::sync::Mutex::new(None),
        }),
    );
    let request = AutomationRunRequest {
        project_id: project.id,
        runner_id: runner_id.to_string(),
        playbook: "site.yml".into(),
        host_ids: vec![host.id],
        mode: JobMode::Check,
        parameters: RunParameters {
            extra_vars: BTreeMap::new(),
            tags: vec![],
            skip_tags: vec![],
            diff: false,
            vault: None,
        },
        timeout_seconds: 60,
        forks: 1,
        cancel_on_lock: true,
        retry_of: None,
        vault_credential_id: None,
        vault_selection: None, deployment_environment:None,
    };
    let review = tokio::time::timeout(Duration::from_secs(20), app.automation_prepare_run(request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(review["owner_confirmation"]["required"], true);
    let review_id = review["review_id"].as_str().unwrap().to_owned();
    let frozen = {
        let reviews = owner.automation.reviews.lock().unwrap();
        let original = reviews.get(&review_id.parse().unwrap()).unwrap();
        assert!(original.policy.production);
        assert!(original.validation.is_valid());
        assert_eq!(original.sessions.len(), 1);
        serde_json::to_value(original.production_owner.as_ref().unwrap().manifest()).unwrap()
    };
    *expected.lock().unwrap() = Some(frozen.clone());
    let row = app
        .automation_confirm_owner_run(
            review_id.clone(),
            crate::StepUp::Passphrase(PASSWORD.into()),
        )
        .await
        .unwrap();
    assert_eq!(row["manifest"], frozen);
    assert_eq!(row["job_id"], frozen["job_id"]);
    assert_eq!(
        row["owner_confirmation"]["manifest_digest"],
        review["owner_confirmation"]["manifest_digest"]
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let jobs = app.automation_jobs().await.unwrap();
            let rows = jobs["jobs"].as_array().unwrap();
            assert_eq!(rows.len(), 1);
            if rows[0]["state"] == "succeeded" {
                assert_eq!(rows[0]["manifest"], frozen);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(app
        .automation_confirm_owner_run(review_id, crate::StepUp::Passphrase(PASSWORD.into()))
        .await
        .is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(submitted.lock().unwrap().is_some());
    app.shutdown().await.unwrap();
    peer.close().await;
    helper_task.await.unwrap();
    ssh_handle.shutdown("owned RUN10 test complete".into());
    tokio::time::timeout(Duration::from_secs(5), ssh_task)
        .await
        .unwrap()
        .unwrap();
    for task in forwards.lock().await.drain(..) {
        task.abort();
        let _ = task.await;
    }
}
