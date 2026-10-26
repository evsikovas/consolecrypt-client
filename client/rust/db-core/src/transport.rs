//! Explicit native transports. No ambient proxy, credential file or default
//! connection string is consulted. TLS always uses the original server identity.
use crate::DbError;
use async_trait::async_trait;
use cc_models::database::{DbConnection, DbTlsMode};
use rustls::{
    client::{
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        WebPkiServerVerifier,
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use secrecy::{ExposeSecret, SecretString};
use std::{collections::HashSet, fmt, io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

pub trait DbStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> DbStream for T {}
pub type DbIo = Box<dyn DbStream>;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DbDestination {
    pub host: String,
    pub port: u16,
}
impl DbDestination {
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, DbError> {
        let host = host.into();
        if port == 0
            || host.is_empty()
            || host.len() > 253
            || host.chars().any(char::is_whitespace)
            || host.contains(['/', '\\', '@', '?', '#', '%', '[', ']'])
            || ServerName::try_from(host.clone()).is_err()
        {
            return Err(DbError::InvalidRequest);
        }
        Ok(Self { host, port })
    }
}
impl fmt::Debug for DbDestination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DbDestination([REDACTED])")
    }
}

/// How one socket reaches the database before any database TLS. Only a
/// protected path may carry a PostgreSQL "prefer" session without TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbPath {
    /// A direct or proxied socket to a non-loopback address.
    Network,
    /// A direct socket whose resolved peer is a loopback address.
    Loopback,
    /// Inside an SSH session, a saved tunnel, a Kubernetes or Docker forward.
    Tunnel,
}
impl DbPath {
    pub fn protected(self) -> bool {
        self != Self::Network
    }
}

/// AppCore implements this with the immutable vault route snapshot. Every
/// discovered replica must use that same connector; there is no direct fallback.
#[async_trait]
pub trait DbConnector: Send + Sync + fmt::Debug {
    async fn connect(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<DbIo, DbError>;

    /// Every socket travels inside an SSH session or a local forward.
    fn tunnelled(&self) -> bool {
        false
    }

    /// The socket and the path it took. Unknown connectors are the network.
    async fn connect_path(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<(DbIo, DbPath), DbError> {
        let path = if self.tunnelled() {
            DbPath::Tunnel
        } else {
            DbPath::Network
        };
        Ok((self.connect(destination, cancel).await?, path))
    }
}

#[derive(Debug)]
pub struct DirectConnector;
#[async_trait]
impl DbConnector for DirectConnector {
    async fn connect(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<DbIo, DbError> {
        Ok(self.connect_path(destination, cancel).await?.0)
    }

    /// Loopback is decided by the connected peer, never by the host string.
    async fn connect_path(
        &self,
        destination: &DbDestination,
        cancel: CancellationToken,
    ) -> Result<(DbIo, DbPath), DbError> {
        DbDestination::new(destination.host.clone(), destination.port)?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(DbError::Cancelled),
            result = TcpStream::connect((destination.host.as_str(), destination.port)) => {
                let stream = result.map_err(|_| DbError::ConnectionFailed)?;
                stream.set_nodelay(true).map_err(|_| DbError::ConnectionFailed)?;
                let peer = stream.peer_addr().map_err(|_| DbError::ConnectionFailed)?;
                let path = if peer.ip().to_canonical().is_loopback() {
                    DbPath::Loopback
                } else {
                    DbPath::Network
                };
                Ok((Box::new(stream) as DbIo, path))
            }
        }
    }
}

#[derive(Default)]
pub struct DbCredentials {
    pub password: Option<SecretString>,
    pub client_key_pem: Option<SecretString>,
}
impl fmt::Debug for DbCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DbCredentials([REDACTED])")
    }
}

