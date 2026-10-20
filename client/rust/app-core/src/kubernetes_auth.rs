//! Device-local Kubernetes authentication. Review tickets, exec permissions,
//! browser verifiers and expiring tokens belong to a single unlocked profile.

use crate::session::Unlocked;
use crate::working_set::Item;
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::auth::{
    exec::{resolve_binary, ExecEngine, ExecRequest, PreparedExec},
    oidc::{LoginFlow, OidcClient, OidcConfig, RefreshIdentity, TokenSet},
    AuthError, Credential,
};
use cc_k8s_core::session::Authentication;
use cc_models::kubernetes::{KubeAuth, KubeCluster, KubeContext};
use cc_models::{ObjectId, VaultObject};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const LIFETIME: Duration = Duration::from_secs(600);
const MAX_PENDING: usize = 4;
const MAX_CACHE: usize = 32;
type Stamp = BTreeMap<ObjectId, i64>;

/// Only return this DTO to the protected local confirmation modal. Arguments
/// and environment values may contain secrets and are wiped after encoding.
#[derive(Serialize)]
pub struct KubeExecReview {
    pub ticket: String,
    pub expires_at_ms: i64,
    pub context_name: String,
    pub binary_path: String,
    pub binary_sha256: String,
    pub invocation_sha256: String,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
}

impl std::fmt::Debug for KubeExecReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KubeExecReview").finish_non_exhaustive()
    }
}

impl Drop for KubeExecReview {
    fn drop(&mut self) {
        self.arguments.zeroize();
        for value in self.environment.values_mut() {
            value.zeroize();
        }
    }
}

#[derive(Serialize)]
pub struct KubeOidcChallenge {
    pub ticket: String,
    pub expires_at_ms: i64,
    pub authorization_url: String,
    pub redirect_uri: String,
    pub loopback: bool,
}

impl std::fmt::Debug for KubeOidcChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KubeOidcChallenge").finish_non_exhaustive()
    }
}

impl Drop for KubeOidcChallenge {
    fn drop(&mut self) {
        self.authorization_url.zeroize();
    }
}

#[derive(Debug)]
pub(crate) struct ResolvedAuth {
    pub authentication: Authentication,
    pub expires_at: Option<SystemTime>,
}

enum PendingKind {
    Exec(Box<PreparedExec>),
    Oidc(Box<PendingOidc>),
}

struct PendingOidc {
    client: OidcClient,
    flow: LoginFlow,
    listener: Option<TcpListener>,
}

struct Pending {
    context_id: ObjectId,
    stamp: Stamp,
    cancel: CancellationToken,
    expires: Instant,
    kind: PendingKind,
}

struct Cache {
    ticket: Uuid,
    stamp: Stamp,
    cancel: CancellationToken,
    credential: Arc<Credential>,
}

#[derive(Clone)]
struct Refresh {
    ticket: Uuid,
    stamp: Stamp,
    cancel: CancellationToken,
    client: OidcClient,
    token: SecretString,
    identity: RefreshIdentity,
}

#[derive(Default)]
struct State {
    closed: bool,
    pending: BTreeMap<Uuid, Pending>,
    active: BTreeMap<Uuid, CancellationToken>,
    cache: BTreeMap<ObjectId, Cache>,
    refresh: BTreeMap<ObjectId, Refresh>,
    approved: BTreeMap<ObjectId, CancellationToken>,
    gates: BTreeMap<ObjectId, Arc<tokio::sync::Mutex<()>>>,
}

struct AuthOperation {
    runtime: Arc<RuntimeAuth>,
    ticket: Uuid,
    cancel: CancellationToken,
}

impl Drop for AuthOperation {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.runtime
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active
            .remove(&self.ticket);
    }
}

pub(crate) struct RuntimeAuth {
    exec: ExecEngine,
    shutdown: CancellationToken,
    state: Mutex<State>,
}

impl Default for RuntimeAuth {
    fn default() -> Self {
        Self {
            exec: ExecEngine::default(),
            shutdown: CancellationToken::new(),
            state: Mutex::new(State::default()),
        }
    }
}

impl std::fmt::Debug for RuntimeAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeAuth").finish_non_exhaustive()
    }
}

impl Drop for RuntimeAuth {
    fn drop(&mut self) {
        self.close();
    }
}

fn invalid(code: &'static str) -> AppError {
    AppError::invalid("kubernetes_auth", code)
}

