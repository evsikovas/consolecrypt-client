//! Closed Node-owner purposes. A prepared value is not evidence of UI approval.
use crate::*;

pub struct PreparedTunnelOwner {
    key: PublicKey,
    start: Timestamp,
    end: Timestamp,
    device: Option<String>,
    statement: OwnerStatement,
}
enum OwnerStatement {
    Offer(Offer),
    Certificate(Certificate),
    Revocation(Revocation),
    Grant { binding: NodeRequestBinding },
}
pub enum TunnelOwnerSigned {
    Offer(Signed<Offer>),
    Certificate(Signed<Certificate>),
    Revocation(Signed<Revocation>),
    Grant(NodeRequestBinding),
}
impl std::fmt::Debug for PreparedTunnelOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedTunnelOwner([redacted])")
    }
}
impl std::fmt::Debug for TunnelOwnerSigned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelOwnerSigned([redacted])")
    }
}
fn header(pin: &OwnerPin, h: &Header, next: bool) -> Result<()> {
    let checkpoint = if next {
        pin.checkpoint.checked_add(1).ok_or(Error::Quota)?
    } else {
        pin.checkpoint
    };
    if h.version != 1
        || h.context != pin.context
        || h.owner_epoch != pin.epoch
        || h.checkpoint != checkpoint
    {
        return Err(Error::WrongCheckpoint);
    }
    Ok(())
}
fn scope(scopes: &Scopes) -> Result<()> {
    scopes.validate()?;
    if scopes.0.as_slice() != [Scope::TunnelOpen] {
        return Err(Error::CapabilityDenied);
    }
    Ok(())
}
impl PreparedTunnelOwner {
    pub fn offer(pin: &OwnerPin, offer: Offer, now: Now) -> Result<Self> {
        validate_pin(pin, now)?;
        offer.validate()?;
        header(pin, &offer.header, true)?;
        scope(&offer.scopes)?;
        if offer.agent_key == pin.key {
            return Err(Error::KeyReuse);
        }
        check_recent(offer.issued_at, now)?;
        check_live(offer.issued_at, offer.expires_at, now)?;
        if offer.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        Ok(Self {
            key: pin.key,
            start: offer.issued_at,
            end: offer.expires_at.min(pin.expires_at),
            device: None,
            statement: OwnerStatement::Offer(offer),
        })
    }
    /// Caller supplies the current independently provisioned pin AFTER offer apply.
    /// Node PoP and exact enrollment origin are checked before owner signing.
    pub fn certificate(
        pin: &OwnerPin,
        offer: Signed<Offer>,
        claim: Signed<Claim>,
        cert: Certificate,
        now: Now,
    ) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(&offer, pin.key)?;
        verify_statement(&claim, offer.body.agent_key)?;
        cert.validate()?;
        header(pin, &cert.header, true)?;
        scope(&cert.scopes)?;
        scope(&cert.capabilities.scopes)?;
        scope(&offer.body.scopes)?;
        scope(&claim.body.capabilities.scopes)?;
        let o = &offer.body;
        let c = &claim.body;
        if o.header.context != pin.context
            || o.header.owner_epoch != pin.epoch
            || o.header.checkpoint > pin.checkpoint
            || c.header != o.header
            || c.invitation_id != o.invitation_id
            || c.offer_hash != o.digest()?
            || c.agent_id != o.agent_id
            || c.agent_key != o.agent_key
            || cert.agent_id != o.agent_id
            || cert.agent_key != o.agent_key
            || cert.key_epoch != 1
            || cert.capabilities != c.capabilities
            || cert.origin
                != (CertificateOrigin::Enrollment {
                    invitation_id: o.invitation_id,
                    claim_hash: c.digest()?,
                    key_fingerprint: c.agent_key.fingerprint(),
                })
        {
            return Err(Error::WrongClaim);
        }
        check_live(o.issued_at, o.expires_at, now)?;
        check_recent(c.issued_at, now)?;
        check_live(cert.not_before, cert.expires_at, now)?;
        if cert.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        Ok(Self {
            key: pin.key,
            start: cert.not_before,
            end: cert.expires_at.min(pin.expires_at),
            device: None,
            statement: OwnerStatement::Certificate(cert),
        })
    }
    /// This creates no Agent admission or lease reset. A fresh LeaseId must be an
    /// explicit owner action. Agent keeps its existing durable one-lease replay.
    pub fn grant(
        pin: &OwnerPin,
        certificate: Signed<Certificate>,
        policy: &TunnelRoutePolicy,
        lease: Lease,
        freshness: Freshness,
        device_id: &str,
        now: Now,
    ) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(&certificate, pin.key)?;
        lease.validate()?;
        freshness.validate()?;
        header(pin, &lease.header, false)?;
        header(pin, &freshness.header, false)?;
        policy.validate()?;
        if policy.client_device_id != device_id {
            return Err(Error::WrongClaim);
        }
        let cert = &certificate.body;
        if cert.header.context != pin.context
            || cert.header.owner_epoch != pin.epoch
            || cert.header.checkpoint > pin.checkpoint
        {
            return Err(Error::WrongContext);
        }
        check_live(cert.not_before, cert.expires_at, now)?;
        check_live(lease.issued_at, lease.expires_at, now)?;
        check_recent(lease.issued_at, now)?;
        check_live(freshness.issued_at, freshness.expires_at, now)?;
        check_recent(freshness.issued_at, now)?;
        if lease.expires_at > cert.expires_at
            || lease.expires_at > pin.expires_at
            || lease.expires_at > freshness.expires_at
            || freshness.expires_at > pin.expires_at
        {
            return Err(Error::Expired);
        }
        let blank = SignatureBytes::from_bytes([0; 64]);
        let binding = NodeRequestBinding {
            certificate,
            lease: Signed {
                body: lease,
                signature: blank,
            },
            freshness: Signed {
                body: freshness,
                signature: blank,
            },
            operation: policy.operation()?,
        };
        policy.check_binding(&binding)?;
        Ok(Self {
            key: pin.key,
            start: binding
                .lease
                .body
                .issued_at
                .max(binding.freshness.body.issued_at)
                .max(binding.certificate.body.not_before),
            end: binding
                .lease
                .body
                .expires_at
                .min(binding.freshness.body.expires_at)
                .min(binding.certificate.body.expires_at)
                .min(pin.expires_at),
            device: Some(device_id.to_owned()),
            statement: OwnerStatement::Grant { binding },
        })
    }
    /// Explicit ORIGINAL V2 preparation; never hashes only its legacy route.
    pub fn grant_original(
        pin: &OwnerPin,
        certificate: Signed<Certificate>,
        policy: &TunnelRoutePolicyV2,
        lease: Lease,
        freshness: Freshness,
        device_id: &str,
        now: Now,
    ) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(&certificate, pin.key)?;
        lease.validate()?;
        freshness.validate()?;
        header(pin, &lease.header, false)?;
        header(pin, &freshness.header, false)?;
        policy.validate()?;
        if policy.route.client_device_id != device_id {
            return Err(Error::WrongClaim);
        }
        let cert = &certificate.body;
        if cert.header.context != pin.context
            || cert.header.owner_epoch != pin.epoch
            || cert.header.checkpoint > pin.checkpoint
        {
            return Err(Error::WrongContext);
        }
        check_live(cert.not_before, cert.expires_at, now)?;
        check_live(lease.issued_at, lease.expires_at, now)?;
        check_recent(lease.issued_at, now)?;
        check_live(freshness.issued_at, freshness.expires_at, now)?;
        check_recent(freshness.issued_at, now)?;
        if lease.expires_at > cert.expires_at
            || lease.expires_at > pin.expires_at
            || lease.expires_at > freshness.expires_at
            || freshness.expires_at > pin.expires_at
        {
            return Err(Error::Expired);
        }
        let blank = SignatureBytes::from_bytes([0; 64]);
        let binding = NodeRequestBinding {
            certificate,
            lease: Signed {
                body: lease,
                signature: blank,
            },
            freshness: Signed {
                body: freshness,
                signature: blank,
            },
            operation: policy.operation()?,
        };
        policy.check_binding(&binding)?;
        Ok(Self {
            key: pin.key,
            start: binding
                .lease
                .body
                .issued_at
                .max(binding.freshness.body.issued_at)
                .max(binding.certificate.body.not_before),
            end: binding
                .lease
                .body
                .expires_at
                .min(binding.freshness.body.expires_at)
                .min(binding.certificate.body.expires_at)
                .min(pin.expires_at),
            device: Some(device_id.to_owned()),
            statement: OwnerStatement::Grant { binding },
        })
    }
    /// CLOSED TunnelOnly certificate replacement, Node epoch1->2 only. Uses
    /// the existing Certificate purpose/domain; dual Node proofs remain required
    /// by the Actor before commit. No owner rotation/capability or TTL increase.
    pub fn rotation_certificate(pin:&OwnerPin,previous:Signed<Certificate>,certificate:Certificate,now:Now)->Result<Self> {
        validate_pin(pin,now)?;verify_statement(&previous,pin.key)?;certificate.validate()?;
        let old=&previous.body;scope(&old.scopes)?;scope(&old.capabilities.scopes)?;
        scope(&certificate.scopes)?;scope(&certificate.capabilities.scopes)?;
        header(pin,&certificate.header,true)?;
        if old.header.context!=pin.context || old.header.owner_epoch!=pin.epoch || old.header.checkpoint>pin.checkpoint {
            return Err(Error::WrongContext)
        }
        if old.key_epoch!=1 || certificate.key_epoch!=2 || certificate.agent_id!=old.agent_id
            || certificate.agent_key==old.agent_key || certificate.agent_key==pin.key {
            return Err(Error::WrongEpoch)
        }
        if certificate.origin!=(CertificateOrigin::Rotation {previous_certificate:old.digest()?,previous_key_epoch:1})
            || certificate.scopes!=old.scopes || certificate.capabilities!=old.capabilities {
            return Err(Error::CapabilityDenied)
        }
        check_live(old.not_before,old.expires_at,now)?;check_live(certificate.not_before,certificate.expires_at,now)?;
        check_recent(certificate.not_before,now)?;
        if certificate.not_before<old.not_before || certificate.expires_at>old.expires_at
            || certificate.expires_at>pin.expires_at {return Err(Error::Expired)}
        let end=certificate.not_before.checked_add(PROOF_TTL)?.min(certificate.expires_at).min(pin.expires_at);
        Ok(Self {key:pin.key,start:certificate.not_before,end,device:None,
            statement:OwnerStatement::Certificate(certificate)})
    }
    /// Revoke the exact CURRENT same-owner TunnelOnly certificate. Expiry of
    /// that certificate is intentionally irrelevant: revocation grants no data.
    /// The real Actor still decides registered-current certificate and CAS.
    pub fn revoke(pin: &OwnerPin, certificate: Signed<Certificate>, revoke: Revocation, now: Now) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(&certificate, pin.key)?;
        certificate.body.validate()?;
        revoke.validate()?;
        header(pin, &revoke.header, true)?;
        scope(&certificate.body.scopes)?;
        scope(&certificate.body.capabilities.scopes)?;
        let cert = &certificate.body;
        if cert.header.context != pin.context || cert.header.owner_epoch != pin.epoch
            || cert.header.checkpoint > pin.checkpoint {
            return Err(Error::WrongContext);
        }
        if revoke.agent_id != cert.agent_id || revoke.key_epoch != cert.key_epoch
            || revoke.certificate_hash != cert.digest()? {
            return Err(Error::WrongClaim);
        }
        check_recent(revoke.issued_at, now)?;
        let end = revoke.issued_at.checked_add(PROOF_TTL)?.min(pin.expires_at);
        check_live(revoke.issued_at, end, now)?;
        Ok(Self { key: pin.key, start: revoke.issued_at, end, device: None,
            statement: OwnerStatement::Revocation(revoke) })
    }
    pub fn recheck(&self, now: Now) -> Result<()> {
        check_live(self.start, self.end, now)
    }
    pub fn expected_device_id(&self) -> Option<&str> {
        self.device.as_deref()
    }
    pub fn expected_key(&self) -> PublicKey {
        self.key
    }
    /// Public domain-separated transcripts only, never secret-key material.
    pub fn signing_transcripts(&self) -> Result<Vec<Vec<u8>>> {
        match &self.statement {
            OwnerStatement::Offer(v) => Ok(vec![v.signing_bytes()?]),
            OwnerStatement::Certificate(v) => Ok(vec![v.signing_bytes()?]),
            OwnerStatement::Revocation(v) => Ok(vec![v.signing_bytes()?]),
            OwnerStatement::Grant { binding } => Ok(vec![
                binding.lease.body.signing_bytes()?,
                binding.freshness.body.signing_bytes()?,
            ]),
        }
    }
    pub fn finish(self, signatures: Vec<[u8; 64]>) -> Result<TunnelOwnerSigned> {
        let mut sig = signatures.into_iter();
        let first = SignatureBytes::from_bytes(sig.next().ok_or(Error::InvalidSignature)?);
        let result = match self.statement {
            OwnerStatement::Offer(body) => {
                let v = Signed {
                    body,
                    signature: first,
                };
                verify_statement(&v, self.key)?;
                TunnelOwnerSigned::Offer(v)
            }
            OwnerStatement::Certificate(body) => {
                let v = Signed {
                    body,
                    signature: first,
                };
                verify_statement(&v, self.key)?;
                TunnelOwnerSigned::Certificate(v)
            }
            OwnerStatement::Revocation(body) => {
                let v = Signed { body, signature: first };
                verify_statement(&v, self.key)?;
                TunnelOwnerSigned::Revocation(v)
            }
            OwnerStatement::Grant { mut binding } => {
                binding.lease.signature = first;
                binding.freshness.signature =
                    SignatureBytes::from_bytes(sig.next().ok_or(Error::InvalidSignature)?);
                verify_statement(&binding.lease, self.key)?;
                verify_statement(&binding.freshness, self.key)?;
                TunnelOwnerSigned::Grant(binding)
            }
        };
        if sig.next().is_some() {
            return Err(Error::InvalidSignature);
        }
        Ok(result)
    }
}


