use super::{AwxError, Value, RESPONSE_LIMIT};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;

struct Checked(Value);
impl<'de> Deserialize<'de> for Checked {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = Checked;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded unique-key AWX JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Checked, E> {
                Ok(Checked(Value::Bool(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Checked, E> {
                Ok(Checked(Value::Null))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Checked, E> {
                Ok(Checked(Value::Number(v.into())))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Checked, E> {
                Ok(Checked(Value::Number(v.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Checked, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Checked(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Checked, E> {
                Ok(Checked(Value::String(v.to_owned())))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Checked, E> {
                Ok(Checked(Value::String(v)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Checked, A::Error> {
                let mut out = Vec::new();
                while let Some(Checked(v)) = a.next_element::<Checked>()? {
                    out.push(v);
                }
                Ok(Checked(Value::Array(out)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Checked, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if out.contains_key(&k) {
                        return Err(serde::de::Error::custom("duplicate AWX key"));
                    }
                    out.insert(k, a.next_value::<Checked>()?.0);
                }
                Ok(Checked(Value::Object(out)))
            }
        }
        de.deserialize_any(JsonVisitor)
    }
}
pub(super) fn parse(bytes: &[u8]) -> Result<Value, AwxError> {
    if bytes.len() > RESPONSE_LIMIT {
        return Err(AwxError::Budget);
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = Checked::deserialize(&mut decoder).map_err(|_| AwxError::Response)?;
    decoder.end().map_err(|_| AwxError::Response)?;
    if !value.0.is_object() {
        return Err(AwxError::Response);
    }
    Ok(value.0)
}
