use crate::{
    postgres_value,
    streaming::RowConsumer,
    transport::{DbConnector, DbCredentials, DbDestination, DbIo, TlsProfile},
    DbColumn, DbError, DbParameter, DbTransportSecurity,
};
use cc_models::database::{DbConnection, DbOptions, DbTlsMode};
use futures::TryStreamExt;
use secrecy::ExposeSecret;
use std::{
    fmt, io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio_postgres::{
    config::{ChannelBinding, SslMode},
    tls::{MakeTlsConnect, TlsConnect},
    types::ToSql,
    Client,
};
use tokio_util::sync::CancellationToken;

pub(crate) struct Postgres {
    client: Client,
    worker: tokio::task::JoinHandle<()>,
    connector: Arc<dyn DbConnector>,
    destination: DbDestination,
    tls: TlsProfile,
    timeout: Duration,
    pub version: String,
    pub security: DbTransportSecurity,
}
impl fmt::Debug for Postgres {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Postgres([SCOPED])")
    }
}
impl Drop for Postgres {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl Postgres {
    pub async fn connect(
        connection: &DbConnection,
        credentials: &DbCredentials,
        connector: Arc<dyn DbConnector>,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        let destination = DbDestination::new(&connection.host, connection.port)?;
        let tls = TlsProfile::new(connection, credentials)?;
        let mut configuration = tokio_postgres::Config::new();
        configuration
            .host(&connection.host)
            .port(connection.port)
            .user(&connection.username);
        if !connection.database.is_empty() {
            configuration.dbname(&connection.database);
        }
        if let Some(password) = &credentials.password {
            configuration.password(password.expose_secret());
        }
        if let DbOptions::Postgresql {
            application_name, ..
        } = &connection.options
        {
            configuration.application_name(application_name);
        }
        let timeout = Duration::from_secs(u64::from(connection.limits.statement_timeout_seconds));
        configuration.options(format!(
            "-c default_transaction_read_only={} -c statement_timeout={} -c idle_in_transaction_session_timeout={} -c standard_conforming_strings=on",
            if connection.production { "on" } else { "off" }, timeout.as_millis(), timeout.as_millis()));
        let (stream, path) = connector.connect_path(&destination, scope.clone()).await?;
        let mode = connection.tls.mode;
        // "prefer" continues without TLS only inside a tunnel or on this
        // computer. Over the network it is "require": a refused TLS request
        // fails instead of sending the password in plaintext.
        configuration.ssl_mode(match mode {
            DbTlsMode::Disable => SslMode::Disable,
            DbTlsMode::Prefer if path.protected() => SslMode::Prefer,
            DbTlsMode::Prefer | DbTlsMode::Require | DbTlsMode::Verify => SslMode::Require,
        });
        // An unverified certificate may belong to an interceptor: the
        // password is used only through SCRAM-SHA-256-PLUS bound to this TLS
        // session, so no cleartext, md5 or unbound SCRAM exchange is answered.
        let bound = mode == DbTlsMode::Require
            && credentials.password.is_some()
            && !connection.tls.allow_unbound_auth;
        configuration.channel_binding(if bound {
            ChannelBinding::Require
        } else {
            ChannelBinding::Prefer
        });
        let refused = Arc::new(AtomicBool::new(false));
        let socket = Socket {
            io: stream,
            guard: (mode == DbTlsMode::Disable && !path.protected())
                .then(|| CleartextGuard::new(refused.clone())),
        };
        let encrypted = Arc::new(AtomicBool::new(false));
        let mut factory = tokio_postgres_rustls::MakeRustlsConnect::new((*tls.config).clone());
        let tls_connect = Tracked {
            inner: MakeTlsConnect::<Socket>::make_tls_connect(&mut factory, &tls.name)
                .map_err(|_| DbError::TlsFailed)?,
            encrypted: encrypted.clone(),
        };
        // Handshake on its own task: owned 'static inputs keep the caller's
        // future Send for every lifetime (rust-lang/rust#102211).
        let configuration = configuration.clone();
        let (client, future) =
            tokio::spawn(async move { configuration.connect_raw(socket, tls_connect).await })
                .await
                .map_err(|_| DbError::ConnectionFailed)?
                .map_err(|error| {
                    if refused.load(Ordering::SeqCst) {
                        DbError::CleartextRefused
                    } else {
                        classify(error)
                    }
                })?;
        let worker = tokio::spawn(async move {
            tokio::select! {
                biased; _ = scope.cancelled() => {}, _ = future => {},
            }
        });
        let security = DbTransportSecurity::of(mode, !encrypted.load(Ordering::SeqCst), path);
        let version = match client.query_one("SHOW server_version", &[]).await {
            Ok(row) => row
                .try_get::<_, String>(0)
                .map_err(|_| DbError::QueryFailed)?,
            Err(error) => {
                worker.abort();
                return Err(classify(error));
            }
        };
        Ok(Self {
            client,
            worker,
            connector,
            destination,
            tls,
            timeout,
            version,
            security,
        })
    }

