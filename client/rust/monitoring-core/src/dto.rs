use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidConfig,
    InvalidQuery,
    AuthUnavailable,
    Transport,
    Timeout,
    ResponseTooLarge,
    InvalidResponse,
    BudgetExceeded,
    UnsupportedRoute,
    UnsupportedResult,
    Busy,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub http_status: Option<u16>,
}
impl Error {
    pub(crate) fn new(code: ErrorCode, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
            http_status: None,
        }
    }
    pub(crate) fn with_http(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }
}
// Error diagnostics intentionally carry static messages, never reqwest errors/URLs/bodies.
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MonitoringError")
            .field("code", &self.code)
            .finish()
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SampleValue {
    Finite {
        value: f64,
    },
    #[serde(rename = "nan")]
    NaN,
    PositiveInfinity,
    NegativeInfinity,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub timestamp: f64,
    pub value: SampleValue,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub labels: BTreeMap<String, String>,
    pub samples: Vec<Sample>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QueryData {
    Vector { series: Vec<Series> },
    Matrix { series: Vec<Series> },
    Scalar { sample: Sample },
    String { timestamp: f64, value: String },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub r#type: String,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryOutcome {
    pub status: String,
    pub http_status: u16,
    pub data: Option<QueryData>,
    pub warnings: Vec<String>,
    pub infos: Vec<String>,
    pub partial: bool,
    pub error: Option<ApiError>,
    pub effective_step_seconds: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointCapability {
    pub supported: bool,
    pub http_status: Option<u16>,
    pub error: Option<Error>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub query: EndpointCapability,
    pub query_range: EndpointCapability,
}
impl Capabilities {
    pub fn supports_mvp(&self) -> bool {
        self.query.supported && self.query_range.supported
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelValues {
    pub status: String,
    pub http_status: u16,
    pub error: Option<ApiError>,
    pub partial: bool,
    pub values: Vec<String>,
    pub warnings: Vec<String>,
    pub infos: Vec<String>,
    pub cached: bool,
}
