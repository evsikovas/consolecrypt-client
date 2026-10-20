use crate::{
    postgres_value,
    streaming::RowConsumer,
    transport::{DbConnector, DbCredentials, DbDestination, TlsProfile},
    DbColumn, DbError, DbParameter,
};
use cc_models::database::{DbConnection, DbOptions, DbTlsMode};
use futures::TryStreamExt;
use secrecy::ExposeSecret;
use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_postgres::{config::SslMode, tls::MakeTlsConnect, types::ToSql, Client, NoTls};
use tokio_util::sync::CancellationToken;

pub(crate) struct Postgres {
    client: Client,
    worker: tokio::task::JoinHandle<()>,
    connector: Arc<dyn DbConnector>,
    destination: DbDestination,
    tls: TlsProfile,
    timeout: Duration,
    pub version: String,
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
        configuration.ssl_mode(match connection.tls.mode {
            DbTlsMode::Disable => SslMode::Disable,
            DbTlsMode::Prefer => SslMode::Prefer,
            DbTlsMode::Require | DbTlsMode::Verify => SslMode::Require,
        });
        let timeout = Duration::from_secs(u64::from(connection.limits.statement_timeout_seconds));
        configuration.options(format!(
            "-c default_transaction_read_only={} -c statement_timeout={} -c idle_in_transaction_session_timeout={} -c standard_conforming_strings=on",
            if connection.production { "on" } else { "off" }, timeout.as_millis(), timeout.as_millis()));
        let stream = connector.connect(&destination, scope.clone()).await?;
        let (client, worker) = if connection.tls.mode == DbTlsMode::Disable {
            let (client, future) = configuration
                .connect_raw(stream, NoTls)
                .await
                .map_err(classify)?;
            let worker = tokio::spawn(async move {
                tokio::select! {
                    biased; _ = scope.cancelled() => {}, _ = future => {},
                }
            });
            (client, worker)
        } else {
            let mut factory = tokio_postgres_rustls::MakeRustlsConnect::new((*tls.config).clone());
            let connector =
                MakeTlsConnect::<crate::transport::DbIo>::make_tls_connect(&mut factory, &tls.name)
                    .map_err(|_| DbError::TlsFailed)?;
            let (client, future) = configuration
                .connect_raw(stream, connector)
                .await
                .map_err(classify)?;
            let worker = tokio::spawn(async move {
                tokio::select! {
                    biased; _ = scope.cancelled() => {}, _ = future => {},
                }
            });
            (client, worker)
        };
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
            let query = self.stream(sql, bindings, sink);
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DbError::Cancelled),
                result = tokio::time::timeout(self.timeout, query) => result.unwrap_or(Err(DbError::Timeout)),
            };
            let affected = match result {
                Ok(affected) => affected,
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
            };
            batch(&self.client, "COMMIT").await.map_err(classify)?;
            sink.statement_done(
                started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                affected,
            )
            .await
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
        let (config, name, mode) = (
            Arc::clone(&self.tls.config),
            self.tls.name.clone(),
            self.tls.mode,
        );
        Box::pin(async move {
            let stream = connector
                .connect(&destination, CancellationToken::new())
                .await?;
            if mode == DbTlsMode::Disable {
                let cancel: futures::future::BoxFuture<'_, _> =
                    Box::pin(token.cancel_query_raw(stream, NoTls));
                cancel.await.map_err(classify)
            } else {
                let mut factory = tokio_postgres_rustls::MakeRustlsConnect::new((*config).clone());
                let tls =
                    MakeTlsConnect::<crate::transport::DbIo>::make_tls_connect(&mut factory, &name)
                        .map_err(|_| DbError::TlsFailed)?;
                let cancel: futures::future::BoxFuture<'_, _> =
                    Box::pin(token.cancel_query_raw(stream, tls));
                cancel.await.map_err(classify)
            }
        })
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
        _ => DbError::QueryFailed,
    }
}
