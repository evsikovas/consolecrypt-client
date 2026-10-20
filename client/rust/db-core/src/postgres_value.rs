//! PostgreSQL parameters use the wire protocol's text format, never SQL
//! interpolation. Results decode the binary format without lossy f64 numerics.
use crate::{DbCell, DbError};
use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::BytesMut;
use std::{error::Error, fmt};
use tokio_postgres::types::{Format, FromSql, IsNull, Kind, ToSql, Type};

#[derive(Debug)]
pub(crate) struct Parameter<'a>(pub &'a DbCell);
impl ToSql for Parameter<'_> {
    fn to_sql(
        &self,
        _: &Type,
        output: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match self.0 {
            DbCell::Null => return Ok(IsNull::Yes),
            DbCell::Bool(value) => {
                output.extend_from_slice(if *value { b"true" } else { b"false" })
            }
            DbCell::Binary(value) => {
                output.extend_from_slice(b"\\x");
                for byte in STANDARD.decode(value)? {
                    const HEX: &[u8] = b"0123456789abcdef";
                    output
                        .extend_from_slice(&[HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]]);
                }
            }
            DbCell::Int(value)
            | DbCell::Uint(value)
            | DbCell::Decimal(value)
            | DbCell::Float(value)
            | DbCell::Text(value)
            | DbCell::Date(value)
            | DbCell::Time(value)
            | DbCell::Timestamp(value)
            | DbCell::Json(value) => output.extend_from_slice(value.as_bytes()),
        }
        Ok(IsNull::No)
    }
    fn accepts(_: &Type) -> bool {
        true
    }
    fn encode_format(&self, _: &Type) -> Format {
        Format::Text
    }
    tokio_postgres::types::to_sql_checked!();
}

pub(crate) struct RawValue<'a>(&'a [u8]);
impl fmt::Debug for RawValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RawValue([REDACTED])")
    }
}
impl<'a> FromSql<'a> for RawValue<'a> {
    fn from_sql(_: &Type, value: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Ok(Self(value))
    }
    fn accepts(_: &Type) -> bool {
        true
    }
}

pub(crate) fn row(row: &tokio_postgres::Row) -> Result<Vec<DbCell>, DbError> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(index, column)| {
            match row
                .try_get::<_, Option<RawValue<'_>>>(index)
                .map_err(|_| DbError::QueryFailed)?
            {
                None => Ok(DbCell::Null),
                Some(raw) => decode(column.type_(), raw.0, 0),
            }
        })
        .collect()
}

