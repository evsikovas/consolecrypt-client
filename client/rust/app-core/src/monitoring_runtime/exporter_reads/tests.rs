//! Owned metadata rows test this workflow gate, NOT a fabricated VerifiedJob,
//! execution signature/approval, installed exporter or public job admission.
use super::*;
use crate::HostDto;
use cc_models::{
    VaultObject,
    automation::{ApprovedTarget, JOB_SIGNATURE_DOMAIN, JobManifest, JobMode, JobPolicy},
};
use cc_monitoring_core::{AuthRef, Route, SourceConfig, TlsConfig};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const PASSWORD: &str = "owned exporter observation fixture password 42";
struct Fixture {
    _dir: tempfile::TempDir,
    app: AppCore,
    source: String,
    host: String,
    profile: String,
    row: Value,
}
impl Fixture {
    async fn new(endpoint: String) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            dir.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("Owned exporter context".into(), PASSWORD.into())
            .await
            .unwrap();
        let host = app
            .save_host(HostDto::new("Owned target", "fixture.example.invalid"))
            .await
            .unwrap();
        let source = app
            .monitoring_source_create(
                0,
                SourceConfig {
                    id: String::new(),
                    name: "Owned".into(),
                    endpoint,
                    route: Route::Direct,
                    tls: TlsConfig::default(),
                    auth: AuthRef::None,
                },
            )
            .await
            .unwrap()
            .sources[0]
            .id
            .clone();
        let (owner, _) = app.unlocked().await.unwrap();
        let profile = owner.profile_id.to_string();
        let job = Uuid::new_v4();
        let digest = "a".repeat(64);
        let manifest = JobManifest {
            version: 1,
            job_id: job,
            tenant_id: Uuid::parse_str(&profile).unwrap(),
            runner_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            revision_digest: digest.clone(),
            inventory_digest: digest.clone(),
            environment_digest: digest.clone(),
            parameters_digest: digest.clone(),
            playbook: "playbook.yml".into(),
            policy: JobPolicy {
                strict_isolation: true,
                cancel_on_lock: true,
                timeout_seconds: 60,
                forks: 1,
                production: false,
            },
            mode: JobMode::Check,
            targets: vec![ApprovedTarget {
                host_id: Uuid::parse_str(&host.id).unwrap(),
                alias: "owned".into(),
                address: "fixture.example.invalid".into(),
                port: 22,
                username: "synthetic".into(),
                route_digest: digest,
                credential_refs: vec![],
            }],
            issued_at: 1_700_000_000,
            expires_at: 1_700_000_060,
            access_expires_at: 1_700_000_060,
            nonce: Uuid::new_v4(),
        };
        let mut hash = Sha256::new();
        hash.update(JOB_SIGNATURE_DOMAIN);
        hash.update(manifest.canonical_bytes().unwrap());
        let row = json!({"version":1,"job_id":job,"runner_id":manifest.runner_id,"project_id":manifest.project_id,"host_ids":[host.id],
            "manifest_digest":hex::encode(hash.finalize()),"manifest":manifest,"state":"succeeded","notification_revision":1});
        let f = Self {
            _dir: dir,
            app,
            source,
            host: host.id,
            profile,
            row,
        };
        f.install(f.row.clone()).await;
        f
    }
    async fn install(&self, row: Value) {
        let (owner, u) = self.app.unlocked().await.unwrap();
        let _p = self.app.hold_session(&owner).await.unwrap();
        let _u = owner.hold_unlocked(&u).await.unwrap();
        let id = row["job_id"].as_str().unwrap().to_owned();
        owner
            .storage()
            .unwrap()
            .write(move |tx| -> AppResult<()> {
                tx.setting_set(
                    "automation.jobs.v1",
                    &json!({"version":1,"records":{id:row}}),
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }
    fn context_action(&self) -> Value {
        json!({"kind":"exporterContext","version":1,"profileId":self.profile,"hostId":self.host,"jobId":self.row["job_id"],"jobRevision":1})
    }
    fn envelope(&self, action: Value) -> String {
        json!({"operationId":Uuid::new_v4(),"sourceId":self.source,"sourceRevision":1,"action":action}).to_string()
    }
    async fn context(&self) -> Value {
        self.app
            .monitoring_read(self.envelope(self.context_action()))
            .await
            .unwrap()["result"]
            .clone()
    }
    fn verification(&self, context: &Value) -> Value {
        json!({"kind":"exporterVerification","version":1,"profileId":self.profile,"notificationScopeId":context["notificationScopeId"],
        "hostId":self.host,"hostRevision":context["hostRevision"],"jobId":self.row["job_id"],"jobRevision":1,
        "exporter":"nodeExporter","selector":{"job":"reviewed","instance":"explicit:9100"},"maxAgeSeconds":120})
    }
}

async fn request(socket: &mut tokio::net::TcpStream, index: usize) -> String {
    use std::collections::BTreeMap;
    let mut raw = Vec::new();
    let header_end = loop {
        let mut data = [0u8; 1024];
        let n = socket.read(&mut data).await.unwrap();
        assert!(n > 0);
        raw.extend_from_slice(&data[..n]);
        assert!(raw.len() < 32768);
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8(raw[..header_end].to_vec()).unwrap();
    assert_eq!(headers.lines().next(), Some("POST /api/v1/query HTTP/1.1"));
    let mut fields = BTreeMap::new();
    for line in headers.lines().skip(1).filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').unwrap();
        assert!(
            fields
                .insert(name.to_ascii_lowercase(), value.trim().to_owned())
                .is_none()
        );
    }
    assert_eq!(
        fields.get("host"),
        Some(&socket.local_addr().unwrap().to_string())
    );
    assert_eq!(
        fields.get("content-type").map(String::as_str),
        Some("application/x-www-form-urlencoded")
    );
    assert!(!fields.contains_key("transfer-encoding"));
    assert!(!fields.contains_key("authorization"));
    let length: usize = fields["content-length"].parse().unwrap();
    assert!(length > 0 && length < 32768 - header_end);
    while raw.len() < header_end + length {
        let mut data = [0u8; 1024];
        let n = socket.read(&mut data).await.unwrap();
        assert!(n > 0);
        raw.extend_from_slice(&data[..n]);
        assert!(raw.len() <= header_end + length);
    }
    assert_eq!(raw.len(), header_end + length);
    let pairs = url::form_urlencoded::parse(&raw[header_end..])
        .into_owned()
        .collect::<Vec<_>>();
    assert_eq!(
        pairs.len(),
        3,
        "only fixed query and Native timeout/limit budgets"
    );
    let pairs = pairs.into_iter().collect::<BTreeMap<_, _>>();
    assert_eq!(pairs.len(), 3, "duplicate form parameters are not accepted");
    assert!(!pairs.contains_key("time"));
    let budgets = cc_monitoring_core::Budgets::default();
    assert_eq!(
        pairs.get("timeout"),
        Some(&format!("{}s", budgets.timeout_seconds))
    );
    assert_eq!(
        pairs.get("limit"),
        Some(&(budgets.max_series + 1).to_string())
    );
    let selector = "{instance=\"explicit:9100\",job=\"reviewed\"}";
    let metric = match index {
        0 | 1 => "up",
        2 => "node_cpu_seconds_total",
        3 => "node_memory_MemTotal_bytes",
        4 => "node_memory_MemAvailable_bytes",
        _ => panic!("unexpected fixed query slot"),
    };
    let expected = if index == 0 {
        format!("{metric}{selector}")
    } else {
        format!("max(time() - timestamp({metric}{selector}))")
    };
    assert_eq!(pairs.get("query"), Some(&expected));
    String::from_utf8(raw).unwrap()
}
async fn response(socket: &mut tokio::net::TcpStream, index: usize, warn: bool) {
    let labels = if index == 0 {
        json!({"job":"reviewed","instance":"explicit:9100","cc_host_id":"never-authority"})
    } else {
        json!({})
    };
    let mut body = json!({"status":"success","data":{"resultType":"vector","result":[{"metric":labels,"value":[1700000000,if index==0{"1"}else{"10"}]}]}});
    if warn {
        body["warnings"] = json!(["untrusted raw endpoint must never escape"]);
    }
    let body = body.to_string();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(head.as_bytes()).await.unwrap();
}

#[test]
fn strict_paired_actions_do_not_accept_caller_status_endpoints_or_optional_nonce_fallback() {
    let base = json!({"operationId":Uuid::new_v4(),"sourceId":Uuid::new_v4(),"sourceRevision":1,
        "action":{"kind":"exporterContext","version":1,"profileId":Uuid::new_v4(),"hostId":Uuid::new_v4(),"jobId":Uuid::new_v4(),"jobRevision":1}});
    assert!(serde_json::from_value::<super::super::ReadRequest>(base.clone()).is_ok());
    for field in [
        "url",
        "endpoint",
        "status",
        "succeeded",
        "approved",
        "runnerReceipt",
        "notificationScopeId",
        "hostRevision",
    ] {
        let mut bad = base.clone();
        bad["action"][field] = json!(true);
        assert!(serde_json::from_value::<super::super::ReadRequest>(bad).is_err());
    }
    let mut verify = base.clone();
    verify["action"]["kind"] = json!("exporterVerification");
    assert!(serde_json::from_value::<super::super::ReadRequest>(verify).is_err());
    let mut verify = base;
    verify["action"]["kind"] = json!("exporterVerification");
    verify["action"]["notificationScopeId"] = json!(Uuid::new_v4());
    verify["action"]["hostRevision"] = json!(1);
    verify["action"]["exporter"] = json!("nodeExporter");
    verify["action"]["selector"] = json!({"job":"reviewed","instance":"explicit:9100"});
    verify["action"]["maxAgeSeconds"] = json!(120);
    assert!(serde_json::from_value::<super::super::ReadRequest>(verify.clone()).is_ok());
    let raw = verify.to_string();
    let duplicate = raw.replace(
        "\"job\":\"reviewed\"",
        "\"job\":\"reviewed\",\"job\":\"reviewed\"",
    );
    assert_ne!(raw, duplicate);
    assert!(serde_json::from_str::<super::super::ReadRequest>(&duplicate).is_err());
    for field in ["url", "status", "approved", "runnerReceipt", "expression"] {
        let mut extra = verify.clone();
        extra["action"][field] = json!(true);
        assert!(serde_json::from_value::<super::super::ReadRequest>(extra).is_err());
    }
}

#[tokio::test]
async fn metadata_context_precedes_credential_route_and_client_creation_and_sends_zero_http() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let f = Fixture::new(format!("https://{}", listener.local_addr().unwrap())).await;
    let (_, u) = f.app.unlocked().await.unwrap();
    let mut settings = u.working().all_vault_settings().pop().unwrap();
    let raw = settings.monitoring.as_mut().unwrap();
    raw["sources"][&f.source]["configuration"]["auth"] =
        json!({"kind":"bearerRef","credentialId":Uuid::new_v4()});
    raw["sources"][&f.source]["configuration"]["route"] =
        json!({"kind":"sshTunnel","routeId":Uuid::new_v4()});
    u.writer
        .put(VaultObject::VaultSettings(settings))
        .await
        .unwrap();
    let view = f.context().await;
    assert_eq!(view["hostId"], f.host);
    assert_eq!(view["kind"], "exporterContext");
    assert!(
        u.monitoring.clients.lock().unwrap().is_empty(),
        "no client cache creation on metadata capture"
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept())
            .await
            .is_err(),
        "zero configured-origin requests"
    );
    assert!(!view.to_string().contains("credentialId"));
    assert!(!view.to_string().contains("fixture.example.invalid"));
    // Verification still rejects the unavailable actual route/reference; metadata
    // is not a fallback granting anonymous or direct HTTP access.
    assert!(
        f.app
            .monitoring_read(f.envelope(f.verification(&view)))
            .await
            .is_err()
    );
    drop(u);
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn context_requires_saved_native_success_and_exact_original_target_tenant_and_digest() {
    let f = Fixture::new("https://never-contact.example.invalid".into()).await;
    for state in ["running", "failed", "unknown", "acknowledged"] {
        let mut changed = f.row.clone();
        changed["state"] = json!(state);
        f.install(changed).await;
        assert!(
            f.app
                .monitoring_read(f.envelope(f.context_action()))
                .await
                .is_err()
        );
    }
    let mut changed = f.row.clone();
    changed["manifest_digest"] = json!("f".repeat(64));
    f.install(changed).await;
    assert!(
        f.app
            .monitoring_read(f.envelope(f.context_action()))
            .await
            .is_err()
    );
    let mut changed = f.row.clone();
    changed["host_ids"] = json!([Uuid::new_v4()]);
    f.install(changed).await;
    assert!(
        f.app
            .monitoring_read(f.envelope(f.context_action()))
            .await
            .is_err()
    );
    f.install(f.row.clone()).await;
    let mut action = f.context_action();
    action["hostId"] = json!(Uuid::new_v4());
    assert!(f.app.monitoring_read(f.envelope(action)).await.is_err());
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_http_returns_only_fixed_observation_original_refs_and_never_raw_labels() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let f = Fixture::new(format!("http://{}", listener.local_addr().unwrap())).await;
    let view = f.context().await;
    let server = tokio::spawn(async move {
        for index in 0..5 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let raw = request(&mut socket, index).await;
            assert!(raw.starts_with("POST /api/v1/query HTTP/1.1\r\n"));
            assert!(!raw.contains("time="));
            response(&mut socket, index, false).await;
        }
    });
    let result = f
        .app
        .monitoring_read(f.envelope(f.verification(&view)))
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(result["result"]["observation"]["state"], "observed");
    assert_eq!(result["result"]["hostId"], f.host);
    assert_eq!(
        result["result"]["notificationScopeId"],
        view["notificationScopeId"]
    );
    assert!(!result.to_string().contains("cc_host_id"));
    assert!(!result.to_string().contains("never-authority"));
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn partial_warning_response_is_typed_unavailable_without_raw_warning_text() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let f = Fixture::new(format!("http://{}", listener.local_addr().unwrap())).await;
    let context = f.context().await;
    let server = tokio::spawn(async move {
        for index in 0..5 {
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket, index).await;
            response(&mut socket, index, index == 1).await;
        }
    });
    let result = f
        .app
        .monitoring_read(f.envelope(f.verification(&context)))
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(result["result"]["observation"]["state"], "unavailable");
    assert!(!result.to_string().contains("untrusted raw endpoint"));
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn actual_saved_job_mutation_during_http_rejects_late_observation_and_guards_do_not_span_network()
 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let f = Fixture::new(format!("http://{}", listener.local_addr().unwrap())).await;
    let context = f.context().await;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        request(&mut first, 0).await;
        entered_tx.send(()).unwrap();
        release_rx.await.unwrap();
        response(&mut first, 0, false).await;
        for index in 1..5 {
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket, index).await;
            response(&mut socket, index, false).await;
        }
    });
    let app = f.app.clone();
    let raw = f.envelope(f.verification(&context));
    let read = tokio::spawn(async move { app.monitoring_read(raw).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    let mut changed = f.row.clone();
    changed["state"] = json!("failed");
    changed["notification_revision"] = json!(2);
    tokio::time::timeout(std::time::Duration::from_secs(5), f.install(changed))
        .await
        .unwrap();
    release_tx.send(()).unwrap();
    assert!(read.await.unwrap().is_err());
    server.await.unwrap();
    f.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_profile_reopen_and_host_revision_change_deny_old_context_before_http() {
    let f = Fixture::new("https://never-contact.example.invalid".into()).await;
    let before = f.context().await;
    f.app.close_profile().await.unwrap();
    f.app.open_profile(f.profile.clone()).await.unwrap();
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    let current = f.context().await;
    assert_ne!(
        before["notificationScopeId"],
        current["notificationScopeId"]
    );
    assert!(
        f.app
            .monitoring_read(f.envelope(f.verification(&before)))
            .await
            .is_err()
    );
    let mut host = f.app.list_hosts().await.unwrap().remove(0);
    host.name = "Changed host".into();
    f.app.save_host(host).await.unwrap();
    assert!(
        f.app
            .monitoring_read(f.envelope(f.verification(&current)))
            .await
            .is_err()
    );
    f.app.shutdown().await.unwrap();
}

type SqlWait = std::pin::Pin<Box<dyn std::future::Future<Output = crate::AppResult<()>> + Send>>;

/// Queue through the real storage worker synchronously. A Pending poll proves
/// this closure is queued, not merely that a Tokio task has been scheduled.
async fn queue_sql_barrier(
    owner: &crate::session::Session,
) -> (
    SqlWait,
    tokio::sync::oneshot::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    use std::task::Poll;
    let storage = owner.storage().unwrap().clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut wait: SqlWait = Box::pin(async move {
        storage
            .write(move |_tx| -> crate::AppResult<()> {
                entered_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .map_err(|_| crate::AppError::internal("owned SQL barrier expired"))?;
                Ok(())
            })
            .await
    });
    std::future::poll_fn(|cx| match wait.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("the real SQL barrier cannot finish before release"),
    })
    .await;
    (wait, entered_rx, release_tx)
}

async fn barrier_entered(entered: tokio::sync::oneshot::Receiver<()>) {
    tokio::time::timeout(std::time::Duration::from_secs(10), entered)
        .await
        .unwrap()
        .unwrap();
}

fn same_revision_readback(f: &Fixture, u: &crate::session::Unlocked, change: &str) {
    use crate::working_set::Readback;
    use cc_models::ObjectPayload;
    use cc_storage_core::LocalState;
    use cc_sync_core::DecryptedObject;
    let working = u.working();
    let object = match change {
        "source" => {
            let mut settings = working.all_vault_settings().pop().unwrap();
            // The existing revision-only watcher deliberately cannot detect
            // this equal-revision replacement; the common content fence must.
            settings.monitoring.as_mut().unwrap()["sources"][&f.source]["configuration"]["name"] =
                json!("Changed by actual equal-revision readback");
            VaultObject::VaultSettings(settings)
        }
        "host" => {
            let mut rdp =
                cc_models::rdp::RdpHost::new("Same revision is no longer SSH", "192.0.2.7");
            rdp.id = crate::dto::parse_id("host", &f.host).unwrap();
            VaultObject::RdpHost(rdp)
        }
        _ => unreachable!(),
    };
    let id = object.id();
    let revision = working.revision_of(id).unwrap();
    working.apply(
        id,
        Readback::Live(Box::new(DecryptedObject {
            object_id: id,
            revision,
            local_state: LocalState::LocalOnly,
            conflict_origin: None,
            payload: ObjectPayload::new(object),
        })),
    );
    assert!(
        u.monitoring
            .operations
            .lock()
            .unwrap()
            .values()
            .all(|op| !op.cancel.is_cancelled()),
        "causal regression must exercise the synchronous fences, not an outer cancellation"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn blocked_pre_http_sql_rechecks_actual_equal_revision_source_and_host_readbacks() {
    use std::{future::Future, task::Poll};
    for change in ["source", "host"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let f = Fixture::new(format!("http://{}", listener.local_addr().unwrap())).await;
        let context = f.context().await;
        let (owner, u) = f.app.unlocked().await.unwrap();
        // First hold the prepare read. Queue a second barrier BEHIND that
        // actual read, so the next poll reaches the pre-HTTP SQL check exactly.
        let (first, entered, release_first) = queue_sql_barrier(&owner).await;
        barrier_entered(entered).await;
        let read = f.app.monitoring_read(f.envelope(f.verification(&context)));
        tokio::pin!(read);
        std::future::poll_fn(|cx| match read.as_mut().poll(cx) {
            Poll::Pending => Poll::Ready(()),
            Poll::Ready(_) => panic!("prepare cannot finish through the blocked SQL worker"),
        })
        .await;
        assert!(f.app.inner.active.try_lock().is_err());
        let (second, entered_second, release_second) = queue_sql_barrier(&owner).await;
        release_first.send(()).unwrap();
        first.await.unwrap();
        barrier_entered(entered_second).await;
        std::future::poll_fn(|cx| match read.as_mut().poll(cx) {
            Poll::Pending => Poll::Ready(()),
            Poll::Ready(_) => panic!("pre-HTTP check cannot finish through second SQL barrier"),
        })
        .await;
        assert!(
            !u.monitoring.clients.lock().unwrap().is_empty(),
            "actual client construction proves prepare finished"
        );
        assert!(
            f.app.inner.active.try_lock().is_err(),
            "original guard held at pre-HTTP SQL await"
        );
        same_revision_readback(&f, &u, change);
        release_second.send(()).unwrap();
        second.await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::select! {
                biased;
                _ = listener.accept() => panic!("no HTTP may dispatch after the actual SQL-wait mutation"),
                result = &mut read => result,
            }
        }).await.unwrap();
        assert!(
            result.is_err(),
            "old common/host checks before SQL would dispatch HTTP"
        );
        f.app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn blocked_final_sql_rechecks_actual_equal_revision_source_and_host_readbacks() {
    use std::{future::Future, task::Poll};
    for change in ["source", "host"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let f = Fixture::new(format!("http://{}", listener.local_addr().unwrap())).await;
        let context = f.context().await;
        let (owner, u) = f.app.unlocked().await.unwrap();
        let (last_tx, last_rx) = tokio::sync::oneshot::channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            for index in 0..4 {
                let (mut socket, _) = listener.accept().await.unwrap();
                request(&mut socket, index).await;
                response(&mut socket, index, false).await;
            }
            let (mut socket, _) = listener.accept().await.unwrap();
            request(&mut socket, 4).await;
            last_tx.send(()).unwrap();
            reply_rx.await.unwrap();
            response(&mut socket, 4, false).await;
        });
        let read = f.app.monitoring_read(f.envelope(f.verification(&context)));
        tokio::pin!(read);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::select! {
                biased;
                result = &mut read => panic!("read completed before owned last response: {result:?}"),
                _ = last_rx => (),
            }
        }).await.unwrap();
        assert!(
            f.app.inner.active.try_lock().is_ok(),
            "no session guard across HTTP"
        );
        let (sql, entered, release) = queue_sql_barrier(&owner).await;
        barrier_entered(entered).await;
        reply_tx.send(()).unwrap();
        // Drive only this actual request until its final held guard proves the
        // network completed and the fresh job SQL read is pending behind us.
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            std::future::poll_fn(|cx| match read.as_mut().poll(cx) {
                Poll::Ready(_) => panic!("result cannot pass the blocked final SQL read"),
                Poll::Pending => {
                    if f.app.inner.active.try_lock().is_err() {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                }
            }),
        )
        .await
        .unwrap();
        same_revision_readback(&f, &u, change);
        release.send(()).unwrap();
        sql.await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), &mut read)
            .await
            .unwrap();
        assert!(
            result.is_err(),
            "old pre-SQL predicates would return an obsolete successful observation"
        );
        server.await.unwrap();
        f.app.shutdown().await.unwrap();
    }
}