    // Boxed with one explicit lifetime: see the note on Mysql::query (rust-lang/rust#102211).
    pub fn query<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [DbParameter],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            self.query_all(&[(sql, bindings)], read_only, sink, cancel)
                .await
        })
    }

    /// Runs statements in one transaction: all commit or none do.
    pub fn query_all<'a>(
        &'a mut self,
        statements: &'a [(&'a str, &'a [DbParameter])],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            let started = Instant::now();
            batch(
                &self.client,
                if read_only {
                    "BEGIN READ ONLY"
                } else {
                    "BEGIN READ WRITE"
                },
            )
            .await
            .map_err(classify)?;
            // Guard qualifies functions and relations; ambient search_path cannot
            // redirect an approved function or operator to attacker-owned schemas.
            let setup = if read_only {
                "SET LOCAL search_path = pg_catalog; SET LOCAL standard_conforming_strings = on"
            } else {
                "SET LOCAL standard_conforming_strings = on"
            };
            if let Err(error) = batch(&self.client, setup).await {
                let _ = batch(&self.client, "ROLLBACK").await;
                return Err(classify(error));
            }
            let query = async {
                // Results are reported in order; nothing commits until all succeed.
                for (sql, bindings) in statements {
                    let affected = self.stream(sql, bindings, &mut *sink).await?;
                    let elapsed = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                    sink.statement_done(elapsed, affected).await?;
                }
                Ok::<_, DbError>(())
            };
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DbError::Cancelled),
                result = tokio::time::timeout(self.timeout, query) => result.unwrap_or(Err(DbError::Timeout)),
            };
            match result {
                Ok(()) => {}
                Err(error) => {
                    // Cancel the server operation, not only the waiting Rust future.
                    // If cleanup stalls, close the socket rather than reuse a live
                    // transaction after a cancelled/expired query.
                    if matches!(error, DbError::Cancelled | DbError::Timeout) {
                        let _ = tokio::time::timeout(Duration::from_secs(2), self.cancel_backend())
                            .await;
                    }
                    if tokio::time::timeout(Duration::from_secs(2), batch(&self.client, "ROLLBACK"))
                        .await
                        .is_err()
                    {
                        self.worker.abort();
                    }
                    return Err(error);
                }
            }
            batch(&self.client, "COMMIT").await.map_err(classify)
        })
    }

    /// Opens a write transaction driven statement by statement (CSV
    /// import); end it with [`Self::end_write`].
    pub(crate) fn begin_write(&mut self) -> futures::future::BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            batch(&self.client, "BEGIN READ WRITE")
                .await
                .map_err(classify)?;
            if let Err(error) =
                batch(&self.client, "SET LOCAL standard_conforming_strings = on").await
            {
                let _ = batch(&self.client, "ROLLBACK").await;
                return Err(classify(error));
            }
            Ok(())
        })
    }

    /// A fixed transaction-control statement (savepoints) of the open
    /// write transaction.
    pub(crate) fn control(
        &mut self,
        sql: &'static str,
    ) -> futures::future::BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move { batch(&self.client, sql).await.map_err(classify) })
    }

    /// One guarded statement inside the open write transaction, within the
    /// session timeout; returns the affected rows.
    pub(crate) fn step<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [DbParameter],
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<u64, DbError>> {
        Box::pin(async move {
            let mut sink = crate::csv_import::Affected::default();
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DbError::Cancelled),
                result = tokio::time::timeout(self.timeout, self.stream(sql, bindings, &mut sink)) =>
                    result.unwrap_or(Err(DbError::Timeout)),
            };
            if matches!(result, Err(DbError::Cancelled | DbError::Timeout)) {
                let _ = tokio::time::timeout(Duration::from_secs(2), self.cancel_backend()).await;
            }
            result.map(|affected| affected.unwrap_or(0))
        })
    }

    /// Commits or rolls back the write transaction; a failed rollback
    /// closes the connection rather than reuse it.
    pub(crate) fn end_write(
        &mut self,
        commit: bool,
    ) -> futures::future::BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            if commit {
                return batch(&self.client, "COMMIT").await.map_err(classify);
            }
            if !matches!(
                tokio::time::timeout(Duration::from_secs(5), batch(&self.client, "ROLLBACK")).await,
                Ok(Ok(()))
            ) {
                self.worker.abort();
            }
            Ok(())
        })
    }

    fn stream<'a>(
        &'a self,
        sql: &'a str,
        bindings: &'a [DbParameter],
        sink: &'a mut (dyn RowConsumer + Send),
    ) -> futures::future::BoxFuture<'a, Result<Option<u64>, DbError>> {
        Box::pin(async move {
            let prepare: futures::future::BoxFuture<'_, _> = Box::pin(self.client.prepare(sql));
            let statement = prepare.await.map_err(classify)?;
            let columns = statement
                .columns()
                .iter()
                .map(|column| DbColumn {
                    name: column.name().into(),
                    type_name: column.type_().name().into(),
                    nullable: true,
                    sensitive: crate::sensitive_column(column.name()),
                    primary_key: false,
                })
                .collect();
            sink.start(columns, None).await?;
            let parameters = bindings
                .iter()
                .map(|parameter| postgres_value::Parameter(&parameter.value))
                .collect::<Vec<_>>();
            let rows = self
                .client
                .query_raw(
                    &statement,
                    parameters.iter().map(|value| value as &(dyn ToSql + Sync)),
                )
                .await
                .map_err(classify)?;
            tokio::pin!(rows);
            while let Some(row) = rows.try_next().await.map_err(classify)? {
                if !sink.row(postgres_value::row(&row)?).await? {
                    // The driver must drain protocol state before this connection
                    // is reused. Cancel instead of reading an unbounded remainder.
                    self.cancel_backend().await?;
                    while rows.try_next().await.is_ok_and(|row| row.is_some()) {}
                    break;
                }
            }
            Ok(rows.rows_affected())
        })
    }

    fn cancel_backend(&self) -> futures::future::BoxFuture<'static, Result<(), DbError>> {
        // Owned copies: the cancel future must not borrow the driver.
        let token = self.client.cancel_token();
        let connector = Arc::clone(&self.connector);
        let destination = self.destination.clone();
        let (config, name) = (Arc::clone(&self.tls.config), self.tls.name.clone());
        Box::pin(async move {
            let stream = connector
                .connect(&destination, CancellationToken::new())
                .await?;
            // The token keeps the session's TLS mode; no password is sent.
            let mut factory = tokio_postgres_rustls::MakeRustlsConnect::new((*config).clone());
            let tls = MakeTlsConnect::<Socket>::make_tls_connect(&mut factory, &name)
                .map_err(|_| DbError::TlsFailed)?;
            let socket = Socket {
                io: stream,
                guard: None,
            };
            let cancel: futures::future::BoxFuture<'_, _> =
                Box::pin(token.cancel_query_raw(socket, tls));
            cancel.await.map_err(classify)
        })
    }
}

