//! Fixed AWX v2 API adapter. This module does not enroll a Runner or grant
//! access to ConsoleCrypt hosts. AWX retains its own inventory, credentials,
//! template policy and RBAC. The profile-scoped facade must supply the existing
//! credential reference and original-unlock cancellation before exposing it.
//! No public bridge, persisted connection or application launch action yet.
use reqwest::{Client, Method, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

pub(crate) mod catalog;
pub(crate) mod runtime;
mod wire;
mod strict_json;
#[cfg(test)]
mod tests;

const RESPONSE_LIMIT: usize = 512 * 1024;
const REQUEST_LIMIT: usize = 32 * 1024;
const PAGE_SIZE: u64 = 50;
const MAX_ID: u64 = 9_007_199_254_740_991;
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const REVIEW_TTL: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AwxError {
    Configuration,
    InvalidInput,
    Cancelled,
    Expired,
    Transport,
    Response,
    Budget,
    Http(u16),
    ChangedTemplate,
    NeedsAwxInput,
    UnsupportedPrompts,
}

pub(crate) enum AwxAuthorization {
    Bearer(Zeroizing<String>),
    Basic {
        username: Zeroizing<String>,
        password: Zeroizing<String>,
    },
}
impl std::fmt::Debug for AwxAuthorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AwxAuthorization([redacted])")
    }
}

