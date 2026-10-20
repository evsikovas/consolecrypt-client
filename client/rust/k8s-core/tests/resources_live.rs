//! Opt-in acceptance against an explicitly provisioned disposable cluster.
//! Never reads the default kubeconfig or changes a production context.
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_k8s_core::{
    gitops::{overview, FluxAction},
    policy::{Audit, AuditEntry, DeviceClass, Policy},
    resources::{
        ListQuery, LogQuery, PodTerminalInput, PodTerminalOutput, PodTerminalRequest, ReadOptions,
        ResourceApi, ResourceChange, ResourceError, ResourceRef, ResourceRelation, WatchOptions,
    },
    session::{Authentication, ConnectOptions, ConnectionRoute, KubernetesSession},
};
use secrecy::SecretString;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;
use tower::BoxError;
use uuid::Uuid;
use zeroize::Zeroizing;

const NAMESPACE: &str = "cc-acceptance";
struct AuditLog;
impl Audit for AuditLog {
    fn record(&self, _: AuditEntry) -> Result<(), BoxError> {
        Ok(())
    }
}
fn session(production: bool) -> KubernetesSession {
    let path = std::env::var("CC_KUBE_ACCEPTANCE_CONFIG")
        .expect("explicit disposable kubeconfig required");
    let source = Zeroizing::new(std::fs::read(path).expect("read explicit disposable kubeconfig"));
    let config: Value = serde_saphyr::from_slice(&source).expect("parse disposable kubeconfig");
    let cluster = &config["clusters"][0]["cluster"];
    let user = &config["users"][0]["user"];
    let decode = |value: &Value| {
        STANDARD
            .decode(value.as_str().expect("inline certificate"))
            .expect("base64 certificate")
    };
    let ca = pem::parse_many(decode(&cluster["certificate-authority-data"])).expect("CA PEM");
    let cert =
        String::from_utf8(decode(&user["client-certificate-data"])).expect("certificate PEM");
    let key = String::from_utf8(decode(&user["client-key-data"])).expect("key PEM");
    let endpoint =
        std::env::var("CC_KUBE_ACCEPTANCE_LOOPBACK").unwrap_or_else(|_| "127.0.0.1:16443".into());
    KubernetesSession::new(
        ConnectOptions {
            server: "https://cc-kube-acceptance.local:6443".parse().unwrap(),
            namespace: NAMESPACE.into(),
            root_certificates: ca.into_iter().map(|cert| cert.into_contents()).collect(),
            tls_server_name: None,
            insecure_skip_tls_verify: false,
            route: ConnectionRoute::SshLoopback(endpoint.parse().expect("loopback endpoint")),
        },
        Authentication::ClientCertificate {
            certificate_pem: cert,
            private_key_pem: SecretString::from(key),
        },
        Policy {
            context_id: Uuid::new_v4(),
            context_name: "acceptance".into(),
            production,
            device: DeviceClass::Desktop,
            allow_exec_in_production: false,
            allow_port_forward_in_production: false,
        },
        Arc::new(AuditLog),
    )
    .expect("explicit acceptance session")
}
fn reference(
    api: &ResourceApi,
    group: &str,
    version: &str,
    resource: &str,
    name: &str,
) -> ResourceRef {
    ResourceRef {
        api: api.resolve(group, version, resource).unwrap(),
        namespace: Some(NAMESPACE.into()),
        name: name.into(),
    }
}

