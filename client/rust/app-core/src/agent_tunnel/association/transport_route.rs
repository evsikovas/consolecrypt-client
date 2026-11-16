//! Child of transport (registration: #[path="transport_route.rs"] mod route;).
//! One closed product frame. Capability never enters a serde Value or log.
use super::*;
use cc_protocol::auth::SecretString;
#[derive(Clone,Copy,PartialEq,Eq)]
pub(in crate::agent_tunnel::association) enum OriginalTransportMode { Admit, Resume }
#[derive(serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct RetiredTerminal {
    format:String,outcome:String,cleanup_observed:bool,drain_recorded:bool,
    lease_hash:Hash,policy_hash:Hash,node:protocol::NodeIdentity,expires_at:Timestamp,
}
/// Constructed only by the SAME pinned SSH reader after exact terminal+EOF.
/// It is not serializable, clonable or a substitute for Node's Resume Permit.
pub(in crate::agent_tunnel::association) struct OriginalRetirementObservation(RetiredTerminal);
impl OriginalRetirementObservation {
    pub(in crate::agent_tunnel::association) fn consume(self,binding:&NodeRequestBinding,
        policy:&TunnelRoutePolicyV2,node:&protocol::NodeIdentity)->AppResult<()> {
        self.0.validate(binding,policy,node)
    }
}
impl RetiredTerminal {
    fn validate(&self,binding:&NodeRequestBinding,policy:&TunnelRoutePolicyV2,node:&protocol::NodeIdentity)->AppResult<()> {
        if self.format!="cc-tunnel-supervised-result/v1" || self.outcome!="transportRetired"
            || !self.cleanup_observed || !self.drain_recorded || !policy.resume_original
            || self.lease_hash!=binding.lease.body.digest().map_err(|_|unknown())?
            || self.policy_hash!=policy.digest().map_err(|_|unknown())?
            || &self.node!=node || self.expires_at!=binding.lease.body.expires_at {
            return Err(unknown())
        }
        Ok(())
    }
}
#[derive(serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Terminal {
    format:String,outcome:String,cleanup_observed:bool,drain_recorded:bool,
}
impl Terminal {
    fn validate(&self)->AppResult<()> {
        if self.format!="cc-tunnel-supervised-result/v1"||self.outcome!="stopped"
            ||!self.cleanup_observed||!self.drain_recorded {return Err(unknown())}
        Ok(())
    }
}