fn auth_error(error: AuthError) -> AppError {
    invalid(match error {
        AuthError::UnsupportedPlatform => "exec_not_supported_on_mobile",
        AuthError::ApprovalRequired | AuthError::CommandChanged => "exec_approval_required",
        AuthError::InteractiveRequired => "exec_interactive_input_required",
        AuthError::Cancelled | AuthError::Closed => "auth_cancelled",
        AuthError::Timeout => "auth_timeout",
        AuthError::ResponseTooLarge => "auth_response_too_large",
        AuthError::InvalidCallback => "oidc_invalid_callback",
        AuthError::InvalidToken => "oidc_invalid_identity",
        AuthError::Rejected => "auth_rejected",
        AuthError::Transport => "auth_transport_failed",
        AuthError::Process => "exec_failed",
        AuthError::InvalidConfiguration => "auth_configuration_invalid",
        AuthError::InvalidResponse => "auth_response_invalid",
    })
}

fn clone_auth(value: &Authentication) -> Authentication {
    match value {
        Authentication::Bearer(token) => Authentication::Bearer(token.clone()),
        Authentication::ClientCertificate {
            certificate_pem,
            private_key_pem,
        } => Authentication::ClientCertificate {
            certificate_pem: certificate_pem.clone(),
            private_key_pem: private_key_pem.clone(),
        },
    }
}

fn connection(unlocked: &Unlocked, id: ObjectId) -> AppResult<(KubeContext, KubeCluster, Stamp)> {
    let snapshot = unlocked.working().snapshot();
    let object = |id| match snapshot.get(&id).map(|entry| &entry.item) {
        Some(Item::Object(value)) => Some(value.as_ref()),
        _ => None,
    };
    let Some(VaultObject::KubeContext(context)) = object(id) else {
        return Err(invalid("context_unavailable"));
    };
    let Some(VaultObject::KubeCluster(cluster)) = object(context.cluster_id) else {
        return Err(invalid("cluster_unavailable"));
    };
    context.validate_cluster(cluster)?;
    let stamp = std::iter::once(context.id)
        .chain(std::iter::once(cluster.id))
        .chain(context.auth.secret_ids())
        .map(|id| {
            snapshot
                .get(&id)
                .map(|entry| (id, entry.revision))
                .ok_or_else(|| invalid("credential_unavailable"))
        })
        .collect::<AppResult<Stamp>>()?;
    Ok((context.clone(), cluster.clone(), stamp))
}

impl RuntimeAuth {
    pub(crate) fn close(&self) {
        self.shutdown.cancel();
        self.exec.close();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        state.pending.clear();
        state.active.clear();
        state.cache.clear();
        state.refresh.clear();
        state.approved.clear();
        state.gates.clear();
    }

    fn scope(&self, unlocked: &Unlocked, stamp: &Stamp) -> AppResult<CancellationToken> {
        let cancel = self.shutdown.child_token();
        if !unlocked
            .working()
            .watch_revisions(stamp.clone(), cancel.clone())
        {
            return Err(invalid("connection_changed"));
        }
        Ok(cancel)
    }

    fn current(
        &self,
        unlocked: &Unlocked,
        stamp: &Stamp,
        cancel: &CancellationToken,
    ) -> AppResult<()> {
        let snapshot = unlocked.working().snapshot();
        if self.shutdown.is_cancelled()
            || cancel.is_cancelled()
            || stamp.iter().any(|(id, revision)| {
                snapshot.get(id).map(|entry| entry.revision) != Some(*revision)
            })
        {
            return Err(invalid("connection_changed"));
        }
        Ok(())
    }