fn binding_origin(context: &Value) -> crate::monitoring_sources::MonitoringExporterBindingOrigin {
    let mut raw = context.clone();
    raw.as_object_mut().unwrap().remove("kind");
    raw.as_object_mut().unwrap().remove("protocol");
    serde_json::from_value(raw).unwrap()
}
fn binding_labels() -> std::collections::BTreeMap<String, String> {
    [
        ("job".into(), "explicit".into()),
        ("instance".into(), "reviewed:9100".into()),
    ]
    .into()
}
#[tokio::test]
async fn exporter_binding_origin_current_metadata_uses_existing_writer_and_catalog_cas() {
    let f = Fixture::new("https://fixture.example.invalid".into()).await;
    let context = f.context().await;
    let saved = f
        .app
        .monitoring_binding_create_with_exporter_origin(
            1,
            "Reviewed exporter".into(),
            f.host.clone(),
            f.source.clone(),
            1,
            binding_labels(),
            Some(binding_origin(&context)),
        )
        .await
        .unwrap();
    assert_eq!(saved.revision, Some(2));
    assert_eq!(saved.bindings.len(), 1);
    assert_eq!(saved.bindings[0].binding.host_id().to_string(), f.host);
    assert!(
        f.app
            .monitoring_binding_create_with_exporter_origin(
                1,
                "Stale".into(),
                f.host.clone(),
                f.source.clone(),
                1,
                binding_labels(),
                Some(binding_origin(&context))
            )
            .await
            .is_err()
    );
    assert_eq!(f.app.monitoring_sources().await.unwrap().revision, Some(2));
    f.app.shutdown().await.unwrap();
}
#[tokio::test]
async fn exporter_binding_origin_failed_job_and_old_unlock_do_not_write() {
    let f = Fixture::new("https://fixture.example.invalid".into()).await;
    let context = f.context().await;
    let mut changed = f.row.clone();
    changed["state"] = json!("failed");
    f.install(changed).await;
    assert!(
        f.app
            .monitoring_binding_create_with_exporter_origin(
                1,
                "Denied".into(),
                f.host.clone(),
                f.source.clone(),
                1,
                binding_labels(),
                Some(binding_origin(&context))
            )
            .await
            .is_err()
    );
    f.install(f.row.clone()).await;
    f.app.lock().await.unwrap();
    f.app.unlock_with_passphrase(PASSWORD.into()).await.unwrap();
    assert!(
        f.app
            .monitoring_binding_create_with_exporter_origin(
                1,
                "Old unlock".into(),
                f.host.clone(),
                f.source.clone(),
                1,
                binding_labels(),
                Some(binding_origin(&context))
            )
            .await
            .is_err()
    );
    let catalog = f.app.monitoring_sources().await.unwrap();
    assert_eq!(catalog.revision, Some(1));
    assert!(catalog.bindings.is_empty());
    f.app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn exporter_binding_origin_queued_actual_sql_rechecks_same_revision_host_kind_before_write() {
    use std::{future::Future, task::Poll};
    let f = Fixture::new("https://fixture.example.invalid".into()).await;
    let context = f.context().await;
    let (owner, u) = f.app.unlocked().await.unwrap();
    let (barrier, entered, release) = queue_sql_barrier(&owner).await;
    barrier_entered(entered).await;
    let write = f.app.monitoring_binding_create_with_exporter_origin(
        1,
        "Queued".into(),
        f.host.clone(),
        f.source.clone(),
        1,
        binding_labels(),
        Some(binding_origin(&context)),
    );
    tokio::pin!(write);
    std::future::poll_fn(|cx| match write.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("actual job SQL must be blocked"),
    })
    .await;
    assert!(f.app.inner.active.try_lock().is_err());
    same_revision_readback(&f, &u, "host");
    release.send(()).unwrap();
    barrier.await.unwrap();
    assert!(write.await.is_err());
    let catalog = f.app.monitoring_sources().await.unwrap();
    assert_eq!(catalog.revision, Some(1));
    assert!(catalog.bindings.is_empty());
    f.app.shutdown().await.unwrap();
}

#[path = "tests/live_node.rs"]
mod live_node;

#[path = "tests/live_cadvisor.rs"]
mod live_cadvisor;
