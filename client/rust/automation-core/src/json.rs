//! The same depth/duplicate-key rules apply to every nested protocol value.
use crate::{AutomationError, Result};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

struct Tree(usize);

impl<'de> DeserializeSeed<'de> for Tree {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Value, D::Error> {
        if self.0 > 32 {
            return Err(de::Error::custom("protocol depth exceeded"));
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Tree {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("bounded JSON value")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| de::Error::custom("nonfinite number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Value, E> {
        Ok(Value::String(v.into()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_none<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Value, A::Error> {
        let mut result = Vec::new();
        while let Some(value) = seq.next_element_seed(Tree(self.0 + 1))? {
            result.push(value);
        }
        Ok(Value::Array(result))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Value, A::Error> {
        let mut result = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if key.chars().count() > 1024 || result.contains_key(&key) {
                return Err(de::Error::custom("duplicate or excessive protocol key"));
            }
            result.insert(key, map.next_value_seed(Tree(self.0 + 1))?);
        }
        Ok(Value::Object(result))
    }
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Value> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = Tree(0)
        .deserialize(&mut deserializer)
        .map_err(|_| AutomationError::Protocol("invalid bounded JSON"))?;
    deserializer
        .end()
        .map_err(|_| AutomationError::Protocol("trailing JSON content"))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_nested_fields_and_excessive_depth_are_rejected() {
        assert!(parse(br#"{"result":{"a":1,"a":2}}"#).is_err());
        assert!(parse(format!("{}null{}", "[".repeat(33), "]".repeat(33)).as_bytes()).is_err());
        assert!(parse(br#"{"result":[true,null,3.5]}"#).is_ok());
        assert!(parse(b"{}{}").is_err());
    }
}