/// The driver socket as a concrete type: `Box<dyn DbStream>` carries an
/// inferred object lifetime that breaks the Send proof of tokio-postgres'
/// TLS handshake (rust-lang/rust#102211).
struct Socket {
    io: DbIo,
    /// Set for plaintext sockets over the network.
    guard: Option<CleartextGuard>,
}

impl tokio::io::AsyncRead for Socket {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let this = &mut *self;
        let poll = std::pin::Pin::new(&mut *this.io).poll_read(cx, buf);
        if let (std::task::Poll::Ready(Ok(())), Some(guard)) = (&poll, &mut this.guard) {
            if !guard.inspect(&buf.filled()[before..]) {
                // The driver never sees the request, so it cannot answer it.
                return std::task::Poll::Ready(Err(io::Error::from(
                    io::ErrorKind::PermissionDenied,
                )));
            }
        }
        poll
    }
}

impl tokio::io::AsyncWrite for Socket {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut *self.io).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.io).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.io).poll_shutdown(cx)
    }
}

/// Reads backend messages until authentication succeeds and stops at an
/// AuthenticationCleartextPassword request (`R`, code 3) before the driver
/// can answer it with the password.
struct CleartextGuard {
    /// Message type, length and (for `R`) the authentication code.
    header: [u8; 9],
    filled: usize,
    /// Body bytes of the current message still to pass.
    skip: usize,
    done: bool,
    refused: Arc<AtomicBool>,
}

