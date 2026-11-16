//! Rust-only fixed-source stream adapter. No listener, proxy, URL rewrite or
//! credential resolver. SSH protects the socket; HTTPS still verifies the
//! ORIGINAL URL identity with the same saved custom CA policy.
use crate::{Error, ErrorCode, Route, SourceConfig};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper_util::rt::TokioIo;
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub trait MonitoringStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> MonitoringStream for T {}
pub type MonitoringIo = Box<dyn MonitoringStream>;
pub type MonitoringConnect<'a> =
    Pin<Box<dyn Future<Output = Result<MonitoringIo, Error>> + Send + 'a>>;

/// Implemented only by the original Native saved-host scope. Not Serde/FRB or
/// an authorization token: its exact source must match the client, and Native
/// must provide original-scope, revocation and mutation fences.
pub trait MonitoringSourceConnector: fmt::Debug + Send + Sync {
    fn source(&self) -> &SourceConfig;
    /// Cheap scope/token/generation checks only, called by actual stream IO.
    fn is_current(&self) -> bool;
    fn connect(&self) -> MonitoringConnect<'_>;
}

pub(crate) struct ScopedHttp {
    connector: Arc<dyn MonitoringSourceConnector>,
    source: SourceConfig,
    tls: Option<Arc<rustls::ClientConfig>>,
}
fn failed() -> Error {
    Error::new(
        ErrorCode::Transport,
        "Scoped monitoring transport unavailable",
    )
}
struct BoundIo {
    inner: MonitoringIo,
    connector: Arc<dyn MonitoringSourceConnector>,
}
impl BoundIo {
    fn check(&self) -> std::io::Result<()> {
        if self.connector.is_current() {
            Ok(())
        } else {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
    }
}
impl AsyncRead for BoundIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Err(e) = self.check() {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.inner).poll_read(cx, out)
    }
}
impl AsyncWrite for BoundIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        value: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Err(e) = self.check() {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.inner).poll_write(cx, value)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if let Err(e) = self.check() {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
struct ConnectionTask(tokio::task::JoinHandle<()>);
impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Exactly two fixed capability requests on one original Agent stream.
/// This private value cannot reconnect or accept a caller URL/query.
pub(crate) struct ProbeSession {
    sender: hyper::client::conn::http1::SendRequest<Empty<Bytes>>,
    connection: ConnectionTask,
    connector: Arc<dyn MonitoringSourceConnector>,
    base: url::Url,
    end: std::time::Instant,
    data_end: std::time::Instant,
    remaining: usize,
    timeout_seconds: u64,
    limit: usize,
    sent: u8,
    reusable: bool,
}
impl ProbeSession {
    pub(crate) async fn next_fixed(&mut self, authorization: Option<&reqwest::header::HeaderValue>)
        -> Result<(Vec<u8>, u16), Error> {
        let end = tokio::time::Instant::from_std(self.data_end);
        tokio::time::timeout_at(end, self.next_fixed_before(authorization)).await
            .map_err(|_| Error::new(ErrorCode::Timeout, "Monitoring probe timed out"))?
    }
    async fn next_fixed_before(&mut self, authorization: Option<&reqwest::header::HeaderValue>)
        -> Result<(Vec<u8>, u16), Error> {
        if self.sent >= 2 || !self.reusable || self.sender.is_closed()
            || !self.connector.is_current() { return Err(failed()); }
        if std::time::Instant::now() >= self.data_end {
            return Err(Error::new(ErrorCode::Timeout,"Monitoring probe timed out"));
        }
        if self.remaining == 0 {
            return Err(Error::new(ErrorCode::ResponseTooLarge, "Monitoring response exceeds byte budget"));
        }
        let range = self.sent == 1;
        self.sent += 1; // A failed dispatch cannot be retried as another request.
        let mut url = self.base.join(if range {"api/v1/query_range"} else {"api/v1/query"})
            .map_err(|_| failed())?;
        {
            let mut fields = url.query_pairs_mut();
            fields.append_pair("query", "vector(1)");
            if range {
                fields.append_pair("start", "0").append_pair("end", "1").append_pair("step", "1");
            } else { fields.append_pair("time", "1"); }
            fields.append_pair("timeout", &format!("{}s", self.timeout_seconds))
                .append_pair("limit", &self.limit.to_string());
        }
        let authority = &url[url::Position::BeforeHost..url::Position::AfterPort];
        let mut frame = hyper::Request::builder().method(hyper::Method::GET)
            .uri(&url[url::Position::BeforePath..]).header(hyper::header::HOST, authority)
            .header(hyper::header::CONNECTION, if range {"close"} else {"keep-alive"});
        if let Some(header) = authorization { frame=frame.header(hyper::header::AUTHORIZATION,header); }
        // The SAME HTTP/1 dispatcher must be ready again after its prior body.
        // Waiting here stays inside next_fixed's original data deadline.
        if self.sender.ready().await.is_err() {self.reusable=false;return Err(failed());}
        if !self.connector.is_current() {self.reusable=false;return Err(failed());}
        if std::time::Instant::now()>=self.data_end {
            self.reusable=false;return Err(Error::new(ErrorCode::Timeout,"Monitoring probe timed out"));
        }
        let mut response=match self.sender.send_request(frame.body(Empty::<Bytes>::new()).map_err(|_|failed())?).await {
            Ok(response)=>response,Err(_)=>{self.reusable=false;return Err(failed());}
        };
        let status=response.status().as_u16();
        self.reusable=!range && response.version()==hyper::Version::HTTP_11
            && !response.headers().get_all(hyper::header::CONNECTION).iter().any(|value|
                value.to_str().map(|text|text.split(',').any(|v|v.trim().eq_ignore_ascii_case("close"))).unwrap_or(true));
        if response.status().is_redirection() {
            self.reusable=false;
            return Err(Error::new(ErrorCode::Transport,"Monitoring redirects are disabled").with_http(status));
        }
        if response.headers().get(hyper::header::CONTENT_LENGTH).is_some_and(|value|
            value.to_str().ok().and_then(|text|text.parse::<u64>().ok()).is_none_or(|n|n>self.remaining as u64)) {
            self.reusable=false;
            return Err(Error::new(ErrorCode::ResponseTooLarge,"Monitoring response exceeds byte budget").with_http(status));
        }
        let mut bytes=Vec::new();
        while let Some(frame)=response.body_mut().frame().await {
            let frame=match frame {Ok(frame)=>frame,Err(_)=>{self.reusable=false;return Err(failed());}};
            if let Some(data)=frame.data_ref() {
                if data.len()>self.remaining {
                    self.reusable=false;
                    return Err(Error::new(ErrorCode::ResponseTooLarge,"Monitoring response exceeds byte budget").with_http(status));
                }
                self.remaining-=data.len();bytes.extend_from_slice(data);
            }
        }
        if !self.connector.is_current() {
            self.reusable=false;return Err(failed());
        }
        if std::time::Instant::now()>=self.data_end {
            self.reusable=false;return Err(Error::new(ErrorCode::Timeout,"Monitoring probe timed out"));
        }
        Ok((bytes,status))
    }
    pub(crate) async fn close_join(&mut self) -> Result<(),Error> {
        self.reusable=false;
        self.connection.0.abort();
        match tokio::time::timeout_at(tokio::time::Instant::from_std(self.end),&mut self.connection.0).await {
            Ok(Ok(()))=>Ok(()),
            Ok(Err(error)) if error.is_cancelled()=>Ok(()),
            _=>Err(failed()),
        }
    }
}

impl ScopedHttp {
    pub(crate) fn new(
        source: &SourceConfig,
        connector: Arc<dyn MonitoringSourceConnector>,
    ) -> Result<Self, Error> {
        let endpoint = source.validate()?;
        if !matches!(source.route, Route::SshHost { .. } | Route::AgentTunnel { .. } | Route::AgentDestination { .. }) || source != connector.source() {
            return Err(Error::new(
                ErrorCode::UnsupportedRoute,
                "Original scoped source adapter required",
            ));
        }
        let tls = if endpoint.scheme() == "https" {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let Some(pem) = &source.tls.custom_ca_pem {
                let certificates = CertificateDer::pem_slice_iter(pem.as_bytes())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| failed())?;
                if certificates.is_empty() {
                    return Err(failed());
                }
                for certificate in certificates {
                    roots.add(certificate).map_err(|_| failed())?;
                }
            }
            let config = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|_| failed())?
            .with_root_certificates(roots)
            .with_no_client_auth();
            Some(Arc::new(config))
        } else {
            None
        };
        Ok(Self {
            connector,
            source: source.clone(),
            tls,
        })
    }

    pub(crate) async fn probe_session(&self, budgets: &crate::Budgets, end: std::time::Instant)
        -> Result<ProbeSession,Error> {
        // Reserve cleanup inside the SAME absolute deadline; never add time to it.
        let data_end=end.checked_sub(std::time::Duration::from_millis(100)).ok_or_else(failed)?;
        if !matches!(self.source.route,Route::AgentDestination{..}|Route::AgentTunnel{..})
            || !self.connector.is_current() || std::time::Instant::now()>=data_end {return Err(failed());}
        let open=async {
        let base=self.source.validate()?;
        let io=BoundIo{inner:self.connector.connect().await?,connector:self.connector.clone()};
        if !self.connector.is_current() {return Err(failed());}
        let io:MonitoringIo=if let Some(tls)=&self.tls {
            let host=match base.host().ok_or_else(failed)? {
                url::Host::Ipv6(ip)=>ip.to_string(),host=>host.to_string(),
            };
            let name=ServerName::try_from(host).map_err(|_|failed())?;
            Box::new(tokio_rustls::TlsConnector::from(tls.clone()).connect(name,io).await
                .map_err(|_|Error::new(ErrorCode::Transport,"Monitoring TLS verification failed"))?)
        }else{Box::new(io)};
        let(sender,connection)=hyper::client::conn::http1::Builder::new().max_buf_size(32*1024)
            .handshake(TokioIo::new(io)).await.map_err(|_|failed())?;
        Ok(ProbeSession{sender,connection:ConnectionTask(tokio::spawn(async move{let _=connection.await;})),
            connector:self.connector.clone(),base,end,data_end,remaining:budgets.max_response_bytes,
            timeout_seconds:budgets.timeout_seconds,limit:budgets.max_series+1,sent:0,reusable:true})
        };
        tokio::time::timeout_at(tokio::time::Instant::from_std(data_end),open).await
            .map_err(|_|Error::new(ErrorCode::Timeout,"Monitoring probe timed out"))?
    }

    pub(crate) async fn get(
        &self,
        request: reqwest::Request,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, u16), Error> {
        let base = self.source.validate()?;
        let url = request.url();
        if request.method() != reqwest::Method::GET
            || url.origin() != base.origin()
            || !self.connector.is_current()
        {
            return Err(failed());
        }
        let suffix = url.path().strip_prefix(base.path()).ok_or_else(failed)?;
        let fixed = matches!(
            suffix,
            "api/v1/query" | "api/v1/query_range" | "api/v1/alerts" | "api/v1/rules"
        ) || suffix
            .strip_prefix("api/v1/label/")
            .and_then(|v| v.strip_suffix("/values"))
            .is_some_and(|label| {
                !label.is_empty()
                    && label.len() <= 128
                    && label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            });
        if !fixed || request.body().is_some() {
            return Err(failed());
        }
        let io = BoundIo {
            inner: self.connector.connect().await?,
            connector: self.connector.clone(),
        };
        if !self.connector.is_current() {
            return Err(failed());
        }
        let io: MonitoringIo = if let Some(tls) = &self.tls {
            let host = match url.host().ok_or_else(failed)? {
                url::Host::Ipv6(ip) => ip.to_string(),
                host => host.to_string(),
            };
            let name = ServerName::try_from(host).map_err(|_| failed())?;
            Box::new(
                tokio_rustls::TlsConnector::from(tls.clone())
                    .connect(name, io)
                    .await
                    .map_err(|_| {
                        Error::new(ErrorCode::Transport, "Monitoring TLS verification failed")
                    })?,
            )
        } else {
            Box::new(io)
        };
        let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
            .max_buf_size(32 * 1024)
            .handshake(TokioIo::new(io))
            .await
            .map_err(|_| failed())?;
        // Held through body consumption. Future cancellation aborts the actual
        // per-request HTTP driver; there is no pooled/detached route listener.
        let _connection = ConnectionTask(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let authority = &url[url::Position::BeforeHost..url::Position::AfterPort];
        let mut frame = hyper::Request::builder()
            .method(hyper::Method::GET)
            .uri(&url[url::Position::BeforePath..])
            .header(hyper::header::HOST, authority)
            .header(hyper::header::CONNECTION, "close");
        if let Some(authorization) = request.headers().get(reqwest::header::AUTHORIZATION) {
            frame = frame.header(hyper::header::AUTHORIZATION, authorization);
        }
        if !self.connector.is_current() {
            return Err(failed());
        }
        let mut response = sender
            .send_request(frame.body(Empty::<Bytes>::new()).map_err(|_| failed())?)
            .await
            .map_err(|_| failed())?;
        let status = response.status().as_u16();
        if response.status().is_redirection() {
            return Err(
                Error::new(ErrorCode::Transport, "Monitoring redirects are disabled")
                    .with_http(status),
            );
        }
        if response
            .headers()
            .get(hyper::header::CONTENT_LENGTH)
            .is_some_and(|v| {
                v.to_str()
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .is_none_or(|n| n > max_bytes as u64)
            })
        {
            return Err(Error::new(
                ErrorCode::ResponseTooLarge,
                "Monitoring response exceeds byte budget",
            )
            .with_http(status));
        }
        let mut bytes = Vec::new();
        while let Some(frame) = response.body_mut().frame().await {
            let frame = frame.map_err(|_| failed())?;
            if let Some(data) = frame.data_ref() {
                if data.len() > max_bytes.saturating_sub(bytes.len()) {
                    return Err(Error::new(
                        ErrorCode::ResponseTooLarge,
                        "Monitoring response exceeds byte budget",
                    )
                    .with_http(status));
                }
                bytes.extend_from_slice(data);
            }
        }
        if !self.connector.is_current() {
            return Err(failed());
        }
        Ok((bytes, status))
    }
}