fn decode(kind: &Type, raw: &[u8], depth: usize) -> Result<DbCell, DbError> {
    if raw.len() > crate::MAX_EVENT_BYTES || depth > 16 {
        return Err(DbError::ResultLimit);
    }
    if let Kind::Domain(inner) = kind.kind() {
        return decode(inner, raw, depth + 1);
    }
    if let Kind::Array(inner) = kind.kind() {
        return array(inner, raw, depth + 1);
    }
    let text = || {
        std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| DbError::QueryFailed)
    };
    Ok(match *kind {
        Type::BOOL if raw.len() == 1 => match raw[0] {
            0 => DbCell::Bool(false),
            1 => DbCell::Bool(true),
            _ => return Err(DbError::QueryFailed),
        },
        Type::INT2 => DbCell::Int(i16::from_be_bytes(fixed(raw)?).to_string()),
        Type::INT4 => DbCell::Int(i32::from_be_bytes(fixed(raw)?).to_string()),
        Type::INT8 => DbCell::Int(i64::from_be_bytes(fixed(raw)?).to_string()),
        Type::OID | Type::XID | Type::CID => {
            DbCell::Uint(u32::from_be_bytes(fixed(raw)?).to_string())
        }
        Type::FLOAT4 => DbCell::Float(f32::from_be_bytes(fixed(raw)?).to_string()),
        Type::FLOAT8 => DbCell::Float(f64::from_be_bytes(fixed(raw)?).to_string()),
        Type::NUMERIC => DbCell::Decimal(numeric(raw)?),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN | Type::XML => {
            DbCell::Text(text()?)
        }
        Type::JSON => DbCell::Json(text()?),
        Type::JSONB if raw.first() == Some(&1) => DbCell::Json(
            std::str::from_utf8(&raw[1..])
                .map_err(|_| DbError::QueryFailed)?
                .into(),
        ),
        Type::BYTEA => DbCell::Binary(STANDARD.encode(raw)),
        Type::UUID => DbCell::Text(
            uuid::Uuid::from_slice(raw)
                .map_err(|_| DbError::QueryFailed)?
                .to_string(),
        ),
        Type::DATE => {
            let days = i32::from_be_bytes(fixed(raw)?);
            DbCell::Date(match days {
                i32::MIN => "-infinity".into(),
                i32::MAX => "infinity".into(),
                _ => chrono::NaiveDate::from_ymd_opt(2000, 1, 1)
                    .unwrap()
                    .checked_add_signed(chrono::Duration::days(i64::from(days)))
                    .ok_or(DbError::QueryFailed)?
                    .to_string(),
            })
        }
        Type::TIMESTAMP | Type::TIMESTAMPTZ => {
            let micros = i64::from_be_bytes(fixed(raw)?);
            DbCell::Timestamp(match micros {
                i64::MIN => "-infinity".into(),
                i64::MAX => "infinity".into(),
                _ => {
                    let timestamp = micros
                        .checked_add(946_684_800_000_000)
                        .ok_or(DbError::QueryFailed)?;
                    let date = chrono::DateTime::from_timestamp_micros(timestamp)
                        .ok_or(DbError::QueryFailed)?;
                    if *kind == Type::TIMESTAMPTZ {
                        date.to_rfc3339()
                    } else {
                        date.naive_utc().to_string()
                    }
                }
            })
        }
        Type::TIME => DbCell::Time(time(i64::from_be_bytes(fixed(raw)?))?),
        Type::TIMETZ if raw.len() == 12 => {
            let clock = time(i64::from_be_bytes(fixed(&raw[..8])?))?;
            let offset = -i64::from(i32::from_be_bytes(fixed(&raw[8..])?));
            let sign = if offset < 0 { '-' } else { '+' };
            let offset = offset.abs();
            DbCell::Time(format!(
                "{clock}{sign}{:02}:{:02}:{:02}",
                offset / 3600,
                offset / 60 % 60,
                offset % 60
            ))
        }
        Type::INTERVAL if raw.len() == 16 => DbCell::Text(format!(
            "{} months {} days {} microseconds",
            i32::from_be_bytes(fixed(&raw[12..])?),
            i32::from_be_bytes(fixed(&raw[8..12])?),
            i64::from_be_bytes(fixed(&raw[..8])?)
        )),
        _ if matches!(kind.kind(), Kind::Enum(_)) => DbCell::Text(text()?),
        // Extensions have arbitrary codecs: preserve their bytes instead of
        // guessing UTF-8 or silently coercing a high precision type.
        _ => DbCell::Binary(STANDARD.encode(raw)),
    })
}

fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], DbError> {
    bytes.try_into().map_err(|_| DbError::QueryFailed)
}
fn time(micros: i64) -> Result<String, DbError> {
    if !(0..=86_400_000_000).contains(&micros) {
        return Err(DbError::QueryFailed);
    }
    Ok(format!(
        "{:02}:{:02}:{:02}.{:06}",
        micros / 3_600_000_000,
        micros / 60_000_000 % 60,
        micros / 1_000_000 % 60,
        micros % 1_000_000
    ))
}

fn numeric(bytes: &[u8]) -> Result<String, DbError> {
    if bytes.len() < 8 || bytes.len() % 2 != 0 {
        return Err(DbError::QueryFailed);
    }
    let count = usize::from(u16::from_be_bytes(fixed(&bytes[..2])?));
    let weight = i16::from_be_bytes(fixed(&bytes[2..4])?);
    let sign = u16::from_be_bytes(fixed(&bytes[4..6])?);
    let scale = usize::from(u16::from_be_bytes(fixed(&bytes[6..8])?));
    if bytes.len() != 8 + count * 2 || scale > 16_383 {
        return Err(DbError::QueryFailed);
    }
    match sign {
        0xc000 => return Ok("NaN".into()),
        0xd000 => return Ok("Infinity".into()),
        0xf000 => return Ok("-Infinity".into()),
        0 | 0x4000 => {}
        _ => return Err(DbError::QueryFailed),
    }
    let digits = bytes[8..]
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    if digits.iter().any(|digit| *digit > 9999) {
        return Err(DbError::QueryFailed);
    }
    let digit_at = |position: i32| -> u16 {
        let index = i32::from(weight) - position;
        if index < 0 {
            0
        } else {
            digits.get(index as usize).copied().unwrap_or(0)
        }
    };
    let mut result = String::new();
    if sign == 0x4000 && digits.iter().any(|digit| *digit != 0) {
        result.push('-');
    }
    if weight < 0 {
        result.push('0');
    } else {
        result.push_str(&digit_at(i32::from(weight)).to_string());
        for position in (0..i32::from(weight)).rev() {
            result.push_str(&format!("{:04}", digit_at(position)));
        }
    }
    if scale > 0 {
        result.push('.');
        let start = result.len();
        for group in 1..=scale.div_ceil(4) {
            result.push_str(&format!("{:04}", digit_at(-(group as i32))));
        }
        result.truncate(start + scale);
    }
    Ok(result)
}