impl CleartextGuard {
    fn new(refused: Arc<AtomicBool>) -> Self {
        Self {
            header: [0; 9],
            filled: 0,
            skip: 0,
            done: false,
            refused,
        }
    }

    fn length(&self) -> usize {
        u32::from_be_bytes([
            self.header[1],
            self.header[2],
            self.header[3],
            self.header[4],
        ]) as usize
    }

    /// Type and length; an `R` message with a body adds its 4-byte code.
    fn wanted(&self) -> usize {
        if self.filled >= 5 && self.header[0] == b'R' && self.length() >= 8 {
            9
        } else {
            5
        }
    }

    /// False when `bytes` carry a cleartext password request.
    fn inspect(&mut self, mut bytes: &[u8]) -> bool {
        while !self.done && !bytes.is_empty() {
            if self.skip > 0 {
                let count = self.skip.min(bytes.len());
                self.skip -= count;
                bytes = &bytes[count..];
                continue;
            }
            let count = (self.wanted() - self.filled).min(bytes.len());
            self.header[self.filled..self.filled + count].copy_from_slice(&bytes[..count]);
            self.filled += count;
            bytes = &bytes[count..];
            if self.filled < self.wanted() {
                continue;
            }
            if self.filled == 9 {
                let code = u32::from_be_bytes([
                    self.header[5],
                    self.header[6],
                    self.header[7],
                    self.header[8],
                ]);
                match code {
                    3 => {
                        self.refused.store(true, Ordering::SeqCst);
                        return false;
                    }
                    0 => self.done = true,
                    _ => {}
                }
            }
            // The length counts itself but not the type byte.
            self.skip = self.length().saturating_sub(4 + self.filled - 5);
            self.filled = 0;
        }
        true
    }
}

/// Records whether the server accepted TLS ("prefer" may continue without).
struct Tracked<T> {
    inner: T,
    encrypted: Arc<AtomicBool>,
}

impl<S, T: TlsConnect<S>> TlsConnect<S> for Tracked<T> {
    type Stream = T::Stream;
    type Error = T::Error;
    type Future = T::Future;

    fn connect(self, stream: S) -> Self::Future {
        // Called only after the server answered the TLS request with `S`.
        self.encrypted.store(true, Ordering::SeqCst);
        self.inner.connect(stream)
    }
}

/// Boxed so the spawned query future stays Send for every lifetime.
fn batch<'a>(
    client: &'a Client,
    sql: &'a str,
) -> futures::future::BoxFuture<'a, Result<(), tokio_postgres::Error>> {
    Box::pin(client.batch_execute(sql))
}

fn classify(error: tokio_postgres::Error) -> DbError {
    match error.as_db_error().map(|error| error.code().code()) {
        Some("28P01" | "28000") => DbError::AuthenticationFailed,
        Some("57014") => DbError::Cancelled,
        Some("25006" | "42501") => DbError::PolicyDenied,
        _ if error.is_closed() => DbError::ConnectionFailed,
        // Driver messages carry no secrets; only their kind is kept.
        None if cause(&error).contains("does not support TLS") => DbError::TlsUnavailable,
        None if cause(&error).contains("channel binding") => DbError::ChannelBindingUnsupported,
        None if error.to_string().contains("TLS") => DbError::TlsFailed,
        None if error.to_string().starts_with("error connecting to server") => {
            DbError::ConnectionFailed
        }
        _ => DbError::QueryFailed,
    }
}