#[derive(Clone)]
pub(crate) struct TlsProfile {
    pub config: Arc<ClientConfig>,
    pub name: String,
    pub mode: DbTlsMode,
    /// The configured host; the server-name override applies to it only.
    host: String,
}
impl fmt::Debug for TlsProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsProfile")
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}
impl TlsProfile {
    /// The connection's pinned CA, or the bundled web PKI roots.
    pub fn roots(connection: &DbConnection) -> Result<RootCertStore, DbError> {
        let mut roots = RootCertStore::empty();
        if let Some(ca) = &connection.tls.certificate_authority_pem {
            for certificate in rustls_pemfile::certs(&mut io::Cursor::new(ca)) {
                roots
                    .add(certificate.map_err(|_| DbError::TlsFailed)?)
                    .map_err(|_| DbError::TlsFailed)?;
            }
            if roots.is_empty() {
                return Err(DbError::TlsFailed);
            }
        } else {
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        }
        Ok(roots)
    }

    pub fn new(connection: &DbConnection, credentials: &DbCredentials) -> Result<Self, DbError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let roots = Self::roots(connection)?;
        let builder = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|_| DbError::TlsFailed)?
            .with_root_certificates(roots);
        let mut config = if let Some(cert) = &connection.auth.client_certificate {
            let certificates = rustls_pemfile::certs(&mut io::Cursor::new(&cert.certificate_pem))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| DbError::TlsFailed)?;
            let private = credentials
                .client_key_pem
                .as_ref()
                .ok_or(DbError::AuthenticationFailed)?;
            let key = rustls_pemfile::private_key(&mut io::Cursor::new(private.expose_secret()))
                .map_err(|_| DbError::TlsFailed)?
                .ok_or(DbError::TlsFailed)?;
            builder
                .with_client_auth_cert(certificates, key)
                .map_err(|_| DbError::TlsFailed)?
        } else {
            builder.with_no_client_auth()
        };
        if connection.tls.mode == DbTlsMode::Require {
            // Explicit encryption-only mode. Signatures still verify; only the
            // chain/name check is omitted. Verify remains the inventory default.
            config
                .dangerous()
                .set_certificate_verifier(Arc::new(EncryptionOnly(provider)));
        }
        let name = connection
            .tls
            .server_name
            .clone()
            .unwrap_or_else(|| connection.host.clone());
        ServerName::try_from(name.clone()).map_err(|_| DbError::TlsFailed)?;
        Ok(Self {
            config: Arc::new(config),
            name,
            mode: connection.tls.mode,
            host: connection.host.clone(),
        })
    }

    /// The configuration for a driver that dials cluster nodes itself
    /// (ScyllaDB shard-aware ports, FEAT-13). The driver names each node by
    /// its address, so a verified certificate is accepted when it is valid
    /// for the configured server name or for that address.
    pub fn for_nodes(&self, connection: &DbConnection) -> Result<Arc<ClientConfig>, DbError> {
        if !matches!(self.mode, DbTlsMode::Prefer | DbTlsMode::Verify) {
            // Require already skips the chain and name check.
            return Ok(self.config.clone());
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let inner = WebPkiServerVerifier::builder_with_provider(
            Arc::new(Self::roots(connection)?),
            provider,
        )
        .build()
        .map_err(|_| DbError::TlsFailed)?;
        let name = ServerName::try_from(self.name.clone()).map_err(|_| DbError::TlsFailed)?;
        let mut config = (*self.config).clone();
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(NamedOrAddress { inner, name }));
        Ok(Arc::new(config))
    }

    pub async fn connect(&self, stream: DbIo, name: Option<&str>) -> Result<DbIo, DbError> {
        self.connect_checked(stream, name)
            .await
            .map_err(|_| DbError::TlsFailed)
    }

    /// Like [`Self::connect`], but tells a server without TLS on that port
    /// ([`DbError::TlsUnavailable`]) apart from a failed verification.
    pub async fn connect_checked(&self, stream: DbIo, name: Option<&str>) -> Result<DbIo, DbError> {
        if self.mode == DbTlsMode::Disable {
            return Ok(stream);
        }
        // Protocols without TLS negotiation never downgrade after a failed
        // handshake, including Prefer. No password is retried over plaintext.
        // Other hosts (discovered replica-set members) verify their own name;
        // the configured host keeps its explicit server-name override.
        let name = match name {
            Some(name) if !name.eq_ignore_ascii_case(&self.host) => name,
            _ => &self.name,
        };
        let name = ServerName::try_from(name.to_owned()).map_err(|_| DbError::TlsFailed)?;
        tokio_rustls::TlsConnector::from(self.config.clone())
            .connect(name, stream)
            .await
            .map(|stream| Box::new(stream) as DbIo)
            .map_err(classify_handshake)
    }
}