/// Parameters only, never an approval. Credentials/inventory/project/template
/// overrides and arbitrary endpoint URLs are deliberately absent.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AwxLaunchParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_vars: Option<Value>,
}
impl AwxLaunchParameters {
    fn validate(&self) -> Result<(), AwxError> {
        if self
            .job_type
            .as_deref()
            .is_some_and(|v| !matches!(v, "run" | "check"))
            || [&self.limit, &self.job_tags, &self.skip_tags]
                .into_iter()
                .flatten()
                .any(|v| v.len() > 512 || v.chars().any(char::is_control))
            || self.extra_vars.as_ref().is_some_and(|v| !v.is_object())
            || serde_json::to_vec(self)
                .map_err(|_| AwxError::InvalidInput)?
                .len()
                > REQUEST_LIMIT
        {
            return Err(AwxError::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AwxTemplate {
    pub id: u64,
    pub name: String,
    pub modified: String,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct AwxTemplatePage {
    pub templates: Vec<AwxTemplate>,
    pub has_next: bool,
    pub page: u64,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct AwxJob {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub started: Option<String>,
    pub finished: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct AwxEvent {
    pub counter: u64,
    pub event: String,
    pub created: Option<String>,
    pub stdout: String,
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct AwxEventPage {
    pub events: Vec<AwxEvent>,
    pub has_next: bool,
    pub after: u64,
}

/// Created only by actual read of this adapter's template/launch endpoint.
/// Not Clone/Serialize/Deserialize and consumed by one explicit send. It is
/// not a Native profile approval; the outer facade must additionally fence its
/// original owner, credential revisions, lifetime and explicit user action.
pub(crate) struct AwxLaunchReview {
    owner: Arc<()>,
    base: Url,
    ca: Option<String>,
    owned_loopback: bool,
    template: AwxTemplate,
    requirements: Value,
    parameters: AwxLaunchParameters,
    expires: Instant,
}
impl AwxLaunchReview {
    pub(crate) fn template(&self) -> &AwxTemplate {
        &self.template
    }
    pub(crate) fn parameters(&self) -> &AwxLaunchParameters {
        &self.parameters
    }
    pub(crate) fn remaining(&self) -> Duration {
        self.expires.saturating_duration_since(Instant::now())
    }
}
#[derive(Debug)]
pub(crate) enum AwxLaunchOutcome {
    Started {
        job: AwxJob,
        parameters_ignored: bool,
    },
    /// Request may have reached AWX. Never automatically retry a POST; open the
    /// original AWX jobs list to reconcile before preparing another launch.
    Unknown,
}

pub(crate) struct AwxClient {
    base: Url,
    ca: Option<String>,
    owned_loopback: bool,
    client: Client,
    authorization: AwxAuthorization,
    expires: Instant,
    cancel: CancellationToken,
    owner: Arc<()>,
}
impl AwxClient {
    pub(crate) fn new(
        endpoint: &str,
        ca: Option<&str>,
        authorization: AwxAuthorization,
        lifetime: Duration,
        cancel: CancellationToken,
    ) -> Result<Self, AwxError> {
        Self::construct(endpoint, ca, authorization, lifetime, cancel, false)
    }
    fn construct(
        endpoint: &str,
        ca: Option<&str>,
        authorization: AwxAuthorization,
        lifetime: Duration,
        cancel: CancellationToken,
        owned_loopback: bool,
    ) -> Result<Self, AwxError> {
        let base = Url::parse(endpoint).map_err(|_| AwxError::Configuration)?;
        if endpoint.len() > 2048
            || base.host_str().is_none()
            || !(base.scheme() == "https"
                || owned_loopback
                    && base.scheme() == "http"
                    && base.host_str() == Some("127.0.0.1"))
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || lifetime.is_zero()
            || lifetime > Duration::from_secs(300)
        {
            return Err(AwxError::Configuration);
        }
        let valid =
            |s: &str| !s.is_empty() && s.len() <= 16 * 1024 && !s.chars().any(char::is_control);
        match &authorization {
            AwxAuthorization::Bearer(token) if !valid(token) || !token.is_ascii() => {
                return Err(AwxError::Configuration);
            }
            AwxAuthorization::Basic { username, password }
                if !valid(username) || !valid(password) || username.contains(':') =>
            {
                return Err(AwxError::Configuration);
            }
            _ => {}
        }
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(READ_TIMEOUT)
            .user_agent("ConsoleCrypt-AWX");
        if let Some(ca) = ca {
            let cfg = cc_monitoring_core::SourceConfig {
                id: "awx-validation".into(),
                name: "AWX".into(),
                endpoint: endpoint.into(),
                route: cc_monitoring_core::Route::Direct,
                tls: cc_monitoring_core::TlsConfig {
                    custom_ca_pem: Some(ca.into()),
                },
                auth: cc_monitoring_core::AuthRef::None,
            };
            cfg.validate().map_err(|_| AwxError::Configuration)?;
            let certificates = reqwest::Certificate::from_pem_bundle(ca.as_bytes())
                .map_err(|_| AwxError::Configuration)?;
            if certificates.is_empty() {
                return Err(AwxError::Configuration);
            }
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        }
        Ok(Self {
            base,
            ca: ca.map(str::to_owned),
            owned_loopback,
            client: builder.build().map_err(|_| AwxError::Configuration)?,
            authorization,
            expires: Instant::now() + lifetime,
            cancel,
            owner: Arc::new(()),
        })
    }
    fn current(&self) -> Result<(), AwxError> {
        if self.cancel.is_cancelled() {
            return Err(AwxError::Cancelled);
        }
        if Instant::now() >= self.expires {
            return Err(AwxError::Expired);
        }
        Ok(())
    }
    fn endpoint(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("validated HTTPS base")
            .pop_if_empty()
            .extend(["api", "v2"])
            .extend(segments)
            .push("");
        url
    }
    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<&AwxLaunchParameters>,
    ) -> Result<Value, AwxError> {
        self.current()?;
        let mut request = self
            .client
            .request(method, url)
            .header(header::ACCEPT, "application/json");
        request = match &self.authorization {
            AwxAuthorization::Bearer(value) => request.bearer_auth(value.as_str()),
            AwxAuthorization::Basic { username, password } => {
                request.basic_auth(username.as_str(), Some(password.as_str()))
            }
        };
        if let Some(body) = body {
            request = request.json(body);
        }
        let work = async {
            let mut response = request.send().await.map_err(|_| AwxError::Transport)?;
            let status = response.status().as_u16();
            if !response.status().is_success() {
                return Err(AwxError::Http(status));
            }
            if response
                .content_length()
                .is_some_and(|n| n > RESPONSE_LIMIT as u64)
            {
                return Err(AwxError::Budget);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| AwxError::Transport)? {
                if bytes
                    .len()
                    .checked_add(chunk.len())
                    .is_none_or(|n| n > RESPONSE_LIMIT)
                {
                    return Err(AwxError::Budget);
                }
                bytes.extend_from_slice(&chunk);
            }
            strict_json::parse(&bytes)
        };
        tokio::select! { biased;
            _ = self.cancel.cancelled() => Err(AwxError::Cancelled),
            result = tokio::time::timeout(READ_TIMEOUT.min(self.expires.saturating_duration_since(Instant::now())), work) => {
                let value = result.map_err(|_| AwxError::Transport)??;
                self.current()?; Ok(value)
            }
        }
    }
    pub(crate) async fn templates(
        &self,
        page: u64,
        search: &str,
    ) -> Result<AwxTemplatePage, AwxError> {
        if !(1..=1000).contains(&page) || search.len() > 256 || search.chars().any(char::is_control)
        {
            return Err(AwxError::InvalidInput);
        }
        let mut url = self.endpoint(&["job_templates"]);
        url.query_pairs_mut()
            .append_pair("page_size", &PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string())
            .append_pair("search", search);
        let value = self.request(Method::GET, url, None).await?;
        let rows = rows(&value)?;
        let templates = rows.iter().map(template).collect::<Result<_, _>>()?;
        Ok(AwxTemplatePage {
            templates,
            has_next: has_next(&value)?,
            page,
        })
    }
    async fn template(&self, id: u64) -> Result<AwxTemplate, AwxError> {
        valid_id(id)?;
        let value = self
            .request(
                Method::GET,
                self.endpoint(&["job_templates", &id.to_string()]),
                None,
            )
            .await?;
        let parsed = template(&value)?;
        if parsed.id != id {
            return Err(AwxError::Response);
        }
        Ok(parsed)
    }
    async fn requirements(
        &self,
        id: u64,
        parameters: &AwxLaunchParameters,
    ) -> Result<Value, AwxError> {
        let value = self
            .request(
                Method::GET,
                self.endpoint(&["job_templates", &id.to_string(), "launch"]),
                None,
            )
            .await?;
        let object = value.as_object().ok_or(AwxError::Response)?;
        for key in ["passwords_needed_to_start", "variables_needed_to_start"] {
            if !object
                .get(key)
                .and_then(Value::as_array)
                .ok_or(AwxError::Response)?
                .is_empty()
            {
                return Err(AwxError::NeedsAwxInput);
            }
        }
        for key in ["credential_needed_to_start", "inventory_needed_to_start"] {
            if object
                .get(key)
                .and_then(Value::as_bool)
                .ok_or(AwxError::Response)?
            {
                return Err(AwxError::NeedsAwxInput);
            }
        }
        for (present, key) in [
            (parameters.job_type.is_some(), "ask_job_type_on_launch"),
            (parameters.limit.is_some(), "ask_limit_on_launch"),
            (parameters.job_tags.is_some(), "ask_tags_on_launch"),
            (parameters.skip_tags.is_some(), "ask_skip_tags_on_launch"),
            (parameters.extra_vars.is_some(), "ask_variables_on_launch"),
        ] {
            if present && object.get(key).and_then(Value::as_bool) != Some(true) {
                return Err(AwxError::UnsupportedPrompts);
            }
        }
        // Project only fields used for this launch review. Saved extra_vars,
        // credential descriptions and other server JSON never enter the review.
        let keys = [
            "passwords_needed_to_start",
            "variables_needed_to_start",
            "credential_needed_to_start",
            "inventory_needed_to_start",
            "ask_job_type_on_launch",
            "ask_limit_on_launch",
            "ask_tags_on_launch",
            "ask_skip_tags_on_launch",
            "ask_variables_on_launch",
        ];
        Ok(Value::Object(
            keys.into_iter()
                .filter_map(|key| object.get(key).map(|v| (key.to_owned(), v.clone())))
                .collect(),
        ))
    }
    pub(crate) async fn prepare_launch(
        &self,
        id: u64,
        parameters: AwxLaunchParameters,
    ) -> Result<AwxLaunchReview, AwxError> {
        parameters.validate()?;
        let template = self.template(id).await?;
        let requirements = self.requirements(id, &parameters).await?;
        self.current()?;
        Ok(AwxLaunchReview {
            owner: self.owner.clone(),
            base: self.base.clone(),
            ca: self.ca.clone(),
            owned_loopback: self.owned_loopback,
            template,
            requirements,
            parameters,
            expires: self.expires.min(Instant::now() + REVIEW_TTL),
        })
    }
    /// Reconstruct transport from an opaque, actually-read review using FRESH
    /// authorization supplied by the original Native credential facade. No URL,
    /// CA, owner identity or lifetime extension can be supplied by the caller.
    /// The review remains non-Clone/non-Serde and launch still consumes it once.
    pub(crate) fn resume_review(
        review: &AwxLaunchReview,
        authorization: AwxAuthorization,
        cancel: CancellationToken,
    ) -> Result<Self, AwxError> {
        let remaining = review.expires.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AwxError::Expired);
        }
        let mut client = Self::construct(
            review.base.as_str(),
            review.ca.as_deref(),
            authorization,
            remaining,
            cancel,
            review.owned_loopback,
        )?;
        client.owner = review.owner.clone();
        client.expires = review.expires;
        client.current()?;
        Ok(client)
    }
    pub(crate) async fn launch(
        &self,
        review: AwxLaunchReview,
    ) -> Result<AwxLaunchOutcome, AwxError> {
        self.launch_observed(review, || {}).await
    }
    pub(crate) async fn launch_observed(
        &self,
        review: AwxLaunchReview,
        before_post: impl FnOnce(),
    ) -> Result<AwxLaunchOutcome, AwxError> {
        self.current()?;
        if !Arc::ptr_eq(&review.owner, &self.owner) || Instant::now() >= review.expires {
            return Err(AwxError::Expired);
        }
        let current = self.template(review.template.id).await?;
        if current.modified != review.template.modified
            || current.name != review.template.name
            || self.requirements(current.id, &review.parameters).await? != review.requirements
        {
            return Err(AwxError::ChangedTemplate);
        }
        self.current()?;
        // This is the sole mutation endpoint. Review is consumed even when the
        // outcome is unknown; no API-level or transport retry is performed.
        before_post();
        match self
            .request(
                Method::POST,
                self.endpoint(&["job_templates", &current.id.to_string(), "launch"]),
                Some(&review.parameters),
            )
            .await
        {
            Ok(value) => {
                let id = value
                    .get("job")
                    .and_then(Value::as_u64)
                    .filter(|n| valid_id(*n).is_ok());
                let ignored = value.get("ignored_fields").and_then(Value::as_object);
                match (id, ignored) {
                    (Some(id), Some(ignored)) => match job(&value, id) {
                        Ok(job) => Ok(AwxLaunchOutcome::Started {
                            job,
                            parameters_ignored: !ignored.is_empty(),
                        }),
                        Err(_) => Ok(AwxLaunchOutcome::Unknown),
                    },
                    _ => Ok(AwxLaunchOutcome::Unknown),
                }
            }
            Err(AwxError::Http(status @ (400 | 401 | 403 | 404 | 405))) => {
                Err(AwxError::Http(status))
            }
            Err(_) => Ok(AwxLaunchOutcome::Unknown),
        }
    }
    pub(crate) async fn job(&self, id: u64) -> Result<AwxJob, AwxError> {
        valid_id(id)?;
        job(
            &self
                .request(Method::GET, self.endpoint(&["jobs", &id.to_string()]), None)
                .await?,
            id,
        )
    }
    pub(crate) async fn events(&self, id: u64, after: u64) -> Result<AwxEventPage, AwxError> {
        valid_id(id)?;
        if after > MAX_ID {
            return Err(AwxError::InvalidInput);
        }
        let mut url = self.endpoint(&["jobs", &id.to_string(), "job_events"]);
        url.query_pairs_mut()
            .append_pair("counter__gt", &after.to_string())
            .append_pair("order_by", "counter")
            .append_pair("page_size", &PAGE_SIZE.to_string());
        let value = self.request(Method::GET, url, None).await?;
        let mut cursor = after;
        let mut events = Vec::new();
        for row in rows(&value)? {
            let counter = row
                .get("counter")
                .and_then(Value::as_u64)
                .filter(|n| *n > cursor && *n <= MAX_ID)
                .ok_or(AwxError::Response)?;
            if row.get("job").and_then(Value::as_u64) != Some(id) {
                return Err(AwxError::Response);
            }
            let raw = row
                .get("stdout")
                .and_then(Value::as_str)
                .ok_or(AwxError::Response)?;
            let stdout = raw
                .chars()
                .filter(|c| {
                    (!c.is_control() || matches!(c, '\n' | '\t'))
                        && !matches!(*c as u32, 0x202a..=0x202e | 0x2066..=0x2069)
                })
                .take(2048)
                .collect();
            events.push(AwxEvent {
                counter,
                event: text(row, "event", 128)?,
                created: timestamp(row, "created")?,
                stdout,
                truncated: raw.chars().count() > 2048,
            });
            cursor = counter;
        }
        Ok(AwxEventPage {
            events,
            has_next: has_next(&value)?,
            after: cursor,
        })
    }
    pub(crate) fn original_job_url(&self, id: u64) -> Result<String, AwxError> {
        valid_id(id)?;
        Ok(self.endpoint(&["jobs", &id.to_string()]).into())
    }
}
fn valid_id(id: u64) -> Result<(), AwxError> {
    if id > 0 && id <= MAX_ID {
        Ok(())
    } else {
        Err(AwxError::InvalidInput)
    }
}
fn text(v: &Value, key: &str, limit: usize) -> Result<String, AwxError> {
    let s = v
        .get(key)
        .and_then(Value::as_str)
        .ok_or(AwxError::Response)?;
    if s.is_empty() || s.len() > limit || s.chars().any(char::is_control) {
        return Err(AwxError::Response);
    }
    Ok(s.to_owned())
}
fn timestamp(v: &Value, key: &str) -> Result<Option<String>, AwxError> {
    let Some(s) = v.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let s = s.as_str().ok_or(AwxError::Response)?;
    if s.len() > 64 || chrono::DateTime::parse_from_rfc3339(s).is_err() {
        return Err(AwxError::Response);
    }
    Ok(Some(s.to_owned()))
}
fn template(v: &Value) -> Result<AwxTemplate, AwxError> {
    let id = v
        .get("id")
        .and_then(Value::as_u64)
        .ok_or(AwxError::Response)?;
    valid_id(id)?;
    Ok(AwxTemplate {
        id,
        name: text(v, "name", 512)?,
        modified: timestamp(v, "modified")?.ok_or(AwxError::Response)?,
    })
}
fn rows(value: &Value) -> Result<&Vec<Value>, AwxError> {
    let rows = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or(AwxError::Response)?;
    if rows.len() > PAGE_SIZE as usize {
        return Err(AwxError::Budget);
    }
    Ok(rows)
}
fn has_next(value: &Value) -> Result<bool, AwxError> {
    match value.get("next") {
        Some(Value::Null) => Ok(false),
        Some(Value::String(s)) if s.len() <= 2048 => Ok(true),
        _ => Err(AwxError::Response),
    }
}
fn job(value: &Value, expected: u64) -> Result<AwxJob, AwxError> {
    if value.get("id").and_then(Value::as_u64) != Some(expected) {
        return Err(AwxError::Response);
    }
    let status = text(value, "status", 32)?;
    if !matches!(
        status.as_str(),
        "new" | "pending" | "waiting" | "running" | "successful" | "failed" | "error" | "canceled"
    ) {
        return Err(AwxError::Response);
    }
    Ok(AwxJob {
        id: expected,
        name: text(value, "name", 512)?,
        status,
        started: timestamp(value, "started")?,
        finished: timestamp(value, "finished")?,
    })
}