#[tokio::test]
#[ignore = "requires explicit isolated k3s + Flux fixtures and SSH loopback"]
async fn isolated_k3s_flux_helm_and_streams() {
    let session = session(false);
    let api = ResourceApi::new(&session);
    let discovered = api.discover().await.unwrap();
    assert!(discovered.server_version.starts_with("v1."));
    let marker = api
        .get(
            &ResourceRef {
                api: api.resolve("", "v1", "namespaces").unwrap(),
                namespace: None,
                name: NAMESPACE.into(),
            },
            ReadOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        marker.object["metadata"]["labels"]["consolecrypt.dev/acceptance"], "0.6.2",
        "refuse writes without disposable cluster marker"
    );
    assert!(api.namespaces().await.unwrap().contains(&NAMESPACE.into()));
    let config = reference(&api, "", "v1", "configmaps", "fixture-config");
    let secrets = api
        .list(
            &api.resolve("", "v1", "secrets").unwrap(),
            &ListQuery {
                namespace: Some(NAMESPACE.into()),
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(secrets.items.len(), 1);
    assert!(secrets.continue_token.is_some());
    let secret = api
        .get(
            &reference(&api, "", "v1", "secrets", "fixture-secret"),
            ReadOptions::default(),
        )
        .await
        .unwrap();
    assert!(!secret.yaml.contains("cc-acceptance-must-stay-masked"));
    assert!(secret.yaml.contains("[redacted]"));
    assert!(api.schema(&config.api).await.unwrap().schema.is_object());
    let access = api.check_permissions(NAMESPACE).await.unwrap();
    assert!(access.can_mutate);

    let production = self::session(true);
    let guarded = ResourceApi::new(&production);
    guarded.discover().await.unwrap();
    let mut candidate = api
        .get(&config, ReadOptions::default())
        .await
        .unwrap()
        .object;
    candidate["data"]["greeting"] = json!("dry-run");
    let yaml = Zeroizing::new(serde_saphyr::to_string(&candidate).unwrap());
    assert!(guarded
        .apply(&config, yaml.clone(), true, false)
        .await
        .is_err());
    if let Err(error) = api.apply(&config, yaml.clone(), true, false).await {
        assert_eq!(error, ResourceError::Conflict);
    }
    // The fixture was initially owned by kubectl. Force is explicit here,
    // matching the separate reviewed conflict override in the application.
    let dry_run = api.apply(&config, yaml.clone(), true, true).await.unwrap();
    assert_eq!(dry_run.object["data"]["greeting"], "dry-run");
    assert_ne!(
        api.get(&config, ReadOptions::default())
            .await
            .unwrap()
            .object["data"]["greeting"],
        "dry-run"
    );
    assert!(!api
        .compare_manifest(&config, yaml.clone())
        .await
        .unwrap()
        .changes
        .is_empty());
    api.apply(&config, yaml, false, true).await.unwrap();
    let current = api.get(&config, ReadOptions::default()).await.unwrap();
    assert_eq!(current.object["data"]["greeting"], "dry-run");
    // Restore fixture content using its new resourceVersion.
    let mut restored = current.object;
    restored["data"]["greeting"] = json!("acceptance");
    api.apply(
        &config,
        Zeroizing::new(serde_saphyr::to_string(&restored).unwrap()),
        false,
        false,
    )
    .await
    .unwrap();

    let stop = CancellationToken::new();
    let (events, mut received) = mpsc::channel(2);
    let watch_api = api.clone();
    let watch_kind = config.api.clone();
    let watch_stop = stop.clone();
    let watch = tokio::spawn(async move {
        watch_api
            .watch(
                &watch_kind,
                WatchOptions {
                    query: ListQuery {
                        namespace: Some(NAMESPACE.into()),
                        field_selector: Some("metadata.name=fixture-config".into()),
                        ..Default::default()
                    },
                    resource_version: None,
                },
                events,
                watch_stop,
            )
            .await
    });
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap()
            .unwrap();
        if matches!(event, ResourceChange::SnapshotPage { .. }) {
            break;
        }
    }
    let mut watched = api
        .get(&config, ReadOptions::default())
        .await
        .unwrap()
        .object;
    watched["data"]["greeting"] = json!("watch");
    api.apply(
        &config,
        Zeroizing::new(serde_saphyr::to_string(&watched).unwrap()),
        false,
        false,
    )
    .await
    .unwrap();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap()
            .unwrap();
        if let ResourceChange::Modified { resource } = event {
            assert_eq!(resource.object["kind"], "ConfigMap");
            assert_eq!(resource.object["data"]["greeting"], "watch");
            break;
        }
    }
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), watch)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut restored = api
        .get(&config, ReadOptions::default())
        .await
        .unwrap()
        .object;
    restored["data"]["greeting"] = json!("acceptance");
    api.apply(
        &config,
        Zeroizing::new(serde_saphyr::to_string(&restored).unwrap()),
        false,
        false,
    )
    .await
    .unwrap();

    let created = reference(&api, "", "v1", "configmaps", "native-created");
    let manifest = Zeroizing::new(
        serde_saphyr::to_string(&json!({"apiVersion":"v1","kind":"ConfigMap",
        "metadata":{"name":"native-created","namespace":NAMESPACE},"data":{"message":"safe"}}))
        .unwrap(),
    );
    api.create(&created, manifest.clone(), true).await.unwrap();
    assert_eq!(
        api.get(&created, ReadOptions::default()).await.unwrap_err(),
        ResourceError::NotFound
    );
    let object = api.create(&created, manifest, false).await.unwrap();
    api.delete(
        &created,
        &object.summary.uid,
        &object.summary.resource_version,
    )
    .await
    .unwrap();

    let kustom = reference(
        &api,
        "kustomize.toolkit.fluxcd.io",
        "v1",
        "kustomizations",
        "fixture-ready",
    );
    let table = api
        .table(
            &kustom.api,
            &ListQuery {
                namespace: Some(NAMESPACE.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(table.columns.iter().any(|column| column.name == "Ready"));
    let objects = table
        .rows
        .into_iter()
        .map(|row| row.resource.object)
        .collect::<Vec<_>>();
    let overview = overview(&objects);
    assert!(overview.ready >= 1);
    assert!(overview.not_ready >= 1);
    assert!(overview.dependencies.iter().any(|edge| edge.missing));
    let flow = cc_k8s_core::gitops::flow_graph(&objects);
    assert!(flow
        .edges
        .iter()
        .any(|edge| edge.relation == "inventory" && edge.to.kind == "Deployment"));
    for (resource, name) in [
        ("gitrepositories", "fixture-local-git"),
        ("ocirepositories", "fixture-local-oci"),
    ] {
        let source = api
            .get(
                &reference(&api, "source.toolkit.fluxcd.io", "v1", resource, name),
                ReadOptions::default(),
            )
            .await
            .unwrap();
        assert!(source.object["status"]["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|condition| condition["type"] == "Ready" && condition["status"] == "True"));
    }
    let broken = api
        .get(
            &reference(
                &api,
                "source.toolkit.fluxcd.io",
                "v1",
                "gitrepositories",
                "fixture-broken-source",
            ),
            ReadOptions::default(),
        )
        .await
        .unwrap();
    let issues = cc_k8s_core::diagnostics::diagnose(&[broken.object]);
    assert!(issues.iter().any(|issue| issue.code == "flux_not_ready"));
    api.flux_action(&kustom, FluxAction::Suspend, Some("acceptance"))
        .await
        .unwrap();
    assert_eq!(
        api.get(&kustom, ReadOptions::default())
            .await
            .unwrap()
            .object["spec"]["suspend"],
        true
    );
    api.flux_action(&kustom, FluxAction::Resume, None)
        .await
        .unwrap();
    api.flux_reconcile_with_source(&kustom).await.unwrap();
    let history = api
        .helm_history(NAMESPACE, Some("fixture-helm"))
        .await
        .unwrap();
    assert!(!history.is_empty());
    let release = api
        .helm_release(NAMESPACE, &history.last().unwrap().secret_name)
        .await
        .unwrap();
    assert_eq!(release.name, "fixture-helm");
    assert!(release.manifest.contains("Deployment"));
    assert!(release.computed_values.is_object());
    let workload = reference(&api, "apps", "v1", "deployments", "podinfo");
    assert!(!api.rollout_history(&workload).await.unwrap().is_empty());
    let relations = api.related_objects(&workload).await.unwrap();
    let replica = relations
        .items
        .iter()
        .find(|related| {
            related.relation == ResourceRelation::Dependent
                && related.reference.api.kind == "ReplicaSet"
        })
        .expect("Deployment must resolve its UID-owned ReplicaSet");
    let reverse = api.related_objects(&replica.reference).await.unwrap();
    assert!(reverse
        .items
        .iter()
        .any(|related| related.relation == ResourceRelation::Owner
            && related.reference.name == "podinfo"));
    let service_relations = api
        .related_objects(&reference(&api, "", "v1", "services", "podinfo"))
        .await
        .unwrap();
    assert!(service_relations
        .items
        .iter()
        .any(|related| related.relation == ResourceRelation::SelectedPod));
    let forward_targets = api
        .forward_targets(&reference(&api, "", "v1", "services", "podinfo"))
        .await
        .unwrap();
    assert!(forward_targets
        .items
        .iter()
        .any(|target| target.ready && target.ports.iter().any(|port| port.port == 9898)));
    let workload_targets = api.forward_targets(&workload).await.unwrap();
    assert!(workload_targets.items.iter().any(|target| target.ready));
    let node_metrics = api.metric_usage(false, None).await.unwrap();
    assert!(node_metrics.iter().any(|metric| metric.resources_available
        && metric
            .allocatable
            .as_ref()
            .and_then(|amount| amount.cpu_cores)
            .is_some_and(|cpu| cpu > 0.0)));
    let pod_metrics = api.metric_usage(true, Some(NAMESPACE)).await.unwrap();
    assert!(pod_metrics
        .iter()
        .any(|metric| metric.resources_available && metric.requests.is_some()));
    let logs = api
        .logs(NAMESPACE, "fixture-shell", &LogQuery::default())
        .await
        .unwrap();
    assert!(logs.text.contains("acceptance-ready"));

    let (stdin, input) = mpsc::channel(4);
    let (output, mut stdout) = mpsc::channel(8);
    let terminal_api = api.clone();
    let task = tokio::spawn(async move {
        terminal_api
            .terminal(
                PodTerminalRequest {
                    namespace: NAMESPACE.into(),
                    pod: "fixture-shell".into(),
                    container: Some("shell".into()),
                    command: Some(vec![
                        "sh".into(),
                        "-c".into(),
                        "read text; printf 'native:%s' \"$text\"".into(),
                    ]),
                    tty: false,
                },
                input,
                output,
                CancellationToken::new(),
            )
            .await
    });
    let connected = tokio::time::timeout(Duration::from_secs(15), stdout.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(connected, PodTerminalOutput::Connected));
    stdin
        .send(PodTerminalInput::Stdin(Zeroizing::new(
            b"acceptance\n".to_vec(),
        )))
        .await
        .unwrap();
    let mut text = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(15), stdout.recv())
            .await
            .unwrap()
            .unwrap()
        {
            PodTerminalOutput::Stdout(bytes) => text.extend(bytes),
            PodTerminalOutput::Exited { code } => {
                assert_eq!(code, Some(0));
                break;
            }
            _ => {}
        }
    }
    task.await.unwrap().unwrap();
    assert_eq!(text, b"native:acceptance");
    let forward = api
        .forward_listener(NAMESPACE, "fixture-shell", 8080, 0)
        .await
        .unwrap();
    let address = forward.address().unwrap();
    assert!(address.ip().is_loopback());
    let (updates, _updates_rx) = mpsc::channel(8);
    let forward = tokio::spawn(async move { forward.serve(updates).await });
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(
        Duration::from_secs(15),
        socket.read_to_string(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(response.contains("acceptance-forward"));
    session.close();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), forward)
            .await
            .unwrap()
            .unwrap(),
        Err(ResourceError::Closed)
    );
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
