use http::{Method, Request, Uri};
use std::{
    collections::HashSet,
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use tower::{BoxError, Layer, Service};
use uuid::Uuid;

const WRITE_WINDOW: Duration = Duration::from_secs(10 * 60);
const FORWARD_CONFIRMATION: Duration = Duration::from_secs(30);
type WriteLease = (CancellationToken, Instant);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Desktop,
    Mobile,
}

/// A fresh guard is created for each unlocked context session. These settings
/// cannot be changed by an HTTP request or by a local CLI.
#[derive(Debug, Clone)]
pub struct Policy {
    pub context_id: Uuid,
    pub context_name: String,
    pub production: bool,
    pub device: DeviceClass,
    pub allow_exec_in_production: bool,
    pub allow_port_forward_in_production: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Read,
    Mutation,
    Exec,
    PortForward,
    AccessReview,
}

/// No request body, query string, headers or credentials are included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub context_id: Uuid,
    pub method: Method,
    pub path: String,
}

/// The production writer must persist a record before the request is sent.
/// A storage failure closes the write path rather than losing the audit.
pub trait Audit: Send + Sync {
    fn record(&self, entry: AuditEntry) -> Result<(), BoxError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("Kubernetes session is locked or closed")]
    Closed,
    #[error("The Kubernetes request path is unsupported or ambiguous")]
    UnsupportedPath,
    #[error("This HTTP method is not supported")]
    UnsupportedMethod,
    #[error("This production context is read-only")]
    ReadOnly,
    #[error("Production exec is disabled")]
    ExecDisabled,
    #[error("Production port forwarding is disabled")]
    PortForwardDisabled,
    #[error("Port forwarding requires a fresh confirmation")]
    ConfirmationRequired,
    #[error("The context name does not match")]
    ContextNameMismatch,
    #[error("A production write window is unavailable on mobile")]
    MobileWriteWindow,
    #[error("An audit record could not be saved")]
    AuditUnavailable,
    #[error("Production contexts require TLS certificate verification")]
    InsecureProductionTls,
}

#[derive(Debug)]
struct State {
    active: bool,
    write_until: Option<Instant>,
    write_cancel: CancellationToken,
    confirmations: HashSet<Uuid>,
}

struct Inner {
    policy: Policy,
    audit: Arc<dyn Audit>,
    state: Mutex<State>,
    cancel: CancellationToken,
}

/// Clones share the write window and revocation state, including the guard
/// installed in a local CLI proxy. The state lock covers audit + dispatch.
#[derive(Clone)]
pub struct ProdGuard(Arc<Inner>);

impl fmt::Debug for ProdGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProdGuard")
            .field("context_id", &self.0.policy.context_id)
            .field("production", &self.0.policy.production)
            .finish_non_exhaustive()
    }
}

/// This capability is private, short-lived, single-use and bound to one exact
/// request and session. Query parameters (including forwarded ports) are bound
/// but never printed. It is not serialized into the CLI's HTTP request.
#[derive(Clone)]
pub struct ForwardConfirmation {
    id: Uuid,
    owner: ProdGuard,
    method: Method,
    uri: Uri,
    expires: Instant,
}

impl fmt::Debug for ForwardConfirmation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ForwardConfirmation([redacted])")
    }
}

impl ProdGuard {
    pub fn new(
        policy: Policy,
        insecure_skip_tls_verify: bool,
        audit: Arc<dyn Audit>,
    ) -> Result<Self, PolicyError> {
        if policy.production && insecure_skip_tls_verify {
            return Err(PolicyError::InsecureProductionTls);
        }
        Ok(Self(Arc::new(Inner {
            policy,
            audit,
            state: Mutex::new(State {
                active: true,
                write_until: None,
                write_cancel: CancellationToken::new(),
                confirmations: HashSet::new(),
            }),
            cancel: CancellationToken::new(),
        })))
    }

    /// Call only after the user submits the context name in the confirmation.
    /// Mobile contexts never gain a write window, regardless of UI state.
    pub fn open_write_window(&self, typed_context_name: &str) -> Result<Instant, PolicyError> {
        let mut state = self.0.state.lock().map_err(|_| PolicyError::Closed)?;
        ensure_active(&state)?;
        if self.0.policy.device == DeviceClass::Mobile {
            return Err(PolicyError::MobileWriteWindow);
        }
        if typed_context_name != self.0.policy.context_name {
            return Err(PolicyError::ContextNameMismatch);
        }
        let expires = Instant::now() + WRITE_WINDOW;
        state.write_cancel.cancel();
        state.write_cancel = CancellationToken::new();
        state.write_until = Some(expires);
        Ok(expires)
    }