fn array(inner: &Type, bytes: &[u8], depth: usize) -> Result<DbCell, DbError> {
    let mut bytes = bytes;
    fn next(bytes: &mut &[u8]) -> Result<i32, DbError> {
        if bytes.len() < 4 {
            return Err(DbError::QueryFailed);
        }
        let value = i32::from_be_bytes(fixed(&bytes[..4])?);
        *bytes = &bytes[4..];
        Ok(value)
    }
    let dimensions = next(&mut bytes)?;
    if !(0..=6).contains(&dimensions) {
        return Err(DbError::QueryFailed);
    }
    let _has_null = next(&mut bytes)?;
    let element_oid = next(&mut bytes)? as u32;
    if element_oid != inner.oid() {
        return Err(DbError::QueryFailed);
    }
    let mut shape = Vec::new();
    let mut count = if dimensions == 0 { 0usize } else { 1 };
    for _ in 0..dimensions {
        let length = next(&mut bytes)?;
        let _lower_bound = next(&mut bytes)?;
        if length < 0 {
            return Err(DbError::QueryFailed);
        }
        count = count
            .checked_mul(length as usize)
            .ok_or(DbError::ResultLimit)?;
        if count > 100_000 {
            return Err(DbError::ResultLimit);
        }
        shape.push(length as usize);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let length = next(&mut bytes)?;
        if length == -1 {
            values.push(serde_json::Value::Null);
            continue;
        }
        if length < 0 || length as usize > bytes.len() {
            return Err(DbError::QueryFailed);
        }
        let value = decode(inner, &bytes[..length as usize], depth)?;
        values.push(match &value {
            DbCell::Null => serde_json::Value::Null,
            DbCell::Bool(value) => serde_json::json!(value),
            DbCell::Json(value) => serde_json::from_str(value).map_err(|_| DbError::QueryFailed)?,
            DbCell::Int(value)
            | DbCell::Uint(value)
            | DbCell::Decimal(value)
            | DbCell::Float(value)
            | DbCell::Text(value)
            | DbCell::Date(value)
            | DbCell::Time(value)
            | DbCell::Timestamp(value)
            | DbCell::Binary(value) => serde_json::Value::String(value.clone()),
        });
        bytes = &bytes[length as usize..];
    }
    if !bytes.is_empty() {
        return Err(DbError::QueryFailed);
    }
    fn reshape(
        values: &mut std::vec::IntoIter<serde_json::Value>,
        shape: &[usize],
    ) -> serde_json::Value {
        if shape.is_empty() {
            return values.next().unwrap_or(serde_json::Value::Null);
        }
        serde_json::Value::Array(
            (0..shape[0])
                .map(|_| reshape(values, &shape[1..]))
                .collect(),
        )
    }
    let json = if shape.is_empty() {
        serde_json::json!([])
    } else {
        reshape(&mut values.into_iter(), &shape)
    };
    Ok(DbCell::Json(
        serde_json::to_string(&json).map_err(|_| DbError::QueryFailed)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_codec_preserves_38_digit_decimal_and_fraction_scale() {
        let mut bytes = Vec::new();
        for number in [
            10u16, 4, 0, 20, 1234, 5678, 9012, 3456, 7890, 1234, 5678, 9012, 3456, 7890,
        ] {
            bytes.extend(number.to_be_bytes());
        }
        assert_eq!(
            numeric(&bytes).unwrap(),
            "12345678901234567890.12345678901234567890"
        );
        let bytes = [0, 1, 255, 255, 64, 0, 0, 6, 0, 12];
        assert_eq!(numeric(&bytes).unwrap(), "-0.001200");
    }
    #[test]
    fn protocol_parameters_are_text_payload_without_sql_quotes() {
        let cell = DbCell::Text("'; DROP TABLE accounts; --".into());
        let mut bytes = BytesMut::new();
        Parameter(&cell).to_sql(&Type::TEXT, &mut bytes).unwrap();
        assert_eq!(bytes.as_ref(), b"'; DROP TABLE accounts; --");
        assert!(matches!(
            Parameter(&cell).encode_format(&Type::TEXT),
            Format::Text
        ));
    }
}
