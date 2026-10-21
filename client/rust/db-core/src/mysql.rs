use crate::{
    streaming::RowConsumer,
    transport::{DbConnector, DbCredentials, DbDestination, LocalRelay},
    DbCell, DbColumn, DbError, DbParameter,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::database::{DbConnection, DbEngine, DbOptions, DbTlsMode};
use mysql_async::{
    consts::{ColumnFlags, ColumnType},
    prelude::Queryable,
    Conn, Opts, OptsBuilder, Params, SslOpts, Value,
};
use secrecy::ExposeSecret;
use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub(crate) struct Mysql {
    connection: Option<Conn>,
    options: Opts,
    _relay: LocalRelay,
    timeout: Duration,
    /// Production sessions are READ ONLY at the session level.
    production: bool,
    pub version: String,
}
impl fmt::Debug for Mysql {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Mysql([SCOPED])")
    }
}
impl Mysql {
    pub async fn connect(
        connection: &DbConnection,
        credentials: &DbCredentials,
        connector: Arc<dyn DbConnector>,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let relay = LocalRelay::tcp(
            connector,
            DbDestination::new(&connection.host, connection.port)?,
            scope,
        )
        .await?;
        let mut builder = OptsBuilder::default()
            .ip_or_hostname(relay.address.ip().to_string())
            .tcp_port(relay.address.port())
            .prefer_socket(false)
            .stmt_cache_size(0)
            .max_allowed_packet(Some(crate::MAX_EVENT_BYTES))
            .user(Some(connection.username.clone()));
        if !connection.database.is_empty() {
            builder = builder.db_name(Some(connection.database.clone()));
        }
        if let Some(password) = &credentials.password {
            builder = builder.pass(Some(password.expose_secret().to_owned()));
        }
        if connection.tls.mode != DbTlsMode::Disable {
            let mut ssl = SslOpts::default().with_danger_tls_hostname_override(Some(
                connection
                    .tls
                    .server_name
                    .clone()
                    .unwrap_or_else(|| connection.host.clone()),
            ));
            if let Some(ca) = &connection.tls.certificate_authority_pem {
                ssl = ssl
                    .with_root_certs(vec![ca.as_bytes().to_vec().into()])
                    .with_disable_built_in_roots(true);
            }
            if connection.tls.mode == DbTlsMode::Require {
                ssl = ssl
                    .with_danger_accept_invalid_certs(true)
                    .with_danger_skip_domain_validation(true);
            }
            if let Some(certificate) = &connection.auth.client_certificate {
                let key = credentials
                    .client_key_pem
                    .as_ref()
                    .ok_or(DbError::AuthenticationFailed)?;
                ssl = ssl.with_client_identity(Some(mysql_async::ClientIdentity::new(
                    certificate.certificate_pem.as_bytes().to_vec().into(),
                    key.expose_secret().as_bytes().to_vec().into(),
                )));
            }
            // Prefer is intentionally never retried as plaintext after a failed
            // handshake. A certificate failure must not expose a password.
            builder = builder.ssl_opts(Some(ssl));
        }
        let options = Opts::from(builder);
        let mut client = Conn::new(options.clone()).await.map_err(classify)?;
        let charset = match &connection.options {
            DbOptions::Mysql { charset } | DbOptions::Mariadb { charset } => charset.as_str(),
            _ => return Err(DbError::InvalidRequest),
        };
        // Inventory validation restricts charset to four fixed identifiers.
        let charset_sql = match charset {
            "utf8mb4" => "SET NAMES utf8mb4",
            "utf8" => "SET NAMES utf8",
            "ascii" => "SET NAMES ascii",
            "latin1" => "SET NAMES latin1",
            _ => return Err(DbError::InvalidRequest),
        };
        client.query_drop(charset_sql).await.map_err(classify)?;
        if connection.production {
            client
                .query_drop("SET SESSION TRANSACTION READ ONLY")
                .await
                .map_err(classify)?;
        }
        let timeout = Duration::from_secs(u64::from(connection.limits.statement_timeout_seconds));
        let deadline_sql = if connection.engine == DbEngine::Mariadb {
            format!("SET SESSION max_statement_time = {}", timeout.as_secs())
        } else {
            format!("SET SESSION max_execution_time = {}", timeout.as_millis())
        };
        client.query_drop(deadline_sql).await.map_err(classify)?;
        let version: Option<String> = client
            .query_first("SELECT VERSION()")
            .await
            .map_err(classify)?;
        Ok(Self {
            connection: Some(client),
            options,
            _relay: relay,
            timeout,
            production: connection.production,
            version: version.ok_or(DbError::QueryFailed)?,
        })
    }

