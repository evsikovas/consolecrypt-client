//! Same original SSH stream, fixed exporter start/stop and actual terminal EOF.
use super::*;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Terminal {
    format: String, phase: String, manifest_sha256: Hash, lease_hash: Hash,
    expires_at: Timestamp, agent_id: AgentId,
    cleanup_observed: bool, leader_joined: bool, remote_drain_observed: bool,
}
pub(in crate::agent_tunnel::association) struct ExporterTerminalObservation {
    pub(in crate::agent_tunnel::association) stopped: bool,
    pub(in crate::agent_tunnel::association) cleanup_observed: bool,
}
impl Terminal {
    fn check(&self, manifest: Hash, lease: Hash, agent: AgentId, expiry: Timestamp)
        -> AppResult<ExporterTerminalObservation> {
        if self.format != "cc-agent-managed-exporter-session/v1" || self.phase != "stopped"
            || self.manifest_sha256 != manifest || self.lease_hash != lease
            || self.agent_id != agent || self.expires_at != expiry
            || !self.cleanup_observed || !self.leader_joined || self.remote_drain_observed {
            return Err(unknown());
        }
        Ok(ExporterTerminalObservation { stopped: true, cleanup_observed: true })
    }
}
impl Peer {
    pub(in crate::agent_tunnel::association) async fn start_exporter_original<T:serde::de::DeserializeOwned>(
        &mut self, core:&Weak<crate::app::Inner>, payload:serde_json::Value, expiry:Timestamp,
    ) -> AppResult<T> {
        let (id, raw) = protocol::request("startExporter", payload)?;
        self.exchange_frame_weak_with_budget(core,"startExporter",id,raw,Some(expiry),Duration::from_secs(15)).await
    }
    /// Write-only stop; only the following real terminal line and EOF can prove join.
    pub(in crate::agent_tunnel::association) async fn stop_exporter_original(
        &mut self, core:&Weak<crate::app::Inner>, manifest:Hash, lease:Hash, expiry:Timestamp,
    ) -> AppResult<()> {
        self.scope.check_current()?;
        if self.requests.len() >= self.request_limit { return Err(unknown()); }
        let (id, raw) = protocol::request("stopExporter",serde_json::json!({
            "manifestSha256":manifest,"leaseHash":lease}))?;
        if self.requests.contains(&id) { return Err(unknown()); }
        self.requests.push(id);
        let end = self.scope.deadline().min(cap_read(&self.until)?)
            .min(Instant::now() + self.scope.remaining_until(expiry)?);
        let scope = self.scope.clone();
        let stream = self.stream.as_mut().and_then(|s|s.stream.as_mut()).ok_or_else(unknown)?;
        bounded_weak(core,&scope,end,async {
            stream.write_all(&raw).await.map_err(|_|unknown())?;
            stream.flush().await.map_err(|_|unknown())
        }).await
    }
    pub(in crate::agent_tunnel::association) async fn observe_exporter_terminal(
        &mut self, manifest:Hash, lease:Hash, agent:AgentId, expiry:Timestamp,
    ) -> AppResult<ExporterTerminalObservation> {
        self.scope.check_current()?;
        let end = self.scope.deadline().min(cap_read(&self.until)?)
            .min(Instant::now() + self.scope.remaining_until(expiry)?);
        let scope = self.scope.clone();
        let stream = self.stream.as_mut().and_then(|s|s.stream.as_mut()).ok_or_else(unknown)?;
        bounded(&scope,end,async {
            let mut raw = Zeroizing::new(Vec::new());
            loop {
                let byte = stream.read_u8().await.map_err(|_|unknown())?;
                raw.push(byte);
                if raw.len()>4096 || byte==b'\r' || !byte.is_ascii() { return Err(unknown()); }
                if byte==b'\n' { break; }
            }
            let terminal:Terminal = serde_json::from_slice(&raw).map_err(|_|unknown())?;
            let observed = terminal.check(manifest,lease,agent,expiry)?;
            let mut tail=[0u8;1];
            if stream.read(&mut tail).await.map_err(|_|unknown())? != 0 { return Err(unknown()); }
            Ok(observed)
        }).await.and_then(|observed| {self.scope.check_current()?;Ok(observed)})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_expiry_lease_or_partial_child_cleanup_is_not_exporter_join() {
        let manifest=Hash::from_bytes([1;32]);let lease=Hash::from_bytes([2;32]);
        let agent=AgentId::new("00000000-0000-4000-8000-000000000001").unwrap();
        let expiry=Timestamp::new(1000).unwrap();
        let mut value=serde_json::json!({"format":"cc-agent-managed-exporter-session/v1","phase":"stopped",
            "manifestSha256":manifest,"leaseHash":lease,"expiresAt":expiry,"agentId":agent,
            "cleanupObserved":true,"leaderJoined":true,"remoteDrainObserved":false});
        let check=|v:&serde_json::Value|serde_json::from_value::<Terminal>(v.clone()).unwrap().check(manifest,lease,agent,expiry);
        assert!(check(&value).is_ok());
        for field in ["cleanupObserved","leaderJoined"] {
            value[field]=serde_json::json!(false);assert!(check(&value).is_err());value[field]=serde_json::json!(true);
        }
        value["expiresAt"]=serde_json::json!(1001);assert!(check(&value).is_err());value["expiresAt"]=serde_json::json!(expiry);
        value["leaseHash"]=serde_json::json!(manifest);assert!(check(&value).is_err());value["leaseHash"]=serde_json::json!(lease);
        value["phase"]=serde_json::json!("unknown");assert!(check(&value).is_err());
        // Actual SAME-stream EOF remains mandatory after this metadata check.
    }
}