    fn insert_pending(self: &Arc<Self>, pending: Pending) -> AppResult<Uuid> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.pending.retain(|_, entry| {
            let keep = entry.expires > Instant::now() && !entry.cancel.is_cancelled();
            if !keep {
                entry.cancel.cancel();
            }
            keep
        });
        if state.closed || pending.cancel.is_cancelled() {
            return Err(invalid("auth_cancelled"));
        }
        if state.pending.len() + state.active.len() >= MAX_PENDING {
            return Err(invalid("too_many_auth_requests"));
        }
        let ticket = Uuid::new_v4();
        let weak = Arc::downgrade(self);
        let cancelled = pending.cancel.clone();
        state.pending.insert(ticket, pending);
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(LIFETIME) => {},
                _ = cancelled.cancelled() => {},
            }
            if let Some(runtime) = weak.upgrade() {
                runtime.discard(ticket);
            }
        });
        Ok(ticket)
    }

    fn discard(&self, ticket: Uuid) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pending) = state.pending.remove(&ticket) {
            pending.cancel.cancel();
        }
        if let Some(cancel) = state.active.remove(&ticket) {
            cancel.cancel();
        }
    }

    fn take(self: &Arc<Self>, ticket: Uuid) -> AppResult<(Pending, AuthOperation)> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let pending = state
            .pending
            .remove(&ticket)
            .ok_or_else(|| invalid("auth_request_expired"))?;
        if state.closed || pending.expires <= Instant::now() || pending.cancel.is_cancelled() {
            pending.cancel.cancel();
            return Err(invalid("auth_request_expired"));
        }
        state.active.insert(ticket, pending.cancel.clone());
        let operation = AuthOperation {
            runtime: self.clone(),
            ticket,
            cancel: pending.cancel.clone(),
        };
        Ok((pending, operation))
    }

    fn store(
        self: &Arc<Self>,
        unlocked: &Unlocked,
        id: ObjectId,
        stamp: Stamp,
        credential: Credential,
    ) -> AppResult<ResolvedAuth> {
        let cancel = self.scope(unlocked, &stamp)?;
        let expiry = credential
            .expires_at
            .duration_since(SystemTime::now())
            .map_err(|_| invalid("auth_expired"))?;
        let resolved = ResolvedAuth {
            authentication: clone_auth(&credential.authentication),
            expires_at: Some(credential.expires_at),
        };
        let ticket = Uuid::new_v4();
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.closed {
                return Err(invalid("auth_cancelled"));
            }
            while state.cache.len() >= MAX_CACHE && !state.cache.contains_key(&id) {
                if let Some((_, old)) = state.cache.pop_first() {
                    old.cancel.cancel();
                }
            }
            if let Some(old) = state.cache.insert(
                id,
                Cache {
                    ticket,
                    stamp,
                    cancel: cancel.clone(),
                    credential: Arc::new(credential),
                },
            ) {
                old.cancel.cancel();
            }
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(expiry) => {},
                _ = cancel.cancelled() => {},
            }
            cancel.cancel();
            if let Some(runtime) = weak.upgrade() {
                let mut state = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
                if state
                    .cache
                    .get(&id)
                    .is_some_and(|entry| entry.ticket == ticket)
                {
                    state.cache.remove(&id);
                }
            }
        });
        Ok(resolved)
    }

    fn store_oidc(
        self: &Arc<Self>,
        unlocked: &Unlocked,
        id: ObjectId,
        stamp: Stamp,
        client: OidcClient,
        tokens: TokenSet,
    ) -> AppResult<ResolvedAuth> {
        let credential = tokens.credential().map_err(auth_error)?;
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(old) = state.refresh.remove(&id) {
                old.cancel.cancel();
            }
        }
        if let Some(token) = &tokens.refresh_token {
            let cancel = self.scope(unlocked, &stamp)?;
            let ticket = Uuid::new_v4();
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.closed {
                return Err(invalid("auth_cancelled"));
            }
            while state.refresh.len() >= MAX_CACHE && !state.refresh.contains_key(&id) {
                if let Some((_, old)) = state.refresh.pop_first() {
                    old.cancel.cancel();
                }
            }
            if let Some(old) = state.refresh.insert(
                id,
                Refresh {
                    ticket,
                    stamp: stamp.clone(),
                    cancel: cancel.clone(),
                    client,
                    token: token.clone(),
                    identity: tokens.refresh_identity(),
                },
            ) {
                old.cancel.cancel();
            }
            let weak = Arc::downgrade(self);
            tokio::spawn(async move {
                cancel.cancelled().await;
                if let Some(runtime) = weak.upgrade() {
                    let mut state = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
                    if state
                        .refresh
                        .get(&id)
                        .is_some_and(|entry| entry.ticket == ticket)
                    {
                        state.refresh.remove(&id);
                    }
                }
            });
        }
        // Access tokens and the previous ID token are discarded here; only the
        // expiring Kubernetes bearer and minimal refresh identity are retained.
        self.store(unlocked, id, stamp, credential)
    }

    pub(crate) async fn resolve(
        self: &Arc<Self>,
        unlocked: &Unlocked,
        context: &KubeContext,
        cluster: &KubeCluster,
        cancellation: CancellationToken,
    ) -> AppResult<ResolvedAuth> {
        let (current, current_cluster, stamp) = connection(unlocked, context.id)?;
        if current != *context || current_cluster != *cluster {
            return Err(invalid("connection_changed"));
        }
        let gate = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.closed {
                return Err(invalid("auth_cancelled"));
            }
            if state.gates.len() >= MAX_CACHE && !state.gates.contains_key(&context.id) {
                state.gates.retain(|_, gate| Arc::strong_count(gate) > 1);
                if state.gates.len() >= MAX_CACHE {
                    return Err(invalid("too_many_auth_requests"));
                }
            }
            state.gates.entry(context.id).or_default().clone()
        };
        let work = async {
            let _gate = gate.lock().await;
            self.current(unlocked, &stamp, &cancellation)?;
            {
                let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(cached) = state.cache.get(&context.id).filter(|cached| {
                    cached.stamp == stamp
                        && !cached.cancel.is_cancelled()
                        && cached.credential.expires_at > SystemTime::now() + Duration::from_secs(5)
                }) {
                    return Ok(ResolvedAuth {
                        authentication: clone_auth(&cached.credential.authentication),
                        expires_at: Some(cached.credential.expires_at),
                    });
                }
            }
            match &context.auth {
                KubeAuth::Token { secret_id } => Ok(ResolvedAuth {
                    authentication: Authentication::Bearer(
                        read_secret(unlocked, *secret_id).await?,
                    ),
                    expires_at: None,
                }),
                KubeAuth::ClientCertificate {
                    certificate_pem,
                    key_secret_id,
                } => Ok(ResolvedAuth {
                    authentication: Authentication::ClientCertificate {
                        certificate_pem: certificate_pem.clone(),
                        private_key_pem: read_secret(unlocked, *key_secret_id).await?,
                    },
                    expires_at: None,
                }),
                KubeAuth::Exec { .. } => {
                    if !self
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .approved
                        .get(&context.id)
                        .is_some_and(|cancel| !cancel.is_cancelled())
                    {
                        return Err(invalid("exec_approval_required"));
                    }
                    let prepared = self
                        .exec
                        .prepare(exec_request(unlocked, context, cluster).await?)
                        .await
                        .map_err(auth_error)?;
                    let credential = self
                        .exec
                        .execute(prepared, cancellation.clone())
                        .await
                        .map_err(auth_error)?;
                    self.current(unlocked, &stamp, &cancellation)?;
                    self.store(unlocked, context.id, stamp, credential)
                }
                KubeAuth::Oidc {
                    refresh_secret_id, ..
                } => {
                    let remembered = self
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .refresh
                        .get(&context.id)
                        .filter(|entry| entry.stamp == stamp && !entry.cancel.is_cancelled())
                        .cloned();
                    let (client, token, identity) = match remembered {
                        Some(entry) => (entry.client, entry.token, Some(entry.identity)),
                        None => {
                            let id =
                                refresh_secret_id.ok_or_else(|| invalid("oidc_login_required"))?;
                            let client =
                                oidc_client(unlocked, &context.auth, cancellation.clone()).await?;
                            (client, read_secret(unlocked, id).await?, None)
                        }
                    };
                    let tokens = client
                        .refresh_with_identity(token, identity.as_ref(), cancellation.clone())
                        .await
                        .map_err(|error| {
                            if matches!(error, AuthError::Rejected | AuthError::InvalidToken) {
                                invalid("oidc_login_required")
                            } else {
                                auth_error(error)
                            }
                        })?;
                    self.current(unlocked, &stamp, &cancellation)?;
                    self.store_oidc(unlocked, context.id, stamp, client, tokens)
                }
                KubeAuth::None => Err(invalid("credentials_required")),
            }
        };
        tokio::select! {
            biased;
            _ = self.shutdown.cancelled() => Err(invalid("auth_cancelled")),
            _ = cancellation.cancelled() => Err(invalid("auth_cancelled")),
            result = work => result,
        }
    }
}