    // A boxed future with one explicit lifetime: an `async fn` here trips the
    // compiler's higher-ranked Send check (rust-lang/rust#102211) when spawned.
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

    /// Runs statements in one transaction: all commit or none do. A
    /// production session leaves its session-level READ ONLY only for a
    /// write the guard permitted in an open production write window (DDL
    /// commits implicitly, outside the explicit READ WRITE transaction), and
    /// returns to it right after.
    pub fn query_all<'a>(
        &'a mut self,
        statements: &'a [(&'a str, &'a [DbParameter])],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            let lift = self.production && !read_only;
            if lift {
                let client = self.connection.as_mut().ok_or(DbError::SessionClosed)?;
                client
                    .query_drop(String::from("SET SESSION TRANSACTION READ WRITE"))
                    .await
                    .map_err(classify)?;
            }
            let result = self.transaction(statements, read_only, sink, cancel).await;
            if lift {
                let restored = match self.connection.as_mut() {
                    Some(client) => client
                        .query_drop(String::from("SET SESSION TRANSACTION READ ONLY"))
                        .await
                        .is_ok(),
                    None => true,
                };
                // Never keep a production connection writable.
                if !restored {
                    self.connection.take();
                }
            }
            result
        })
    }

    fn transaction<'a>(
        &'a mut self,
        statements: &'a [(&'a str, &'a [DbParameter])],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<(), DbError>> {
        let owned: Vec<(String, &'a [DbParameter])> = statements
            .iter()
            .map(|(sql, bindings)| ((*sql).to_owned(), *bindings))
            .collect();
        Box::pin(async move {
            let started = Instant::now();
            let client = self.connection.as_mut().ok_or(DbError::SessionClosed)?;
            client
                .query_drop(String::from(if read_only {
                    "START TRANSACTION READ ONLY"
                } else {
                    "START TRANSACTION READ WRITE"
                }))
                .await
                .map_err(classify)?;
            let id = client.id();
            let operation = async {
                // Results are reported in order; nothing commits until all succeed.
                for (sql, bindings) in owned {
                    let affected = Self::stream(&mut *client, sql, bindings, &mut *sink).await?;
                    sink.statement_done(started.elapsed().as_millis() as u64, Some(affected))
                        .await?;
                }
                Ok::<_, DbError>(())
            };
            let result = tokio::select! {
                biased; _ = cancel.cancelled() => Err(DbError::Cancelled),
                result = tokio::time::timeout(self.timeout, operation) => result.unwrap_or(Err(DbError::Timeout)),
            };
            match result {
                Ok(()) => client
                    .query_drop(String::from("COMMIT"))
                    .await
                    .map_err(classify),
                Err(error) => {
                    if matches!(
                        error,
                        DbError::Cancelled | DbError::Timeout | DbError::ResultLimit
                    ) {
                        let options = self.options.clone();
                        let _ = tokio::time::timeout(Duration::from_secs(2), async move {
                            let mut control = Conn::new(options).await.map_err(classify)?;
                            // ID comes from this driver's authenticated handshake.
                            control
                                .query_drop(format!("KILL QUERY {id}"))
                                .await
                                .map_err(classify)?;
                            control.disconnect().await.map_err(classify)
                        })
                        .await;
                    }
                    let cleanup = tokio::time::timeout(
                        Duration::from_secs(2),
                        client.query_drop(String::from("ROLLBACK")),
                    )
                    .await;
                    if !matches!(cleanup, Ok(Ok(()))) {
                        self.connection.take();
                    }
                    Err(error)
                }
            }
        })
    }

    async fn stream(
        client: &mut Conn,
        sql: String,
        bindings: &[DbParameter],
        sink: &mut (dyn RowConsumer + Send),
    ) -> Result<u64, DbError> {
        let parameters = bindings
            .iter()
            .map(|binding| parameter(&binding.value))
            .collect::<Result<Vec<_>, _>>()?;
        let mut result = client
            .exec_iter(sql, Params::Positional(parameters))
            .await
            .map_err(classify)?;
        let columns = result.columns_ref().to_vec();
        sink.start(
            columns
                .iter()
                .map(|column| DbColumn {
                    name: column.name_str().into_owned(),
                    type_name: format!("{:?}", column.column_type()),
                    nullable: !column.flags().contains(ColumnFlags::NOT_NULL_FLAG),
                    sensitive: crate::sensitive_column(&column.name_str()),
                    primary_key: column.flags().contains(ColumnFlags::PRI_KEY_FLAG),
                })
                .collect(),
            None,
        )
        .await?;
        while let Some(row) = result.next().await.map_err(classify)? {
            let values = row
                .unwrap()
                .into_iter()
                .zip(&columns)
                .map(|(value, column)| cell(value, column))
                .collect::<Result<Vec<_>, _>>()?;
            if !sink.row(values).await? {
                return Err(DbError::ResultLimit);
            }
        }
        // Compound CALL results are not silently discarded: script statements
        // must be split and reviewed before reaching this single-result driver.
        if !result.is_empty() {
            return Err(DbError::InvalidRequest);
        }
        Ok(result.affected_rows())
    }
}

