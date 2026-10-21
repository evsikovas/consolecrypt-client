//! ClickHouse over its HTTP(S) interface. Every request carries the session's
//! server-side limits; read-only requests run with `readonly=2`. Query text is
//! the POST body, values travel as typed server-side query parameters, and the
//! credentials only ever appear in redacted request headers.
use crate::{
    streaming::RowConsumer,
    transport::{DbConnector, DbCredentials, DbDestination, LocalRelay, TlsProfile},
    DbCell, DbColumn, DbError, DbObjectKind, DbObjectRef, DbParameter, DbTreeNode, DbTreePage,
    DEFAULT_TREE_PAGE, MAX_EVENT_BYTES,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::database::{DbConnection, DbOptions, DbTlsMode};
use futures::{future::BoxFuture, StreamExt};
use percent_encoding::{percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::header::HeaderValue;
use rustls::{
    client::{
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        WebPkiServerVerifier,
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
    DigitallySignedStruct, SignatureScheme,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::value::RawValue;
use std::{
    fmt,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const FORMAT: &str = "JSONCompactEachRowWithNamesAndTypes";
/// Server-side cap for read-only results; the result sink's byte budget
/// normally stops a stream long before this.
const RESULT_ROW_CAP: u64 = 1_000_000;
const KILL_TIMEOUT: Duration = Duration::from_secs(2);
/// Everything but RFC 3986 unreserved characters is percent-encoded.
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub(crate) struct ClickHouse {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    user: String,
    password: Option<SecretString>,
    certificate_auth: bool,
    database: String,
    settings: Vec<(String, u64)>,
    production: bool,
    /// The user's settings profile is `readonly=1`: the server refuses every
    /// setting change, and already refuses every write.
    profile_read_only: bool,
    timeout: Duration,
    _relay: LocalRelay,
    pub version: String,
}

impl fmt::Debug for ClickHouse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClickHouse([SCOPED])")
    }
}

/// What one HTTP statement produced.
struct Outcome {
    affected: Option<u64>,
    /// The consumer stopped reading; the server query must be killed.
    stopped: bool,
}

impl ClickHouse {
    pub async fn connect(
        connection: &DbConnection,
        credentials: &DbCredentials,
        connector: Arc<dyn DbConnector>,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        let settings = match &connection.options {
            DbOptions::Clickhouse { settings } => settings
                .iter()
                .map(|(name, value)| (name.clone(), *value))
                .collect(),
            _ => return Err(DbError::InvalidRequest),
        };
        let destination = DbDestination::new(&connection.host, connection.port)?;
        let relay = LocalRelay::tcp(connector, destination, scope).await?;
        let connect_timeout =
            Duration::from_secs(u64::from(connection.limits.connect_timeout_seconds.max(1)));
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .http1_only()
            .connect_timeout(connect_timeout)
            .pool_idle_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(2)
            .user_agent("ConsoleCrypt");
        let endpoint = if connection.tls.mode == DbTlsMode::Disable {
            format!("http://127.0.0.1:{}/", relay.address.port())
        } else {
            // TCP always goes to the relay; the TLS identity stays the real
            // server name. A DNS name resolves to the relay (keeping SNI); an
            // IP identity has no SNI, so its verifier is pinned to that IP.
            let profile = TlsProfile::new(connection, credentials)?;
            let mut config = (*profile.config).clone();
            config.alpn_protocols = vec![b"http/1.1".to_vec()];
            let host = match profile.name.parse::<IpAddr>() {
                Ok(address) => {
                    if profile.mode != DbTlsMode::Require {
                        let provider = Arc::new(rustls::crypto::ring::default_provider());
                        let inner = WebPkiServerVerifier::builder_with_provider(
                            Arc::new(TlsProfile::roots(connection)?),
                            provider,
                        )
                        .build()
                        .map_err(|_| DbError::TlsFailed)?;
                        config
                            .dangerous()
                            .set_certificate_verifier(Arc::new(PinnedName {
                                inner,
                                name: ServerName::IpAddress(address.into()),
                            }));
                    }
                    "127.0.0.1".to_owned()
                }
                Err(_) => {
                    builder = builder.resolve(&profile.name, relay.address);
                    profile.name.to_ascii_lowercase()
                }
            };
            builder = builder.tls_backend_preconfigured(config);
            format!("https://{host}:{}/", relay.address.port())
        };
        let mut driver = Self {
            client: builder.build().map_err(|_| DbError::ConnectionFailed)?,
            endpoint: reqwest::Url::parse(&endpoint).map_err(|_| DbError::InvalidRequest)?,
            user: connection.username.clone(),
            password: credentials
                .password
                .as_ref()
                .map(|password| SecretString::from(password.expose_secret().to_owned())),
            certificate_auth: connection.auth.client_certificate.is_some()
                && credentials.password.is_none(),
            database: connection.database.clone(),
            settings,
            production: connection.production,
            profile_read_only: false,
            timeout: Duration::from_secs(u64::from(connection.limits.statement_timeout_seconds)),
            _relay: relay,
            version: String::new(),
        };
        let deadline = connect_timeout + driver.timeout;
        let probe = async {
            match driver.probe().await {
                // A readonly=1 profile refuses even readonly=2 and the limits.
                Err(DbError::PolicyDenied) => {
                    driver.profile_read_only = true;
                    driver.probe().await
                }
                result => result,
            }
        };
        let (version, readonly) = tokio::time::timeout(deadline, probe)
            .await
            .map_err(|_| DbError::Timeout)??;
        // Production must be read-only on the server, not only in the guard.
        if (driver.production || driver.profile_read_only) && readonly == 0 {
            return Err(DbError::PolicyDenied);
        }
        driver.version = version;
        Ok(driver)
    }

    /// Server version and the effective `readonly` level of read requests.
    async fn probe(&mut self) -> Result<(String, u64), DbError> {
        let mut rows = Rows::default();
        self.query(
            "SELECT version(), toUInt64(getSetting('readonly'))",
            &[],
            true,
            &mut rows,
            CancellationToken::new(),
        )
        .await?;
        let row = rows.rows.first().ok_or(DbError::QueryFailed)?;
        let version = row.first().map(text).ok_or(DbError::QueryFailed)?;
        let readonly = row
            .get(1)
            .map(text)
            .and_then(|value| value.parse().ok())
            .ok_or(DbError::QueryFailed)?;
        Ok((version, readonly))
    }

    /// One HTTP request with this session's settings. Every request has a
    /// fresh `query_id` so it can be killed from a separate request.
    fn request(
        &self,
        query_id: &str,
        read_only: bool,
        bindings: &[DbParameter],
    ) -> Result<reqwest::RequestBuilder, DbError> {
        let mut query = String::new();
        let mut pair = |name: &str, value: &[u8]| {
            if !query.is_empty() {
                query.push('&');
            }
            query.extend(percent_encode(name.as_bytes(), QUERY_VALUE));
            query.push('=');
            query.extend(percent_encode(value, QUERY_VALUE));
        };
        pair("query_id", query_id.as_bytes());
        if !self.database.is_empty() {
            pair("database", self.database.as_bytes());
        }
        pair("default_format", FORMAT.as_bytes());
        if !self.profile_read_only {
            // Inventory validation admits only fixed, bounded setting names.
            for (name, value) in &self.settings {
                pair(name, value.to_string().as_bytes());
            }
            pair(
                "max_execution_time",
                self.timeout.as_secs().to_string().as_bytes(),
            );
            pair("output_format_json_quote_64bit_integers", b"1");
            pair("output_format_json_quote_decimals", b"1");
            pair("output_format_json_quote_denormals", b"1");
            pair("output_format_json_validate_utf8", b"1");
            // A failure after the first rows arrives as plain text, which can
            // never parse as a row (the default writes `["Code: ..."]`).
            pair("http_write_exception_in_output_format", b"0");
            if read_only || self.production {
                // readonly=2: no writes, no DDL, and `readonly` itself is frozen.
                pair("readonly", b"2");
                pair("max_result_rows", RESULT_ROW_CAP.to_string().as_bytes());
                pair("result_overflow_mode", b"break");
                pair("cancel_http_readonly_queries_on_client_close", b"1");
            }
        }
        for binding in bindings {
            if !valid_parameter_name(&binding.name) {
                return Err(DbError::InvalidRequest);
            }
            pair(
                &format!("param_{}", binding.name),
                &parameter_value(&binding.value)?,
            );
        }
        let mut url = self.endpoint.clone();
        url.set_query(Some(&query));
        let mut user = HeaderValue::from_bytes(self.user.as_bytes())
            .map_err(|_| DbError::AuthenticationFailed)?;
        user.set_sensitive(true);
        let mut request = self.client.post(url).header("X-ClickHouse-User", user);
        if let Some(password) = &self.password {
            let mut key = HeaderValue::from_bytes(password.expose_secret().as_bytes())
                .map_err(|_| DbError::AuthenticationFailed)?;
            key.set_sensitive(true);
            request = request.header("X-ClickHouse-Key", key);
        }
        if self.certificate_auth {
            request = request.header("X-ClickHouse-SSL-Certificate-Auth", "on");
        }
        Ok(request)
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
    ) -> BoxFuture<'a, Result<(), DbError>> {
        let sql = sql.to_owned();
        Box::pin(async move {
            let started = Instant::now();
            let query_id = Uuid::new_v4().to_string();
            let request = self.request(&query_id, read_only, bindings)?.body(sql);
            let operation = stream(request, self.profile_read_only, &mut *sink);
            // The server enforces max_execution_time; the client deadline is a
            // backstop (and the only one under a readonly=1 profile).
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(DbError::Cancelled),
                result = tokio::time::timeout(self.timeout + Duration::from_secs(1), operation) =>
                    result.unwrap_or(Err(DbError::Timeout)),
            };
            match result {
                Ok(outcome) => {
                    if outcome.stopped {
                        self.kill(query_id).await;
                    }
                    sink.statement_done(
                        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                        outcome.affected,
                    )
                    .await
                }
                Err(error) => {
                    // The response stream is already dropped; stop the server
                    // query too, not only the waiting Rust future.
                    if matches!(
                        error,
                        DbError::Cancelled | DbError::Timeout | DbError::ResultLimit
                    ) {
                        self.kill(query_id).await;
                    }
                    Err(error)
                }
            }
        })
    }

    /// `KILL QUERY` on a separate request. Best effort, bounded.
    fn kill(&self, query_id: String) -> BoxFuture<'static, ()> {
        let binding = DbParameter {
            name: "cc_query_id".into(),
            value: DbCell::Text(query_id),
            type_hint: None,
        };
        let request = self
            .request(
                &Uuid::new_v4().to_string(),
                true,
                std::slice::from_ref(&binding),
            )
            .map(|request| request.body("KILL QUERY WHERE query_id = {cc_query_id:String} ASYNC"));
        Box::pin(async move {
            if let Ok(request) = request {
                let _ = tokio::time::timeout(KILL_TIMEOUT, async move {
                    if let Ok(response) = request.send().await {
                        let _ = response.bytes().await;
                    }
                })
                .await;
            }
        })
    }

    /// `SHOW CREATE` text for a database, table, view or dictionary.
    pub fn ddl<'a>(
        &'a mut self,
        object: &'a DbObjectRef,
    ) -> BoxFuture<'a, Result<String, DbError>> {
        Box::pin(async move {
            let sql = ddl_statement(object)?;
            let mut rows = Rows::default();
            self.query(&sql, &[], true, &mut rows, CancellationToken::new())
                .await?;
            rows.rows
                .first()
                .and_then(|row| row.first())
                .map(text)
                .ok_or(DbError::QueryFailed)
        })
    }

    /// Databases → tables, views and dictionaries (engine) → columns, skip
    /// indexes and projections. Names travel as bound parameters.
    pub fn browse<'a>(
        &'a mut self,
        parent: Option<&'a DbObjectRef>,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<DbTreePage, DbError>> {
        Box::pin(async move {
            let string = |name: &str, value: &str| DbParameter {
                name: name.into(),
                value: DbCell::Text(value.into()),
                type_hint: None,
            };
            let Some(parent) = parent else {
                let rows = self
                    .catalogue(
                        "SELECT name FROM system.databases \
                         WHERE name NOT IN ('INFORMATION_SCHEMA', 'information_schema') \
                         ORDER BY name = 'system', name LIMIT 10000",
                        &[],
                    )
                    .await?;
                return Ok(DbTreePage {
                    nodes: rows
                        .iter()
                        .map(|row| {
                            node(
                                object(&row[0], DbObjectKind::Database, &row[0], None),
                                true,
                                None,
                            )
                        })
                        .collect(),
                    next_cursor: None,
                });
            };
            match parent.kind {
                DbObjectKind::Database => {
                    let mut rows = self
                        .catalogue(
                            &format!(
                                "SELECT name, engine FROM system.tables \
                                 WHERE database = {{cc_database:String}} AND NOT is_temporary \
                                 AND name > {{cc_after:String}} ORDER BY name LIMIT {}",
                                DEFAULT_TREE_PAGE + 1
                            ),
                            &[
                                string("cc_database", &parent.name),
                                string("cc_after", cursor.unwrap_or_default()),
                            ],
                        )
                        .await?;
                    let next_cursor = (rows.len() > DEFAULT_TREE_PAGE).then(|| {
                        rows.truncate(DEFAULT_TREE_PAGE);
                        rows[DEFAULT_TREE_PAGE - 1][0].clone()
                    });
                    Ok(DbTreePage {
                        nodes: rows
                            .iter()
                            .map(|row| {
                                node(
                                    object(&parent.name, relation_kind(&row[1]), &row[0], None),
                                    true,
                                    Some(row[1].clone()),
                                )
                            })
                            .collect(),
                        next_cursor,
                    })
                }
                DbObjectKind::Table
                | DbObjectKind::View
                | DbObjectKind::MaterializedView
                | DbObjectKind::Dictionary => {
                    let bindings = [
                        string("cc_database", &parent.database),
                        string("cc_table", &parent.name),
                    ];
                    let table = Some(parent.name.clone());
                    let columns = self
                        .catalogue(
                            "SELECT name, type, default_kind, default_expression, \
                             toString(is_in_sorting_key) FROM system.columns \
                             WHERE database = {cc_database:String} AND table = {cc_table:String} \
                             ORDER BY position LIMIT 10000",
                            &bindings,
                        )
                        .await?;
                    let mut nodes: Vec<_> = columns
                        .iter()
                        .map(|row| DbTreeNode {
                            reference: object(
                                &parent.database,
                                DbObjectKind::Column,
                                &row[0],
                                table.clone(),
                            ),
                            has_children: false,
                            detail: Some(row[1].clone()),
                            nullable: Some(column_kind(&row[1]).1),
                            default_value: match row[2].as_str() {
                                _ if row[3].is_empty() => None,
                                "" | "DEFAULT" => Some(row[3].clone()),
                                kind => Some(format!("{kind} {}", row[3])),
                            },
                            primary_key: row[4] == "1",
                            foreign_key: None,
                        })
                        .collect();
                    if parent.kind == DbObjectKind::Table {
                        let indexes = self
                            .catalogue(
                                "SELECT name, type_full, expr FROM system.data_skipping_indices \
                                 WHERE database = {cc_database:String} AND table = {cc_table:String} \
                                 ORDER BY name LIMIT 10000",
                                &bindings,
                            )
                            .await?;
                        nodes.extend(indexes.iter().map(|row| {
                            node(
                                object(
                                    &parent.database,
                                    DbObjectKind::Index,
                                    &row[0],
                                    table.clone(),
                                ),
                                false,
                                Some(format!("{} ({})", row[1], row[2])),
                            )
                        }));
                        // system.projections is newer than some supported servers.
                        if let Ok(projections) = self
                            .catalogue(
                                "SELECT name, type FROM system.projections \
                                 WHERE database = {cc_database:String} AND table = {cc_table:String} \
                                 ORDER BY name LIMIT 10000",
                                &bindings,
                            )
                            .await
                        {
                            nodes.extend(projections.iter().map(|row| {
                                node(
                                    object(
                                        &parent.database,
                                        DbObjectKind::Projection,
                                        &row[0],
                                        table.clone(),
                                    ),
                                    false,
                                    Some(row[1].clone()),
                                )
                            }));
                        }
                    }
                    Ok(DbTreePage {
                        nodes,
                        next_cursor: None,
                    })
                }
                _ => Ok(DbTreePage {
                    nodes: Vec::new(),
                    next_cursor: None,
                }),
            }
        })
    }

    fn catalogue<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [DbParameter],
    ) -> BoxFuture<'a, Result<Vec<Vec<String>>, DbError>> {
        Box::pin(async move {
            let mut rows = Rows::default();
            self.query(sql, bindings, true, &mut rows, CancellationToken::new())
                .await?;
            Ok(rows
                .rows
                .iter()
                .map(|row| row.iter().map(text).collect())
                .collect())
        })
    }
}

