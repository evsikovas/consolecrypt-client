//! Bounded Prometheus read API. No administration, URL discovery, or credential resolver.
//! Dropping a request future cancels its HTTP operation and releases its concurrency permit.
#![forbid(unsafe_code)]

pub mod alerts;
mod auth;
mod config;
mod dto;
pub mod exporter_verification;
pub mod panels;
mod parse;
mod scoped_stream;
pub use alerts::{AlertResponse, AlertRulesResponse};
pub use auth::SourceAuthorization;
pub use config::*;
pub use dto::*;
pub use scoped_stream::{
    MonitoringConnect, MonitoringIo, MonitoringSourceConnector, MonitoringStream,
};

use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    redirect::Policy,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use url::Url;
use zeroize::Zeroizing;

pub struct MonitoringClient {
    source: SourceConfig,
    base: Url,
    http: reqwest::Client,
    scoped: Option<scoped_stream::ScopedHttp>,
    budgets: Budgets,
    permits: Arc<Semaphore>,
    credential: Option<SourceAuthorization>,
    cache: Mutex<HashMap<String, (Instant, LabelValues)>>,
}
impl fmt::Debug for MonitoringClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MonitoringClient")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
impl MonitoringClient {
    /// Anonymous connections only until AppCore supplies the scoped adapter.
    /// An auth reference fails closed; it is never treated as anonymous fallback.
    pub fn new(source: SourceConfig, budgets: Budgets) -> Result<Self, Error> {
        Self::build(source, budgets, None, None)
    }
    /// Rust-only AppCore boundary. Never bind material constructors to FRB.
    pub fn with_authorization(
        source: SourceConfig,
        budgets: Budgets,
        authorization: SourceAuthorization,
    ) -> Result<Self, Error> {
        Self::build(source, budgets, Some(authorization), None)
    }
    /// Rust-only original Native saved-host stream boundary; no JSON/FRB
    /// constructor or credential lookup. Direct/legacy routes cannot use it.
    pub fn with_scoped_connector(
        source: SourceConfig,
        budgets: Budgets,
        authorization: Option<SourceAuthorization>,
        connector: Arc<dyn MonitoringSourceConnector>,
    ) -> Result<Self, Error> {
        let scoped = scoped_stream::ScopedHttp::new(&source, connector)?;
        Self::build(source, budgets, authorization, Some(scoped))
    }
    fn build(
        source: SourceConfig,
        budgets: Budgets,
        credential: Option<SourceAuthorization>,
        scoped: Option<scoped_stream::ScopedHttp>,
    ) -> Result<Self, Error> {
        let base = source.validate()?;
        budgets.validate()?;
        if !matches!(
            (&source.route, &scoped),
            (Route::Direct, None) | (Route::SshHost { .. }, Some(_))
        ) {
            return Err(Error::new(
                ErrorCode::UnsupportedRoute,
                "Route requires an AppCore transport adapter",
            ));
        }
        if source.auth != AuthRef::None && credential.is_none() {
            return Err(Error::new(
                ErrorCode::AuthUnavailable,
                "Unlock and authorize this source through AppCore",
            ));
        }
        if credential.as_ref().is_some_and(|c| {
            c.source != source || source.auth == AuthRef::None || c.expires <= Instant::now()
        }) {
            return Err(Error::new(
                ErrorCode::AuthUnavailable,
                "Credential grant is invalid or expired",
            ));
        }
        let mut builder = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(budgets.timeout_seconds))
            .connect_timeout(Duration::from_secs(5))
            .no_proxy(); // configured endpoint only; no ambient proxy credential forwarding
        if let Some(pem) = &source.tls.custom_ca_pem {
            let certificates =
                reqwest::Certificate::from_pem_bundle(pem.as_bytes()).map_err(|_| {
                    Error::new(ErrorCode::InvalidConfig, "Invalid custom CA certificate")
                })?;
            if certificates.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidConfig,
                    "Empty custom CA certificate",
                ));
            }
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        }
        let http = builder
            .build()
            .map_err(|_| Error::new(ErrorCode::InvalidConfig, "Cannot initialize TLS client"))?;
        Ok(Self {
            source,
            base,
            http,
            scoped,
            permits: Arc::new(Semaphore::new(budgets.max_concurrent_requests)),
            budgets,
            credential,
            cache: Mutex::new(HashMap::new()),
        })
    }
    fn validate_query(&self, expression: &str) -> Result<(), Error> {
        if expression.trim().is_empty() || expression.len() > self.budgets.max_query_bytes {
            Err(Error::new(
                ErrorCode::InvalidQuery,
                "Empty query or expression exceeds budget",
            ))
        } else {
            Ok(())
        }
    }
    fn ensure_authorization(&self) -> Result<(), Error> {
        if self
            .credential
            .as_ref()
            .is_some_and(|c| c.expires <= Instant::now())
        {
            Err(Error::new(
                ErrorCode::AuthUnavailable,
                "Credential grant expired",
            ))
        } else {
            Ok(())
        }
    }
    pub async fn query(&self, expression: &str, time: Option<f64>) -> Result<QueryOutcome, Error> {
        self.validate_query(expression)?;
        if time.is_some_and(|t| !valid_timestamp(t)) {
            return Err(invalid_query());
        }
        let mut fields = vec![("query", expression.to_owned())];
        if let Some(time) = time {
            fields.push(("time", time.to_string()));
        }
        let (value, status) = self.request("api/v1/query", fields, true).await?;
        let outcome =
            parse::envelope(value, status, &self.budgets, None).map_err(|e| e.with_http(status))?;
        self.ensure_authorization()?;
        Ok(outcome)
    }
    pub async fn query_range(
        &self,
        expression: &str,
        start: f64,
        end: f64,
        step_seconds: f64,
    ) -> Result<QueryOutcome, Error> {
        self.validate_query(expression)?;
        if !valid_timestamp(start)
            || !valid_timestamp(end)
            || end < start
            || !step_seconds.is_finite()
            || step_seconds <= 0.0
        {
            return Err(invalid_query());
        }
        let step =
            step_seconds.max((end - start) / (self.budgets.max_points_per_series - 1) as f64);
        let (value, status) = self
            .request(
                "api/v1/query_range",
                vec![
                    ("query", expression.into()),
                    ("start", start.to_string()),
                    ("end", end.to_string()),
                    ("step", step.to_string()),
                ],
                true,
            )
            .await?;
        let outcome = parse::envelope(value, status, &self.budgets, Some(step))
            .map_err(|e| e.with_http(status))?;
        if outcome
            .data
            .as_ref()
            .is_some_and(|data| !matches!(data, QueryData::Matrix { .. }))
        {
            return Err(parse::invalid());
        }
        if let Some(QueryData::Matrix { series }) = &outcome.data {
            let requested_points = (((end - start) / step + 1e-9).floor() as usize)
                .saturating_add(1)
                .min(self.budgets.max_points_per_series);
            if series.iter().any(|series| {
                series.samples.len() > requested_points
                    || series
                        .samples
                        .iter()
                        .any(|sample| sample.timestamp < start || sample.timestamp > end)
            }) {
                return Err(parse::invalid().with_http(status));
            }
        }
        self.ensure_authorization()?;
        Ok(outcome)
    }
    /// Tests behavior, never a backend product/version string.
    pub async fn probe(&self) -> Capabilities {
        let query = capability(self.query("vector(1)", Some(1.0)).await, false);
        let query_range = capability(self.query_range("vector(1)", 0.0, 1.0, 1.0).await, true);
        Capabilities { query, query_range }
    }
    pub async fn label_values(&self, label: &str) -> Result<LabelValues, Error> {
        self.ensure_authorization()?;
        if label.is_empty()
            || label.len() > 128
            || !label
                .bytes()
                .enumerate()
                .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
        {
            return Err(invalid_query());
        }
        if let Some((at, values)) = self.cache.lock().map_err(|_| parse::invalid())?.get(label) {
            if at.elapsed() < Duration::from_secs(60) {
                let mut values = values.clone();
                values.cached = true;
                return Ok(values);
            }
        }
        let (value, status) = self
            .request(
                &format!("api/v1/label/{label}/values"),
                vec![("limit", (self.budgets.max_metadata_values + 1).to_string())],
                false,
            )
            .await?;
        let api_status = value
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| parse::invalid().with_http(status))?;
        if !matches!(api_status, "success" | "error")
            || (api_status == "success" && !(200..300).contains(&status))
        {
            return Err(parse::invalid().with_http(status));
        }
        let error = if api_status == "error" {
            Some(ApiError {
                r#type: value
                    .get("errorType")
                    .and_then(Value::as_str)
                    .ok_or_else(parse::invalid)?
                    .into(),
                message: value
                    .get("error")
                    .and_then(Value::as_str)
                    .ok_or_else(parse::invalid)?
                    .into(),
            })
        } else {
            None
        };
        if api_status == "success" && value.get("data").is_none() {
            return Err(parse::invalid().with_http(status));
        }
        let values = parse::strings(value.get("data"))?;
        if values.len() > self.budgets.max_metadata_values || values.iter().any(|v| v.len() > 4096)
        {
            return Err(Error::new(
                ErrorCode::BudgetExceeded,
                "Label values exceed configured budget",
            ));
        }
        let warnings = parse::strings(value.get("warnings"))?;
        let result = LabelValues {
            status: api_status.into(),
            http_status: status,
            partial: !warnings.is_empty() || (error.is_some() && !values.is_empty()),
            error,
            values,
            warnings,
            infos: parse::strings(value.get("infos"))?,
            cached: false,
        };
        self.ensure_authorization()?;
        if result.error.is_some() {
            return Ok(result);
        }
        let mut cache = self.cache.lock().map_err(|_| parse::invalid())?;
        cache.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
        if cache.len() >= 16 {
            cache.clear();
        }
        cache.insert(label.into(), (Instant::now(), result.clone()));
        Ok(result)
    }
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
    async fn request(
        &self,
        path: &str,
        fields: Vec<(&str, String)>,
        post: bool,
    ) -> Result<(Value, u16), Error> {
        self.request_with_response_budget(path, fields, post, self.budgets.max_response_bytes)
            .await
    }
    fn decode_response(&self, bytes:&[u8], status:u16) -> Result<(Value,u16),Error> {
        let mut value: Value =
            serde_json::from_slice(bytes).map_err(|_| parse::invalid().with_http(status))?;
        // Treat backend text as untrusted; credential echoes must not enter DTOs.
        if let Some(credential) = &self.credential {
            redact_value(&mut value, &credential.redact);
            redact_value(&mut value, std::slice::from_ref(&credential.authorization));
            if let Some((_, token)) = credential.authorization.split_once(' ') {
                redact_value(&mut value, &[Zeroizing::new(token.into())]);
            }
        }
        Ok((value, status))
    }
    async fn request_with_response_budget(
        &self,
        path: &str,
        mut fields: Vec<(&str, String)>,
        post: bool,
        max_response_bytes: usize,
    ) -> Result<(Value, u16), Error> {
        let max_response_bytes = max_response_bytes.min(self.budgets.max_response_bytes);
        let _permit = self.permits.clone().try_acquire_owned().map_err(|_| {
            Error::new(
                ErrorCode::Busy,
                "Monitoring request concurrency budget reached",
            )
        })?;
        let url = self
            .base
            .join(path)
            .map_err(|_| Error::new(ErrorCode::InvalidConfig, "Invalid configured endpoint"))?;
        // Callers cannot supply path; every path is generated by this module.
        let mut request = if post {
            fields.push(("timeout", format!("{}s", self.budgets.timeout_seconds)));
            // Request one extra row to detect servers that implement the limit.
            fields.push(("limit", (self.budgets.max_series + 1).to_string()));
            if self.scoped.is_some() {
                self.http.get(url).query(&fields)
            } else {
                self.http.post(url).form(&fields)
            }
        } else {
            self.http.get(url).query(&fields)
        };
        if let Some(credential) = &self.credential {
            if credential.expires <= Instant::now() {
                return Err(Error::new(
                    ErrorCode::AuthUnavailable,
                    "Credential grant expired",
                ));
            }
            let mut header = HeaderValue::from_str(&credential.authorization)
                .map_err(|_| Error::new(ErrorCode::AuthUnavailable, "Invalid credential grant"))?;
            header.set_sensitive(true);
            request = request.header(AUTHORIZATION, header);
        }
        let budget_timeout = Duration::from_secs(self.budgets.timeout_seconds);
        let request_timeout = self
            .credential
            .as_ref()
            .map(|c| {
                c.expires
                    .saturating_duration_since(Instant::now())
                    .min(budget_timeout)
            })
            .unwrap_or(budget_timeout);
        let operation = async {
            let (bytes, status) = if let Some(scoped) = &self.scoped {
                let prepared = request.build().map_err(transport_error)?;
                scoped.get(prepared, max_response_bytes).await?
            } else {
                let mut response = request.send().await.map_err(transport_error)?;
                let status = response.status().as_u16();
                if response.status().is_redirection() {
                    return Err(Error::new(
                        ErrorCode::Transport,
                        "Monitoring redirects are disabled",
                    )
                    .with_http(status));
                }
                if response
                    .content_length()
                    .is_some_and(|n| n > max_response_bytes as u64)
                {
                    return Err(Error::new(
                        ErrorCode::ResponseTooLarge,
                        "Monitoring response exceeds byte budget",
                    )
                    .with_http(status));
                }
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
                    if chunk.len() > max_response_bytes.saturating_sub(bytes.len()) {
                        return Err(Error::new(
                            ErrorCode::ResponseTooLarge,
                            "Monitoring response exceeds byte budget",
                        )
                        .with_http(status));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                (bytes, status)
            };
            self.decode_response(&bytes, status)
        };
        tokio::time::timeout(request_timeout, operation)
            .await
            .map_err(|_| {
                if self
                    .credential
                    .as_ref()
                    .is_some_and(|c| c.expires <= Instant::now())
                {
                    Error::new(ErrorCode::AuthUnavailable, "Credential grant expired")
                } else {
                    Error::new(ErrorCode::Timeout, "Monitoring request timed out")
                }
            })?
    }
}
fn valid_timestamp(t: f64) -> bool {
    t.is_finite() && t.abs() <= 253_402_300_799.0
}
fn invalid_query() -> Error {
    Error::new(ErrorCode::InvalidQuery, "Invalid query time, step or label")
}
fn transport_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::new(ErrorCode::Timeout, "Monitoring request timed out")
    } else {
        Error::new(
            ErrorCode::Transport,
            "Monitoring connection or TLS verification failed",
        )
    }
}
fn redact_value(value: &mut Value, secrets: &[Zeroizing<String>]) {
    fn clean(s: &mut String, secrets: &[Zeroizing<String>]) {
        for secret in secrets {
            if !secret.is_empty() {
                *s = s.replace(secret.as_str(), "[REDACTED]");
            }
        }
    }
    match value {
        Value::String(s) => clean(s, secrets),
        Value::Array(values) => {
            for value in values {
                redact_value(value, secrets);
            }
        }
        Value::Object(values) => {
            let original = std::mem::take(values);
            for (mut key, mut value) in original {
                clean(&mut key, secrets);
                redact_value(&mut value, secrets);
                values.insert(key, value);
            }
        }
        _ => {}
    }
}
fn capability(result: Result<QueryOutcome, Error>, range: bool) -> EndpointCapability {
    match result {
        Err(error) => EndpointCapability {
            supported: false,
            http_status: error.http_status,
            error: Some(error),
            warnings: vec![],
        },
        Ok(outcome) => {
            let expected = match &outcome.data {
                Some(QueryData::Vector { series }) if !range => {
                    series.len() == 1
                        && series[0].samples.len() == 1
                        && series[0].samples[0].value == SampleValue::Finite { value: 1.0 }
                }
                Some(QueryData::Matrix { series }) if range => {
                    series.len() == 1
                        && series[0].samples.len() == 2
                        && series[0]
                            .samples
                            .iter()
                            .all(|s| s.value == SampleValue::Finite { value: 1.0 })
                }
                _ => false,
            };
            let supported = outcome.status == "success" && expected && !outcome.partial;
            EndpointCapability {
                supported,
                http_status: Some(outcome.http_status),
                error: (!supported).then(|| {
                    Error::new(
                        ErrorCode::InvalidResponse,
                        "Capability probe did not return the expected complete result",
                    )
                }),
                warnings: outcome.warnings,
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod scoped_stream_tests;