async fn read_secret(unlocked: &Unlocked, id: ObjectId) -> AppResult<SecretString> {
    Ok(SecretString::from(
        unlocked.writer.read_secret(id).await?.value.expose_secret(),
    ))
}

/// Finder-launched applications may lack Homebrew in PATH. Resolve these
/// standard installation locations without executing login-shell startup
/// files. This complete value is still displayed and hash-bound in consent.
pub(crate) fn default_exec_path() -> String {
    let inherited = std::env::var("PATH").unwrap_or_default();
    #[cfg(target_os = "macos")]
    {
        let mut paths: Vec<_> = std::env::split_paths(&inherited).collect();
        for path in ["/opt/homebrew/bin", "/usr/local/bin"] {
            let path = std::path::PathBuf::from(path);
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        std::env::join_paths(paths)
            .ok()
            .and_then(|value| value.into_string().ok())
            .unwrap_or(inherited)
    }
    #[cfg(not(target_os = "macos"))]
    {
        inherited
    }
}

async fn exec_request(
    unlocked: &Unlocked,
    context: &KubeContext,
    cluster: &KubeCluster,
) -> AppResult<ExecRequest> {
    let KubeAuth::Exec {
        command,
        args_secret_id,
        env,
        api_version,
        provide_cluster_info,
        interactive_mode,
    } = &context.auth
    else {
        return Err(invalid("not_exec_authentication"));
    };
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return Err(invalid("exec_not_supported_on_mobile"));
    }
    let encoded_args = read_secret(unlocked, *args_secret_id).await?;
    let args: Zeroizing<Vec<String>> = Zeroizing::new(
        serde_json::from_str(encoded_args.expose_secret())
            .map_err(|_| invalid("exec_arguments_invalid"))?,
    );
    // A deliberate allowlist excludes arbitrary application tokens, signing
    // keys and dynamic loader injection variables from the child environment.
    let mut environment = BTreeMap::new();
    for name in [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "USERPROFILE",
        "SYSTEMROOT",
        "WINDIR",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
        "AWS_PROFILE",
        "AWS_DEFAULT_PROFILE",
        "AWS_CONFIG_FILE",
        "AWS_SHARED_CREDENTIALS_FILE",
        "AWS_REGION",
        "AWS_DEFAULT_REGION",
        "AWS_SDK_LOAD_CONFIG",
        "CLOUDSDK_CONFIG",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "AZURE_CONFIG_DIR",
    ] {
        if let Ok(value) = std::env::var(name) {
            environment.insert(name.to_string(), SecretString::from(value));
        }
    }
    environment.insert("PATH".into(), SecretString::from(default_exec_path()));
    for (name, id) in env {
        // Imported variables are shown in the explicit command review and
        // included in the approval hash, including loader-related variables.
        environment.insert(name.clone(), read_secret(unlocked, *id).await?);
    }
    let path = environment
        .get("PATH")
        .map(ExposeSecret::expose_secret)
        .unwrap_or("");
    let binary = resolve_binary(command, path).map_err(auth_error)?;
    let cluster_info = provide_cluster_info.then(|| cc_k8s_core::auth::exec::cluster_info(cluster));
    Ok(ExecRequest {
        context_id: context
            .id
            .to_string()
            .parse()
            .map_err(|_| invalid("invalid_context"))?,
        binary,
        args: args
            .iter()
            .map(|value| SecretString::from(value.as_str()))
            .collect(),
        environment,
        api_version: api_version.clone(),
        interactive_mode: *interactive_mode,
        cluster_info,
    })
}