/// Streams one response into the sink. The first two lines name and type the
/// columns; an empty body is a statement without a result set.
fn stream<'a>(
    request: reqwest::RequestBuilder,
    exception_rows: bool,
    sink: &'a mut (dyn RowConsumer + Send),
) -> BoxFuture<'a, Result<Outcome, DbError>> {
    Box::pin(async move {
        let response = request.send().await.map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(server_error(response).await);
        }
        let written = response
            .headers()
            .get("X-ClickHouse-Summary")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .and_then(|summary| {
                summary["written_rows"]
                    .as_str()
                    .and_then(|rows| rows.parse::<u64>().ok())
            });
        let mut body = response.bytes_stream();
        let mut decoder = Decoder {
            exception_rows,
            ..Decoder::default()
        };
        let stopped = Outcome {
            affected: None,
            stopped: true,
        };
        let mut buffer: Vec<u8> = Vec::new();
        let mut start = 0;
        loop {
            if let Some(offset) = buffer[start..].iter().position(|byte| *byte == b'\n') {
                let line = buffer[start..start + offset].trim_ascii();
                start += offset + 1;
                if !line.is_empty() && !decoder.line(line, sink).await? {
                    return Ok(stopped);
                }
                continue;
            }
            // One row line is bounded; consumed lines are dropped per chunk.
            if buffer.len() - start > 2 * MAX_EVENT_BYTES {
                return Err(DbError::ResultLimit);
            }
            buffer.drain(..start);
            start = 0;
            match body.next().await {
                Some(chunk) => buffer.extend_from_slice(&chunk.map_err(transport_error)?),
                None => {
                    let line = buffer.trim_ascii();
                    if !line.is_empty() && !decoder.line(line, sink).await? {
                        return Ok(stopped);
                    }
                    break;
                }
            }
        }
        match decoder.kinds {
            Some(_) => Ok(Outcome {
                affected: None,
                stopped: false,
            }),
            None if decoder.names.is_none() => {
                sink.start(Vec::new(), None).await?;
                Ok(Outcome {
                    affected: written,
                    stopped: false,
                })
            }
            // A names line without types: the response was cut off.
            None => Err(DbError::QueryFailed),
        }
    })
}

