//! Fixed outbound SSH carrier control, never Node admission or a destination.
//! No refresh/retry, arbitrary HTTP path, account token export or portable Permit.
use super::*;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
/// Exact original API provenance with no signer/key custody while idle.
#[derive(Clone)]
pub struct OriginalRelayApi {client:std::sync::Weak<super::Inner>}
impl std::fmt::Debug for OriginalRelayApi {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str("OriginalRelayApi([weak])")}
}
impl OriginalRelayApi {
    pub fn upgrade_original(&self)->Option<ApiClient> {self.client.upgrade().map(|inner|ApiClient{inner})}
    pub fn matches_original(&self,api:&ApiClient)->bool {std::sync::Weak::ptr_eq(&self.client,&Arc::downgrade(&api.inner))}
}
impl ApiClient {
    pub fn original_relay_api(&self)->OriginalRelayApi {OriginalRelayApi{client:Arc::downgrade(&self.inner)}}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RelayControlReference {
    pub association_id:uuid::Uuid, pub control_id:uuid::Uuid,
    pub user_id:cc_protocol::UserId, pub device_id:cc_protocol::DeviceId,
    pub session_id:cc_protocol::SessionId, pub server_url:String,
    pub expires_at_unix_seconds:u64,
}
impl RelayControlReference {
    fn check(&self,api:&ApiClient,subject:&ReaderAuthenticatedSubject)->Result<u64,RelaySlotError> {
        subject.check_current(api).map_err(|_|RelaySlotError::Unavailable)?;
        let now=unix_seconds()?;
        let left=self.expires_at_unix_seconds.checked_sub(now).filter(|n|*n>0&&*n<=86400)
            .ok_or(RelaySlotError::Rejected)?;
        if self.association_id.is_nil() || self.control_id.is_nil()
            || self.user_id!=subject.user_id() || self.device_id!=subject.device_id()
            || self.session_id!=subject.session_id() || self.server_url!=subject.server_url()
            || api.config().server_url.as_str()!=self.server_url {
            return Err(RelaySlotError::Rejected)
        }
        Ok(left)
    }
}
fn unix_seconds()->Result<u64,RelaySlotError> {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d|d.as_secs()).map_err(|_|RelaySlotError::Unavailable)
}
fn control_token(raw:&str)->bool {
    let Some(token)=raw.strip_prefix("ccrc_") else{return false};
    token.len()==43 && token.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
        && token.as_bytes().last().is_some_and(|b|b"AEIMQUYcgkosw048".contains(b))
}
/// One nonserializable credential, delivered only by explicit pinned-SSH bootstrap.
pub struct RelayControlBootstrap(Zeroizing<String>);
impl std::fmt::Debug for RelayControlBootstrap {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str("RelayControlBootstrap([redacted])")}
}
impl RelayControlBootstrap {
    pub fn expose_for_bootstrap(&self)->&str {self.0.as_str()}
}
pub struct RelayControlRegistration {
    reference:RelayControlReference,
    bootstrap:Option<RelayControlBootstrap>,
    deadline:tokio::time::Instant,
}
/// Captured BEFORE registration dispatch, with the caller's one fresh UUID.
/// The owner retains this even on a lost response, so cleanup names the same
/// registration under the SAME original authenticated subject. Not serializable.
pub struct RelayControlRegistrationAttempt {
    reference:RelayControlReference,subject:ReaderAuthenticatedSubject,
    deadline:tokio::time::Instant,dispatched:bool,retirement_attempted:bool,
}
impl std::fmt::Debug for RelayControlRegistrationAttempt {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str("RelayControlRegistrationAttempt([redacted])")}
}
impl RelayControlRegistrationAttempt {
    pub fn reference(&self)->&RelayControlReference {&self.reference}
}
impl std::fmt::Debug for RelayControlRegistration {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str("RelayControlRegistration([redacted])")}
}
impl RelayControlRegistration {
    pub fn reference(&self)->&RelayControlReference {&self.reference}
    pub fn take_bootstrap_once(&mut self)->Result<RelayControlBootstrap,RelaySlotError> {
        if tokio::time::Instant::now()>=self.deadline {return Err(RelaySlotError::Rejected)}
        self.bootstrap.take().ok_or(RelaySlotError::Rejected)
    }
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Registered<'a> {control_id:String,#[serde(borrow)] agent_token:&'a str,expires_in_seconds:u64}

