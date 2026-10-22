//! CQL values in the result grid and as bound parameters. Scalars become the
//! grid's typed cells (big numbers keep every digit as text, blobs are
//! binary); collections, tuples, vectors and user types become JSON text in
//! the shape `toJson()`/`fromJson()` use (blobs inside are `0x…` strings,
//! map keys are strings). Bound values are converted to the exact column type
//! the server reported for each `?` marker, so text typed into the grid is
//! never interpreted as CQL.
use crate::{DbCell, DbError};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, NaiveDate, NaiveTime, Timelike};
use num_bigint::BigInt;
use scylla::frame::response::result::{CollectionType, ColumnType, NativeType};
use scylla::value::{
    Counter, CqlDate, CqlDecimal, CqlDuration, CqlTime, CqlTimestamp, CqlTimeuuid, CqlValue,
    CqlVarint,
};
use serde_json::{Map, Value};
use std::{net::IpAddr, str::FromStr};

const MAX_DEPTH: usize = 32;
/// 2^31: CQL `date` counts days with the Unix epoch at this value.
const DATE_EPOCH: i64 = 1 << 31;
const NANOS_PER_DAY: i64 = 86_400_000_000_000;

/// The CQL spelling of a column type (`map<text, frozen<list<int>>>`).
pub(crate) fn type_name(column: &ColumnType<'_>) -> String {
    let frozen = |frozen: bool, inner: String| {
        if frozen {
            format!("frozen<{inner}>")
        } else {
            inner
        }
    };
    match column {
        ColumnType::Native(native) => native_name(native).into(),
        ColumnType::Collection { frozen: f, typ } => frozen(
            *f,
            match typ {
                CollectionType::List(item) => format!("list<{}>", type_name(item)),
                CollectionType::Set(item) => format!("set<{}>", type_name(item)),
                CollectionType::Map(key, value) => {
                    format!("map<{}, {}>", type_name(key), type_name(value))
                }
                _ => "collection".into(),
            },
        ),
        ColumnType::Vector { typ, dimensions } => {
            format!("vector<{}, {dimensions}>", type_name(typ))
        }
        ColumnType::UserDefinedType { frozen: f, definition } => {
            frozen(*f, definition.name.to_string())
        }
        ColumnType::Tuple(items) => format!(
            "tuple<{}>",
            items.iter().map(type_name).collect::<Vec<_>>().join(", ")
        ),
        _ => "custom".into(),
    }
}