fn cause(error: &tokio_postgres::Error) -> String {
    std::error::Error::source(error)
        .map(ToString::to_string)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{DbPath, DirectConnector};
    use async_trait::async_trait;
    use secrecy::SecretString;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    const PASSWORD: &str = "canary-pg-password";
    const SSL_REQUEST: [u8; 8] = [0, 0, 0, 8, 0x04, 0xd2, 0x16, 0x2f];

    /// Dials the fake server but reports the path a real route would take.
    #[derive(Debug)]
    struct Route(DbPath);

    #[async_trait]
    impl DbConnector for Route {
        async fn connect(
            &self,
            destination: &DbDestination,
            cancel: CancellationToken,
        ) -> Result<DbIo, DbError> {
            DirectConnector.connect(destination, cancel).await
        }
        async fn connect_path(
            &self,
            destination: &DbDestination,
            cancel: CancellationToken,
        ) -> Result<(DbIo, DbPath), DbError> {
            Ok((self.connect(destination, cancel).await?, self.0))
        }
    }

    trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
    impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

    /// What a fake server received, and where the startup message ended.
    struct Received {
        bytes: Vec<u8>,
        startup_end: usize,
    }
    impl Received {
        fn leaked(&self) -> bool {
            self.bytes
                .windows(PASSWORD.len())
                .any(|window| window == PASSWORD.as_bytes())
        }
        /// The client answered the authentication request.
        fn answered(&self) -> bool {
            self.startup_end > 0 && self.bytes.len() > self.startup_end
        }
    }

    /// A fake server for one connection: answers the TLS request with `S`
    /// (when it has a certificate) or `N`, then asks for `authentication` and
    /// records every byte the client sent.
    async fn fake(
        tls: Option<Arc<rustls::ServerConfig>>,
        authentication: &'static [u8],
    ) -> (u16, tokio::task::JoinHandle<Received>) {
        async fn run(
            mut stream: Box<dyn Io>,
            tls: Option<Arc<rustls::ServerConfig>>,
            authentication: &[u8],
            received: &mut Received,
        ) -> io::Result<()> {
            let mut head = [0; 8];
            stream.read_exact(&mut head).await?;
            received.bytes.extend_from_slice(&head);
            let length = if head == SSL_REQUEST {
                if let Some(config) = tls {
                    stream.write_all(b"S").await?;
                    let acceptor = tokio_rustls::TlsAcceptor::from(config);
                    stream = Box::new(acceptor.accept(stream).await?);
                } else {
                    stream.write_all(b"N").await?;
                }
                let mut length = [0; 4];
                stream.read_exact(&mut length).await?;
                received.bytes.extend_from_slice(&length);
                u32::from_be_bytes(length) as usize - 4
            } else {
                u32::from_be_bytes(head[..4].try_into().unwrap()) as usize - 8
            };
            let mut rest = vec![0; length];
            stream.read_exact(&mut rest).await?;
            received.bytes.extend_from_slice(&rest);
            received.startup_end = received.bytes.len();
            stream.write_all(authentication).await?;
            let mut buffer = [0; 1024];
            while let Ok(Ok(count @ 1..)) =
                tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buffer)).await
            {
                received.bytes.extend_from_slice(&buffer[..count]);
            }
            Ok(())
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut received = Received {
                bytes: Vec::new(),
                startup_end: 0,
            };
            let _ = run(Box::new(socket), tls, authentication, &mut received).await;
            received
        });
        (port, task)
    }

    const CLEARTEXT: &[u8] = &[b'R', 0, 0, 0, 8, 0, 0, 0, 3];
    const MD5: &[u8] = &[b'R', 0, 0, 0, 12, 0, 0, 0, 5, 1, 2, 3, 4];
    const SCRAM_ONLY: &[u8] = b"R\0\0\0\x17\0\0\0\x0aSCRAM-SHA-256\0\0";

    fn connection(port: u16, mode: DbTlsMode) -> DbConnection {
        let mut connection = DbConnection::new(
            "fake",
            "127.0.0.1",
            cc_models::database::DbEngine::Postgresql,
        );
        connection.port = port;
        connection.username = "postgres".into();
        connection.tls.mode = mode;
        connection
    }

    async fn attempt(connection: &DbConnection, path: Option<DbPath>) -> Result<Postgres, DbError> {
        let credentials = DbCredentials {
            password: Some(SecretString::from(PASSWORD)),
            client_key_pem: None,
        };
        let connector: Arc<dyn DbConnector> = match path {
            Some(path) => Arc::new(Route(path)),
            None => Arc::new(DirectConnector),
        };
        Postgres::connect(
            connection,
            &credentials,
            connector,
            CancellationToken::new(),
        )
        .await
    }

    fn certificate() -> (Arc<rustls::ServerConfig>, String) {
        let key = rcgen::KeyPair::generate().unwrap();
        let certificate = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
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
        (Arc::new(config), certificate.pem())
    }

    #[tokio::test]
    async fn prefer_over_the_network_never_sends_the_password_without_tls() {
        let (port, server) = fake(None, CLEARTEXT).await;
        let result = attempt(&connection(port, DbTlsMode::Prefer), Some(DbPath::Network)).await;
        assert_eq!(result.err(), Some(DbError::TlsUnavailable));
        // Only the TLS request reached the server: no startup, no password.
        assert_eq!(server.await.unwrap().bytes, SSL_REQUEST);
    }

    #[tokio::test]
    async fn prefer_continues_without_tls_on_loopback_and_through_tunnels() {
        // DirectConnector to 127.0.0.1: the resolved peer is loopback.
        for path in [None, Some(DbPath::Tunnel)] {
            let (port, server) = fake(None, CLEARTEXT).await;
            // The fake server stops after the password; the session fails later.
            let result = attempt(&connection(port, DbTlsMode::Prefer), path).await;
            assert!(result.is_err());
            assert!(server.await.unwrap().leaked(), "{path:?}");
        }
    }

    #[tokio::test]
    async fn disabled_tls_refuses_a_cleartext_password_over_the_network_only() {
        let (port, server) = fake(None, CLEARTEXT).await;
        let result = attempt(&connection(port, DbTlsMode::Disable), Some(DbPath::Network)).await;
        assert_eq!(result.err(), Some(DbError::CleartextRefused));
        let received = server.await.unwrap();
        assert!(!received.leaked() && !received.answered());
        // md5 answers a challenge; no TLS was the user's explicit choice.
        let (port, server) = fake(None, MD5).await;
        let _ = attempt(&connection(port, DbTlsMode::Disable), Some(DbPath::Network)).await;
        let received = server.await.unwrap();
        assert!(!received.leaked() && received.answered());
        for path in [Some(DbPath::Tunnel), None] {
            let (port, server) = fake(None, CLEARTEXT).await;
            let _ = attempt(&connection(port, DbTlsMode::Disable), path).await;
            assert!(server.await.unwrap().leaked(), "{path:?}");
        }
    }

    #[tokio::test]
    async fn require_without_channel_binding_never_sends_the_password() {
        // An interceptor with any certificate asks for an unbound password.
        for authentication in [CLEARTEXT, MD5, SCRAM_ONLY] {
            for path in [DbPath::Network, DbPath::Tunnel] {
                let (config, _) = certificate();
                let (port, server) = fake(Some(config), authentication).await;
                let result = attempt(&connection(port, DbTlsMode::Require), Some(path)).await;
                assert_eq!(result.err(), Some(DbError::ChannelBindingUnsupported));
                let received = server.await.unwrap();
                // Nothing after the startup message: no password or SASL reply.
                assert!(received.startup_end > 0 && !received.answered());
                assert!(!received.leaked(), "{authentication:?}");
            }
        }
        // The explicit opt-out accepts older servers inside TLS.
        let (config, _) = certificate();
        let (port, server) = fake(Some(config), CLEARTEXT).await;
        let mut unbound = connection(port, DbTlsMode::Require);
        unbound.tls.allow_unbound_auth = true;
        let _ = attempt(&unbound, Some(DbPath::Network)).await;
        assert!(server.await.unwrap().leaked());
    }

    #[tokio::test]
    async fn verified_tls_needs_no_channel_binding() {
        for mode in [DbTlsMode::Verify, DbTlsMode::Prefer] {
            let (config, authority) = certificate();
            let (port, server) = fake(Some(config), CLEARTEXT).await;
            let mut verified = connection(port, mode);
            verified.tls.certificate_authority_pem = Some(authority);
            let _ = attempt(&verified, Some(DbPath::Network)).await;
            assert!(server.await.unwrap().leaked(), "{mode:?}");
        }
    }

    #[test]
    fn cleartext_guard_finds_the_request_across_reads_and_after_notices() {
        let mut stream = Vec::new();
        // A NoticeResponse, then the cleartext request.
        stream.extend_from_slice(&[b'N', 0, 0, 0, 6, b'M', 0]);
        stream.extend_from_slice(CLEARTEXT);
        for chunk in [1, 2, 3, stream.len()] {
            let refused = Arc::new(AtomicBool::new(false));
            let mut guard = CleartextGuard::new(refused.clone());
            let passed = stream.chunks(chunk).all(|bytes| guard.inspect(bytes));
            assert!(!passed && refused.load(Ordering::SeqCst), "{chunk}");
        }
        // SASL, md5 and data after AuthenticationOk pass.
        let mut guard = CleartextGuard::new(Arc::new(AtomicBool::new(false)));
        assert!(guard.inspect(MD5));
        assert!(guard.inspect(&[b'R', 0, 0, 0, 8, 0, 0, 0, 0]));
        assert!(guard.inspect(CLEARTEXT));
    }
}