async fn oidc_client(
    unlocked: &Unlocked,
    auth: &KubeAuth,
    cancel: CancellationToken,
) -> AppResult<OidcClient> {
    let KubeAuth::Oidc {
        issuer,
        client_id,
        client_secret_id,
        certificate_authority_pem,
        scopes,
        ..
    } = auth
    else {
        return Err(invalid("not_oidc_authentication"));
    };
    let client_secret = match client_secret_id {
        Some(id) => Some(read_secret(unlocked, *id).await?),
        None => None,
    };
    OidcClient::discover(
        OidcConfig {
            issuer: Url::parse(issuer).map_err(|_| invalid("oidc_issuer_invalid"))?,
            client_id: client_id.clone(),
            client_secret,
            scopes: scopes.clone(),
            certificate_authority_pem: certificate_authority_pem.clone(),
        },
        cancel,
    )
    .await
    .map_err(auth_error)
}

impl AppCore {
    pub async fn kube_exec_review(&self, context_id: String) -> AppResult<KubeExecReview> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("context_id", &context_id)?;
        let (context, cluster, stamp) = connection(&unlocked, id)?;
        let auth = &unlocked.kube.auth;
        let cancel = auth.scope(&unlocked, &stamp)?;
        let prepared = auth
            .exec
            .prepare(exec_request(&unlocked, &context, &cluster).await?)
            .await
            .map_err(auth_error)?;
        let _owner = self.hold_session(&owner).await?;
        let _unlocked = owner.hold_unlocked(&unlocked).await?;
        auth.current(&unlocked, &stamp, &cancel)?;
        let mut review = KubeExecReview {
            ticket: String::new(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + LIFETIME.as_millis() as i64,
            context_name: context.name,
            binary_path: prepared.binary_path().to_string_lossy().into_owned(),
            binary_sha256: prepared.binary_sha256(),
            invocation_sha256: prepared.invocation_sha256(),
            arguments: prepared
                .arguments_for_consent()
                .iter()
                .map(|v| v.expose_secret().to_owned())
                .collect(),
            environment: prepared
                .environment_for_consent()
                .iter()
                .map(|(k, v)| (k.clone(), v.expose_secret().to_owned()))
                .collect(),
        };
        review.ticket = auth
            .insert_pending(Pending {
                context_id: id,
                stamp,
                cancel,
                expires: Instant::now() + LIFETIME,
                kind: PendingKind::Exec(Box::new(prepared)),
            })?
            .to_string();
        Ok(review)
    }

