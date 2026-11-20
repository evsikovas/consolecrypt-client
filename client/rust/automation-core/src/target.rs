//! Typed requests to an explicitly scoped native target adapter. This contract
//! grants no access: the application must verify its frozen signed job scope.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{future::Future, pin::Pin};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRequest {
    pub version: u8,
    #[serde(rename = "type")]
    pub kind: String,
    pub request_id: Uuid,
    pub job_id: Uuid,
    pub manifest_digest: String,
    pub host_id: Uuid,
    pub operation: String,
    pub body: Value,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetFailure {
    TargetDenied,
    ApprovalExpired,
    Cancelled,
    OutputLimit,
    ConnectionLost,
    OperationFailed,
    UnsupportedOperation,
}

pub type TargetFuture =
    Pin<Box<dyn Future<Output = std::result::Result<Value, TargetFailure>> + Send>>;

pub trait TargetHandler: Send + Sync + 'static {
    fn handle(&self, request: TargetRequest) -> TargetFuture;
    fn revoke(&self, job_id: Uuid, manifest_digest: &str);
}
