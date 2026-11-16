//! One same-SSH collector session; terminal metadata grants no authority.
use super::*;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Terminal {
    format: String,
    collector_id: ResourceId,
    lease_hash: Hash,
    state: String,
    cleanup_observed: bool,
    loss_unknown: bool,
}
pub(in crate::agent_tunnel::association) struct CollectorTerminalObservation {
    pub(in crate::agent_tunnel::association) stopped: bool,
    pub(in crate::agent_tunnel::association) cleanup_observed: bool,
}
impl Terminal {
    fn check(&self, collector: ResourceId, lease: Hash) -> AppResult<CollectorTerminalObservation> {
        if self.format != "cc-agent-collector-result/v1" || self.collector_id != collector
            || self.lease_hash != lease || !matches!(self.state.as_str(), "stopped" | "unknown")
            || self.state == "stopped" && !self.cleanup_observed || !self.loss_unknown
        { return Err(unknown()); }
        Ok(CollectorTerminalObservation { stopped: self.state == "stopped", cleanup_observed: self.cleanup_observed })
    }
}
impl Peer {
    /// Fixed ten-second installed-unit launch plus five-second dispatch/receipt
    /// allowance, clipped by the SAME original scope, consent and signed expiry.
    /// This reply budget creates no authority and never changes a lease deadline.
    pub(in crate::agent_tunnel::association) async fn start_collector_original<T:serde::de::DeserializeOwned>(
        &mut self,core:&Weak<crate::app::Inner>,payload:serde_json::Value,expiry:Timestamp,
    )->AppResult<T> {
        let(id,raw)=protocol::request("startCollector",payload)?;
        self.exchange_frame_weak_with_budget(core,"startCollector",id,raw,Some(expiry),Duration::from_secs(15)).await
    }
    /// Stop retires only the existing owned collector; it cannot renew a lease,
    /// perform another Actor.admit, start an exporter or choose a destination.
    pub(in crate::agent_tunnel::association) async fn stop_collector_original(
        &mut self, core: &Weak<crate::app::Inner>, collector: ResourceId,
        lease: Hash, expiry: Timestamp,
    ) -> AppResult<()> {
        self.scope.check_current()?;
        if self.requests.len() >= self.request_limit { return Err(unknown()); }
        let (id, raw) = protocol::request("stopCollector", serde_json::json!({
            "collectorId": collector, "leaseHash": lease,
        }))?;
        if self.requests.contains(&id) { return Err(unknown()); }
        self.requests.push(id);
        let end = self.scope.deadline().min(cap_read(&self.until)?)
            .min(Instant::now() + self.scope.remaining_until(expiry)?);
        let scope = self.scope.clone();
        let stream = self.stream.as_mut().and_then(|s| s.stream.as_mut()).ok_or_else(unknown)?;
        bounded_weak(core, &scope, end, async {
            stream.write_all(&raw).await.map_err(|_| unknown())?;
            stream.flush().await.map_err(|_| unknown())
        }).await
    }
    /// A real joined/empty unit and actual SSH EOF are required. A marker,
    /// process exit alone, local Stop, or peer-supplied delivered count is not.
    pub(in crate::agent_tunnel::association) async fn observe_collector_terminal(
        &mut self, collector: ResourceId, lease: Hash, expiry: Timestamp,
    ) -> AppResult<CollectorTerminalObservation> {
        self.scope.check_current()?;
        let end = self.scope.deadline().min(cap_read(&self.until)?)
            .min(Instant::now() + self.scope.remaining_until(expiry)?);
        let scope = self.scope.clone();
        let stream = self.stream.as_mut().and_then(|s| s.stream.as_mut()).ok_or_else(unknown)?;
        bounded(&scope, end, async {
            let mut raw = Zeroizing::new(Vec::new());
            loop {
                let byte = stream.read_u8().await.map_err(|_| unknown())?;
                raw.push(byte);
                if raw.len() > 4096 || byte == b'\r' || !byte.is_ascii() { return Err(unknown()); }
                if byte == b'\n' { break; }
            }
            let terminal: Terminal = serde_json::from_slice(&raw).map_err(|_| unknown())?;
            let observation = terminal.check(collector, lease)?;
            let mut tail = [0u8; 1];
            if stream.read(&mut tail).await.map_err(|_| unknown())? != 0 { return Err(unknown()); }
            Ok(observation)
        }).await.and_then(|observation| { self.scope.check_current()?; Ok(observation) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_cleanup_does_not_upgrade_unknown_collection_or_wrong_original_resource() {
        let id = ResourceId::new("00000000-0000-4000-8000-000000000001").unwrap();
        let other = ResourceId::new("00000000-0000-4000-8000-000000000002").unwrap();
        let lease = Hash::from_bytes([17; 32]);
        let terminal = Terminal { format: "cc-agent-collector-result/v1".into(), collector_id: id,
            lease_hash: lease, state: "unknown".into(), cleanup_observed: true, loss_unknown: true };
        let observation = terminal.check(id, lease).unwrap();
        assert!(!observation.stopped && observation.cleanup_observed);
        assert!(terminal.check(other, lease).is_err());
        assert!(terminal.check(id, Hash::from_bytes([18; 32])).is_err());
        let mut terminal = terminal;
        terminal.state = "stopped".into(); terminal.cleanup_observed = false;
        assert!(terminal.check(id, lease).is_err());
        // Actual same-SSH EOF and joined unit checks remain runtime requirements.
    }
}
