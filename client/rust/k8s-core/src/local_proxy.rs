//! A revocable, loopback-only HTTPS endpoint for explicitly launched desktop
//! CLIs. Its generated kubeconfig contains only a temporary local capability.
use crate::{
    policy::{classify, PolicyError},
    session::KubernetesSession,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, Limited};
use hyper::{body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use kube::Client;
use secrecy::{ExposeSecret, SecretString};
use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("The local Kubernetes endpoint could not be initialized")]
    Unavailable,
    #[error("The Kubernetes session is closed")]
    Closed,
}

pub struct LocalApiProxy {
    address: SocketAddr,
    certificate_pem: String,
    token: SecretString,
    cancel: CancellationToken,
    task: tokio::task::AbortHandle,
}
impl std::fmt::Debug for LocalApiProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalApiProxy")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}
impl Drop for LocalApiProxy {
    fn drop(&mut self) {
        self.close();
    }
}
impl LocalApiProxy {
    pub async fn start(session: &KubernetesSession) -> Result<Self, ProxyError> {
        let cancel = session.guard().cancellation().child_token();
        if cancel.is_cancelled() {
            return Err(ProxyError::Closed);
        }
        let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()])
            .map_err(|_| ProxyError::Unavailable)?;
        let certificate_pem = cert.cert.pem();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())
                    .into(),
            )
            .map_err(|_| ProxyError::Unavailable)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| ProxyError::Unavailable)?;
        let address = listener.local_addr().map_err(|_| ProxyError::Unavailable)?;
        let token = SecretString::from(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        let context = Arc::new(ProxyContext {
            client: session.client(),
            token: token.clone(),
            authority: address.to_string(),
            cancel: cancel.clone(),
        });
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let task = tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(32));
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = context.cancel.cancelled() => break,
                    Some(_) = connections.join_next(), if !connections.is_empty() => {},
                    accepted = listener.accept() => {
                        let Ok((stream, peer)) = accepted else { break; };
                        if !peer.ip().is_loopback() { continue; }
                        let Ok(slot) = slots.clone().try_acquire_owned() else { continue; };
                        let slot = Arc::new(slot);
                        let acceptor = acceptor.clone();
                        let context = context.clone();
                        connections.spawn(async move {
                            let cancel = context.cancel.clone();
                            let run = async move {
                                let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await else { return; };
                                let service = service_fn(move |request| relay(context.clone(), slot.clone(), request));
                                let _ = http1::Builder::new().max_buf_size(64*1024)
                                    .serve_connection(TokioIo::new(stream), service).with_upgrades().await;
                            };
                            tokio::select! { biased; _ = cancel.cancelled() => {}, _ = run => {} }
                        });
                    }
                }
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        if cancel.is_cancelled() {
            task.abort();
            return Err(ProxyError::Closed);
        }
        Ok(Self {
            address,
            certificate_pem,
            token,
            cancel,
            task: task.abort_handle(),
        })
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn cancellation(&self) -> CancellationToken {
        self.cancel.clone()
    }
    pub fn close(&self) {
        self.cancel.cancel();
        self.task.abort();
    }
    pub fn kubeconfig(&self, namespace: &str) -> Result<Zeroizing<Vec<u8>>, ProxyError> {
        if self.cancel.is_cancelled() {
            return Err(ProxyError::Closed);
        }
        // JSON is also a valid kubeconfig; serializing escapes namespace data.
        let mut config = serde_json::json!({
            "apiVersion":"v1","kind":"Config","current-context":"consolecrypt",
            "clusters":[{"name":"consolecrypt","cluster":{
                "server":format!("https://{}",self.address),
                "certificate-authority-data":STANDARD.encode(self.certificate_pem.as_bytes())
            }}],
            "users":[{"name":"consolecrypt","user":{"token":self.token.expose_secret()}}],
            "contexts":[{"name":"consolecrypt","context":{"cluster":"consolecrypt","user":"consolecrypt","namespace":namespace}}]
        });
        let bytes = serde_json::to_vec(&config)
            .map(Zeroizing::new)
            .map_err(|_| ProxyError::Unavailable);
        if let serde_json::Value::String(mut token) = config["users"][0]["user"]["token"].take() {
            token.zeroize();
        }
        bytes
    }
}

struct ProxyContext {
    client: Client,
    token: SecretString,
    authority: String,
    cancel: CancellationToken,
}