fn parameter(cell: &DbCell) -> Result<Value, DbError> {
    Ok(match cell {
        DbCell::Null => Value::NULL,
        DbCell::Bool(value) => Value::Int(i64::from(*value)),
        DbCell::Int(value) => Value::Int(value.parse().map_err(|_| DbError::InvalidRequest)?),
        DbCell::Uint(value) => Value::UInt(value.parse().map_err(|_| DbError::InvalidRequest)?),
        DbCell::Float(value) => Value::Double(value.parse().map_err(|_| DbError::InvalidRequest)?),
        DbCell::Binary(value) => Value::Bytes(
            STANDARD
                .decode(value)
                .map_err(|_| DbError::InvalidRequest)?,
        ),
        DbCell::Decimal(value)
        | DbCell::Text(value)
        | DbCell::Date(value)
        | DbCell::Time(value)
        | DbCell::Timestamp(value)
        | DbCell::Json(value) => Value::Bytes(value.as_bytes().to_vec()),
    })
}
fn cell(value: Value, column: &mysql_async::Column) -> Result<DbCell, DbError> {
    Ok(match value {
        Value::NULL => DbCell::Null,
        Value::Int(value) => DbCell::Int(value.to_string()),
        Value::UInt(value) => DbCell::Uint(value.to_string()),
        Value::Float(value) => DbCell::Float(value.to_string()),
        Value::Double(value) => DbCell::Float(value.to_string()),
        Value::Bytes(value) => {
            if value.len() > crate::MAX_EVENT_BYTES {
                return Err(DbError::ResultLimit);
            }
            match column.column_type() {
                ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => {
                    DbCell::Decimal(String::from_utf8(value).map_err(|_| DbError::QueryFailed)?)
                }
                ColumnType::MYSQL_TYPE_JSON => {
                    DbCell::Json(String::from_utf8(value).map_err(|_| DbError::QueryFailed)?)
                }
                _ if column.character_set() == 63 => DbCell::Binary(STANDARD.encode(value)),
                _ => DbCell::Text(String::from_utf8(value).map_err(|_| DbError::QueryFailed)?),
            }
        }
        Value::Date(year, month, day, hour, minute, second, micros) => {
            let date = format!("{year:04}-{month:02}-{day:02}");
            if column.column_type() == ColumnType::MYSQL_TYPE_DATE {
                DbCell::Date(date)
            } else {
                DbCell::Timestamp(format!(
                    "{date} {hour:02}:{minute:02}:{second:02}.{micros:06}"
                ))
            }
        }
        Value::Time(negative, days, hour, minute, second, micros) => DbCell::Time(format!(
            "{}{:02}:{minute:02}:{second:02}.{micros:06}",
            if negative { "-" } else { "" },
            days * 24 + u32::from(hour)
        )),
    })
}
fn classify(error: mysql_async::Error) -> DbError {
    match &error {
        mysql_async::Error::Server(error) => match error.code {
            1045 => DbError::AuthenticationFailed,
            1142 | 1290 | 1792 => DbError::PolicyDenied,
            1317 | 3024 | 1969 => DbError::Cancelled,
            _ => DbError::QueryFailed,
        },
        // Prefer and Require never continue without TLS.
        mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer) => {
            DbError::TlsUnavailable
        }
        // mysql_clear_password stays disabled: the password is never sent as is.
        mysql_async::Error::Driver(mysql_async::DriverError::CleartextPluginDisabled) => {
            DbError::CleartextRefused
        }
        _ => {
            let text = error.to_string().to_ascii_lowercase();
            if text.contains("tls") || text.contains("ssl") || text.contains("certificate") {
                DbError::TlsFailed
            } else {
                DbError::ConnectionFailed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::DirectConnector;
    use secrecy::SecretString;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const PASSWORD: &str = "canary-mysql-password";

    fn packet(sequence: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = (payload.len() as u32).to_le_bytes()[..3].to_vec();
        packet.push(sequence);
        packet.extend_from_slice(payload);
        packet
    }

    /// A fake MySQL 8 without TLS: after the handshake response it switches
    /// to mysql_clear_password and records every byte the client sent.
    async fn fake() -> (u16, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            // PROTOCOL_41, SECURE_CONNECTION, PLUGIN_AUTH, CONNECT_WITH_DB… but no SSL.
            let capabilities: u32 = 0x0028_a209;
            let mut handshake = vec![10];
            handshake.extend_from_slice(b"8.0.36\0");
            handshake.extend_from_slice(&7u32.to_le_bytes());
            handshake.extend_from_slice(b"abcdefgh\0");
            handshake.extend_from_slice(&capabilities.to_le_bytes()[..2]);
            handshake.push(45);
            handshake.extend_from_slice(&2u16.to_le_bytes());
            handshake.extend_from_slice(&capabilities.to_le_bytes()[2..]);
            handshake.push(21);
            handshake.extend_from_slice(&[0; 10]);
            handshake.extend_from_slice(b"ijklmnopqrst\0");
            handshake.extend_from_slice(b"mysql_native_password\0");
            let mut received = Vec::new();
            if stream.write_all(&packet(0, &handshake)).await.is_err() {
                return received;
            }
            let mut buffer = [0; 1024];
            let mut switched = false;
            while let Ok(Ok(count @ 1..)) =
                tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buffer)).await
            {
                received.extend_from_slice(&buffer[..count]);
                if !switched {
                    switched = true;
                    let mut switch = vec![0xfe];
                    switch.extend_from_slice(b"mysql_clear_password\0");
                    let _ = stream.write_all(&packet(2, &switch)).await;
                }
            }
            received
        });
        (port, task)
    }

    async fn attempt(port: u16, mode: DbTlsMode) -> Result<Mysql, DbError> {
        let mut connection = DbConnection::new("fake", "127.0.0.1", DbEngine::Mysql);
        connection.port = port;
        connection.username = "root".into();
        connection.tls.mode = mode;
        let credentials = DbCredentials {
            password: Some(SecretString::from(PASSWORD)),
            client_key_pem: None,
        };
        Mysql::connect(
            &connection,
            &credentials,
            Arc::new(DirectConnector),
            CancellationToken::new(),
        )
        .await
    }

    #[tokio::test]
    async fn cleartext_password_plugin_is_refused() {
        let (port, server) = fake().await;
        assert_eq!(
            attempt(port, DbTlsMode::Disable).await.err(),
            Some(DbError::CleartextRefused)
        );
        let received = server.await.unwrap();
        // The handshake response arrived (a native-password hash), the password did not.
        assert!(!received.is_empty());
        assert!(!received
            .windows(PASSWORD.len())
            .any(|window| window == PASSWORD.as_bytes()));
    }

    #[tokio::test]
    async fn prefer_and_require_never_continue_without_tls() {
        for mode in [DbTlsMode::Prefer, DbTlsMode::Require, DbTlsMode::Verify] {
            let (port, server) = fake().await;
            assert_eq!(
                attempt(port, mode).await.err(),
                Some(DbError::TlsUnavailable)
            );
            // Not even the handshake response (user name) was sent.
            assert!(server.await.unwrap().is_empty(), "{mode:?}");
        }
    }
}