/// A server that answers a ClientHello with something that is not TLS (or
/// just hangs up) does not offer TLS on that port; anything else, such as a
/// certificate the profile rejects, is a TLS failure.
pub(crate) fn classify_handshake(error: io::Error) -> DbError {
    if let Some(inner) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        return match inner {
            rustls::Error::InvalidMessage(_) => DbError::TlsUnavailable,
            _ => DbError::TlsFailed,
        };
    }
    match error.kind() {
        io::ErrorKind::UnexpectedEof
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::InvalidData => DbError::TlsUnavailable,
        _ => DbError::TlsFailed,
    }
}

/// Full verification against the configured server name, else against the
/// name the driver gave (a node's address).
#[derive(Debug)]
struct NamedOrAddress {
    inner: Arc<WebPkiServerVerifier>,
    name: ServerName<'static>,
}
impl ServerCertVerifier for NamedOrAddress {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.inner
            .verify_server_cert(end_entity, intermediates, &self.name, ocsp_response, now)
            .or_else(|_| {
                self.inner.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    ocsp_response,
                    now,
                )
            })
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner
            .verify_tls12_signature(message, certificate, signature)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner
            .verify_tls13_signature(message, certificate, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[derive(Debug)]
struct EncryptionOnly(Arc<rustls::crypto::CryptoProvider>);
impl ServerCertVerifier for EncryptionOnly {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Owns a loopback listener and every accepted stream. Dropping or cancelling
/// it aborts all pumps. No listener survives the unlocked native session.
pub(crate) struct LocalRelay {
    pub address: SocketAddr,
    cancel: CancellationToken,
    worker: tokio::task::JoinHandle<()>,
}
impl fmt::Debug for LocalRelay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LocalRelay([SCOPED])")
    }
}
impl Drop for LocalRelay {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.worker.abort();
    }
}

impl LocalRelay {
    /// mysql_async owns its socket creation. Relay only this one immutable
    /// destination while its own rustls connector retains the original name.
    pub async fn tcp(
        connector: Arc<dyn DbConnector>,
        destination: DbDestination,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        Self::start(connector, Arc::new(vec![destination]), None, None, scope).await
    }

    /// The CQL driver dials only this listener. Each accepted socket goes to
    /// the first destination that answers, in order, through `tls` when set.
    pub async fn failover(
        connector: Arc<dyn DbConnector>,
        destinations: Vec<DbDestination>,
        tls: Option<TlsProfile>,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        Self::start(connector, Arc::new(destinations), None, tls, scope).await
    }

