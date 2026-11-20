//! Actual AppCore HTTP lifecycle tests; metadata is explicit fixture data,
//! not a fake server-policy admission/writer capability. Existing MockServer
//! still owns normal authenticated profile/vault operations and crypto shapes.
use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::State,
    response::{IntoResponse, Response},
    Router,
};
use std::sync::Mutex;
use tokio::sync::Semaphore;

#[derive(Clone)]
struct HttpState {
    upstream: String,
    client: reqwest::Client,
    metadata: Arc<Mutex<Option<(String, serde_json::Value)>>>,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}
struct Gate {
    server: MockServer,
    url: String,
    state: HttpState,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.state.release.add_permits(16);
        self.task.abort();
    }
}
async fn route(State(state): State<HttpState>, request: axum::http::Request<Body>) -> Response {
    let value = {
        let expected = state.metadata.lock().unwrap();
        expected
            .as_ref()
            .filter(|(path, _)| {
                request.method() == axum::http::Method::GET && request.uri().path() == path
            })
            .map(|(_, value)| value.clone())
    };
    if let Some(value) = value {
        state.entered.add_permits(1);
        let permit = state.release.acquire().await.unwrap();
        permit.forget();
        return axum::Json(value).into_response();
    }
    // Fixed trusted fixture origin only, no caller URL/redirect/external fetch.
    let (mut parts, body) = request.into_parts();
    parts.headers.remove(axum::http::header::HOST);
    let bytes = to_bytes(body, 4 * 1024 * 1024).await.unwrap();
    let url = format!(
        "{}{}",
        state.upstream.trim_end_matches('/'),
        parts.uri.path_and_query().unwrap().as_str()
    );
    let response = state
        .client
        .request(parts.method, url)
        .headers(parts.headers)
        .body(bytes)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    assert!(bytes.len() <= 4 * 1024 * 1024);
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .unwrap()
}
impl Gate {
    async fn start() -> Self {
        let server = MockServer::start().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let state = HttpState {
            upstream: server.url().to_string(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            metadata: Arc::new(Mutex::new(None)),
            entered: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
        };
        let router = Router::new().fallback(route).with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            server,
            url,
            state,
            task,
        }
    }
    async fn profile(&self, app: &AppCore) {
        app.create_synced_profile(
            "Owned HTTP admission fixture".into(),
            self.url.clone(),
            format!("{}@example.test", uuid::Uuid::new_v4().simple()),
            ACCOUNT.into(),
            AccountMode::Register,
        )
        .await
        .unwrap();
        app.create_vault(PASS.into()).await.unwrap();
        let context = Context::capture(app).await.unwrap();
        let path = format!(
            "/v1/vaults/{}/compatibility/monitoring-settings",
            context.vault
        );
        let body = json!({"version":1,"scope":"personalAccount","accountId":context.account,"revision":0,"state":"off",
            "minimumClientVersion":null,"offerMinimumClientVersion":"2.0.0","rosterDigest":"a".repeat(64),
            "checkpoint":{"vaultId":context.vault,"vaultEpoch":self.server.epoch(context.vault).unwrap(),
                "vaultSequence":self.server.latest_sequence(context.vault)}});
        *self.state.metadata.lock().unwrap() = Some((path, body));
    }
    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.state.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    fn release(&self) {
        self.state.release.add_permits(1);
    }
}
fn begin(app: &AppCore) -> tokio::task::JoinHandle<AppResult<SettingsPolicyReview>> {
    let app = app.clone();
    tokio::spawn(async move { app.settings_policy_observe().await })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_http_observation_returns_off_data_without_admission() {
    let gate = Gate::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    gate.profile(&app).await;
    gate.release();
    let result = app.settings_policy_observe().await.unwrap();
    assert_eq!(result.policy.state(), SettingsPolicyState::Off);
    assert!(app
        .settings_checkpoint_recapture(result)
        .await
        .is_err());
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_blocked_http_is_cancelled_on_lock_without_holding_unlock() {
    let gate = Gate::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    gate.profile(&app).await;
    let task = begin(&app);
    gate.entered().await;
    tokio::time::timeout(Duration::from_secs(3), app.lock())
        .await
        .expect("HTTP must not retain unlock guard")
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(crate::AppError::VaultLocked)));
    gate.release();
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_late_http_cannot_retarget_after_profile_switch() {
    let gate = Gate::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    gate.profile(&app).await;
    let task = begin(&app);
    gate.entered().await;
    tokio::time::timeout(Duration::from_secs(3), app.close_profile())
        .await
        .unwrap()
        .unwrap();
    app.create_local_profile("New owned local context".into(), PASS.into())
        .await
        .unwrap();
    gate.release();
    assert!(tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_late_http_refuses_generic_settings_mutation_and_preserves_it() {
    let gate = Gate::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    gate.profile(&app).await;
    let task = begin(&app);
    gate.entered().await;
    let mut generic = app.get_vault_settings().await.unwrap();
    generic.vault_name = "Fresh owned HTTP metadata".into();
    app.save_vault_settings(generic).await.unwrap();
    gate.release();
    assert!(tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(
        app.get_vault_settings().await.unwrap().vault_name,
        "Fresh owned HTTP metadata"
    );
    app.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_late_http_refuses_future_legacy_payload_without_rewrite() {
    let gate = Gate::start().await;
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    gate.profile(&app).await;
    let task = begin(&app);
    gate.entered().await;
    let expected = json!({"version":999,"revision":7,"opaqueFuture":"owned HTTP retain exact"});
    {
        let (owner, u) = app.unlocked().await.unwrap();
        let _profile = app.hold_session(&owner).await.unwrap();
        let _unlock = owner.hold_unlocked(&u).await.unwrap();
        let mut settings = u
            .working()
            .all_vault_settings()
            .into_iter()
            .max_by_key(|s| s.updated_at)
            .unwrap();
        settings.legacy_payload = Some(expected.clone());
        u.writer
            .put(VaultObject::VaultSettings(settings))
            .await
            .unwrap();
    }
    gate.release();
    assert!(tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    let (_, u) = app.unlocked().await.unwrap();
    let latest = u
        .working()
        .all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .unwrap();
    assert_eq!(latest.legacy_payload, Some(expected));
    drop(u);
    app.shutdown().await.unwrap();
}