    pub fn write_window_remaining(&self) -> Option<Duration> {
        let state = self.0.state.lock().ok()?;
        state
            .active
            .then_some(state.write_until)?
            .and_then(|until| until.checked_duration_since(Instant::now()))
    }

    pub fn close_write_window(&self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.write_cancel.cancel();
            state.write_until = None;
        }
    }

    /// Revoke on lock, context switch, logout or loss of the vault scope.
    /// A closed guard can never be reactivated; reconnect creates a fresh one.
    pub fn close(&self) {
        self.0.cancel.cancel();
        if let Ok(mut state) = self.0.state.lock() {
            state.active = false;
            state.write_cancel.cancel();
            state.write_until = None;
            state.confirmations.clear();
        }
    }

    /// Call after the user approves a specific port-forward action.
    pub fn confirm_port_forward(
        &self,
        method: Method,
        uri: Uri,
    ) -> Result<ForwardConfirmation, PolicyError> {
        let mut state = self.0.state.lock().map_err(|_| PolicyError::Closed)?;
        ensure_active(&state)?;
        if classify(&method, &uri)? != Operation::PortForward {
            return Err(PolicyError::UnsupportedPath);
        }
        if self.0.policy.production && !self.0.policy.allow_port_forward_in_production {
            return Err(PolicyError::PortForwardDisabled);
        }
        // Bound abandoned confirmations; callers can retry after confirming.
        if state.confirmations.len() >= 64 {
            state.confirmations.clear();
        }
        let id = Uuid::new_v4();
        state.confirmations.insert(id);
        Ok(ForwardConfirmation {
            id,
            owner: self.clone(),
            method,
            uri,
            expires: Instant::now() + FORWARD_CONFIRMATION,
        })
    }

    fn dispatch<B, R>(
        &self,
        request: Request<B>,
        send: impl FnOnce(Request<B>, Option<WriteLease>) -> R,
    ) -> Result<R, PolicyError> {
        let mut state = self.0.state.lock().map_err(|_| PolicyError::Closed)?;
        ensure_active(&state)?;
        let operation = classify(request.method(), request.uri())?;
        let production = self.0.policy.production;
        match operation {
            Operation::Exec if production && !self.0.policy.allow_exec_in_production => {
                return Err(PolicyError::ExecDisabled);
            }
            Operation::PortForward => {
                if production && !self.0.policy.allow_port_forward_in_production {
                    return Err(PolicyError::PortForwardDisabled);
                }
                let valid = request
                    .extensions()
                    .get::<ForwardConfirmation>()
                    .is_some_and(|confirmation| {
                        Arc::ptr_eq(&self.0, &confirmation.owner.0)
                            && confirmation.method == *request.method()
                            && confirmation.uri == *request.uri()
                            && Instant::now() < confirmation.expires
                            && state.confirmations.remove(&confirmation.id)
                    });
                if !valid {
                    return Err(PolicyError::ConfirmationRequired);
                }
            }
            Operation::Mutation if production => {
                if self.0.policy.device == DeviceClass::Mobile
                    || !state
                        .write_until
                        .is_some_and(|until| Instant::now() < until)
                {
                    return Err(PolicyError::ReadOnly);
                }
                self.0
                    .audit
                    .record(AuditEntry {
                        context_id: self.0.policy.context_id,
                        method: request.method().clone(),
                        path: request.uri().path().to_owned(),
                    })
                    .map_err(|_| PolicyError::AuditUnavailable)?;
                if !state
                    .write_until
                    .is_some_and(|until| Instant::now() < until)
                {
                    return Err(PolicyError::ReadOnly);
                }
            }
            _ => {}
        }
        // Revocation and dispatch are serialized. The middleware also cancels
        // pending futures; the session owner closes established streams.
        if self.0.cancel.is_cancelled() {
            return Err(PolicyError::Closed);
        }
        let write_lease = if production && operation == Operation::Mutation {
            state
                .write_until
                .map(|until| (state.write_cancel.clone(), until))
        } else {
            None
        };
        Ok(send(request, write_lease))
    }
}

fn ensure_active(state: &State) -> Result<(), PolicyError> {
    if state.active {
        Ok(())
    } else {
        Err(PolicyError::Closed)
    }
}

