//! Result projection only; admission/auth/fences remain in monitoring_read.
use cc_monitoring_core::{Error, ErrorCode};
use serde::Serialize;
use serde_json::{json, Value};

const MAX_OUTPUT_BYTES: usize = cc_monitoring_core::alerts::MAX_ALERT_RESPONSE_BYTES;

pub(super) fn result<T: Serialize>(response: Result<T, Error>) -> Result<Value, serde_json::Error> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(raw.len())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
            Ok(raw.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    match response {
        Err(error) => Ok(json!({"error":error})),
        Ok(response) => {
            // Check the serialized DTO before allocating a second JSON tree.
            // New field names/unknown-state descriptors may expand HTTP input.
            if serde_json::to_writer(Counter(MAX_OUTPUT_BYTES), &response).is_err() {
                return Ok(json!({"error":Error{code:ErrorCode::BudgetExceeded,
                    message:"Monitoring alert result exceeds output budget".into(),http_status:None}}));
            }
            serde_json::to_value(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dto_expansion_cannot_exceed_wire_budget_or_leak_partial_data() {
        let response = result(Ok::<_, Error>(
            json!({"backendText":"x".repeat(MAX_OUTPUT_BYTES)}),
        ))
        .unwrap();
        assert_eq!(response["error"]["code"], "budget_exceeded");
        assert!(response.get("backendText").is_none());
        assert!(response.to_string().len() < 256);
    }
}