fn reply(status: StatusCode, message: &'static str) -> Response<ProxyBody> {
    let body = serde_json::json!({"apiVersion":"v1","kind":"Status","status":"Failure","code":status.as_u16(),"message":message}).to_string();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(
            Full::new(Bytes::from(body))
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .expect("static response")
}

async fn relay(
    context: Arc<ProxyContext>,
    slot: Arc<OwnedSemaphorePermit>,
    mut incoming: Request<Incoming>,
) -> Result<Response<ProxyBody>, Infallible> {
    if context.cancel.is_cancelled() {
        return Ok(reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "The ConsoleCrypt session is closed",
        ));
    }
    let authorized = incoming.headers().get_all("authorization").iter().count() == 1
        && incoming
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|value| {
                value
                    .as_bytes()
                    .ct_eq(context.token.expose_secret().as_bytes())
                    .into()
            });
    if !authorized {
        return Ok(reply(
            StatusCode::UNAUTHORIZED,
            "The local session token is invalid",
        ));
    }
    if incoming.uri().scheme().is_some()
        || incoming.uri().authority().is_some()
        || incoming.headers().get_all("host").iter().count() != 1
        || incoming.headers().get("host").and_then(|v| v.to_str().ok()) != Some(&context.authority)
        || incoming.headers().contains_key("origin")
        || classify(incoming.method(), incoming.uri()).is_err()
    {
        return Ok(reply(
            StatusCode::BAD_REQUEST,
            "The local Kubernetes request is invalid",
        ));
    }
    let websocket = incoming
        .headers()
        .get("upgrade")
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"websocket"));
    if incoming.headers().contains_key("upgrade") && !websocket {
        return Ok(reply(
            StatusCode::BAD_REQUEST,
            "Use a CLI with Kubernetes WebSocket support",
        ));
    }
    let downstream_upgrade = websocket.then(|| hyper::upgrade::on(&mut incoming));
    let (parts, body) = incoming.into_parts();
    let body = tokio::select! {
        biased;
        _ = context.cancel.cancelled() => return Ok(reply(StatusCode::SERVICE_UNAVAILABLE, "The ConsoleCrypt session is closed")),
        result = tokio::time::timeout(Duration::from_secs(15), Limited::new(body, 2*1024*1024).collect()) => match result {
            Ok(Ok(body)) => body.to_bytes(),
            _ => return Ok(reply(StatusCode::PAYLOAD_TOO_LARGE, "The request body exceeds the local limit or timed out")),
        }
    };
    let mut request = Request::builder().method(parts.method).uri(parts.uri);
    // Authentication, Host, cookies, proxy and impersonation headers from the
    // CLI are never forwarded. kube-client supplies the real native identity.
    for name in ["accept", "content-type", "content-encoding", "user-agent"] {
        if let Some(value) = parts.headers.get(name) {
            request = request.header(name, value);
        }
    }
    if websocket {
        for name in [
            "upgrade",
            "connection",
            "sec-websocket-key",
            "sec-websocket-version",
            "sec-websocket-protocol",
        ] {
            if let Some(value) = parts.headers.get(name) {
                request = request.header(name, value);
            }
        }
    }
    let Ok(request) = request.body(kube::client::Body::from(body)) else {
        return Ok(reply(
            StatusCode::BAD_REQUEST,
            "The local Kubernetes request is invalid",
        ));
    };
    let mut upstream = tokio::select! {
        biased;
        _ = context.cancel.cancelled() => return Ok(reply(StatusCode::SERVICE_UNAVAILABLE, "The ConsoleCrypt session is closed")),
        result = tokio::time::timeout(Duration::from_secs(30), context.client.send(request)) => match result {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                let mut source:Option<&(dyn std::error::Error+'static)> = Some(&error);
                let mut policy = false;
                while let Some(error) = source {
                    policy |= error.downcast_ref::<PolicyError>().is_some();
                    source = error.source();
                }
                return Ok(reply(if policy { StatusCode::FORBIDDEN } else { StatusCode::BAD_GATEWAY },
                    if policy { "ConsoleCrypt blocked this request. Check the production policy, write window and audit storage." }
                    else { "The Kubernetes API request failed" }));
            },
            Err(_) => return Ok(reply(StatusCode::GATEWAY_TIMEOUT, "The Kubernetes API request timed out")),
        }
    };
    if upstream.status() == StatusCode::SWITCHING_PROTOCOLS {
        let Some(downstream) = downstream_upgrade else {
            return Ok(reply(
                StatusCode::BAD_GATEWAY,
                "An unexpected protocol upgrade was rejected",
            ));
        };
        let upstream_upgrade = hyper::upgrade::on(&mut upstream);
        tokio::spawn(async move {
            let _slot = slot;
            let transfer = async {
                let (Ok(remote), Ok(local)) = tokio::join!(upstream_upgrade, downstream) else {
                    return;
                };
                let _ = tokio::io::copy_bidirectional(
                    &mut TokioIo::new(local),
                    &mut TokioIo::new(remote),
                )
                .await;
            };
            tokio::select! { biased; _ = context.cancel.cancelled() => {}, _ = transfer => {} }
        });
    }
    let (mut parts, body) = upstream.into_parts();
    parts.extensions.clear();
    let mut headers = http::HeaderMap::new();
    for (name, value) in &parts.headers {
        if matches!(
            name.as_str(),
            "content-type"
                | "content-length"
                | "content-encoding"
                | "warning"
                | "upgrade"
                | "connection"
                | "sec-websocket-accept"
                | "sec-websocket-protocol"
        ) {
            headers.append(name.clone(), value.clone());
        }
    }
    parts.headers = headers;
    parts
        .headers
        .insert("cache-control", http::HeaderValue::from_static("no-store"));
    Ok(Response::from_parts(
        parts,
        body.map_err(|error| Box::new(error) as BoxError)
            .boxed_unsync(),
    ))
}