// Closed MetricsOnly owner counterparts; existing Tunnel/Telemetry constructors
// and statement signing domains are unchanged. No generic scope selector.
fn metrics_scope(scopes: &Scopes) -> Result<()> {
    scopes.validate()?;
    if scopes.0.as_slice() != [Scope::MetricsCollect] { return Err(Error::CapabilityDenied); }
    Ok(())
}
impl PreparedTunnelOwner {
    pub fn metrics_offer(pin: &OwnerPin, offer: Offer, now: Now) -> Result<Self> {
        validate_pin(pin, now)?;
        offer.validate()?;
        header(pin, &offer.header, true)?;
        metrics_scope(&offer.scopes)?;
        if offer.agent_key == pin.key {
            return Err(Error::KeyReuse);
        }
        check_recent(offer.issued_at, now)?;
        check_live(offer.issued_at, offer.expires_at, now)?;
        if offer.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        Ok(Self {
            key: pin.key,
            start: offer.issued_at,
            end: offer.expires_at.min(pin.expires_at).min(offer.issued_at.checked_add(PROOF_TTL)?),
            device: None,
            statement: OwnerStatement::Offer(offer),
        })
    }
    pub fn metrics_certificate(
        pin: &OwnerPin,
        offer: Signed<Offer>,
        claim: Signed<Claim>,
        cert: Certificate,
        now: Now,
    ) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(&offer, pin.key)?;
        verify_statement(&claim, offer.body.agent_key)?;
        cert.validate()?;
        header(pin, &cert.header, true)?;
        metrics_scope(&cert.scopes)?;
        metrics_scope(&cert.capabilities.scopes)?;
        metrics_scope(&offer.body.scopes)?;
        metrics_scope(&claim.body.capabilities.scopes)?;
        let o = &offer.body;
        let c = &claim.body;
        if o.header.context != pin.context
            || o.header.owner_epoch != pin.epoch
            || o.header.checkpoint > pin.checkpoint
            || c.header != o.header
            || c.invitation_id != o.invitation_id
            || c.offer_hash != o.digest()?
            || c.agent_id != o.agent_id
            || c.agent_key != o.agent_key
            || cert.agent_id != o.agent_id
            || cert.agent_key != o.agent_key
            || cert.key_epoch != 1
            || cert.capabilities != c.capabilities
            || cert.origin
                != (CertificateOrigin::Enrollment {
                    invitation_id: o.invitation_id,
                    claim_hash: c.digest()?,
                    key_fingerprint: c.agent_key.fingerprint(),
                })
        {
            return Err(Error::WrongClaim);
        }
        check_live(o.issued_at, o.expires_at, now)?;
        check_recent(c.issued_at, now)?;
        check_live(cert.not_before, cert.expires_at, now)?;
        if cert.expires_at > pin.expires_at {
            return Err(Error::Expired);
        }
        Ok(Self {
            key: pin.key,
            start: now.0,
            end: cert.expires_at.min(pin.expires_at).min(now.0.checked_add(PROOF_TTL)?).min(c.issued_at.checked_add(PROOF_TTL)?).min(o.expires_at),
            device: None,
            statement: OwnerStatement::Certificate(cert),
        })
    }
    pub fn metrics_revoke(pin: &OwnerPin, certificate: Signed<Certificate>, revoke: Revocation, now: Now) -> Result<Self> {
        validate_pin(pin, now)?;
        verify_statement(&certificate, pin.key)?;
        certificate.body.validate()?;
        revoke.validate()?;
        header(pin, &revoke.header, true)?;
        metrics_scope(&certificate.body.scopes)?;
        metrics_scope(&certificate.body.capabilities.scopes)?;
        let cert = &certificate.body;
        if cert.header.context != pin.context || cert.header.owner_epoch != pin.epoch
            || cert.header.checkpoint > pin.checkpoint {
            return Err(Error::WrongContext);
        }
        if revoke.agent_id != cert.agent_id || revoke.key_epoch != cert.key_epoch
            || revoke.certificate_hash != cert.digest()? {
            return Err(Error::WrongClaim);
        }
        check_recent(revoke.issued_at, now)?;
        let end = revoke.issued_at.checked_add(PROOF_TTL)?.min(pin.expires_at);
        check_live(revoke.issued_at, end, now)?;
        Ok(Self { key: pin.key, start: revoke.issued_at, end, device: None,
            statement: OwnerStatement::Revocation(revoke) })
    }
    /// Existing operation-lease/freshness purposes, exact MetricsCollect only.
    /// The immutable collector resource is resolved by the owned Agent adapter.
    pub fn metrics_grant(pin:&OwnerPin,certificate:Signed<Certificate>,collector:ResourceId,
        lease:Lease,freshness:Freshness,now:Now)->Result<Self> {
        validate_pin(pin,now)?;verify_statement(&certificate,pin.key)?;
        let cert=&certificate.body;cert.validate()?;
        metrics_scope(&cert.scopes)?;metrics_scope(&cert.capabilities.scopes)?;
        if cert.header.context!=pin.context || cert.header.owner_epoch!=pin.epoch
            || cert.header.checkpoint>pin.checkpoint {return Err(Error::WrongContext)}
        lease.validate()?;freshness.validate()?;
        header(pin,&lease.header,false)?;header(pin,&freshness.header,false)?;
        check_live(cert.not_before,cert.expires_at,now)?;
        check_live(lease.issued_at,lease.expires_at,now)?;check_recent(lease.issued_at,now)?;
        check_live(freshness.issued_at,freshness.expires_at,now)?;check_recent(freshness.issued_at,now)?;
        let operation=Operation::MetricsCollect {collector};
        if lease.operation!=operation || lease.agent_id!=cert.agent_id || lease.key_epoch!=cert.key_epoch
            || lease.certificate_hash!=cert.digest()? || lease.header!=freshness.header {
            return Err(Error::WrongClaim)
        }
        if lease.expires_at>cert.expires_at || lease.expires_at>pin.expires_at
            || lease.expires_at>freshness.expires_at || freshness.expires_at>pin.expires_at {
            return Err(Error::Expired)
        }
        let start=now.0.max(lease.issued_at).max(freshness.issued_at).max(cert.not_before);
        let end=lease.expires_at.min(freshness.expires_at).min(cert.expires_at)
            .min(pin.expires_at).min(now.0.checked_add(PROOF_TTL)?)
            .min(lease.issued_at.checked_add(PROOF_TTL)?).min(freshness.issued_at.checked_add(PROOF_TTL)?);
        check_live(start,end,now)?;
        let blank=SignatureBytes::from_bytes([0;64]);
        let binding=NodeRequestBinding {certificate,
            lease:Signed {body:lease,signature:blank},freshness:Signed {body:freshness,signature:blank},operation};
        Ok(Self {key:pin.key,start,end,device:None,statement:OwnerStatement::Grant {binding}})
    }
}