    async fn start(
        connector: Arc<dyn DbConnector>,
        destinations: Arc<Vec<DbDestination>>,
        authentication: Option<Arc<SocksCredentials>>,
        tls: Option<TlsProfile>,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        if destinations.is_empty() || destinations.len() > 64 || scope.is_cancelled() {
            return Err(DbError::InvalidRequest);
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| DbError::ConnectionFailed)?;
        let address = listener
            .local_addr()
            .map_err(|_| DbError::ConnectionFailed)?;
        let cancel = scope.child_token();
        let token = cancel.clone();
        let worker = tokio::spawn(async move {
            let capacity = Arc::new(Semaphore::new(16));
            let mut children = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => break,
                    Some(_) = children.join_next(), if !children.is_empty() => {},
                    accepted = listener.accept() => {
                        let Ok((mut local, peer)) = accepted else { break };
                        if !peer.ip().is_loopback() { continue }
                        let Ok(permit) = capacity.clone().try_acquire_owned() else { continue };
                        let connector = connector.clone();
                        let destinations = destinations.clone();
                        let authentication = authentication.clone();
                        let tls = tls.clone();
                        let cancel = token.clone();
                        children.spawn(async move {
                            let _permit = permit;
                            let operation = async {
                                let candidates = if let Some(auth) = &authentication {
                                    vec![socks_request(&mut local, auth, &destinations).await?]
                                } else {
                                    destinations.to_vec()
                                };
                                let mut opened = Err(DbError::InvalidRequest);
                                for destination in &candidates {
                                    opened = async {
                                        let remote = connector.connect(destination, cancel.clone()).await?;
                                        match &tls {
                                            Some(tls) => tls.connect(remote, Some(&destination.host)).await,
                                            None => Ok(remote),
                                        }
                                    }
                                    .await;
                                    if opened.is_ok() {
                                        break;
                                    }
                                }
                                let remote = opened?;
                                if authentication.is_some() {
                                    // Do not announce success before upstream TLS verifies.
                                    local.write_all(&[5,0,0,1,127,0,0,1,0,0]).await.map_err(|_| DbError::ConnectionFailed)?;
                                }
                                Ok::<_, DbError>(remote)
                            };
                            let remote = tokio::select! {
                                biased;
                                _ = cancel.cancelled() => return,
                                result = tokio::time::timeout(Duration::from_secs(10), operation) => result,
                            };
                            let Ok(Ok(mut remote)) = remote else { return };
                            tokio::select! {
                                biased;
                                _ = cancel.cancelled() => {},
                                _ = tokio::io::copy_bidirectional(&mut local, &mut remote) => {},
                            }
                        });
                    }
                }
            }
            children.abort_all();
            while children.join_next().await.is_some() {}
        });
        Ok(Self {
            address,
            cancel,
            worker,
        })
    }
}