/// Classify the actual wire method and path. Encoded separators, dot segments,
/// duplicate slashes, proxy routes and unknown streaming subresources fail
/// closed. Exec and port-forward are not ordinary reads even when using GET.
pub fn classify(method: &Method, uri: &Uri) -> Result<Operation, PolicyError> {
    // Install this layer before the fixed API-server base-URI layer. Callers
    // cannot choose an origin through an absolute URL or CONNECT authority.
    if uri.scheme().is_some() || uri.authority().is_some() {
        return Err(PolicyError::UnsupportedPath);
    }
    let read = method == Method::GET || method == Method::HEAD;
    if !read
        && !matches!(
            *method,
            Method::POST | Method::PUT | Method::PATCH | Method::DELETE
        )
    {
        return Err(PolicyError::UnsupportedMethod);
    }
    let path = uri.path();
    if !path.starts_with('/')
        || path.len() > 8192
        || path.contains("//")
        || path
            .as_bytes()
            .iter()
            .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, b'/' | b'.' | b'-' | b'_' | b':'))
    {
        return Err(PolicyError::UnsupportedPath);
    }
    let segments: Vec<_> = path.trim_start_matches('/').split('/').collect();
    if segments
        .iter()
        .any(|s| *s == "." || *s == ".." || s.is_empty())
    {
        return Err(PolicyError::UnsupportedPath);
    }
    if read
        && (matches!(
            path,
            "/api" | "/apis" | "/version" | "/healthz" | "/livez" | "/readyz"
        ) || segments[0] == "openapi")
    {
        return Ok(Operation::Read);
    }
    let (group, rest) = match segments.as_slice() {
        ["api", "v1", rest @ ..] => ("", rest),
        ["apis", group, _version, rest @ ..] => (*group, rest),
        ["apis", _group] if read => return Ok(Operation::Read),
        _ => return Err(PolicyError::UnsupportedPath),
    };
    if rest.is_empty() {
        return if read {
            Ok(Operation::Read)
        } else {
            Err(PolicyError::UnsupportedPath)
        };
    }
    // Legacy /api/v1/proxy/... and /api/v1/watch/... do not use the ordinary
    // resource grammar. Watches use ?watch=true on the resource endpoint.
    if matches!(rest[0], "proxy" | "watch") {
        return Err(PolicyError::UnsupportedPath);
    }
    let rest = match rest {
        ["namespaces", _namespace, tail @ ..]
            if !tail.is_empty() && !matches!(tail[0], "status" | "finalize") =>
        {
            tail
        }
        _ => rest,
    };
    let (resource, subresource) = match rest {
        [resource] | [resource, _] => (*resource, None),
        [resource, _, subresource] => (*resource, Some(*subresource)),
        _ => return Err(PolicyError::UnsupportedPath),
    };
    if group.is_empty() && resource == "pods" {
        match subresource {
            Some("exec" | "attach") if method == Method::GET || method == Method::POST => {
                return Ok(Operation::Exec);
            }
            Some("portforward") if method == Method::GET || method == Method::POST => {
                return Ok(Operation::PortForward);
            }
            _ => {}
        }
    }
    // These virtual reviews only report the caller's permissions; creation
    // persists no object and cannot authorize an action.
    if group == "authorization.k8s.io"
        && matches!(
            resource,
            "selfsubjectaccessreviews" | "selfsubjectrulesreviews"
        )
        && rest.len() == 1
        && method == Method::POST
    {
        return Ok(Operation::AccessReview);
    }
    if subresource == Some("proxy") {
        return Err(PolicyError::UnsupportedPath);
    }
    if read {
        match subresource {
            None | Some("status" | "scale") => Ok(Operation::Read),
            Some("log") if group.is_empty() && resource == "pods" => Ok(Operation::Read),
            _ => Err(PolicyError::UnsupportedPath),
        }
    } else {
        Ok(Operation::Mutation)
    }
}

#[derive(Debug, Clone)]
pub struct GuardLayer(pub ProdGuard);

impl<S> Layer<S> for GuardLayer {
    type Service = GuardedService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GuardedService {
            inner,
            guard: self.0.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GuardedService<S> {
    inner: S,
    guard: ProdGuard,
}

impl<S, B> Service<Request<B>> for GuardedService<S>
where
    S: Service<Request<B>> + Send,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
    S::Response: Send + 'static,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let session_cancel = self.guard.0.cancel.clone();
        match self
            .guard
            .dispatch(request, |request, lease| (self.inner.call(request), lease))
        {
            Ok((future, write_lease)) => Box::pin(async move {
                let write_revoked = async {
                    if let Some((cancel, until)) = write_lease {
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => {}
                            _ = tokio::time::sleep_until(until.into()) => {}
                        }
                    } else {
                        std::future::pending::<()>().await;
                    }
                };
                tokio::select! {
                    biased;
                    _ = session_cancel.cancelled() =>
                        Err(Box::new(PolicyError::Closed) as BoxError),
                    _ = write_revoked =>
                        Err(Box::new(PolicyError::ReadOnly) as BoxError),
                    result = future => result.map_err(Into::into),
                }
            }),
            Err(error) => Box::pin(async move { Err(Box::new(error) as BoxError) }),
        }
    }
}

#[cfg(test)]
mod tests;
