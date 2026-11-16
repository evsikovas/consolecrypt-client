//! Fixed product service attach over an already verified dedicated SSH session.
//! Observed metadata is DATA, never enrollment or target/material authority.
use crate::{AutomationError, EnvironmentProbe, HelperPeer, Result, ServiceLocalNamespace};
use cc_crypto_core::{DevicePublicKeys, ServiceExecutionOrigin};
use cc_ssh_core::SshSession;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;
fn denied() -> AutomationError {
    AutomationError::Invalid("original service endpoint unavailable")
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceRemoteConfig {
    pub identity_id: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    Unbound,
    Pending,
    Enrolled,
    Restored,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePeerObservation {
    pub version: u8,
    pub signing: String,
    pub recipient: String,
    pub persistent_namespace: ServiceLocalNamespace,
    pub service_state: ServiceState,
    pub environment_digest: String,
    pub helper_digest: String,
    pub original_origin: Option<ServiceExecutionOrigin>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Offer {
    signing: String,
    recipient: String,
    probe: Value,
    persistent_namespace: ServiceLocalNamespace,
    service_state: ServiceState,
}
fn key(raw: &str) -> Result<[u8; 32]> {
    let value: [u8; 32] = hex::decode(raw)
        .map_err(|_| denied())?
        .try_into()
        .map_err(|_| denied())?;
    if hex::encode(value) != raw {
        return Err(denied());
    }
    Ok(value)
}
impl ServiceRemoteConfig {
    pub fn identity(&self) -> Result<Uuid> {
        let id = Uuid::parse_str(&self.identity_id).map_err(|_| denied())?;
        if id.is_nil() || id.to_string() != self.identity_id {
            return Err(denied());
        }
        Ok(id)
    }
    pub fn command(&self) -> Result<String> {
        let id = self.identity()?;
        Ok(format!(
            "exec '/opt/cc-runtime/bin/python' -I -B '/opt/consolecrypt/deployment/service_control.py' '{id}'"
        ))
    }
    /// Fixed one-shot observation works even after the original Node exited.
    /// It starts no Node/Helper and never opens an enrollment/recovery lane.
    pub async fn results(&self, session: &SshSession, body: Value) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            version: u8,
            r#type: String,
            request_id: String,
            result: Value,
            error: Option<Value>,
        }
        let identity = self.identity()?;
        let request = Uuid::new_v4().to_string();
        let mut input = serde_json::to_vec(
            &json!({"version":1,"request_id":request,"operation":"service_results","body":body}),
        )
        .map_err(|_| denied())?;
        if input.len() > 8191 {
            return Err(denied());
        }
        input.push(b'\n');
        let command = format!(
            "exec '/opt/cc-runtime/bin/python' -I -B '/opt/consolecrypt/deployment/service_control.py' --results '{identity}'"
        );
        let output = tokio::time::timeout(
            Duration::from_secs(30),
            session.exec_with_private_input(&command, &input, 65536),
        )
        .await
        .map_err(|_| AutomationError::Timeout)?
        .map_err(|_| denied())?;
        if output.exit_status != Some(0)
            || output.exit_signal.is_some()
            || !output.stderr.is_empty()
            || !output.stdout.ends_with(b"\n")
        {
            return Err(denied());
        }
        let value = crate::json::parse(&output.stdout)?;
        let reply: Reply = serde_json::from_value(value).map_err(|_| denied())?;
        if reply.version != 1
            || reply.r#type != "response"
            || reply.request_id != request
            || reply.error.is_some()
        {
            return Err(denied());
        }
        Ok(reply.result)
    }
    pub async fn connect(&self, session: &SshSession) -> Result<Arc<HelperPeer>> {
        let channel =
            tokio::time::timeout(Duration::from_secs(10), session.open_exec(&self.command()?))
                .await
                .map_err(|_| AutomationError::Timeout)?
                .map_err(|_| denied())?;
        let (read, write) = tokio::io::split(channel.into_stream());
        // Service owns capsule-only target channels; never attach a Native generic
        // TargetHandler or general Vault resolver to this process.
        Ok(HelperPeer::connect(read, write))
    }
    pub async fn observe(
        &self,
        peer: &Arc<HelperPeer>,
        raw_probe: Value,
    ) -> Result<ServicePeerObservation> {
        let expected = self.identity()?;
        let probe = EnvironmentProbe::parse(raw_probe.clone())?;
        if probe.system != "Linux"
            || !probe.non_root
            || probe.capabilities.get("service_local_executor_v1") != Some(&true)
        {
            return Err(denied());
        }
        probe.require_trusted_worker_bootstrap()?;
        let helper = raw_probe
            .get("helper_digest")
            .and_then(Value::as_str)
            .filter(|s| cc_models::automation::is_digest(s))
            .ok_or_else(denied)?
            .to_string();
        let raw = peer
            .request("service_offer", json!({}), Duration::from_secs(30))
            .await?;
        let offer: Offer = serde_json::from_value(raw).map_err(|_| denied())?;
        let public = DevicePublicKeys {
            signing: key(&offer.signing)?,
            encryption: key(&offer.recipient)?,
        };
        if offer.persistent_namespace.identity_id != expected {
            return Err(denied());
        }
        offer.persistent_namespace.validate_public(&public)?;
        let actual = EnvironmentProbe::parse(offer.probe.clone())?;
        if actual.environment_digest != probe.environment_digest
            || offer.probe.get("helper_digest").and_then(Value::as_str) != Some(helper.as_str())
        {
            return Err(denied());
        }
        let origin = match raw_probe.get("service_origin") {
            None | Some(Value::Null) => None,
            Some(value) => {
                let origin: ServiceExecutionOrigin =
                    serde_json::from_value(value.clone()).map_err(|_| denied())?;
                origin.validate().map_err(|_| denied())?;
                if origin.signer_key_sha256 != hex::encode(Sha256::digest(public.signing))
                    || origin.recipient_key_sha256 != hex::encode(Sha256::digest(public.encryption))
                    || origin.environment_digest != probe.environment_digest
                    || origin.helper_digest != helper
                {
                    return Err(denied());
                }
                Some(origin)
            }
        };
        if matches!(
            offer.service_state,
            ServiceState::Enrolled | ServiceState::Restored
        ) && origin.is_none()
        {
            return Err(denied());
        }
        Ok(ServicePeerObservation {
            version: 1,
            signing: offer.signing,
            recipient: offer.recipient,
            persistent_namespace: offer.persistent_namespace,
            service_state: offer.service_state,
            environment_digest: probe.environment_digest,
            helper_digest: helper,
            original_origin: origin,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn product_attach_has_only_fixed_paths_and_one_canonical_identity_reference() {
        let id = Uuid::new_v4();
        let config = ServiceRemoteConfig {
            identity_id: id.to_string(),
        };
        assert_eq!(
            config.command().unwrap(),
            format!(
                "exec '/opt/cc-runtime/bin/python' -I -B '/opt/consolecrypt/deployment/service_control.py' '{id}'"
            )
        );
        for bad in [
            Uuid::nil().to_string(),
            "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA".into(),
            "$(touch BAD)".into(),
            "../x".into(),
        ] {
            assert!(ServiceRemoteConfig { identity_id: bad }.command().is_err());
        }
    }
    #[test]
    fn service_reference_has_no_free_url_or_command_fields() {
        let id = Uuid::new_v4();
        let raw = json!({"identity_id":id,"command":"custom"});
        assert!(serde_json::from_value::<ServiceRemoteConfig>(raw).is_err());
        assert!(
            serde_json::from_value::<ServiceRemoteConfig>(json!({"identity_id":true})).is_err()
        );
    }
}