#[derive(Default)]
struct Decoder {
    /// The server writes a mid-stream failure as a one-string row (readonly=1
    /// profiles cannot switch that off).
    exception_rows: bool,
    names: Option<Vec<String>>,
    kinds: Option<Vec<Kind>>,
}

impl Decoder {
    /// false when the consumer asks to stop.
    async fn line(
        &mut self,
        line: &[u8],
        sink: &mut (dyn RowConsumer + Send),
    ) -> Result<bool, DbError> {
        // A mid-stream server exception is not a JSON array and fails here.
        let line = String::from_utf8_lossy(line);
        match (&self.names, &self.kinds) {
            (None, _) => {
                self.names = Some(serde_json::from_str(&line).map_err(|_| DbError::QueryFailed)?);
                Ok(true)
            }
            (Some(names), None) => {
                let types: Vec<String> =
                    serde_json::from_str(&line).map_err(|_| DbError::QueryFailed)?;
                if types.len() != names.len() {
                    return Err(DbError::QueryFailed);
                }
                let columns = names
                    .iter()
                    .zip(&types)
                    .map(|(name, type_name)| DbColumn {
                        name: name.clone(),
                        type_name: type_name.clone(),
                        nullable: column_kind(type_name).1,
                        sensitive: crate::sensitive_column(name),
                        primary_key: false,
                    })
                    .collect();
                self.kinds = Some(types.iter().map(|name| column_kind(name).0).collect());
                sink.start(columns, None).await?;
                Ok(true)
            }
            (Some(_), Some(kinds)) => {
                let values: Vec<&RawValue> =
                    serde_json::from_str(&line).map_err(|_| DbError::QueryFailed)?;
                if self.exception_rows && values.len() == 1 && exception_row(values[0]) {
                    return Err(DbError::QueryFailed);
                }
                if values.len() != kinds.len() {
                    return Err(DbError::QueryFailed);
                }
                let row = values
                    .iter()
                    .zip(kinds)
                    .map(|(value, kind)| cell(value, *kind))
                    .collect::<Result<Vec<_>, _>>()?;
                sink.row(row).await
            }
        }
    }
}

