use cc_models::database::{DbEngine, DbTlsMode};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub const DEFAULT_TREE_PAGE: usize = 200;
pub const MAX_EVENT_ROWS: usize = 128;
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
pub const DEFAULT_RESULT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_QUERY_BYTES: usize = 1024 * 1024;
pub const MAX_PARAMETER_COUNT: usize = 1024;

/// Numbers that may exceed Dart/JavaScript precision keep their wire spelling.
/// Binary is base64; JSON is serialized JSON text. No cell implicitly becomes SQL.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DbCell {
    Null,
    Bool(bool),
    Int(String),
    Uint(String),
    Decimal(String),
    Float(String),
    Text(String),
    Date(String),
    Time(String),
    Timestamp(String),
    Binary(String),
    Json(String),
}

impl Zeroize for DbCell {
    fn zeroize(&mut self) {
        match self {
            Self::Null => {}
            Self::Bool(value) => value.zeroize(),
            Self::Int(value)
            | Self::Uint(value)
            | Self::Decimal(value)
            | Self::Float(value)
            | Self::Text(value)
            | Self::Date(value)
            | Self::Time(value)
            | Self::Timestamp(value)
            | Self::Binary(value)
            | Self::Json(value) => value.zeroize(),
        }
    }
}
impl Drop for DbCell {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl std::fmt::Debug for DbCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Uint(_) => "uint",
            Self::Decimal(_) => "decimal",
            Self::Float(_) => "float",
            Self::Text(_) => "text",
            Self::Date(_) => "date",
            Self::Time(_) => "time",
            Self::Timestamp(_) => "timestamp",
            Self::Binary(_) => "binary",
            Self::Json(_) => "json",
        };
        f.debug_struct("DbCell")
            .field("kind", &kind)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbColumn {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    pub sensitive: bool,
    #[serde(default)]
    pub primary_key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbServerReadOnly {
    /// Native setup established the engine's read-only transaction/session.
    Enforced,
    /// Redis ACL or MongoDB roles were inspected and found read-only.
    RoleVerified,
    /// Only the native query guard is known to enforce read-only access.
    Unverified,
}

/// What actually protects a live session, shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbTransportSecurity {
    /// TLS with a verified certificate chain and server name.
    Verified,
    /// TLS without certificate verification ("require").
    Unverified,
    /// No database TLS; a tunnel or this computer carries the socket.
    Tunnel,
    /// No TLS over the network.
    Plaintext,
}
impl DbTransportSecurity {
    pub(crate) fn of(mode: DbTlsMode, plaintext: bool, path: crate::transport::DbPath) -> Self {
        if plaintext || mode == DbTlsMode::Disable {
            if path.protected() {
                Self::Tunnel
            } else {
                Self::Plaintext
            }
        } else if mode == DbTlsMode::Require {
            Self::Unverified
        } else {
            Self::Verified
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbSessionInfo {
    pub id: String,
    pub connection_id: String,
    pub engine: DbEngine,
    pub server_version: String,
    pub production: bool,
    pub read_only: bool,
    pub server_read_only: DbServerReadOnly,
    pub transport_security: DbTransportSecurity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbObjectKind {
    Database,
    Schema,
    Table,
    View,
    MaterializedView,
    Sequence,
    Function,
    Procedure,
    Column,
    Index,
    Constraint,
    Trigger,
    Dictionary,
    Projection,
    RedisDatabase,
    RedisKey,
    Collection,
    /// A CQL user-defined type.
    UserType,
    /// A CQL user-defined aggregate.
    Aggregate,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbObjectRef {
    pub database: String,
    #[serde(default)]
    pub schema: Option<String>,
    pub kind: DbObjectKind,
    pub name: String,
    /// Table or collection for column/index/constraint/trigger children.
    #[serde(default)]
    pub parent_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbTreeNode {
    pub reference: DbObjectRef,
    pub has_children: bool,
    pub detail: Option<String>,
    pub nullable: Option<bool>,
    pub default_value: Option<String>,
    pub primary_key: bool,
    pub foreign_key: Option<DbObjectRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbTreePage {
    pub nodes: Vec<DbTreeNode>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbParameter {
    pub name: String,
    pub value: DbCell,
    #[serde(default)]
    pub type_hint: Option<String>,
}

/// Inputs never carry production/read-only flags; native session policy is
/// loaded from the vault and cannot be changed by a query request.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbQueryRequest {
    pub text: String,
    #[serde(default)]
    pub parameters: Vec<DbParameter>,
    #[serde(default)]
    pub approval_ticket: Option<String>,
}

impl Zeroize for DbQueryRequest {
    fn zeroize(&mut self) {
        self.text.zeroize();
        for parameter in &mut self.parameters {
            parameter.value.zeroize();
        }
        self.parameters.clear();
        if let Some(ticket) = &mut self.approval_ticket {
            ticket.zeroize();
        }
        self.approval_ticket = None;
    }
}
impl Drop for DbQueryRequest {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl std::fmt::Debug for DbQueryRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbQueryRequest")
            .field("text", &"[REDACTED]")
            .field("parameter_count", &self.parameters.len())
            .field(
                "approval_ticket",
                &self.approval_ticket.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DbQueryEvent {
    Started {
        result_index: u32,
        columns: Vec<DbColumn>,
        editable_table: Option<DbObjectRef>,
    },
    Rows {
        result_index: u32,
        rows: Vec<Vec<DbCell>>,
    },
    Notice {
        message: String,
    },
    Paused {
        result_index: u32,
        loaded_rows: u64,
        has_more: bool,
    },
    StatementDone {
        result_index: u32,
        elapsed_ms: u64,
        affected_rows: Option<u64>,
        truncated: bool,
    },
    Done,
    Cancelled,
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbExportFormat {
    Csv,
    Tsv,
    Json,
    Ndjson,
    Markdown,
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbExportOptions {
    pub format: DbExportFormat,
    pub header: bool,
    pub bom: bool,
    pub delimiter: char,
    pub quote: char,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbExportProgress {
    pub id: String,
    pub rows: u64,
    pub bytes: u64,
    pub done: bool,
    pub cancelled: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DbError {
    #[error("invalid database request")]
    InvalidRequest,
    #[error("database operation is not allowed by the session policy")]
    PolicyDenied,
    #[error("database query requires explicit approval")]
    ApprovalRequired,
    #[error("database connection failed")]
    ConnectionFailed,
    #[error("database authentication failed")]
    AuthenticationFailed,
    #[error("database TLS verification failed")]
    TlsFailed,
    /// The server refused TLS on a path where a password must not travel in
    /// plaintext ("prefer" over the network is never downgraded).
    #[error("database server does not support TLS")]
    TlsUnavailable,
    /// "require" without a verified certificate needs SCRAM channel binding.
    #[error("database server did not use channel binding")]
    ChannelBindingUnsupported,
    /// The server asked for a cleartext password over an unencrypted network socket.
    #[error("database server requested a cleartext password without TLS")]
    CleartextRefused,
    #[error("database query failed")]
    QueryFailed,
    #[error("database operation timed out")]
    Timeout,
    #[error("database operation was cancelled")]
    Cancelled,
    #[error("database result exceeded the configured limit")]
    ResultLimit,
    #[error("database session is closed")]
    SessionClosed,
    /// The coordinator is not in the connection's local data center.
    #[error("database coordinator is in another data center")]
    DatacenterMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigint_and_decimal_wire_values_preserve_every_digit() {
        for cell in [
            DbCell::Int("-9223372036854775808".into()),
            DbCell::Uint("18446744073709551615".into()),
            DbCell::Decimal("12345678901234567890.12345678901234567890".into()),
        ] {
            let json = serde_json::to_string(&cell).unwrap();
            assert_eq!(serde_json::from_str::<DbCell>(&json).unwrap(), cell);
            assert!(serde_json::from_str::<serde_json::Value>(&json).unwrap()["value"].is_string());
        }
    }

    #[test]
    fn requests_cannot_override_session_policy_or_leak_query_and_approval_in_debug() {
        let json = serde_json::json!({
            "text": "SELECT 'canary-password'",
            "parameters": [],
            "approval_ticket": "canary-approval",
            "production": false
        });
        assert!(serde_json::from_value::<DbQueryRequest>(json).is_err());
        let request = DbQueryRequest {
            text: "SELECT 'canary-password'".into(),
            parameters: vec![DbParameter {
                name: "token".into(),
                value: DbCell::Text("canary-token".into()),
                type_hint: None,
            }],
            approval_ticket: Some("canary-approval".into()),
        };
        assert!(!format!("{request:?}").contains("canary"));
    }
}
