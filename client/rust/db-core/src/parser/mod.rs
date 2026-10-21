//! Parsing is an authorization boundary: drivers execute only this parsed form,
//! never the original editor text. Unknown syntax cannot become a read permit.
use crate::guard::QueryRisk;
use crate::types::{
    DbCell, DbError, DbParameter, DbQueryRequest, MAX_PARAMETER_COUNT, MAX_QUERY_BYTES,
};
use cc_models::database::DbEngine;
use std::collections::BTreeSet;
use zeroize::{Zeroize, Zeroizing};

pub mod editor;
mod mongo;
mod redis;
pub(crate) use redis::quote as redis_quote;
mod sql;
mod template;

pub const MAX_STATEMENTS: usize = 256;

pub enum ParsedStatement {
    Sql {
        text: String,
        parameters: Vec<DbParameter>,
    },
    Redis {
        argv: Vec<Vec<u8>>,
    },
    Mongo {
        command: serde_json::Value,
    },
}

impl std::fmt::Debug for ParsedStatement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Sql { .. } => "sql",
            Self::Redis { .. } => "redis",
            Self::Mongo { .. } => "mongo",
        };
        f.debug_struct("ParsedStatement")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

impl Drop for ParsedStatement {
    fn drop(&mut self) {
        match self {
            Self::Sql { text, parameters } => {
                text.zeroize();
                for parameter in parameters {
                    wipe_parameter(parameter);
                }
            }
            Self::Redis { argv } => argv.zeroize(),
            Self::Mongo { command } => wipe_json(command),
        }
    }
}

pub(crate) fn wipe_parameter(parameter: &mut DbParameter) {
    parameter.name.zeroize();
    parameter.type_hint.zeroize();
    match &mut parameter.value {
        DbCell::Null => {}
        DbCell::Bool(value) => value.zeroize(),
        DbCell::Int(value)
        | DbCell::Uint(value)
        | DbCell::Decimal(value)
        | DbCell::Float(value)
        | DbCell::Text(value)
        | DbCell::Date(value)
        | DbCell::Time(value)
        | DbCell::Timestamp(value)
        | DbCell::Binary(value)
        | DbCell::Json(value) => value.zeroize(),
    }
}

fn wipe_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(value) => value.zeroize(),
        serde_json::Value::Array(values) => values.iter_mut().for_each(wipe_json),
        serde_json::Value::Object(values) => {
            for (mut key, mut value) in std::mem::take(values) {
                key.zeroize();
                wipe_json(&mut value);
            }
        }
        _ => *value = serde_json::Value::Null,
    }
}

/// A safe replacement for a refused command (Redis `KEYS` → `SCAN`). The
/// suggestion contains editor text, so Debug never prints it.
#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub struct RefusalHint {
    pub code: &'static str,
    pub suggestion: String,
}

impl std::fmt::Debug for RefusalHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefusalHint")
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

/// Hint for a request the parser refuses; None when no replacement exists.
pub(crate) fn refusal_hint(engine: DbEngine, request: &DbQueryRequest) -> Option<RefusalHint> {
    match engine {
        DbEngine::Redis if request.text.len() <= MAX_QUERY_BYTES => {
            redis::refusal_hint(&request.text)
        }
        _ => None,
    }
}

pub(crate) struct ClassifiedStatement {
    pub parsed: ParsedStatement,
    pub risk: QueryRisk,
    pub reason: &'static str,
}

pub(crate) fn parse(
    engine: DbEngine,
    default_schema: &str,
    request: &DbQueryRequest,
) -> Result<Vec<ClassifiedStatement>, DbError> {
    if request.text.is_empty()
        || request.text.len() > MAX_QUERY_BYTES
        || request.text.as_bytes().contains(&0)
        || request.parameters.len() > MAX_PARAMETER_COUNT
    {
        return Err(DbError::InvalidRequest);
    }
    let mut names = BTreeSet::new();
    let mut bytes = request.text.len();
    for parameter in &request.parameters {
        if !template::valid_name(&parameter.name) || !names.insert(&parameter.name) {
            return Err(DbError::InvalidRequest);
        }
        let serialized =
            Zeroizing::new(serde_json::to_vec(parameter).map_err(|_| DbError::InvalidRequest)?);
        bytes = bytes.saturating_add(serialized.len());
    }
    if bytes > MAX_QUERY_BYTES {
        return Err(DbError::ResultLimit);
    }
    let result = match engine {
        DbEngine::Redis => redis::parse(request),
        DbEngine::Mongodb => mongo::parse(request),
        DbEngine::Postgresql | DbEngine::Mysql | DbEngine::Mariadb | DbEngine::Clickhouse => {
            sql::parse(engine, default_schema, request)
        }
    }?;
    if result.is_empty() || result.len() > MAX_STATEMENTS {
        Err(DbError::InvalidRequest)
    } else {
        Ok(result)
    }
}