    pub async fn kube_exec_approve(&self, ticket: String) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let auth = &unlocked.kube.auth;
        let (pending, _operation) =
            auth.take(ticket.parse().map_err(|_| invalid("invalid_auth_ticket"))?)?;
        let PendingKind::Exec(prepared) = pending.kind else {
            return Err(invalid("wrong_auth_request"));
        };
        let _owner = self.hold_session(&owner).await?;
        let _unlocked = owner.hold_unlocked(&unlocked).await?;
        auth.current(&unlocked, &pending.stamp, &pending.cancel)?;
        auth.exec.approve(&prepared).map_err(auth_error)?;
        let approved_cancel = auth.scope(&unlocked, &pending.stamp)?;
        if let Some(old) = auth
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .approved
            .insert(pending.context_id, approved_cancel.clone())
        {
            old.cancel();
        }
        let weak = Arc::downgrade(auth);
        let cancel = approved_cancel;
        let context = pending.context_id;
        let exec_context = prepared.context_id();
        tokio::spawn(async move {
            cancel.cancelled().await;
            if let Some(auth) = weak.upgrade() {
                let mut state = auth.state.lock().unwrap_or_else(|e| e.into_inner());
                if state
                    .approved
                    .get(&context)
                    .is_some_and(CancellationToken::is_cancelled)
                {
                    state.approved.remove(&context);
                    auth.exec.revoke(exec_context);
                }
            }
        });
        Ok(())
    }

    pub async fn kube_oidc_begin(&self, context_id: String) -> AppResult<KubeOidcChallenge> {
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("context_id", &context_id)?;
        let (context, _, stamp) = connection(&unlocked, id)?;
        let auth = &unlocked.kube.auth;
        let cancel = auth.scope(&unlocked, &stamp)?;
        let client = oidc_client(&unlocked, &context.auth, cancel.clone()).await?;
        let listener = if cfg!(any(target_os = "android", target_os = "ios")) {
            None
        } else {
            Some(
                TcpListener::bind("127.0.0.1:0")
                    .await
                    .map_err(|_| invalid("oidc_callback_unavailable"))?,
            )
        };
        let redirect = match &listener {
            Some(listener) => Url::parse(&format!(
                "http://127.0.0.1:{}/oauth/{}",
                listener
                    .local_addr()
                    .map_err(|_| invalid("oidc_callback_unavailable"))?
                    .port(),
                Uuid::new_v4()
            )),
            None => Url::parse("consolecrypt://kubernetes/oauth/callback"),
        }
        .map_err(|_| invalid("oidc_callback_unavailable"))?;
        let flow = client.begin(redirect).map_err(auth_error)?;
        let mut challenge = KubeOidcChallenge {
            ticket: String::new(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + LIFETIME.as_millis() as i64,
            authorization_url: flow.authorization_url().to_string(),
            redirect_uri: flow.redirect_uri().to_string(),
            loopback: listener.is_some(),
        };
        let _owner = self.hold_session(&owner).await?;
        let _unlocked = owner.hold_unlocked(&unlocked).await?;
        auth.current(&unlocked, &stamp, &cancel)?;
        challenge.ticket = auth
            .insert_pending(Pending {
                context_id: id,
                stamp,
                cancel,
                expires: Instant::now() + LIFETIME,
                kind: PendingKind::Oidc(Box::new(PendingOidc {
                    client,
                    flow,
                    listener,
                })),
            })?
            .to_string();
        Ok(challenge)
    }

    pub async fn kube_oidc_complete(
        &self,
        ticket: String,
        callback_url: Option<String>,
    ) -> AppResult<()> {
        let (owner, unlocked) = self.unlocked().await?;
        let auth = &unlocked.kube.auth;
        let (pending, _operation) =
            auth.take(ticket.parse().map_err(|_| invalid("invalid_auth_ticket"))?)?;
        let PendingKind::Oidc(pending_oidc) = pending.kind else {
            return Err(invalid("wrong_auth_request"));
        };
        let PendingOidc {
            client,
            flow,
            listener,
        } = *pending_oidc;
        auth.current(&unlocked, &pending.stamp, &pending.cancel)?;
        let callback = match (listener, callback_url) {
            (Some(listener), None) => {
                loopback_callback(listener, &flow, pending.cancel.clone(), pending.expires).await?
            }
            (None, Some(callback)) => {
                let callback = Zeroizing::new(callback);
                Url::parse(&callback).map_err(|_| invalid("oidc_invalid_callback"))?
            }
            _ => return Err(invalid("oidc_invalid_callback")),
        };
        let tokens = flow
            .finish(&callback, pending.cancel.clone())
            .await
            .map_err(auth_error)?;
        let _owner = self.hold_session(&owner).await?;
        let _unlocked = owner.hold_unlocked(&unlocked).await?;
        auth.current(&unlocked, &pending.stamp, &pending.cancel)?;
        auth.store_oidc(&unlocked, pending.context_id, pending.stamp, client, tokens)?;
        pending.cancel.cancel();
        Ok(())
    }

    pub async fn kube_auth_cancel(&self, ticket: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        unlocked
            .kube
            .auth
            .discard(ticket.parse().map_err(|_| invalid("invalid_auth_ticket"))?);
        Ok(())
    }
}