use sha2::{Digest as _,Sha256};
use serde_json::json;
use std::collections::BTreeSet;
fn metrics_json_body(value:&serde_json::Value)->Result<Vec<u8>> {let mut b=Vec::new();crate::canonical_json(value,&mut b)?;Ok(b)}
fn metrics_manifest_keys(value:&serde_json::Value,keys:&[&str])->Result<()> {
    let o=value.as_object().ok_or(Error::InvalidWire)?;
    if o.len()!=keys.len() || keys.iter().any(|k|!o.contains_key(*k)){return Err(Error::InvalidWire)}Ok(())
}
fn metrics_manifest_integer(v:&serde_json::Value,min:u64,max:u64)->Result<u64> {
    v.as_u64().filter(|n|min<=*n&&*n<=max).ok_or(Error::InvalidWire)
}
fn metrics_manifest_name(v:&serde_json::Value)->Result<&str> {
    let s=v.as_str().ok_or(Error::InvalidWire)?;let b=s.as_bytes();
    if b.is_empty()||b.len()>32||!b[0].is_ascii_alphabetic()
        ||!b.iter().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(b)){return Err(Error::InvalidWire)}Ok(s)
}
fn metrics_manifest_endpoint(value:&serde_json::Value,scrape:bool)->Result<(String,String,String)> {
    let raw=value.as_str().ok_or(Error::InvalidWire)?;
    if raw.len()>512 || !raw.is_ascii(){return Err(Error::ScopeDenied)}
    let(scheme,tail)=if let Some(v)=raw.strip_prefix("http://"){("http",v)}
        else if let Some(v)=raw.strip_prefix("https://"){("https",v)}else{return Err(Error::ScopeDenied)};
    let(auth,path)=tail.split_once('/').ok_or(Error::ScopeDenied)?;
    let addr:std::net::SocketAddr=auth.parse().map_err(|_|Error::ScopeDenied)?;
    if !addr.ip().is_loopback()||addr.port()==0||addr.to_string()!=auth
        ||(scrape&&path!="metrics")||(!scrape&&!matches!(path,"api/v1/write"|"api/v1/push")) {
        return Err(Error::ScopeDenied)
    }
    if !scrape && (scheme!="http"||!auth.starts_with("127.0.0.1:")){return Err(Error::ScopeDenied)}
    Ok((scheme.to_owned(),auth.to_owned(),format!("/{path}")))
}
/// Public runtime metadata only. No Actor/Permit/key or grant is serialized.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct MetricsRuntime {
    pub kind:String,pub upstream_tag:String,pub platform:String,
    pub binary_sha256:String,pub binary_bytes:u64,
}
pub fn metrics_manifest_runtime(v:&serde_json::Value)->Result<Option<MetricsRuntime>> {
    if metrics_json_body(v)?.len()>8192{return Err(Error::Quota)}
    if v["schema_version"]==json!(1) {
        if v.get("runtime").is_some(){return Err(Error::InvalidWire)}
        return Ok(None)
    }
    if v["schema_version"]!=json!(2){return Err(Error::CapabilityDenied)}
    let disk=v.get("profile").is_some()||v.get("bufferMode").is_some();
    if disk {
        metrics_manifest_keys(v,&["schema_version","mode","targets","labels","scrape_interval_seconds","receiver","buffer","runtime","profile","bufferMode"])?;
        if v["profile"]!="maintained-prometheus-disk-512mib-2h-v1"||v["bufferMode"]!="persistentNativeWal"
            ||v["buffer"]!=json!({"max_bytes":536870912u64,"max_age_seconds":7200}){return Err(Error::CapabilityDenied)}
    }else{
        metrics_manifest_keys(v,&["schema_version","mode","targets","labels","scrape_interval_seconds","receiver","buffer","runtime"])?;
    }
    let r:MetricsRuntime=serde_json::from_value(v["runtime"].clone()).map_err(|_|Error::InvalidWire)?;
    if disk {
        if r.kind!="prometheusHardmode"||r.upstream_tag!="3.16.0"
            ||!matches!(r.platform.as_str(),"linux-x86_64"|"linux-aarch64")
            ||!(64..=192*1024*1024).contains(&r.binary_bytes)||r.binary_sha256.len()!=64
            ||!r.binary_sha256.bytes().all(|c|c.is_ascii_digit()||(b'a'..=b'f').contains(&c)){
            return Err(Error::CapabilityDenied)
        }
        // The selected signed release/actual ELF check is a Node prerequisite,
        // not inferred from this owner's public manifest metadata.
        return Ok(Some(r))
    }
    let valid=r.kind=="vmagent" && r.upstream_tag=="v1.156.0" && match r.platform.as_str() {
        "linux-x86_64"=>r.binary_sha256=="2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6"&&r.binary_bytes==23209584,
        "linux-aarch64"=>r.binary_sha256=="564346d8acd0979b62c2671ad586d63b243a925307bfa54d0d5f94bd50dda4f5"&&r.binary_bytes==21299768,
        _=>false,
    };
    if !valid{return Err(Error::CapabilityDenied)}
    Ok(Some(r))
}
pub fn validate_metrics_manifest(v:&serde_json::Value,expected_host:&str)->Result<Hash> {
    if metrics_json_body(v)?.len()>8192{return Err(Error::Quota)}
    if metrics_manifest_runtime(v)?.is_some() {
        let mut structural=v.clone();structural.as_object_mut().ok_or(Error::InvalidWire)?.remove("runtime");
        if v.get("profile").is_some() {
            structural.as_object_mut().ok_or(Error::InvalidWire)?.remove("profile");
            structural.as_object_mut().ok_or(Error::InvalidWire)?.remove("bufferMode");
            structural["buffer"]=json!({"max_bytes":33554432u64,"max_age_seconds":300});
            for target in v["targets"].as_array().ok_or(Error::InvalidWire)? {
                metrics_manifest_integer(&target["series_limit"],256,50000)?;
            }
        }
        structural["schema_version"]=json!(1);
        let _structural_only=validate_metrics_manifest(&structural,expected_host)?;
        return Ok(Hash::from_bytes(Sha256::digest(metrics_json_body(v)?).into()))
    }

    if metrics_json_body(v)?.len()>8192{return Err(Error::Quota)}
    metrics_manifest_keys(v,&["schema_version","mode","targets","labels","scrape_interval_seconds","receiver","buffer"])?;
    if v["schema_version"]!=json!(1)||v["mode"]!="remote_write" {return Err(Error::CapabilityDenied)}
    metrics_manifest_keys(&v["labels"],&["cc_host_id","site","environment"])?;
    let host=v["labels"]["cc_host_id"].as_str().ok_or(Error::WrongClaim)?;
    let id=uuid::Uuid::parse_str(host).map_err(|_|Error::WrongClaim)?;
    if id.is_nil()||id.to_string()!=host{return Err(Error::WrongClaim)}
    metrics_manifest_name(&v["labels"]["site"])?;metrics_manifest_name(&v["labels"]["environment"])?;
    metrics_manifest_integer(&v["scrape_interval_seconds"],1,300)?;
    metrics_manifest_keys(&v["receiver"],&["url"])?;metrics_manifest_endpoint(&v["receiver"]["url"],false)?;
    metrics_manifest_keys(&v["buffer"],&["max_bytes","max_age_seconds"])?;
    if v["buffer"]["max_bytes"]!=json!(33554432)||v["buffer"]["max_age_seconds"]!=json!(300){return Err(Error::ScopeDenied)}
    let targets=v["targets"].as_array().ok_or(Error::InvalidWire)?;
    if targets.is_empty()||targets.len()>16{return Err(Error::Quota)}
    let(mut ids,mut urls)=(BTreeSet::new(),BTreeSet::new());
    for t in targets {
        metrics_manifest_keys(t,&["id","url","kind","sample_limit","series_limit","response_bytes","label_limit"])?;
        if !ids.insert(metrics_manifest_name(&t["id"])?.to_owned())||!urls.insert(t["url"].as_str().ok_or(Error::InvalidWire)?.to_owned()) {return Err(Error::Replay)}
        metrics_manifest_endpoint(&t["url"],true)?;
        if !matches!(t["kind"].as_str(),Some("application"|"node_exporter"|"cadvisor")){return Err(Error::CapabilityDenied)}
        metrics_manifest_integer(&t["sample_limit"],1,50000)?;metrics_manifest_integer(&t["series_limit"],1,50000)?;
        metrics_manifest_integer(&t["response_bytes"],256,8*1024*1024)?;metrics_manifest_integer(&t["label_limit"],4,64)?;
    }
    if host!=expected_host{return Err(Error::WrongClaim)}
    Ok(Hash::from_bytes(Sha256::digest(metrics_json_body(v)?).into()))
}

/// Metadata address of the exact approved manifest, not a signing domain.
pub fn metrics_collector_id(manifest_hash:Hash)->Result<ResourceId> {
    let mut bytes=[0u8;16];bytes.copy_from_slice(&manifest_hash.as_bytes()[..16]);
    bytes[6]=(bytes[6]&0x0f)|0x80;bytes[8]=(bytes[8]&0x3f)|0x80;
    ResourceId::new(&uuid::Uuid::from_bytes(bytes).to_string())
}

include!("owner_exporter.rs");
