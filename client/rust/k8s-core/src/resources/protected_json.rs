//! Temporary parsed objects may hold credentials even when their public
//! projection is masked. Keep every unvisited value protected on early errors.
use serde_json::Value;
use std::ops::{Deref, DerefMut};
use zeroize::Zeroize;

pub(crate) struct ProtectedJson(pub Value);
impl std::fmt::Debug for ProtectedJson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProtectedJson([redacted])")
    }
}
impl ProtectedJson {
    pub fn into_inner(mut self) -> Value {
        self.0.take()
    }
}
impl Deref for ProtectedJson {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}
impl DerefMut for ProtectedJson {
    fn deref_mut(&mut self) -> &mut Value {
        &mut self.0
    }
}
impl Drop for ProtectedJson {
    fn drop(&mut self) {
        wipe_json(&mut self.0);
    }
}
pub(crate) fn wipe_json(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(wipe_json),
        Value::Object(fields) => {
            for (mut name, mut value) in std::mem::take(fields) {
                name.zeroize();
                wipe_json(&mut value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wipes_unvisited_nested_strings_and_keys() {
        let mut value = serde_json::json!({
            "items":[{"data":{"key":"first"}},{"stringData":{"token":"unvisited"}}]
        });
        wipe_json(&mut value);
        assert_eq!(value, serde_json::json!({}));
    }
}
