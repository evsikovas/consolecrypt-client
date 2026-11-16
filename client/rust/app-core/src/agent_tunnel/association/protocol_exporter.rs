//! Closed observations from the SAME pinned Node carrier, never caller policy.
use super::*;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::agent_tunnel::association) struct ExporterCatalog {
    pub policy: ManagedExporterPolicy,
    pub manifest_sha256: Hash,
}
impl ExporterCatalog {
    pub(in crate::agent_tunnel::association) fn check(&self, host: ObjectId, agent: AgentId) -> AppResult<()> {
        if self.policy.host_id != host.to_string() || self.policy.agent_id != agent
            || self.policy.digest().map_err(|_| unavailable())? != self.manifest_sha256 {
            return Err(unavailable());
        }
        Ok(())
    }
    pub(in crate::agent_tunnel::association) fn public(&self) -> Value {
        serde_json::json!({"policy":self.policy,"manifestSha256":self.manifest_sha256})
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::agent_tunnel::association) struct ExporterProbe {
    pub node: NodeIdentity,
    pub owner_pin: OwnerPin,
    pub actor_revision: u64,
    #[serde(deserialize_with = "required_certificate")]
    pub certificate: Option<Certificate>,
    pub revoked: bool,
    pub advertised_scopes: Vec<Scope>,
    #[serde(deserialize_with = "required_exporter")]
    pub exporter: Option<ExporterCatalog>,
}
fn required_exporter<'de, D: serde::Deserializer<'de>>(d: D)
    -> std::result::Result<Option<ExporterCatalog>, D::Error> {
    Option::<ExporterCatalog>::deserialize(d)
}
impl ExporterProbe {
    pub(in crate::agent_tunnel::association) fn checked(self, scope: &OriginalScope, host: ObjectId, initial_enrollment: bool)
        -> AppResult<(Probe, ExporterCatalog)> {
        if self.advertised_scopes.as_slice() != [Scope::ExportersManage] {
            return Err(unavailable());
        }
        let exporter = self.exporter.ok_or_else(unavailable)?;
        exporter.check(host, self.node.agent_id)?;
        let probe = Probe { node: self.node, owner_pin: self.owner_pin,
            actor_revision: self.actor_revision, certificate: self.certificate, revoked: self.revoked };
        if initial_enrollment { initial(scope, &probe)?; }
        else { scope.bind_owner_pin(&probe.owner_pin)?; }
        Ok((probe, exporter))
    }
    pub(in crate::agent_tunnel::association) fn checked_revocation(self, scope: &OriginalScope) -> AppResult<Probe> {
        if self.advertised_scopes.as_slice() != [Scope::ExportersManage] || self.exporter.is_some() {
            return Err(unavailable());
        }
        scope.bind_owner_pin(&self.owner_pin)?;
        Ok(Probe { node: self.node, owner_pin: self.owner_pin, actor_revision: self.actor_revision,
            certificate: self.certificate, revoked: self.revoked })
    }
}