async fn loopback_callback(
    listener: TcpListener,
    flow: &LoginFlow,
    cancel: CancellationToken,
    expires: Instant,
) -> AppResult<Url> {
    let work = async {
        loop {
            let (mut stream, peer) = listener
                .accept()
                .await
                .map_err(|_| invalid("oidc_callback_unavailable"))?;
            if !peer.ip().is_loopback() {
                continue;
            }
            let request = tokio::time::timeout(Duration::from_secs(5), async {
                let mut bytes = Zeroizing::new(Vec::new());
                let mut byte = [0u8; 1];
                while bytes.len() < 32 * 1024 && !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).await?;
                    bytes.push(byte[0]);
                }
                Ok::<_, std::io::Error>(bytes)
            })
            .await;
            let callback = request.ok().and_then(Result::ok).and_then(|bytes| {
                let header = std::str::from_utf8(&bytes).ok()?;
                if !header.ends_with("\r\n\r\n") {
                    return None;
                }
                let mut words = header.lines().next()?.split_whitespace();
                if words.next()? != "GET" {
                    return None;
                }
                let path = words.next()?;
                if !path.starts_with('/') || path.starts_with("//") {
                    return None;
                }
                let expected_host = format!("127.0.0.1:{}", flow.redirect_uri().port()?);
                let hosts: Vec<_> = header
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
                    .collect();
                if hosts.len() != 1 || hosts[0].1.trim() != expected_host {
                    return None;
                }
                let callback = flow.redirect_uri().join(path).ok()?;
                flow.validate_callback(&callback).ok()?;
                Some(callback)
            });
            let response = if callback.is_some() {
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 27\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\nReturn to ConsoleCrypt now."
            } else {
                "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
            };
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                stream.write_all(response.as_bytes()),
            )
            .await;
            if let Some(callback) = callback {
                return Ok(callback);
            }
        }
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(invalid("auth_cancelled")),
        result = tokio::time::timeout(expires.saturating_duration_since(Instant::now()),work) =>
            result.map_err(|_|invalid("auth_timeout"))?,
    }
}

#[cfg(all(test, unix, not(any(target_os = "android", target_os = "ios"))))]
#[path = "kubernetes_auth_tests.rs"]
mod tests;