fn exception_row(value: &RawValue) -> bool {
    let raw = value.get();
    raw.starts_with("\"Code: ") && raw.contains("DB::Exception")
}

/// Cell family of a ClickHouse type name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Int,
    Uint,
    Decimal,
    Float,
    Bool,
    Text,
    Date,
    Time,
    Timestamp,
    Json,
}

/// The cell family and whether the type admits NULL.
fn column_kind(type_name: &str) -> (Kind, bool) {
    let mut name = type_name.trim();
    let mut nullable = false;
    loop {
        if let Some(inner) = wrapped(name, "LowCardinality") {
            name = inner;
        } else if let Some(inner) = wrapped(name, "Nullable") {
            name = inner;
            nullable = true;
        } else {
            break;
        }
    }
    let base = name.split('(').next().unwrap_or_default().trim();
    let kind = match base {
        "Int8" | "Int16" | "Int32" | "Int64" | "Int128" | "Int256" => Kind::Int,
        "UInt8" | "UInt16" | "UInt32" | "UInt64" | "UInt128" | "UInt256" => Kind::Uint,
        "Float32" | "Float64" | "BFloat16" => Kind::Float,
        "Decimal" | "Decimal32" | "Decimal64" | "Decimal128" | "Decimal256" => Kind::Decimal,
        "Bool" => Kind::Bool,
        "Date" | "Date32" => Kind::Date,
        "DateTime" | "DateTime64" => Kind::Timestamp,
        "Time" | "Time64" => Kind::Time,
        "Array" | "Map" | "Tuple" | "Nested" | "JSON" | "Object" | "Variant" | "Dynamic"
        | "Point" | "Ring" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon" => {
            Kind::Json
        }
        _ => Kind::Text,
    };
    (kind, nullable)
}

fn wrapped<'a>(name: &'a str, wrapper: &str) -> Option<&'a str> {
    name.strip_prefix(wrapper)?
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')
}

/// One JSON cell. Numbers keep their exact wire spelling; 64-bit and wider
/// integers, decimals and non-finite floats arrive quoted.
fn cell(value: &RawValue, kind: Kind) -> Result<DbCell, DbError> {
    let raw = value.get();
    if raw == "null" {
        return Ok(DbCell::Null);
    }
    if kind == Kind::Json {
        return Ok(DbCell::Json(raw.to_owned()));
    }
    let scalar = if raw.starts_with('"') {
        serde_json::from_str::<String>(raw).map_err(|_| DbError::QueryFailed)?
    } else {
        raw.to_owned()
    };
    Ok(match kind {
        Kind::Int => DbCell::Int(scalar),
        Kind::Uint => DbCell::Uint(scalar),
        Kind::Decimal => DbCell::Decimal(scalar),
        Kind::Float => DbCell::Float(scalar),
        Kind::Bool => match raw {
            "true" => DbCell::Bool(true),
            "false" => DbCell::Bool(false),
            _ => DbCell::Text(scalar),
        },
        Kind::Date => DbCell::Date(scalar),
        Kind::Time => DbCell::Time(scalar),
        Kind::Timestamp => DbCell::Timestamp(scalar),
        Kind::Text | Kind::Json => DbCell::Text(scalar),
    })
}

/// A query parameter value in ClickHouse's escaped text format.
fn parameter_value(value: &DbCell) -> Result<Vec<u8>, DbError> {
    Ok(match value {
        DbCell::Null => b"\\N".to_vec(),
        DbCell::Bool(value) => value.to_string().into_bytes(),
        DbCell::Binary(value) => escape(
            &STANDARD
                .decode(value)
                .map_err(|_| DbError::InvalidRequest)?,
        ),
        DbCell::Int(value)
        | DbCell::Uint(value)
        | DbCell::Decimal(value)
        | DbCell::Float(value)
        | DbCell::Text(value)
        | DbCell::Date(value)
        | DbCell::Time(value)
        | DbCell::Timestamp(value)
        | DbCell::Json(value) => escape(value.as_bytes()),
    })
}

