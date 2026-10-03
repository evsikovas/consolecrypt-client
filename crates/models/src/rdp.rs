//! Independent payload kind: pre-RDP clients cannot edit this as an SSH host.
//! Permissions and certificate decisions stay local to each RDP session.
use crate::{
    host::{Host, SshBackend},
    ValidationError,
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_RDP_PORT: u16 = 3389;
pub const fn default_width() -> u16 {
    1280
}
pub const fn default_height() -> u16 {
    720
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RdpHost {
    #[serde(flatten)]
    pub host: Host,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default = "default_width")]
    pub desktop_width: u16,
    #[serde(default = "default_height")]
    pub desktop_height: u16,
}
impl std::ops::Deref for RdpHost {
    type Target = Host;
    fn deref(&self) -> &Host {
        &self.host
    }
}
impl std::ops::DerefMut for RdpHost {
    fn deref_mut(&mut self) -> &mut Host {
        &mut self.host
    }
}
impl RdpHost {
    pub fn new(name: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            host: Host::new(name, address),
            domain: None,
            desktop_width: default_width(),
            desktop_height: default_height(),
        }
    }
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.host.validate()?;
        if !(200..=4096).contains(&self.desktop_width)
            || self.desktop_width % 2 != 0
            || !(200..=2160).contains(&self.desktop_height)
        {
            return Err(ValidationError::new("desktop", "unsupported dimensions"));
        }
        if self
            .domain
            .as_ref()
            .is_some_and(|s| s.len() > 255 || s.chars().any(char::is_control))
        {
            return Err(ValidationError::new("domain", "invalid domain"));
        }
        if !self.host.jump_chain.is_empty()
            || self.host.jump_profile_id.is_some()
            || self.host.proxy_id.is_some()
            || self.host.proxy_command.is_some()
            || self.host.agent_forwarding
            || self.host.backend != SshBackend::Native
        {
            return Err(ValidationError::new(
                "protocol",
                "SSH options are not RDP options",
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KekClass, ObjectKind, ObjectPayload, VaultObject};
    #[test]
    fn separate_kind_roundtrip_and_legacy_shape() {
        let h = RdpHost::new("Windows", "windows.example.test");
        h.validate().unwrap();
        let id = h.host.id;
        let value = serde_json::to_value(ObjectPayload::new(VaultObject::RdpHost(h))).unwrap();
        assert_eq!(value["kind"], "rdp_host");
        assert_eq!(value["data"]["desktop_width"], 1280);
        assert!(value["data"].get("password").is_none());
        let back: ObjectPayload = serde_json::from_value(value).unwrap();
        assert_eq!(back.object.id(), id);
        assert_eq!(back.object.kind(), ObjectKind::RdpHost);
        assert_eq!(back.object.kind().kek_class(), KekClass::Inventory);
    }
    #[test]
    fn rejects_ssh_transport_and_invalid_geometry() {
        let mut h = RdpHost::new("Windows", "windows.example.test");
        h.host.jump_chain.push(crate::ObjectId::new());
        assert!(h.validate().is_err());
        h.host.jump_chain.clear();
        h.desktop_width = 1279;
        assert!(h.validate().is_err());
    }
}
