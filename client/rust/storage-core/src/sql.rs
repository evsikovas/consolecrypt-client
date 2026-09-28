//! Conversions between domain types and SQLite column values.

use crate::error::{Result, StorageError};
use cc_protocol::sync::EncryptedBody;
use cc_protocol::{Bytes, Timestamp};
use chrono::SecondsFormat;
use rusqlite::Row;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::str::FromStr;

/// RFC 3339 with microseconds and `Z`, so lexical order == time order.
pub(crate) fn ts(t: &Timestamp) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub(crate) fn now() -> Timestamp {
    chrono::Utc::now()
}

pub(crate) fn parse_ts(s: &str, table: &'static str, column: &'static str) -> Result<Timestamp> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&chrono::Utc))
        .map_err(|e| StorageError::corrupt(table, column, e))
}

pub(crate) fn parse_opt_ts(
    s: Option<String>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<Timestamp>> {
    s.map(|s| parse_ts(&s, table, column)).transpose()
}

pub(crate) fn parse_id<T: FromStr>(s: &str, table: &'static str, column: &'static str) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    s.parse::<T>()
        .map_err(|e| StorageError::corrupt(table, column, e))
}

pub(crate) fn parse_opt_id<T: FromStr>(
    s: Option<String>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<T>>
where
    T::Err: std::fmt::Display,
{
    s.map(|s| parse_id(&s, table, column)).transpose()
}

/// Serialize a unit-variant enum to its serde string form.
pub(crate) fn enum_str<T: Serialize>(v: &T) -> Result<String> {
    match serde_json::to_value(v)? {
        serde_json::Value::String(s) => Ok(s),
        _ => Err(StorageError::Invalid(
            "enum does not serialize as a string".into(),
        )),
    }
}

pub(crate) fn parse_enum<T: DeserializeOwned>(
    s: &str,
    table: &'static str,
    column: &'static str,
) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .map_err(|e| StorageError::corrupt(table, column, e))
}

pub(crate) fn parse_opt_enum<T: DeserializeOwned>(
    s: Option<String>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<T>> {
    s.map(|s| parse_enum(&s, table, column)).transpose()
}

pub(crate) fn opt_json<T: Serialize>(v: &Option<T>) -> Result<Option<String>> {
    v.as_ref()
        .map(|v| serde_json::to_string(v).map_err(StorageError::from))
        .transpose()
}

pub(crate) fn parse_opt_json<T: DeserializeOwned>(
    s: Option<String>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<T>> {
    s.map(|s| serde_json::from_str(&s).map_err(|e| StorageError::corrupt(table, column, e)))
        .transpose()
}

/// Column values of an optional [`EncryptedBody`]:
/// `(format, ciphertext, nonce, wrapped_dek, wrapped_dek_nonce)`.
pub(crate) type BodyCols<'a> = (
    Option<i64>,
    Option<&'a [u8]>,
    Option<&'a [u8]>,
    Option<&'a [u8]>,
    Option<&'a [u8]>,
);

pub(crate) fn body_cols(body: Option<&EncryptedBody>) -> BodyCols<'_> {
    match body {
        Some(b) => (
            Some(i64::from(b.format)),
            Some(b.ciphertext.as_slice()),
            Some(b.nonce.as_slice()),
            Some(b.wrapped_dek.as_slice()),
            Some(b.wrapped_dek_nonce.as_slice()),
        ),
        None => (None, None, None, None, None),
    }
}

/// Read an optional body from 5 consecutive columns starting at `first`.
pub(crate) fn read_body(
    row: &Row<'_>,
    first: usize,
    table: &'static str,
) -> Result<Option<EncryptedBody>> {
    let format: Option<i64> = row.get(first)?;
    let ciphertext: Option<Vec<u8>> = row.get(first + 1)?;
    let nonce: Option<Vec<u8>> = row.get(first + 2)?;
    let wrapped_dek: Option<Vec<u8>> = row.get(first + 3)?;
    let wrapped_dek_nonce: Option<Vec<u8>> = row.get(first + 4)?;
    match (format, ciphertext, nonce, wrapped_dek, wrapped_dek_nonce) {
        (None, None, None, None, None) => Ok(None),
        (Some(format), Some(c), Some(n), Some(w), Some(wn)) => Ok(Some(EncryptedBody {
            format: u16::try_from(format)
                .map_err(|_| StorageError::corrupt(table, "format", "out of range"))?,
            ciphertext: Bytes::new(c),
            nonce: Bytes::new(n),
            wrapped_dek: Bytes::new(w),
            wrapped_dek_nonce: Bytes::new(wn),
        })),
        _ => Err(StorageError::corrupt(
            table,
            "ciphertext",
            "partial body columns",
        )),
    }
}