fn native_name(native: &NativeType) -> &'static str {
    match native {
        NativeType::Ascii => "ascii",
        NativeType::Boolean => "boolean",
        NativeType::Blob => "blob",
        NativeType::Counter => "counter",
        NativeType::Date => "date",
        NativeType::Decimal => "decimal",
        NativeType::Double => "double",
        NativeType::Duration => "duration",
        NativeType::Float => "float",
        NativeType::Int => "int",
        NativeType::BigInt => "bigint",
        NativeType::Text => "text",
        NativeType::Timestamp => "timestamp",
        NativeType::Inet => "inet",
        NativeType::SmallInt => "smallint",
        NativeType::TinyInt => "tinyint",
        NativeType::Time => "time",
        NativeType::Timeuuid => "timeuuid",
        NativeType::Uuid => "uuid",
        NativeType::Varint => "varint",
        _ => "custom",
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(2 + bytes.len() * 2);
    text.push_str("0x");
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn varint(value: &CqlVarint) -> String {
    BigInt::from_signed_bytes_be(value.as_signed_bytes_be_slice()).to_string()
}

fn decimal(value: &CqlDecimal) -> String {
    let (bytes, scale) = value.as_signed_be_bytes_slice_and_exponent();
    let unscaled = BigInt::from_signed_bytes_be(bytes);
    if scale <= 0 {
        let zeros = "0".repeat(scale.unsigned_abs().min(10_000) as usize);
        return if unscaled == BigInt::from(0) {
            "0".into()
        } else {
            format!("{unscaled}{zeros}")
        };
    }
    let negative = unscaled < BigInt::from(0);
    let digits = if negative {
        (-unscaled).to_string()
    } else {
        unscaled.to_string()
    };
    let scale = scale as usize;
    let digits = if digits.len() <= scale {
        format!("{}{digits}", "0".repeat(scale + 1 - digits.len()))
    } else {
        digits
    };
    let (whole, fraction) = digits.split_at(digits.len() - scale);
    format!("{}{whole}.{fraction}", if negative { "-" } else { "" })
}

fn timestamp(value: CqlTimestamp) -> String {
    DateTime::from_timestamp_millis(value.0).map_or_else(
        || value.0.to_string(),
        |time| time.format("%Y-%m-%d %H:%M:%S%.3fZ").to_string(),
    )
}

fn date(value: CqlDate) -> String {
    let days = i64::from(value.0) - DATE_EPOCH;
    NaiveDate::from_ymd_opt(1970, 1, 1)
        .and_then(|epoch| epoch.checked_add_signed(chrono::Duration::days(days)))
        .map_or_else(|| days.to_string(), |date| date.format("%Y-%m-%d").to_string())
}

fn time(value: CqlTime) -> String {
    let nanos = value.0;
    if !(0..NANOS_PER_DAY).contains(&nanos) {
        return nanos.to_string();
    }
    let seconds = nanos / 1_000_000_000;
    format!(
        "{:02}:{:02}:{:02}.{:09}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        nanos % 1_000_000_000
    )
}

/// `1y2mo3d4h5m6s7ms8us9ns`, as CQL writes durations (`0s` when empty).
fn duration(value: &CqlDuration) -> String {
    let negative = value.months < 0 || value.days < 0 || value.nanoseconds < 0;
    let months = i64::from(value.months).unsigned_abs();
    let days = i64::from(value.days).unsigned_abs();
    let mut nanos = value.nanoseconds.unsigned_abs();
    let mut text = String::new();
    if negative {
        text.push('-');
    }
    let mut push = |amount: u64, unit: &str| {
        if amount > 0 {
            text.push_str(&format!("{amount}{unit}"));
        }
    };
    push(months / 12, "y");
    push(months % 12, "mo");
    push(days, "d");
    for (size, unit) in [
        (3_600_000_000_000u64, "h"),
        (60_000_000_000, "m"),
        (1_000_000_000, "s"),
        (1_000_000, "ms"),
        (1_000, "us"),
        (1, "ns"),
    ] {
        push(nanos / size, unit);
        nanos %= size;
    }
    if text.is_empty() || text == "-" {
        "0s".into()
    } else {
        text
    }
}

fn float(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or_else(|| Value::String(value.to_string()), Value::Number)
}

/// A nested value in `toJson()` shape.
pub(crate) fn json(value: &CqlValue, depth: usize) -> Result<Value, DbError> {
    if depth > MAX_DEPTH {
        return Err(DbError::ResultLimit);
    }
    let nested = |value: &CqlValue| json(value, depth + 1);
    Ok(match value {
        CqlValue::Ascii(text) | CqlValue::Text(text) => Value::String(text.clone()),
        CqlValue::Boolean(value) => Value::Bool(*value),
        CqlValue::Blob(bytes) => Value::String(hex(bytes)),
        CqlValue::Counter(Counter(value)) | CqlValue::BigInt(value) => Value::from(*value),
        CqlValue::Int(value) => Value::from(*value),
        CqlValue::SmallInt(value) => Value::from(*value),
        CqlValue::TinyInt(value) => Value::from(*value),
        CqlValue::Varint(value) => {
            let text = varint(value);
            text.parse::<i64>()
                .map_or(Value::String(text), Value::from)
        }
        CqlValue::Decimal(value) => Value::String(decimal(value)),
        CqlValue::Double(value) => float(*value),
        CqlValue::Float(value) => float(f64::from(*value)),
        CqlValue::Date(value) => Value::String(date(*value)),
        CqlValue::Time(value) => Value::String(time(*value)),
        CqlValue::Timestamp(value) => Value::String(timestamp(*value)),
        CqlValue::Duration(value) => Value::String(duration(value)),
        CqlValue::Inet(address) => Value::String(address.to_string()),
        CqlValue::Uuid(uuid) => Value::String(uuid.to_string()),
        CqlValue::Timeuuid(uuid) => Value::String(uuid.as_ref().to_string()),
        CqlValue::Empty => Value::Null,
        CqlValue::List(items) | CqlValue::Set(items) | CqlValue::Vector(items) => {
            Value::Array(items.iter().map(nested).collect::<Result<_, _>>()?)
        }
        CqlValue::Tuple(items) => Value::Array(
            items
                .iter()
                .map(|item| item.as_ref().map_or(Ok(Value::Null), nested))
                .collect::<Result<_, _>>()?,
        ),
        CqlValue::Map(entries) => {
            let mut object = Map::new();
            for (key, value) in entries {
                let key = match nested(key)? {
                    Value::String(text) => text,
                    other => other.to_string(),
                };
                object.insert(key, nested(value)?);
            }
            Value::Object(object)
        }
        CqlValue::UserDefinedType { fields, .. } => {
            let mut object = Map::new();
            for (name, value) in fields {
                object.insert(
                    name.clone(),
                    value.as_ref().map_or(Ok(Value::Null), nested)?,
                );
            }
            Value::Object(object)
        }
        _ => Value::Null,
    })
}

/// One grid cell.
pub(crate) fn cell(value: Option<&CqlValue>) -> Result<DbCell, DbError> {
    let Some(value) = value else {
        return Ok(DbCell::Null);
    };
    Ok(match value {
        CqlValue::Ascii(text) | CqlValue::Text(text) => DbCell::Text(text.clone()),
        CqlValue::Boolean(value) => DbCell::Bool(*value),
        CqlValue::Blob(bytes) => {
            if bytes.len() > crate::MAX_EVENT_BYTES {
                return Err(DbError::ResultLimit);
            }
            DbCell::Binary(STANDARD.encode(bytes))
        }
        CqlValue::Counter(Counter(value)) | CqlValue::BigInt(value) => DbCell::Int(value.to_string()),
        CqlValue::Int(value) => DbCell::Int(value.to_string()),
        CqlValue::SmallInt(value) => DbCell::Int(value.to_string()),
        CqlValue::TinyInt(value) => DbCell::Int(value.to_string()),
        CqlValue::Varint(value) => DbCell::Int(varint(value)),
        CqlValue::Decimal(value) => DbCell::Decimal(decimal(value)),
        CqlValue::Double(value) => DbCell::Float(value.to_string()),
        CqlValue::Float(value) => DbCell::Float(value.to_string()),
        CqlValue::Date(value) => DbCell::Date(date(*value)),
        CqlValue::Time(value) => DbCell::Time(time(*value)),
        CqlValue::Timestamp(value) => DbCell::Timestamp(timestamp(*value)),
        CqlValue::Duration(value) => DbCell::Text(duration(value)),
        CqlValue::Inet(address) => DbCell::Text(address.to_string()),
        CqlValue::Uuid(uuid) => DbCell::Text(uuid.to_string()),
        CqlValue::Timeuuid(uuid) => DbCell::Text(uuid.as_ref().to_string()),
        CqlValue::Empty => DbCell::Null,
        other => DbCell::Json(json(other, 0)?.to_string()),
    })
}

fn invalid<T>(_: T) -> DbError {
    DbError::InvalidRequest
}

/// The text of a scalar cell, or the JSON of a collection cell.
fn cell_text(cell: &DbCell) -> Option<String> {
    match cell {
        DbCell::Null => None,
        DbCell::Bool(value) => Some(value.to_string()),
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Json(v) => Some(v.clone()),
        DbCell::Binary(v) => Some(v.clone()),
    }
}

/// A bound value of `column` (None is NULL).
pub(crate) fn bind(cell: &DbCell, column: &ColumnType<'_>) -> Result<Option<CqlValue>, DbError> {
    if matches!(cell, DbCell::Null) {
        return Ok(None);
    }
    match column {
        ColumnType::Native(NativeType::Blob) => {
            let bytes = match cell {
                DbCell::Binary(value) => STANDARD.decode(value).map_err(invalid)?,
                other => blob_text(&cell_text(other).unwrap_or_default())?,
            };
            Ok(Some(CqlValue::Blob(bytes)))
        }
        ColumnType::Native(NativeType::Boolean) => Ok(Some(match cell {
            DbCell::Bool(value) => CqlValue::Boolean(*value),
            other => scalar(&cell_text(other).unwrap_or_default(), column)?,
        })),
        ColumnType::Native(_) => scalar(&cell_text(cell).unwrap_or_default(), column).map(Some),
        _ => {
            let text = cell_text(cell).unwrap_or_default();
            let value: Value = serde_json::from_str(&text).map_err(invalid)?;
            from_json(&value, column, 0).map(Some)
        }
    }
}

fn blob_text(text: &str) -> Result<Vec<u8>, DbError> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .ok_or(DbError::InvalidRequest)?;
    if digits.len() % 2 != 0 {
        return Err(DbError::InvalidRequest);
    }
    (0..digits.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&digits[index..index + 2], 16).map_err(invalid))
        .collect()
}

