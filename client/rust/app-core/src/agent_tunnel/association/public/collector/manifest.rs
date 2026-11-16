//! Pure observation validation shared with the exact Node wire schema.
//! The immutable Agent catalog and actual Actor permit supply the authority.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Manifest(pub(super) Value);
impl Manifest {
    pub(super) fn validate(&self, host: ObjectId) -> AppResult<Hash> {
        protocol::metrics_manifest_digest(&self.0,host)
    }
    pub(super) fn review(&self) -> Value { self.0.clone() }
    pub(super) fn disk(&self)->bool {self.0.get("profile").is_some()}
    pub(super) fn project_profile(&self,view:&mut Value)->AppResult<()> {
        if !self.disk(){return Ok(())}
        if !view.is_object(){return Err(unavailable())}
        view["profile"]=json!("maintained-prometheus-disk-512mib-2h-v1");
        view["bufferMode"]=json!("persistentNativeWal");
        view["diskBudget"]=json!({"maxChargedBytes":536870912u64,"maxResidenceSeconds":7200,
            "minBavailBytes":134217728u64,"operationalWalBytes":67108864u64,
            "checkpointBytes":67108864u64,"reservedBytes":134217728u64,"maxRootEntries":256});
        view.as_object_mut().ok_or_else(unavailable)?.remove("crashReplay");
        view["automaticRecovery"]=json!(false);
        view["requiresFreshGrantForReplay"]=json!(true);
        Ok(())
    }
}