fn escape(bytes: &[u8]) -> Vec<u8> {
    let mut escaped = Vec::with_capacity(bytes.len());
    for byte in bytes {
        match byte {
            b'\\' => escaped.extend_from_slice(b"\\\\"),
            b'\t' => escaped.extend_from_slice(b"\\t"),
            b'\n' => escaped.extend_from_slice(b"\\n"),
            b'\r' => escaped.extend_from_slice(b"\\r"),
            b'\0' => escaped.extend_from_slice(b"\\0"),
            0x08 => escaped.extend_from_slice(b"\\b"),
            0x0c => escaped.extend_from_slice(b"\\f"),
            _ => escaped.push(*byte),
        }
    }
    escaped
}

fn valid_parameter_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// A back-quoted identifier; ClickHouse unescapes backslash sequences inside.
pub(crate) fn quote_identifier(name: &str) -> Result<String, DbError> {
    if name.is_empty() || name.len() > 1024 {
        return Err(DbError::InvalidRequest);
    }
    let mut quoted = String::with_capacity(name.len() + 2);
    quoted.push('`');
    for character in name.chars() {
        match character {
            '`' => quoted.push_str("\\`"),
            '\\' => quoted.push_str("\\\\"),
            '\0' => quoted.push_str("\\0"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character => quoted.push(character),
        }
    }
    quoted.push('`');
    Ok(quoted)
}

fn ddl_statement(object: &DbObjectRef) -> Result<String, DbError> {
    let keyword = match object.kind {
        DbObjectKind::Database => {
            return Ok(format!(
                "SHOW CREATE DATABASE {}",
                quote_identifier(&object.name)?
            ))
        }
        DbObjectKind::Table => "TABLE",
        DbObjectKind::View | DbObjectKind::MaterializedView => "VIEW",
        DbObjectKind::Dictionary => "DICTIONARY",
        _ => return Err(DbError::InvalidRequest),
    };
    let name = if object.database.is_empty() {
        quote_identifier(&object.name)?
    } else {
        format!(
            "{}.{}",
            quote_identifier(&object.database)?,
            quote_identifier(&object.name)?
        )
    };
    Ok(format!("SHOW CREATE {keyword} {name}"))
}

fn relation_kind(engine: &str) -> DbObjectKind {
    match engine {
        "View" | "LiveView" | "WindowView" => DbObjectKind::View,
        "MaterializedView" => DbObjectKind::MaterializedView,
        "Dictionary" => DbObjectKind::Dictionary,
        _ => DbObjectKind::Table,
    }
}

fn object(
    database: &str,
    kind: DbObjectKind,
    name: &str,
    parent_name: Option<String>,
) -> DbObjectRef {
    DbObjectRef {
        database: database.into(),
        schema: None,
        kind,
        name: name.into(),
        parent_name,
    }
}

fn node(reference: DbObjectRef, has_children: bool, detail: Option<String>) -> DbTreeNode {
    DbTreeNode {
        reference,
        has_children,
        detail,
        nullable: None,
        default_value: None,
        primary_key: false,
        foreign_key: None,
    }
}

fn text(cell: &DbCell) -> String {
    match cell {
        DbCell::Null => String::new(),
        DbCell::Bool(value) => value.to_string(),
        DbCell::Int(value)
        | DbCell::Uint(value)
        | DbCell::Decimal(value)
        | DbCell::Float(value)
        | DbCell::Text(value)
        | DbCell::Date(value)
        | DbCell::Time(value)
        | DbCell::Timestamp(value)
        | DbCell::Binary(value)
        | DbCell::Json(value) => value.clone(),
    }
}

/// Maps a ClickHouse failure to a fixed error; server text never leaves.
async fn server_error(response: reqwest::Response) -> DbError {
    let header = response
        .headers()
        .get("X-ClickHouse-Exception-Code")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u32>().ok());
    let status = response.status();
    let code = match header {
        Some(code) => Some(code),
        None => {
            // Older servers: "Code: 164. DB::Exception: ...". Read a bounded prefix.
            let mut body = response.bytes_stream();
            let mut prefix = Vec::new();
            while prefix.len() < 64 {
                match body.next().await {
                    Some(Ok(chunk)) => prefix.extend_from_slice(&chunk),
                    _ => break,
                }
            }
            std::str::from_utf8(&prefix[..prefix.len().min(64)])
                .ok()
                .and_then(|text| text.strip_prefix("Code: "))
                .and_then(|text| text.split('.').next())
                .and_then(|code| code.trim().parse().ok())
        }
    };
    match code {
        Some(516 | 192 | 193 | 194) => DbError::AuthenticationFailed,
        Some(164 | 497 | 452) => DbError::PolicyDenied,
        Some(394) => DbError::Cancelled,
        Some(159 | 209) => DbError::Timeout,
        Some(158 | 396) => DbError::ResultLimit,
        None if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN =>
        {
            DbError::AuthenticationFailed
        }
        _ => DbError::QueryFailed,
    }
}

/// Transport failures carry no request detail (reqwest errors embed the URL).
fn transport_error(error: reqwest::Error) -> DbError {
    let error = error.without_url();
    if error.is_timeout() {
        DbError::Timeout
    } else if tls_cause(&error) {
        DbError::TlsFailed
    } else {
        DbError::ConnectionFailed
    }
}