fn parse_timestamp(text: &str) -> Result<i64, DbError> {
    let text = text.trim();
    if let Ok(millis) = text.parse::<i64>() {
        return Ok(millis);
    }
    if let Ok(time) = DateTime::parse_from_rfc3339(&text.replacen(' ', "T", 1)) {
        return Ok(time.timestamp_millis());
    }
    let without_zone = text.trim_end_matches('Z').trim_end_matches('z');
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
    ] {
        if let Ok(time) = chrono::NaiveDateTime::parse_from_str(without_zone, format) {
            return Ok(time.and_utc().timestamp_millis());
        }
        if let Ok(date) = NaiveDate::parse_from_str(without_zone, format) {
            return Ok(date
                .and_hms_opt(0, 0, 0)
                .ok_or(DbError::InvalidRequest)?
                .and_utc()
                .timestamp_millis());
        }
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f%z", "%Y-%m-%dT%H:%M:%S%.f%z"] {
        if let Ok(time) = DateTime::parse_from_str(text, format) {
            return Ok(time.timestamp_millis());
        }
    }
    Err(DbError::InvalidRequest)
}

fn parse_duration(text: &str) -> Result<CqlDuration, DbError> {
    let text = text.trim();
    let (negative, mut rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if rest.is_empty() {
        return Err(DbError::InvalidRequest);
    }
    let (mut months, mut days, mut nanos) = (0i64, 0i64, 0i64);
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if digits == 0 {
            return Err(DbError::InvalidRequest);
        }
        let amount: i64 = rest[..digits].parse().map_err(invalid)?;
        rest = &rest[digits..];
        let unit_length = rest.find(|c: char| c.is_ascii_digit()).unwrap_or(rest.len());
        let unit = rest[..unit_length].to_ascii_lowercase();
        rest = &rest[unit_length..];
        let add = |total: &mut i64, factor: i64| -> Result<(), DbError> {
            *total = amount
                .checked_mul(factor)
                .and_then(|value| total.checked_add(value))
                .ok_or(DbError::InvalidRequest)?;
            Ok(())
        };
        match unit.as_str() {
            "y" => add(&mut months, 12)?,
            "mo" => add(&mut months, 1)?,
            "w" => add(&mut days, 7)?,
            "d" => add(&mut days, 1)?,
            "h" => add(&mut nanos, 3_600_000_000_000)?,
            "m" => add(&mut nanos, 60_000_000_000)?,
            "s" => add(&mut nanos, 1_000_000_000)?,
            "ms" => add(&mut nanos, 1_000_000)?,
            "us" | "µs" => add(&mut nanos, 1_000)?,
            "ns" => add(&mut nanos, 1)?,
            _ => return Err(DbError::InvalidRequest),
        }
    }
    let sign = if negative { -1 } else { 1 };
    Ok(CqlDuration {
        months: i32::try_from(months * sign).map_err(invalid)?,
        days: i32::try_from(days * sign).map_err(invalid)?,
        nanoseconds: nanos * sign,
    })
}