/// Issued only after exact204 from the one original authenticated dispatch.
pub struct RelayControlDispatch {
    reference:RelayControlReference,slot:uuid::Uuid,
    subject:ReaderAuthenticatedSubject,
}
impl std::fmt::Debug for RelayControlDispatch {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str("RelayControlDispatch([redacted])")}
}
/// Bytes only. Native must independently bind its original complete association,
/// saved Host revision/SSH keys/credentials, current cert/pin and owner scope.
pub struct RelayControlStream {
    stream:RelayBinaryStream,reference:RelayControlReference,deadline:tokio::time::Instant,
}
impl std::fmt::Debug for RelayControlStream {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str("RelayControlStream([redacted])")}
}
impl RelayControlStream {
    pub fn reference(&self)->&RelayControlReference {&self.reference}
    pub fn into_original_stream(self)->(RelayBinaryStream,RelayControlReference,tokio::time::Instant) {
        (self.stream,self.reference,self.deadline)
    }
}
impl RelayControlDispatch {
    pub async fn connect_client_once(self,api:&ApiClient,slot:&RelaySlot)->Result<RelayControlStream,RelaySlotError> {
        let left=self.reference.check(api,&self.subject)?;
        if slot.id()!=self.slot {return Err(RelaySlotError::Rejected)}
        slot.check_control_owner(api,&self.subject,left)?;
        let stream=slot.connect_client_once().await?;
        self.reference.check(api,&self.subject).map_err(|_|RelaySlotError::OutcomeUnknown)?;
        stream.check_original(api).map_err(|_|RelaySlotError::OutcomeUnknown)?;
        let deadline=slot.control_deadline().map_err(|_|RelaySlotError::OutcomeUnknown)?.min(tokio::time::Instant::now()
            +Duration::from_secs(self.reference.check(api,&self.subject).map_err(|_|RelaySlotError::OutcomeUnknown)?));
        if tokio::time::Instant::now()>=deadline {return Err(RelaySlotError::OutcomeUnknown)}
        Ok(RelayControlStream{stream,reference:self.reference,deadline})
    }
}
impl ApiClient {
    pub fn prepare_relay_control_registration(&self,subject:ReaderAuthenticatedSubject,
        association:uuid::Uuid,control_id:uuid::Uuid,ttl_seconds:u64)->Result<RelayControlRegistrationAttempt,RelaySlotError> {
        if association.is_nil() || control_id.is_nil() || !(30..=86400).contains(&ttl_seconds) {
            return Err(RelaySlotError::Rejected)
        }
        subject.check_current(self).map_err(|_|RelaySlotError::Unavailable)?;
        let started=tokio::time::Instant::now();let wall=unix_seconds()?;
        let reference=RelayControlReference{association_id:association,control_id,
            user_id:subject.user_id(),device_id:subject.device_id(),session_id:subject.session_id(),
            server_url:subject.server_url().to_owned(),expires_at_unix_seconds:wall.checked_add(ttl_seconds)
                .ok_or(RelaySlotError::Rejected)?};
        Ok(RelayControlRegistrationAttempt{reference,subject,deadline:started+Duration::from_secs(ttl_seconds),
            dispatched:false,retirement_attempted:false})
    }
    pub async fn create_relay_control_once(&self,attempt:&mut RelayControlRegistrationAttempt)
        ->Result<RelayControlRegistration,RelaySlotError> {
        if attempt.dispatched || attempt.retirement_attempted {return Err(RelaySlotError::Rejected)}
        attempt.subject.check_current(self).map_err(|_|RelaySlotError::Unavailable)?;
        let started=tokio::time::Instant::now();let wall=unix_seconds()?;
        let ttl_seconds=attempt.deadline.checked_duration_since(started).map(|d|d.as_secs())
            .filter(|n|(30..=86400).contains(n)).ok_or(RelaySlotError::Rejected)?;
        let body=serde_json::to_vec(&serde_json::json!({"associationId":attempt.reference.association_id,
            "controlId":attempt.reference.control_id,"ttlSeconds":ttl_seconds}))
            .map_err(|_|RelaySlotError::Rejected)?;
        attempt.dispatched=true; // Never retried, even if proof/transport becomes uncertain.
        let(status,raw)=self.relay_exchange_status_once(&attempt.subject,Method::POST,"/v1/relay/control",body,
            started+Duration::from_secs(5)).await?;
        if status!=StatusCode::OK {return Err(RelaySlotError::OutcomeUnknown)}
        let created:Registered=serde_json::from_slice(&raw).map_err(|_|RelaySlotError::OutcomeUnknown)?;
        let id=uuid::Uuid::parse_str(&created.control_id).map_err(|_|RelaySlotError::OutcomeUnknown)?;
        if id.is_nil() || id.to_string()!=created.control_id || id!=attempt.reference.control_id || !control_token(created.agent_token)
            || created.expires_in_seconds==0 || created.expires_in_seconds>ttl_seconds {
            return Err(RelaySlotError::OutcomeUnknown)
        }
        attempt.subject.check_current(self).map_err(|_|RelaySlotError::OutcomeUnknown)?;
        let deadline=(started+Duration::from_secs(created.expires_in_seconds)).min(attempt.deadline);
        if tokio::time::Instant::now()>=deadline {return Err(RelaySlotError::OutcomeUnknown)}
        let mut reference=attempt.reference.clone();
        reference.expires_at_unix_seconds=reference.expires_at_unix_seconds.min(wall.checked_add(created.expires_in_seconds)
            .ok_or(RelaySlotError::OutcomeUnknown)?);
        reference.check(self,&attempt.subject).map_err(|_|RelaySlotError::OutcomeUnknown)?;
        Ok(RelayControlRegistration{reference,bootstrap:Some(RelayControlBootstrap(Zeroizing::new(created.agent_token.to_owned()))),deadline})
    }
    /// Exact known-ID cleanup after success OR a lost registration response.
    /// No bootstrap token is recovered; expiry is not a prerequisite to retire.
    pub async fn retire_relay_control_attempt_once(&self,attempt:&mut RelayControlRegistrationAttempt)
        ->Result<(),RelaySlotError> {
        if !attempt.dispatched || attempt.retirement_attempted {return Err(RelaySlotError::Rejected)}
        attempt.subject.check_current(self).map_err(|_|RelaySlotError::Unavailable)?;
        attempt.retirement_attempted=true;
        let path=format!("/v1/relay/control/{}",attempt.reference.control_id);
        let(status,raw)=self.relay_exchange_status_once(&attempt.subject,Method::DELETE,&path,Vec::new(),
            tokio::time::Instant::now()+Duration::from_secs(5)).await?;
        if status!=StatusCode::NO_CONTENT || !raw.is_empty() {return Err(RelaySlotError::OutcomeUnknown)}
        attempt.subject.check_current(self).map_err(|_|RelaySlotError::OutcomeUnknown)
    }
    /// Caller retains ownership of the slot before this possibly committed write.
    /// Capability is consumed once. Error/uncertainty requires original-slot
    /// retirement by that owner, never another dispatch or token recovery.
    pub async fn dispatch_relay_control_once(&self,subject:ReaderAuthenticatedSubject,
        reference:&RelayControlReference,slot:&mut RelaySlot)->Result<RelayControlDispatch,RelaySlotError> {
        let remaining=reference.check(self,&subject)?;
        slot.check_control_owner(self,&subject,remaining)?;
        let token=slot.take_agent_capability()?.provision_once();
        #[derive(Serialize)]
        #[serde(rename_all="camelCase")]
        struct Dispatch<'a>{slot_id:uuid::Uuid,agent_token:&'a str}
        let body=serde_json::to_vec(&Dispatch{slot_id:slot.id(),agent_token:token.expose_secret()})
            .map_err(|_|RelaySlotError::Rejected)?;
        let path=format!("/v1/relay/control/{}/requests",reference.control_id);
        let(status,raw)=self.relay_exchange_status_once(&subject,Method::POST,&path,body,
            tokio::time::Instant::now()+Duration::from_secs(5)).await?;
        if status!=StatusCode::NO_CONTENT || !raw.is_empty() {return Err(RelaySlotError::OutcomeUnknown)}
        reference.check(self,&subject).map_err(|_|RelaySlotError::OutcomeUnknown)?;
        Ok(RelayControlDispatch{reference:reference.clone(),slot:slot.id(),subject})
    }
    pub async fn revoke_relay_control_once(&self,subject:ReaderAuthenticatedSubject,
        reference:&RelayControlReference)->Result<(),RelaySlotError> {
        reference.check(self,&subject)?;
        let path=format!("/v1/relay/control/{}",reference.control_id);
        let(status,raw)=self.relay_exchange_status_once(&subject,Method::DELETE,&path,Vec::new(),
            tokio::time::Instant::now()+Duration::from_secs(5)).await?;
        if status!=StatusCode::NO_CONTENT || !raw.is_empty() {return Err(RelaySlotError::OutcomeUnknown)}
        subject.check_current(self).map_err(|_|RelaySlotError::OutcomeUnknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_token_is_exact_separate_canonical_role() {
        assert!(control_token(&format!("ccrc_{}A","A".repeat(42))));
        for value in [format!("ccrl_{}A","A".repeat(42)),format!("ccrc_{}B","A".repeat(42)),
            format!("ccrc_{}A?x","A".repeat(42)),"ccrc_".into()] {assert!(!control_token(&value));}
    }
    #[test]
    fn registration_reply_rejects_duplicate_and_unknown_fields() {
        let id=uuid::Uuid::new_v4();let token=format!("ccrc_{}A","A".repeat(42));
        let valid=format!(r#"{{"controlId":"{id}","agentToken":"{token}","expiresInSeconds":120}}"#);
        assert!(serde_json::from_str::<Registered>(&valid).is_ok());
        for extra in [r#", "path":"/tmp/control""#,r#", "expiresInSeconds":300"#] {
            assert!(serde_json::from_str::<Registered>(&format!("{}{}{}",&valid[..valid.len()-1],extra,"}")).is_err());
        }
    }

    #[test]
    fn token_is_borrowed_only_from_held_sensitive_raw_on_success_and_rejection() {
        use zeroize::Zeroize;
        let id=uuid::Uuid::new_v4();let token=format!("ccrc_{}A","A".repeat(42));
        let mut raw=Zeroizing::new(format!(r#"{{"controlId":"{id}","agentToken":"{token}","expiresInSeconds":60}}"#).into_bytes());
        let parsed:Registered=serde_json::from_slice(&raw).unwrap();
        let start=raw.as_ptr() as usize;let borrowed=parsed.agent_token.as_ptr() as usize;
        assert!(borrowed>=start&&borrowed+parsed.agent_token.len()<=start+raw.len());drop(parsed);
        raw.pop();raw.extend_from_slice(br#", "unknown":true}"#);
        assert!(serde_json::from_slice::<Registered>(&raw).is_err());
        raw.as_mut_slice().zeroize();assert!(raw.iter().all(|b|*b==0));
    }
}