/// io::Error::source() skips its wrapped error, so descend through get_ref().
fn tls_cause(error: &(dyn std::error::Error + 'static)) -> bool {
    if error.is::<rustls::Error>() {
        return true;
    }
    if let Some(inner) = error
        .downcast_ref::<std::io::Error>()
        .and_then(|error| error.get_ref())
    {
        if tls_cause(inner) {
            return true;
        }
    }
    error.source().is_some_and(tls_cause)
}

/// TLS verification against the configured identity regardless of the
/// loopback address the HTTP client dialled.
#[derive(Debug)]
struct PinnedName {
    inner: Arc<WebPkiServerVerifier>,
    name: ServerName<'static>,
}

impl ServerCertVerifier for PinnedName {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.inner
            .verify_server_cert(end_entity, intermediates, &self.name, ocsp_response, now)
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

/// Collects small driver-internal results (version, catalogue, DDL).
#[derive(Default)]
struct Rows {
    rows: Vec<Vec<DbCell>>,
}

#[async_trait]
impl RowConsumer for Rows {
    async fn start(&mut self, _: Vec<DbColumn>, _: Option<DbObjectRef>) -> Result<(), DbError> {
        Ok(())
    }

    async fn row(&mut self, row: Vec<DbCell>) -> Result<bool, DbError> {
        if self.rows.len() >= DEFAULT_TREE_PAGE * 50 {
            return Ok(false);
        }
        self.rows.push(row);
        Ok(true)
    }

    async fn statement_done(&mut self, _: u64, _: Option<u64>) -> Result<(), DbError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str) -> Box<RawValue> {
        RawValue::from_string(text.into()).unwrap()
    }

    #[test]
    fn types_map_to_cell_families_and_nullability() {
        for (name, kind, nullable) in [
            ("UInt64", Kind::Uint, false),
            ("Int256", Kind::Int, false),
            ("Nullable(Int8)", Kind::Int, true),
            ("LowCardinality(Nullable(String))", Kind::Text, true),
            ("LowCardinality(String)", Kind::Text, false),
            ("Decimal(18, 3)", Kind::Decimal, false),
            ("Decimal128(4)", Kind::Decimal, false),
            ("Float32", Kind::Float, false),
            ("Bool", Kind::Bool, false),
            ("Date32", Kind::Date, false),
            ("DateTime('Europe/Moscow')", Kind::Timestamp, false),
            ("Nullable(DateTime64(3, 'UTC'))", Kind::Timestamp, true),
            ("Array(Nullable(UInt8))", Kind::Json, false),
            ("Map(String, UInt64)", Kind::Json, false),
            ("Tuple(a UInt8, b String)", Kind::Json, false),
            ("JSON", Kind::Json, false),
            ("UUID", Kind::Text, false),
            ("IPv6", Kind::Text, false),
            ("Enum8('a' = 1, 'b' = 2)", Kind::Text, false),
            ("FixedString(16)", Kind::Text, false),
        ] {
            assert_eq!(column_kind(name), (kind, nullable), "{name}");
        }
    }

    #[test]
    fn cells_keep_wire_spelling() {
        assert_eq!(
            cell(&raw("\"18446744073709551615\""), Kind::Uint).unwrap(),
            DbCell::Uint("18446744073709551615".into())
        );
        assert_eq!(
            cell(&raw("-12"), Kind::Int).unwrap(),
            DbCell::Int("-12".into())
        );
        // Unquoted decimals (readonly=1 profiles cannot request quoting) keep every digit.
        assert_eq!(
            cell(&raw("12345678901234567890.123456789"), Kind::Decimal).unwrap(),
            DbCell::Decimal("12345678901234567890.123456789".into())
        );
        assert_eq!(
            cell(&raw("\"nan\""), Kind::Float).unwrap(),
            DbCell::Float("nan".into())
        );
        assert_eq!(cell(&raw("true"), Kind::Bool).unwrap(), DbCell::Bool(true));
        assert_eq!(cell(&raw("null"), Kind::Int).unwrap(), DbCell::Null);
        assert_eq!(
            cell(&raw("\"2024-01-02 03:04:05.123\""), Kind::Timestamp).unwrap(),
            DbCell::Timestamp("2024-01-02 03:04:05.123".into())
        );
        assert_eq!(
            cell(&raw("\"a\\tb\""), Kind::Text).unwrap(),
            DbCell::Text("a\tb".into())
        );
        assert_eq!(
            cell(&raw("{\"a\":[1,\"2\"]}"), Kind::Json).unwrap(),
            DbCell::Json("{\"a\":[1,\"2\"]}".into())
        );
    }

    #[test]
    fn mid_stream_failures_never_become_rows() {
        let failure = raw("\"Code: 395. DB::Exception: Value passed to 'throwIf' (X)\"");
        assert!(exception_row(&failure));
        assert!(!exception_row(&raw("\"Code: none\"")));
        assert!(!exception_row(&raw("395")));
    }

    #[test]
    fn identifiers_are_quoted_and_escaped() {
        assert_eq!(quote_identifier("events").unwrap(), "`events`");
        assert_eq!(quote_identifier("a`b").unwrap(), "`a\\`b`");
        assert_eq!(quote_identifier("a\\`; DROP").unwrap(), "`a\\\\\\`; DROP`");
        assert_eq!(quote_identifier("x\ny").unwrap(), "`x\\ny`");
        assert!(quote_identifier("").is_err());
        let object = DbObjectRef {
            database: "my db".into(),
            schema: None,
            kind: DbObjectKind::Dictionary,
            name: "d`x".into(),
            parent_name: None,
        };
        assert_eq!(
            ddl_statement(&object).unwrap(),
            "SHOW CREATE DICTIONARY `my db`.`d\\`x`"
        );
        let column = DbObjectRef {
            kind: DbObjectKind::Column,
            ..object
        };
        assert!(ddl_statement(&column).is_err());
    }

    #[test]
    fn parameters_use_the_escaped_text_format() {
        assert_eq!(parameter_value(&DbCell::Null).unwrap(), b"\\N");
        assert_eq!(parameter_value(&DbCell::Bool(true)).unwrap(), b"true");
        assert_eq!(
            parameter_value(&DbCell::Text("a\\b\tc\nd'e".into())).unwrap(),
            b"a\\\\b\\tc\\nd'e"
        );
        assert_eq!(
            parameter_value(&DbCell::Binary(STANDARD.encode([0u8, 0xff]))).unwrap(),
            b"\\0\xff"
        );
        assert!(valid_parameter_name("ccp1"));
        assert!(!valid_parameter_name("a&b"));
        assert!(!valid_parameter_name("1a"));
    }

    #[test]
    fn debug_output_never_shows_credentials() {
        let password = SecretString::from("canary-password".to_owned());
        assert!(!format!("{password:?}").contains("canary"));
    }

    /// The server layer alone refuses writes: the guard is bypassed and the
    /// driver is called directly with a production (readonly=2) session.
    /// Opt-in: CC_DB_LIVE_CLICKHOUSE=host:port:user:password:database.
    #[tokio::test]
    async fn server_refuses_writes_without_the_client_guard() {
        let Ok(spec) = std::env::var("CC_DB_LIVE_CLICKHOUSE") else {
            eprintln!("CC_DB_LIVE_CLICKHOUSE not set; skipped");
            return;
        };
        let parts: Vec<&str> = spec.splitn(5, ':').collect();
        let mut connection =
            DbConnection::new("live", parts[0], cc_models::database::DbEngine::Clickhouse);
        connection.port = parts[1].parse().unwrap();
        connection.username = parts[2].into();
        connection.database = parts[4].into();
        connection.tls.mode = DbTlsMode::Disable;
        connection.production = true;
        let credentials = DbCredentials {
            password: Some(SecretString::from(parts[3].to_owned())),
            client_key_pem: None,
        };
        let open = |connection: DbConnection| {
            let credentials = DbCredentials {
                password: credentials.password.clone(),
                client_key_pem: None,
            };
            async move {
                ClickHouse::connect(
                    &connection,
                    &credentials,
                    Arc::new(crate::transport::DirectConnector),
                    CancellationToken::new(),
                )
                .await
                .unwrap()
            }
        };
        async fn run(driver: &mut ClickHouse, sql: &str, read_only: bool) -> Result<(), DbError> {
            let mut rows = Rows::default();
            driver
                .query(sql, &[], read_only, &mut rows, CancellationToken::new())
                .await
        }
        let connection_template = connection.clone();
        let mut development = open(DbConnection {
            production: false,
            ..connection.clone()
        })
        .await;
        run(
            &mut development,
            "DROP TABLE IF EXISTS cc_server_probe",
            false,
        )
        .await
        .unwrap();
        run(
            &mut development,
            "CREATE TABLE cc_server_probe (id UInt8) ENGINE = MergeTree ORDER BY id",
            false,
        )
        .await
        .unwrap();
        let mut driver = open(connection).await;
        assert!(!format!("{driver:?}").contains(parts[3]));
        for sql in [
            "INSERT INTO cc_server_probe VALUES (1)",
            "INSERT INTO cc_server_probe SELECT 2",
            "ALTER TABLE cc_server_probe DELETE WHERE 1",
            "ALTER TABLE cc_server_probe ADD COLUMN x UInt8",
            "CREATE TABLE cc_server_other (id UInt8) ENGINE = Memory",
            "DROP TABLE cc_server_probe",
            "TRUNCATE TABLE cc_server_probe",
            "OPTIMIZE TABLE cc_server_probe FINAL",
            "SYSTEM DROP DNS CACHE",
            "SET readonly = 0",
            "SELECT 1 SETTINGS readonly = 0",
        ] {
            // Production keeps readonly=2 even for a request marked as a write.
            for read_only in [true, false] {
                assert_eq!(
                    run(&mut driver, sql, read_only).await,
                    Err(DbError::PolicyDenied),
                    "{sql}"
                );
            }
        }
        // A read-only request is refused writes in a development session too.
        assert_eq!(
            run(
                &mut development,
                "INSERT INTO cc_server_probe VALUES (1)",
                true
            )
            .await,
            Err(DbError::PolicyDenied)
        );
        // DDL text comes back through quoted identifiers, also read-only.
        run(
            &mut development,
            "CREATE OR REPLACE VIEW `cc server``view` AS SELECT id FROM cc_server_probe",
            false,
        )
        .await
        .unwrap();
        let reference = |kind, name: &str| DbObjectRef {
            database: parts[4].into(),
            schema: None,
            kind,
            name: name.into(),
            parent_name: None,
        };
        let table = driver
            .ddl(&reference(DbObjectKind::Table, "cc_server_probe"))
            .await
            .unwrap();
        assert!(table.starts_with("CREATE TABLE"), "{table}");
        assert!(table.contains("MergeTree"), "{table}");
        let view = driver
            .ddl(&reference(DbObjectKind::View, "cc server`view"))
            .await
            .unwrap();
        assert!(view.starts_with("CREATE VIEW"), "{view}");
        let database = driver
            .ddl(&reference(DbObjectKind::Database, parts[4]))
            .await
            .unwrap();
        assert!(database.starts_with("CREATE DATABASE"), "{database}");
        assert_eq!(
            driver
                .ddl(&reference(DbObjectKind::Table, "cc_missing`; DROP TABLE x"))
                .await,
            Err(DbError::QueryFailed)
        );
        run(&mut development, "DROP VIEW `cc server``view`", false)
            .await
            .unwrap();
        // A failure after rows were streamed is an error, never a data row.
        let failing = [
            "SELECT throwIf(number = 50000) FROM numbers(100000)",
            "SELECT number, throwIf(number = 50000) FROM numbers(100000)",
        ];
        for sql in failing {
            // Small blocks: the first rows are streamed before the failure.
            let sql = format!("{sql} SETTINGS max_block_size = 1000");
            let mut all = All(0);
            assert_eq!(
                development
                    .query(&sql, &[], true, &mut all, CancellationToken::new())
                    .await,
                Err(DbError::QueryFailed),
                "{sql}"
            );
            assert!(all.0 > 0, "{sql}");
        }
        // A readonly=1 settings profile refuses every setting, readonly=2
        // included; the session still opens, read-only on the server.
        run(
            &mut development,
            "CREATE USER OR REPLACE cc_profile_probe IDENTIFIED BY 'cc-profile-probe' \
             SETTINGS readonly = 1, max_block_size = 1000",
            false,
        )
        .await
        .unwrap();
        run(
            &mut development,
            "GRANT SELECT ON *.* TO cc_profile_probe",
            false,
        )
        .await
        .unwrap();
        let mut profile = ClickHouse::connect(
            &DbConnection {
                username: "cc_profile_probe".into(),
                ..connection_template.clone()
            },
            &DbCredentials {
                password: Some(SecretString::from("cc-profile-probe".to_owned())),
                client_key_pem: None,
            },
            Arc::new(crate::transport::DirectConnector),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(profile.profile_read_only);
        assert!(!profile.version.is_empty());
        let mut rows = Rows::default();
        profile
            .query(
                "SELECT count() FROM cc_server_probe",
                &[],
                true,
                &mut rows,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(rows.rows, vec![vec![DbCell::Uint("0".into())]]);
        assert_eq!(
            run(
                &mut profile,
                "INSERT INTO cc_server_probe VALUES (1)",
                false
            )
            .await,
            Err(DbError::PolicyDenied)
        );
        for sql in failing {
            let mut all = All(0);
            assert_eq!(
                profile
                    .query(sql, &[], true, &mut all, CancellationToken::new())
                    .await,
                Err(DbError::QueryFailed),
                "{sql}"
            );
            assert!(all.0 > 0, "{sql}");
        }
        run(&mut development, "DROP USER cc_profile_probe", false)
            .await
            .unwrap();
        // Note: readonly=2 does not cover INSERT INTO FUNCTION (url(), s3(), ...);
        // only the client guard refuses those (covered in tests/clickhouse_live.rs).
        run(&mut development, "DROP TABLE cc_server_probe", false)
            .await
            .unwrap();
    }

    /// Dials one local address whatever the destination, like an SSH tunnel.
    #[derive(Debug)]
    struct Tunnel(std::net::SocketAddr);

    #[async_trait]
    impl DbConnector for Tunnel {
        async fn connect(
            &self,
            _: &DbDestination,
            _: CancellationToken,
        ) -> Result<crate::transport::DbIo, DbError> {
            let stream = tokio::net::TcpStream::connect(self.0)
                .await
                .map_err(|_| DbError::ConnectionFailed)?;
            Ok(Box::new(stream))
        }
    }

    /// What the fake server saw: SNI, request line and whether the password
    /// arrived in its header.
    #[derive(Debug, Default, Clone)]
    struct Seen {
        sni: Option<String>,
        target: String,
        key_header: bool,
    }

    /// A TLS "ClickHouse" answering the connection probe, with a certificate
    /// for `names` issued by the returned CA.
    async fn tls_server(
        names: &[&str],
    ) -> (
        std::net::SocketAddr,
        String,
        Arc<std::sync::Mutex<Vec<Seen>>>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut authority = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        authority.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let authority_key = rcgen::KeyPair::generate().unwrap();
        let authority_pem = authority.self_signed(&authority_key).unwrap().pem();
        let issuer = rcgen::Issuer::new(authority, authority_key);
        let key = rcgen::KeyPair::generate().unwrap();
        let certificate = rcgen::CertificateParams::new(
            names
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap()
        .signed_by(&key, &issuer)
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
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let Ok(mut stream) = acceptor.accept(socket).await else {
                    continue;
                };
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(count) => request.extend_from_slice(&buffer[..count]),
                    }
                }
                let head = String::from_utf8_lossy(&request).to_string();
                log.lock().unwrap().push(Seen {
                    sni: stream.get_ref().1.server_name().map(str::to_owned),
                    target: head.lines().next().unwrap_or_default().to_owned(),
                    key_header: head
                        .to_ascii_lowercase()
                        .contains("x-clickhouse-key: cc-test-ch"),
                });
                let body =
                    "[\"version()\",\"readonly\"]\n[\"String\",\"UInt64\"]\n[\"25.8.0\",\"2\"]\n";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        (address, authority_pem, seen)
    }

    async fn connect_tls(
        address: std::net::SocketAddr,
        host: &str,
        server_name: Option<&str>,
        mode: DbTlsMode,
        authority: &str,
    ) -> Result<ClickHouse, DbError> {
        let mut connection =
            DbConnection::new("tls", host, cc_models::database::DbEngine::Clickhouse);
        connection.username = "cc".into();
        connection.tls.mode = mode;
        connection.tls.certificate_authority_pem = Some(authority.into());
        connection.tls.server_name = server_name.map(str::to_owned);
        ClickHouse::connect(
            &connection,
            &DbCredentials {
                password: Some(SecretString::from("cc-test-ch".to_owned())),
                client_key_pem: None,
            },
            Arc::new(Tunnel(address)),
            CancellationToken::new(),
        )
        .await
    }

    #[tokio::test]
    async fn tls_verifies_the_database_name_through_a_tunnel() {
        // DNS identity: SNI and verification use the database host, while TCP
        // goes to the local relay. Credentials travel only in headers.
        let (address, authority, seen) = tls_server(&["db.example.test"]).await;
        let driver = connect_tls(
            address,
            "db.example.test",
            None,
            DbTlsMode::Verify,
            &authority,
        )
        .await
        .unwrap();
        assert_eq!(driver.version, "25.8.0");
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].sni.as_deref(), Some("db.example.test"));
        assert!(seen[0].key_header);
        assert!(seen[0].target.starts_with("POST /?query_id="));
        assert!(!seen[0].target.contains("cc-test-ch"));
        // A name override is honoured for an IP host.
        connect_tls(
            address,
            "10.1.2.3",
            Some("db.example.test"),
            DbTlsMode::Verify,
            &authority,
        )
        .await
        .unwrap();
        // A certificate for another name is refused, also via `prefer`.
        for mode in [DbTlsMode::Verify, DbTlsMode::Prefer] {
            assert_eq!(
                connect_tls(address, "other.example.test", None, mode, &authority)
                    .await
                    .err(),
                Some(DbError::TlsFailed)
            );
        }
        // `require` encrypts without checking the name.
        connect_tls(
            address,
            "other.example.test",
            None,
            DbTlsMode::Require,
            &authority,
        )
        .await
        .unwrap();

        // IP identity: no SNI, verification pinned to the database IP rather
        // than the loopback address the client dialled.
        let (address, authority, _) = tls_server(&["10.1.2.3"]).await;
        connect_tls(address, "10.1.2.3", None, DbTlsMode::Verify, &authority)
            .await
            .unwrap();
        assert_eq!(
            connect_tls(address, "10.9.9.9", None, DbTlsMode::Verify, &authority)
                .await
                .err(),
            Some(DbError::TlsFailed)
        );
        let (address, authority, _) = tls_server(&["127.0.0.1"]).await;
        assert_eq!(
            connect_tls(address, "10.1.2.3", None, DbTlsMode::Verify, &authority)
                .await
                .err(),
            Some(DbError::TlsFailed)
        );
    }

    /// Accepts every row.
    struct All(usize);

    #[async_trait]
    impl RowConsumer for All {
        async fn start(&mut self, _: Vec<DbColumn>, _: Option<DbObjectRef>) -> Result<(), DbError> {
            Ok(())
        }
        async fn row(&mut self, _: Vec<DbCell>) -> Result<bool, DbError> {
            self.0 += 1;
            Ok(true)
        }
        async fn statement_done(&mut self, _: u64, _: Option<u64>) -> Result<(), DbError> {
            Ok(())
        }
    }
}