fn parse_decimal(text: &str) -> Result<CqlDecimal, DbError> {
    let text = text.trim();
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(index) => (
            &text[..index],
            text[index + 1..].parse::<i32>().map_err(invalid)?,
        ),
        None => (text, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || digits == "-" || digits == "+" {
        return Err(DbError::InvalidRequest);
    }
    let unscaled = BigInt::from_str(&digits).map_err(invalid)?;
    let scale = i32::try_from(fraction.len())
        .map_err(invalid)?
        .checked_sub(exponent)
        .ok_or(DbError::InvalidRequest)?;
    Ok(CqlDecimal::from_signed_be_bytes_and_exponent(
        unscaled.to_signed_bytes_be(),
        scale,
    ))
}

/// A scalar typed as text.
fn scalar(text: &str, column: &ColumnType<'_>) -> Result<CqlValue, DbError> {
    let ColumnType::Native(native) = column else {
        let value: Value = serde_json::from_str(text).map_err(invalid)?;
        return from_json(&value, column, 0);
    };
    let trimmed = text.trim();
    Ok(match native {
        NativeType::Ascii => {
            if !text.is_ascii() {
                return Err(DbError::InvalidRequest);
            }
            CqlValue::Ascii(text.to_owned())
        }
        NativeType::Text => CqlValue::Text(text.to_owned()),
        NativeType::Boolean => CqlValue::Boolean(match trimmed.to_ascii_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => return Err(DbError::InvalidRequest),
        }),
        NativeType::Blob => CqlValue::Blob(blob_text(trimmed)?),
        NativeType::Counter => CqlValue::Counter(Counter(trimmed.parse().map_err(invalid)?)),
        NativeType::BigInt => CqlValue::BigInt(trimmed.parse().map_err(invalid)?),
        NativeType::Int => CqlValue::Int(trimmed.parse().map_err(invalid)?),
        NativeType::SmallInt => CqlValue::SmallInt(trimmed.parse().map_err(invalid)?),
        NativeType::TinyInt => CqlValue::TinyInt(trimmed.parse().map_err(invalid)?),
        NativeType::Varint => CqlValue::Varint(CqlVarint::from_signed_bytes_be(
            BigInt::from_str(trimmed).map_err(invalid)?.to_signed_bytes_be(),
        )),
        NativeType::Decimal => CqlValue::Decimal(parse_decimal(trimmed)?),
        NativeType::Double => CqlValue::Double(trimmed.parse().map_err(invalid)?),
        NativeType::Float => CqlValue::Float(trimmed.parse().map_err(invalid)?),
        NativeType::Uuid => CqlValue::Uuid(uuid::Uuid::parse_str(trimmed).map_err(invalid)?),
        NativeType::Timeuuid => {
            let uuid = uuid::Uuid::parse_str(trimmed).map_err(invalid)?;
            if uuid.get_version_num() != 1 {
                return Err(DbError::InvalidRequest);
            }
            CqlValue::Timeuuid(CqlTimeuuid::from(uuid))
        }
        NativeType::Inet => CqlValue::Inet(trimmed.parse::<IpAddr>().map_err(invalid)?),
        NativeType::Timestamp => CqlValue::Timestamp(CqlTimestamp(parse_timestamp(trimmed)?)),
        NativeType::Date => {
            let days = match NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
                Ok(date) => {
                    date.signed_duration_since(NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch"))
                        .num_days()
                }
                Err(_) => trimmed.parse::<i64>().map_err(invalid)?,
            };
            CqlValue::Date(CqlDate(
                u32::try_from(days + DATE_EPOCH).map_err(invalid)?,
            ))
        }
        NativeType::Time => {
            let time = NaiveTime::parse_from_str(trimmed, "%H:%M:%S%.f")
                .or_else(|_| NaiveTime::parse_from_str(trimmed, "%H:%M"))
                .map_err(invalid)?;
            CqlValue::Time(CqlTime(
                i64::from(time.num_seconds_from_midnight()) * 1_000_000_000
                    + i64::from(time.nanosecond()),
            ))
        }
        NativeType::Duration => CqlValue::Duration(parse_duration(trimmed)?),
        _ => return Err(DbError::InvalidRequest),
    })
}