impl Peer {
    pub(in crate::agent_tunnel::association) async fn observe_tunnel_retirement(
        &mut self,binding:&NodeRequestBinding,policy:&TunnelRoutePolicyV2,node:&protocol::NodeIdentity,
    )->AppResult<OriginalRetirementObservation> {
        self.scope.check_current()?;
        if !policy.resume_original {return Err(unavailable())}
        let end=self.scope.deadline().min(cap_read(&self.until)?)
            .min(Instant::now()+Duration::from_secs(20));
        let scope=self.scope.clone();
        let stream=self.stream.as_mut().and_then(|s|s.stream.as_mut()).ok_or_else(unknown)?;
        let observed=bounded(&scope,end,async {
            let mut raw=Zeroizing::new(Vec::new());
            loop {
                let byte=stream.read_u8().await.map_err(|_|unknown())?;raw.push(byte);
                if raw.len()>4096 || byte==b'\r' || !byte.is_ascii() {return Err(unknown())}
                if byte==b'\n' {break}
            }
            let terminal:RetiredTerminal=serde_json::from_slice(&raw).map_err(|_|unknown())?;
            terminal.validate(binding,policy,node)?;
            let mut tail=[0u8;1];
            if stream.read(&mut tail).await.map_err(|_|unknown())?!=0 {return Err(unknown())}
            Ok(OriginalRetirementObservation(terminal))
        }).await?;
        self.scope.check_current()?;
        Ok(observed)
    }
    pub(in crate::agent_tunnel::association) async fn start_tunnel_transport<T:serde::de::DeserializeOwned>(
        &mut self,core:&std::sync::Weak<crate::app::Inner>,expected_revision:u64,pin:&OwnerPin,
        binding:&NodeRequestBinding,policy:&TunnelRoutePolicyV2,
        client_der:&[u8],slot:Uuid,capability:&SecretString,expires:Timestamp,idle:u64,mode:OriginalTransportMode,
    )->AppResult<T> {
        let now=self.scope.now()?;
        if mode==OriginalTransportMode::Resume && !policy.resume_original {return Err(unavailable())}
        verify_binding_v2(pin,binding,policy,now).map_err(|_|unavailable())?;
        if expected_revision==0||client_der.is_empty()||client_der.len()>2048||slot.is_nil()
            ||expires>binding.lease.body.expires_at||expires<=now.0||idle==0
            ||idle>policy.route.limits.idle_timeout_seconds {return Err(unavailable())}
        let secret=capability.expose_secret();
        if secret.len()!=48||!secret.starts_with("ccrl_")
            ||!secret.as_bytes()[5..].iter().all(|b|b.is_ascii_alphanumeric()||*b==b'_'||*b==b'-') {
            return Err(unavailable())
        }
        const PLACEHOLDER:&str="CC_PRIVATE_NATIVE_RELAY_CAPABILITY";
        let kind=match mode {OriginalTransportMode::Admit=>"startTunnelTransport",
            OriginalTransportMode::Resume=>"resumeTunnelTransport"};
        let(id,template)=protocol::request(kind,serde_json::json!({
            "expectedRev":expected_revision,"expectedPin":pin,"binding":binding,"policy":policy,
            "clientCertificateHex":hex::encode(client_der),
            "relay":{"slotId":slot,"capability":PLACEHOLDER,"expiresAt":expires,
                "idleSeconds":idle,"maxCiphertextBytes":8388608}
        }))?;
        let marker=format!("\"{PLACEHOLDER}\"");
        let positions:Vec<_>=template.windows(marker.len()).enumerate().filter_map(|(n,b)|
            (b==marker.as_bytes()).then_some(n)).collect();
        if positions.len()!=1{return Err(unavailable())}
        let at=positions[0];let mut raw=Zeroizing::new(Vec::with_capacity(template.len()+secret.len()));
        raw.extend_from_slice(&template[..at]);raw.push(b'"');raw.extend_from_slice(secret.as_bytes());raw.push(b'"');
        raw.extend_from_slice(&template[at+marker.len()..]);
        if raw.len()>protocol::MAX_FRAME||!raw.is_ascii(){return Err(unavailable())}
        self.exchange_frame_weak(core,kind,id,raw,Some(binding.lease.body.expires_at)).await
    }
    /// Same exclusively owned SSH channel, after explicit original TLS Stop.
    /// This is a process/journal observation, NEVER a transferable Permit.
    pub(in crate::agent_tunnel::association) async fn observe_tunnel_terminal(
        &mut self,
    )->AppResult<()> {
        self.scope.check_current()?;
        let end=self.scope.deadline().min(cap_read(&self.until)?)
            .min(Instant::now()+Duration::from_secs(20));
        let scope=self.scope.clone();
        let stream=self.stream.as_mut().and_then(|s|s.stream.as_mut()).ok_or_else(unknown)?;
        bounded(&scope,end,async {
            let mut raw=Zeroizing::new(Vec::new());
            loop {
                let byte=stream.read_u8().await.map_err(|_|unknown())?;
                raw.push(byte);
                if raw.len()>4096||byte==b'\r'||!byte.is_ascii(){return Err(unknown())}
                if byte==b'\n'{break}
            }
            let terminal:Terminal=serde_json::from_slice(&raw).map_err(|_|unknown())?;
            terminal.validate()?;
            let mut tail=[0u8;1];
            if stream.read(&mut tail).await.map_err(|_|unknown())?!=0{return Err(unknown())}
            Ok(())
        }).await?;
        self.scope.check_current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_tunnel_terminal_is_closed_metadata_not_a_caller_join_flag() {
        let valid=serde_json::json!({"format":"cc-tunnel-supervised-result/v1","outcome":"stopped",
            "cleanupObserved":true,"drainRecorded":true});
        serde_json::from_value::<Terminal>(valid.clone()).unwrap().validate().unwrap();
        let mut extra=valid.clone();extra["joined"]=serde_json::json!(true);
        assert!(serde_json::from_value::<Terminal>(extra).is_err());
        for field in ["cleanupObserved","drainRecorded"] {
            let mut bad=valid.clone();bad[field]=serde_json::json!(false);
            assert!(serde_json::from_value::<Terminal>(bad).unwrap().validate().is_err());
        }
        let mut unknown=valid;unknown["outcome"]=serde_json::json!("unknown");
        assert!(serde_json::from_value::<Terminal>(unknown).unwrap().validate().is_err());
        // Positive SSH ownership/EOF and actual Agent drain remain runtime
        // requirements in observe_tunnel_terminal; these values confer nothing.
    }
}
