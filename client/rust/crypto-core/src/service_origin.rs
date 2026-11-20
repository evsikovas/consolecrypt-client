//! Signed service-local origin DATA. It does not authorize a connection.
use crate::ServiceDelegationError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceExecutionOrigin {
    pub version: u8,
    pub service_id: String,
    pub runner_revision: u64,
    pub runner_configuration_digest: String,
    pub signer_key_sha256: String,
    pub recipient_key_sha256: String,
    pub environment_digest: String,
    pub helper_digest: String,
    pub known_hosts_digest: String,
    pub backend: String,
    pub network: String,
}
impl ServiceExecutionOrigin {
    pub fn validate(&self) -> Result<(), ServiceDelegationError> {
        let id =
            uuid::Uuid::parse_str(&self.service_id).map_err(|_| ServiceDelegationError::Invalid)?;
        if self.version != 1
            || id.is_nil()
            || id.to_string() != self.service_id
            || !(1..=9_007_199_254_740_991).contains(&self.runner_revision)
            || self.backend != "linuxNativeSsh"
            || self.network != "serviceLocal"
            || [
                &self.runner_configuration_digest,
                &self.signer_key_sha256,
                &self.recipient_key_sha256,
                &self.environment_digest,
                &self.helper_digest,
                &self.known_hosts_digest,
            ]
            .into_iter()
            .any(|v| !cc_models::automation::is_digest(v))
        {
            return Err(ServiceDelegationError::Invalid);
        }
        Ok(())
    }

    /// Both ends derive this from the reviewed descriptor and actual first hop.
    /// No caller-supplied digest can replace the descriptor or endpoint.
    pub fn stream_scope(&self, host: &str, port: u16) -> Result<[u8; 32], ServiceDelegationError> {
        self.validate()?;
        if host.is_empty() || host.len() > 253 || port == 0 || host.chars().any(char::is_control) {
            return Err(ServiceDelegationError::Invalid);
        }
        let mut bytes = b"ConsoleCrypt ServiceLocalOrigin v1\0".to_vec();
        bytes.extend(
            serde_json::to_vec(&serde_json::json!({"origin":self,"host":host,"port":port}))
                .map_err(|_| ServiceDelegationError::Invalid)?,
        );
        Ok(Sha256::digest(bytes).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn origin() -> ServiceExecutionOrigin {
        ServiceExecutionOrigin {
            version: 1,
            service_id: uuid::Uuid::new_v4().to_string(),
            runner_revision: 1,
            runner_configuration_digest: "a".repeat(64),
            signer_key_sha256: "b".repeat(64),
            recipient_key_sha256: "c".repeat(64),
            environment_digest: "d".repeat(64),
            helper_digest: "e".repeat(64),
            known_hosts_digest: "f".repeat(64),
            backend: "linuxNativeSsh".into(),
            network: "serviceLocal".into(),
        }
    }
    #[test]
    fn service_scope_binds_each_original_origin_field_and_endpoint() {
        let base = origin();
        let digest = base.stream_scope("192.0.2.1", 22).unwrap();
        assert_eq!(digest, base.stream_scope("192.0.2.1", 22).unwrap());
        assert_ne!(digest, base.stream_scope("192.0.2.2", 22).unwrap());
        assert_ne!(digest, base.stream_scope("192.0.2.1", 2222).unwrap());
        for key in [
            "service_id",
            "runner_revision",
            "runner_configuration_digest",
            "signer_key_sha256",
            "recipient_key_sha256",
            "environment_digest",
            "helper_digest",
            "known_hosts_digest",
        ] {
            let mut value = serde_json::to_value(&base).unwrap();
            value[key] = match key {
                "service_id" => serde_json::json!(uuid::Uuid::new_v4().to_string()),
                "runner_revision" => serde_json::json!(2),
                _ => serde_json::json!("1".repeat(64)),
            };
            let changed: ServiceExecutionOrigin = serde_json::from_value(value).unwrap();
            assert_ne!(digest, changed.stream_scope("192.0.2.1", 22).unwrap());
        }
    }
    #[test]
    fn desktop_unknown_backend_duplicate_and_invalid_endpoint_are_not_service_origin() {
        let base = origin();
        let mut bad = base.clone();
        bad.network = "desktop".into();
        assert!(bad.validate().is_err());
        bad = base.clone();
        bad.backend = "openssh".into();
        assert!(bad.validate().is_err());
        assert!(base.stream_scope("host\nother", 22).is_err());
        assert!(base.stream_scope("host", 0).is_err());
        let json = serde_json::to_string(&base).unwrap();
        assert!(
            serde_json::from_str::<ServiceExecutionOrigin>(&json.replacen(
                "{",
                "{\"version\":1,",
                1
            ))
            .is_err()
        );
    }
}