/// A JSON value (the grid's text for collections) as `column`.
fn from_json(value: &Value, column: &ColumnType<'_>, depth: usize) -> Result<CqlValue, DbError> {
    if depth > MAX_DEPTH {
        return Err(DbError::ResultLimit);
    }
    let item = |value: &Value, column: &ColumnType<'_>| from_json(value, column, depth + 1);
    let optional = |value: &Value, column: &ColumnType<'_>| {
        if value.is_null() {
            Ok(None)
        } else {
            item(value, column).map(Some)
        }
    };
    match (column, value) {
        (ColumnType::Native(NativeType::Boolean), Value::Bool(value)) => {
            Ok(CqlValue::Boolean(*value))
        }
        (ColumnType::Native(_), Value::String(text)) => scalar(text, column),
        (ColumnType::Native(_), Value::Number(number)) => scalar(&number.to_string(), column),
        (ColumnType::Native(_), Value::Bool(value)) => scalar(&value.to_string(), column),
        (ColumnType::Collection { typ, .. }, _) => match (typ, value) {
            (CollectionType::List(inner), Value::Array(items)) => Ok(CqlValue::List(
                items.iter().map(|v| item(v, inner)).collect::<Result<_, _>>()?,
            )),
            (CollectionType::Set(inner), Value::Array(items)) => Ok(CqlValue::Set(
                items.iter().map(|v| item(v, inner)).collect::<Result<_, _>>()?,
            )),
            (CollectionType::Map(key, inner), Value::Object(entries)) => Ok(CqlValue::Map(
                entries
                    .iter()
                    .map(|(name, value)| {
                        let key = match key.as_ref() {
                            ColumnType::Native(_) => scalar(name, key)?,
                            other => {
                                let parsed: Value =
                                    serde_json::from_str(name).map_err(invalid)?;
                                item(&parsed, other)?
                            }
                        };
                        Ok((key, item(value, inner)?))
                    })
                    .collect::<Result<_, DbError>>()?,
            )),
            _ => Err(DbError::InvalidRequest),
        },
        (ColumnType::Vector { typ, dimensions }, Value::Array(items)) => {
            if items.len() != usize::from(*dimensions) {
                return Err(DbError::InvalidRequest);
            }
            Ok(CqlValue::Vector(
                items.iter().map(|v| item(v, typ)).collect::<Result<_, _>>()?,
            ))
        }
        (ColumnType::Tuple(types), Value::Array(items)) => {
            if items.len() != types.len() {
                return Err(DbError::InvalidRequest);
            }
            Ok(CqlValue::Tuple(
                items
                    .iter()
                    .zip(types)
                    .map(|(value, column)| optional(value, column))
                    .collect::<Result<_, _>>()?,
            ))
        }
        (ColumnType::UserDefinedType { definition, .. }, Value::Object(fields)) => {
            if fields
                .keys()
                .any(|name| !definition.field_types.iter().any(|(field, _)| field == name))
            {
                return Err(DbError::InvalidRequest);
            }
            Ok(CqlValue::UserDefinedType {
                keyspace: definition.keyspace.to_string(),
                name: definition.name.to_string(),
                fields: definition
                    .field_types
                    .iter()
                    .map(|(name, column)| {
                        let value = fields.get(name.as_ref()).unwrap_or(&Value::Null);
                        Ok((name.to_string(), optional(value, column)?))
                    })
                    .collect::<Result<_, DbError>>()?,
            })
        }
        _ => Err(DbError::InvalidRequest),
    }
}