pub(crate) struct SocksCredentials {
    pub username: SecretString,
    pub password: SecretString,
}
/// Loopback SOCKS5 endpoint for the MongoDB driver: random per-session
/// credentials, only the listed destinations, TLS performed by the relay.
pub(crate) struct MongoRelay {
    pub relay: LocalRelay,
    pub authentication: Arc<SocksCredentials>,
}
impl fmt::Debug for MongoRelay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MongoRelay([SCOPED])")
    }
}
impl MongoRelay {
    pub async fn new(
        connector: Arc<dyn DbConnector>,
        destinations: HashSet<DbDestination>,
        tls: TlsProfile,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        let authentication = Arc::new(SocksCredentials {
            username: SecretString::from(Uuid::new_v4().simple().to_string()),
            password: SecretString::from(format!(
                "{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            )),
        });
        let relay = LocalRelay::start(
            connector,
            Arc::new(destinations.into_iter().collect()),
            Some(authentication.clone()),
            Some(tls),
            scope,
        )
        .await?;
        Ok(Self {
            relay,
            authentication,
        })
    }
}

async fn socks_request(
    stream: &mut TcpStream,
    credentials: &SocksCredentials,
    destinations: &[DbDestination],
) -> Result<DbDestination, DbError> {
    async fn byte(stream: &mut TcpStream) -> Result<u8, DbError> {
        stream
            .read_u8()
            .await
            .map_err(|_| DbError::ConnectionFailed)
    }
    if byte(stream).await? != 5 {
        return Err(DbError::AuthenticationFailed);
    }
    let count = byte(stream).await? as usize;
    if count == 0 {
        return Err(DbError::AuthenticationFailed);
    }
    let mut methods = vec![0; count];
    stream
        .read_exact(&mut methods)
        .await
        .map_err(|_| DbError::ConnectionFailed)?;
    if !methods.contains(&2) {
        return Err(DbError::AuthenticationFailed);
    }
    stream
        .write_all(&[5, 2])
        .await
        .map_err(|_| DbError::ConnectionFailed)?;
    if byte(stream).await? != 1 {
        return Err(DbError::AuthenticationFailed);
    }
    let mut user = Zeroizing::new(vec![0; byte(stream).await? as usize]);
    stream
        .read_exact(&mut user)
        .await
        .map_err(|_| DbError::ConnectionFailed)?;
    let mut password = Zeroizing::new(vec![0; byte(stream).await? as usize]);
    stream
        .read_exact(&mut password)
        .await
        .map_err(|_| DbError::ConnectionFailed)?;
    // Random per-session secrets; compare every byte without early exit.
    fn matches(actual: &[u8], expected: &[u8]) -> bool {
        let mut difference = actual.len() ^ expected.len();
        for index in 0..actual.len().max(expected.len()) {
            difference |= usize::from(
                actual.get(index).copied().unwrap_or(0) ^ expected.get(index).copied().unwrap_or(0),
            );
        }
        difference == 0
    }
    if !matches(&user, credentials.username.expose_secret().as_bytes())
        || !matches(&password, credentials.password.expose_secret().as_bytes())
    {
        return Err(DbError::AuthenticationFailed);
    }
    stream
        .write_all(&[1, 0])
        .await
        .map_err(|_| DbError::ConnectionFailed)?;
    if byte(stream).await? != 5 || byte(stream).await? != 1 || byte(stream).await? != 0 {
        return Err(DbError::PolicyDenied);
    }
    let host = match byte(stream).await? {
        1 => {
            let mut bytes = [0; 4];
            stream
                .read_exact(&mut bytes)
                .await
                .map_err(|_| DbError::ConnectionFailed)?;
            std::net::Ipv4Addr::from(bytes).to_string()
        }
        4 => {
            let mut bytes = [0; 16];
            stream
                .read_exact(&mut bytes)
                .await
                .map_err(|_| DbError::ConnectionFailed)?;
            std::net::Ipv6Addr::from(bytes).to_string()
        }
        3 => {
            let length = byte(stream).await? as usize;
            let mut bytes = vec![0; length];
            stream
                .read_exact(&mut bytes)
                .await
                .map_err(|_| DbError::ConnectionFailed)?;
            String::from_utf8(bytes).map_err(|_| DbError::InvalidRequest)?
        }
        _ => return Err(DbError::InvalidRequest),
    };
    let port = stream
        .read_u16()
        .await
        .map_err(|_| DbError::ConnectionFailed)?;
    let destination = DbDestination::new(host, port)?;
    if !destinations.contains(&destination) {
        return Err(DbError::PolicyDenied);
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::database::DbEngine;

    /// A CA and a server certificate for `names` signed by it.
    fn certificates(names: &[&str]) -> (String, rustls::ServerConfig) {
        let mut authority = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        authority.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let authority_key = rcgen::KeyPair::generate().unwrap();
        let authority_pem = authority.self_signed(&authority_key).unwrap().pem();
        let issuer = rcgen::Issuer::new(authority, authority_key);
        let key = rcgen::KeyPair::generate().unwrap();
        let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        let certificate = rcgen::CertificateParams::new(names)
            .unwrap()
            .signed_by(&key, &issuer)
            .unwrap();
        let server = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
        )
        .unwrap();
        (authority_pem, server)
    }

    /// Whether a node at `address` with a certificate for `names` passes
    /// the node configuration of a connection to `host`.
    async fn accepted(host: &str, names: &[&str], address: &str) -> bool {
        let (authority, server) = certificates(names);
        let mut connection = DbConnection::new("t", host, DbEngine::Cassandra);
        connection.tls.mode = DbTlsMode::Verify;
        connection.tls.certificate_authority_pem = Some(authority);
        let profile = TlsProfile::new(&connection, &DbCredentials::default()).unwrap();
        let config = profile.for_nodes(&connection).unwrap();
        let (client, server_io) = tokio::io::duplex(64 * 1024);
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
        let serving = tokio::spawn(async move { acceptor.accept(server_io).await.is_ok() });
        let name = ServerName::IpAddress(address.parse::<std::net::IpAddr>().unwrap().into());
        let ok = tokio_rustls::TlsConnector::from(config)
            .connect(name, client)
            .await
            .is_ok();
        let _ = serving.await;
        ok
    }

    #[tokio::test]
    async fn nodes_pass_by_the_configured_name_or_their_address() {
        // The driver names a node by its address (FEAT-13).
        assert!(accepted("db.example", &["db.example"], "10.0.0.5").await);
        assert!(accepted("db.example", &["10.0.0.5"], "10.0.0.5").await);
        assert!(!accepted("db.example", &["other.example"], "10.0.0.5").await);
        assert!(!accepted("db.example", &["10.0.0.6"], "10.0.0.5").await);
    }
}
