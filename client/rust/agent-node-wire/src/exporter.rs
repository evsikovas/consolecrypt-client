//! One immutable node_exporter manifest. No paths, shell, user, root, Docker,
//! install/download, adoption, scope extension or caller clock is represented.
use crate::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ExporterArtifact {
    pub name:String,pub version:String,pub platform:String,
    pub archive_sha256:Hash,pub binary_sha256:Hash,pub binary_bytes:u64,
    pub notice_inventory_sha256:Hash,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ManagedExporterPolicy {
    pub format:String,pub host_id:String,pub agent_id:AgentId,
    pub artifact:ExporterArtifact,pub bind_ip:String,pub bind_port:u16,
    pub collectors:Vec<String>,pub max_seconds:u32,
}
fn nonzero(hash:Hash)->bool {hash.as_bytes().iter().any(|b|*b!=0)}
impl ManagedExporterPolicy {
    pub fn validate(&self)->Result<()> {
        let id=uuid::Uuid::parse_str(&self.host_id).map_err(|_|Error::WrongClaim)?;
        let archive=match self.artifact.platform.as_str() {
            "linux-x86_64"=>"b51d8a76aa2a9156a55d501aca6276fae09e262259a5e4e831d2c2222f084e63",
            "linux-aarch64"=>"ad35b605f9954b9f1ffddf5ba054bdc5a98d790b9eae5291e1eeb83f1ecbd0e7",
            _=>return Err(Error::CapabilityDenied),
        };
        if self.format!="cc-agent-managed-node-exporter/v1" || id.is_nil() || id.to_string()!=self.host_id
            || self.artifact.name!="node_exporter" || self.artifact.version!="v1.12.1"
            || hex::encode(self.artifact.archive_sha256.as_bytes())!=archive
            || !nonzero(self.artifact.binary_sha256) || !nonzero(self.artifact.notice_inventory_sha256)
            || !(64..=64*1024*1024).contains(&self.artifact.binary_bytes)
            || self.bind_ip!="127.0.0.1" || self.bind_port!=19100
            || self.collectors!=["cpu","meminfo"] || !(30..=300).contains(&self.max_seconds) {
            return Err(Error::ScopeDenied)
        }
        Ok(())
    }
    pub fn digest(&self)->Result<Hash> {
        self.validate()?;
        let value=serde_json::to_value(self).map_err(|_|Error::InvalidWire)?;
        let mut bytes=Vec::new();crate::wire::canonical_json(&value,&mut bytes)?;
        if bytes.len()>4096{return Err(Error::Quota)}
        Ok(Hash::from_bytes(Sha256::digest(bytes).into()))
    }
    pub fn operation(&self)->Result<Operation> {Ok(Operation::ExportersManage{manifest:self.digest()?})}
    pub fn check_binding(&self,binding:&NodeRequestBinding)->Result<()> {
        self.validate()?;
        let cert=&binding.certificate.body;let lease=&binding.lease.body;
        if cert.scopes.0.as_slice()!=[Scope::ExportersManage] || cert.capabilities.scopes!=cert.scopes
            || self.agent_id!=cert.agent_id || lease.agent_id!=cert.agent_id
            || lease.key_epoch!=cert.key_epoch || lease.certificate_hash!=cert.digest()?
            || lease.operation!=self.operation()? || binding.operation!=lease.operation
            || lease.header!=binding.freshness.body.header
            || lease.expires_at.seconds().checked_sub(lease.issued_at.seconds())
                .is_none_or(|seconds|seconds>self.max_seconds as u64) {
            return Err(Error::WrongClaim)
        }
        Ok(())
    }
}