/// A CQL literal of a grid cell for scripts and previews (collections stay
/// `fromJson('…')`, which needs the column's type on the server).
pub(crate) fn literal(cell: &DbCell) -> String {
    let quote = |text: &str| format!("'{}'", text.replace('\'', "''"));
    match cell {
        DbCell::Null => "null".into(),
        DbCell::Bool(value) => value.to_string(),
        DbCell::Int(v) | DbCell::Uint(v) | DbCell::Decimal(v) | DbCell::Float(v)
            if v.parse::<f64>().is_ok_and(f64::is_finite) =>
        {
            v.clone()
        }
        DbCell::Float(v) if matches!(v.as_str(), "NaN" | "inf" | "-inf") => match v.as_str() {
            "NaN" => "NaN".into(),
            "inf" => "Infinity".into(),
            _ => "-Infinity".into(),
        },
        DbCell::Binary(v) => STANDARD
            .decode(v)
            .map(|bytes| hex(&bytes))
            .unwrap_or_else(|_| quote(v)),
        DbCell::Json(v) => format!("fromJson({})", quote(v)),
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v) => quote(v),
    }
}

/// The JSON value of a grid cell for `INSERT … JSON` (blobs as `0x…`).
pub(crate) fn json_of_cell(cell: &DbCell) -> Value {
    match cell {
        DbCell::Null => Value::Null,
        DbCell::Bool(value) => Value::Bool(*value),
        DbCell::Int(v) | DbCell::Uint(v) => v
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(v.clone())),
        DbCell::Float(v) => v
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| Value::String(v.clone()), Value::Number),
        DbCell::Binary(v) => Value::String(
            STANDARD
                .decode(v)
                .map(|bytes| hex(&bytes))
                .unwrap_or_else(|_| v.clone()),
        ),
        DbCell::Json(v) => serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.clone())),
        DbCell::Decimal(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v) => Value::String(v.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scylla::frame::response::result::UserDefinedType;
    use std::{borrow::Cow, sync::Arc};

    fn native(native: NativeType) -> ColumnType<'static> {
        ColumnType::Native(native)
    }

    #[test]
    fn scalars_become_typed_cells_without_losing_digits() {
        let huge = BigInt::from_str("123456789012345678901234567890").unwrap();
        let cases = [
            (CqlValue::Int(-5), DbCell::Int("-5".into())),
            (
                CqlValue::Varint(CqlVarint::from_signed_bytes_be(huge.to_signed_bytes_be())),
                DbCell::Int("123456789012345678901234567890".into()),
            ),
            (
                CqlValue::Decimal(CqlDecimal::from_signed_be_bytes_and_exponent(
                    BigInt::from(-12345).to_signed_bytes_be(),
                    4,
                )),
                DbCell::Decimal("-1.2345".into()),
            ),
            (
                CqlValue::Decimal(CqlDecimal::from_signed_be_bytes_and_exponent(
                    BigInt::from(5).to_signed_bytes_be(),
                    3,
                )),
                DbCell::Decimal("0.005".into()),
            ),
            (CqlValue::Blob(vec![0xca, 0xfe]), DbCell::Binary("yv4=".into())),
            (CqlValue::Counter(Counter(7)), DbCell::Int("7".into())),
            (
                CqlValue::Timestamp(CqlTimestamp(1_700_000_000_123)),
                DbCell::Timestamp("2023-11-14 22:13:20.123Z".into()),
            ),
            (
                CqlValue::Date(CqlDate((1u32 << 31) + 19_000)),
                DbCell::Date("2022-01-08".into()),
            ),
            (
                CqlValue::Time(CqlTime(3_723_000_000_004)),
                DbCell::Time("01:02:03.000000004".into()),
            ),
            (
                CqlValue::Duration(CqlDuration {
                    months: 14,
                    days: 3,
                    nanoseconds: 3_723_000_000_000,
                }),
                DbCell::Text("1y2mo3d1h2m3s".into()),
            ),
            (CqlValue::Inet("::1".parse().unwrap()), DbCell::Text("::1".into())),
            (CqlValue::Empty, DbCell::Null),
        ];
        for (value, expected) in cases {
            assert_eq!(cell(Some(&value)).unwrap(), expected, "{value:?}");
        }
        assert_eq!(cell(None).unwrap(), DbCell::Null);
    }

    fn address() -> ColumnType<'static> {
        ColumnType::UserDefinedType {
            frozen: true,
            definition: Arc::new(UserDefinedType {
                name: Cow::Borrowed("address"),
                keyspace: Cow::Borrowed("shop"),
                field_types: vec![
                    (Cow::Borrowed("street"), native(NativeType::Text)),
                    (Cow::Borrowed("zip"), native(NativeType::Int)),
                ],
            }),
        }
    }

    #[test]
    fn collections_tuples_and_user_types_roundtrip_as_json() {
        let map = ColumnType::Collection {
            frozen: false,
            typ: CollectionType::Map(
                Box::new(native(NativeType::Int)),
                Box::new(ColumnType::Collection {
                    frozen: true,
                    typ: CollectionType::Set(Box::new(native(NativeType::Blob))),
                }),
            ),
        };
        assert_eq!(type_name(&map), "map<int, frozen<set<blob>>>");
        let value = CqlValue::Map(vec![(
            CqlValue::Int(1),
            CqlValue::Set(vec![CqlValue::Blob(vec![1, 255])]),
        )]);
        let DbCell::Json(text) = &cell(Some(&value)).unwrap() else {
            panic!("json");
        };
        let text = text.clone();
        assert_eq!(text, r#"{"1":["0x01ff"]}"#);
        assert_eq!(bind(&DbCell::Json(text), &map).unwrap(), Some(value));

        let tuple = ColumnType::Tuple(vec![native(NativeType::Text), native(NativeType::Uuid)]);
        let value = CqlValue::Tuple(vec![
            Some(CqlValue::Text("a".into())),
            None,
        ]);
        let DbCell::Json(text) = &cell(Some(&value)).unwrap() else {
            panic!("json");
        };
        let text = text.clone();
        assert_eq!(text, r#"["a",null]"#);
        assert_eq!(bind(&DbCell::Json(text), &tuple).unwrap(), Some(value));

        let udt = address();
        assert_eq!(type_name(&udt), "frozen<address>");
        let bound = bind(&DbCell::Text(r#"{"street":"Main 1"}"#.into()), &udt).unwrap();
        assert_eq!(
            bound,
            Some(CqlValue::UserDefinedType {
                keyspace: "shop".into(),
                name: "address".into(),
                fields: vec![
                    ("street".into(), Some(CqlValue::Text("Main 1".into()))),
                    ("zip".into(), None)
                ],
            })
        );
        assert!(bind(&DbCell::Text(r#"{"city":"x"}"#.into()), &udt).is_err());
        assert!(bind(&DbCell::Text("[1,2]".into()), &map).is_err());
    }

    #[test]
    fn typed_text_binds_to_the_reported_column_type() {
        let cases = [
            (NativeType::Int, DbCell::Text(" 42 ".into()), CqlValue::Int(42)),
            (NativeType::Boolean, DbCell::Text("TRUE".into()), CqlValue::Boolean(true)),
            (NativeType::Blob, DbCell::Text("0xCAFE".into()), CqlValue::Blob(vec![0xca, 0xfe])),
            (NativeType::Blob, DbCell::Binary("yv4=".into()), CqlValue::Blob(vec![0xca, 0xfe])),
            (
                NativeType::Timestamp,
                DbCell::Timestamp("2023-11-14 22:13:20.123Z".into()),
                CqlValue::Timestamp(CqlTimestamp(1_700_000_000_123)),
            ),
            (
                NativeType::Timestamp,
                DbCell::Text("2023-11-14T22:13:20+00:00".into()),
                CqlValue::Timestamp(CqlTimestamp(1_700_000_000_000)),
            ),
            (
                NativeType::Date,
                DbCell::Date("2022-01-08".into()),
                CqlValue::Date(CqlDate((1u32 << 31) + 19_000)),
            ),
            (
                NativeType::Time,
                DbCell::Time("01:02:03.000000004".into()),
                CqlValue::Time(CqlTime(3_723_000_000_004)),
            ),
            (
                NativeType::Duration,
                DbCell::Text("1y2mo3d1h2m3s".into()),
                CqlValue::Duration(CqlDuration {
                    months: 14,
                    days: 3,
                    nanoseconds: 3_723_000_000_000,
                }),
            ),
            (
                NativeType::Decimal,
                DbCell::Decimal("-1.2345".into()),
                CqlValue::Decimal(CqlDecimal::from_signed_be_bytes_and_exponent(
                    BigInt::from(-12345).to_signed_bytes_be(),
                    4,
                )),
            ),
        ];
        for (typ, input, expected) in cases {
            assert_eq!(
                bind(&input, &native(typ.clone())).unwrap(),
                Some(expected),
                "{typ:?}"
            );
        }
        assert_eq!(bind(&DbCell::Null, &native(NativeType::Int)).unwrap(), None);
        for (typ, input) in [
            (NativeType::Int, "x"),
            (NativeType::Ascii, "ж"),
            (NativeType::Uuid, "nope"),
            (NativeType::Timeuuid, "123e4567-e89b-42d3-a456-426614174000"),
            (NativeType::Blob, "cafe"),
            (NativeType::Duration, "5 parsecs"),
        ] {
            assert!(bind(&DbCell::Text(input.into()), &native(typ.clone())).is_err(), "{typ:?}");
        }
    }

    #[test]
    fn literals_and_json_for_scripts() {
        assert_eq!(literal(&DbCell::Text("it's".into())), "'it''s'");
        assert_eq!(literal(&DbCell::Binary("yv4=".into())), "0xcafe");
        assert_eq!(literal(&DbCell::Int("5".into())), "5");
        assert_eq!(literal(&DbCell::Null), "null");
        assert_eq!(literal(&DbCell::Json("[1]".into())), "fromJson('[1]')");
        assert_eq!(json_of_cell(&DbCell::Binary("yv4=".into())), Value::String("0xcafe".into()));
        assert_eq!(json_of_cell(&DbCell::Json("{\"a\":1}".into()))["a"], 1);
    }
}
