//! Exact dcff6802 control contract. Typed bodies are observations, not grants.
use super::*;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

pub(super) const FORMAT: &str = "cc-native-node-enroll/v1";
pub(super) const MAX_FRAME: usize = 16384;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    format: String,
    request_id: Uuid,
    kind: String,
    payload: Value,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NodeIdentity {
    pub format: u32,
    pub agent_id: AgentId,
    pub key_epoch: u64,
    pub public_key: PublicKey,
    pub key_fingerprint: Hash,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Probe {
    pub node: NodeIdentity,
    pub owner_pin: OwnerPin,
    pub actor_revision: u64,
    #[serde(deserialize_with = "required_certificate")]
    pub certificate: Option<Certificate>,
    pub revoked: bool,
}
fn required_certificate<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Certificate>, D::Error> {
    Option::<Certificate>::deserialize(d)
}
#[derive(Clone,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct MetricsCollector {
    pub collector_id:ResourceId,pub manifest_sha256:Hash,pub manifest:Value,
}
impl MetricsCollector {
    pub(super) fn check(&self,host:ObjectId)->AppResult<()> {
        let digest=metrics_manifest_digest(&self.manifest,host)?;
        if digest!=self.manifest_sha256||self.collector_id!=metrics_collector_id(digest).map_err(|_|unavailable())?{return Err(unavailable())}
        // Shared digest-derived UUIDv8 ResourceId plus full SHA bind this exact manifest.
        // No random root config ID can retarget an unused MetricsCollect lease.
        Ok(())
    }
    pub(super) fn public(&self)->Value {
        serde_json::json!({"collectorId":self.collector_id,"manifestSha256":self.manifest_sha256,"manifest":self.manifest})
    }
}
/// Disk budgets/profile are part of the ACTUAL full resource hash. The common
/// copy below is used only to reuse existing closed endpoint/target validation;
/// its volatile hash is deliberately ignored and never signed or admitted.
pub(super) fn metrics_manifest_digest(v:&Value,host:ObjectId)->AppResult<Hash> {
    if v["schema_version"]==serde_json::json!(2) {
        return validate_metrics_manifest(v,&host.to_string()).map_err(|_|unavailable());
    }
    if !v.get("profile").is_some() && !v.get("bufferMode").is_some() {
        return validate_metrics_manifest(v,&host.to_string()).map_err(|_|unavailable());
    }
    if v["schema_version"]!=serde_json::json!(1)||v.get("runtime").is_some(){return Err(unavailable());}
    let expected=["schema_version","mode","targets","labels","scrape_interval_seconds","receiver","buffer","profile","bufferMode"];
    let object=v.as_object().ok_or_else(unavailable)?;
    if object.len()!=expected.len()||expected.iter().any(|k|!object.contains_key(*k))
        ||v["profile"]!="maintained-prometheus-disk-512mib-2h-v1"||v["bufferMode"]!="persistentNativeWal"
        ||v["buffer"]!=serde_json::json!({"max_bytes":536870912u64,"max_age_seconds":7200}) {return Err(unavailable());}
    let mut common=v.clone();let fields=common.as_object_mut().ok_or_else(unavailable)?;
    fields.remove("profile");fields.remove("bufferMode");
    common["buffer"]=serde_json::json!({"max_bytes":33554432,"max_age_seconds":300});
    let _structural_only=validate_metrics_manifest(&common,&host.to_string()).map_err(|_|unavailable())?;
    if v["targets"].as_array().ok_or_else(unavailable)?.iter().any(|target|
        target["series_limit"].as_u64().is_none_or(|n|n<256)) {return Err(unavailable());}
    let mut bytes=canonical(v)?;bytes.pop();
    if bytes.len()>8192{return Err(unavailable());}
    Ok(Hash::from_bytes(Sha256::digest(bytes.as_slice()).into()))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct MetricsProbe {
    pub node:NodeIdentity,pub owner_pin:OwnerPin,pub actor_revision:u64,
    #[serde(deserialize_with="required_certificate")]
    pub certificate:Option<Certificate>,pub revoked:bool,
    pub advertised_scopes:Vec<Scope>,
    #[serde(deserialize_with="required_collector")]
    pub collector:Option<MetricsCollector>,
}
impl MetricsProbe {
    pub(super) fn checked(self,scope:&OriginalScope,host:ObjectId,initial_enrollment:bool)->AppResult<(Probe,MetricsCollector)> {
        if self.advertised_scopes.as_slice()!=[Scope::MetricsCollect]{return Err(unavailable())}
        let collector=self.collector.ok_or_else(unavailable)?;collector.check(host)?;
        let p=Probe{node:self.node,owner_pin:self.owner_pin,actor_revision:self.actor_revision,certificate:self.certificate,revoked:self.revoked};
        if initial_enrollment {initial(scope,&p)?} else {scope.bind_owner_pin(&p.owner_pin)?;}
        Ok((p,collector))
    }
}
// A mandatory null collector is accepted ONLY by the revocation verb. Missing
// is rejected, and enrollment/collection require Some plus actual host hashing.
fn required_collector<'de,D:serde::Deserializer<'de>>(d:D)->std::result::Result<Option<MetricsCollector>,D::Error>{
    Option::<MetricsCollector>::deserialize(d)
}
impl MetricsProbe {
    pub(super) fn checked_revocation(self,scope:&OriginalScope)->AppResult<Probe> {
        if self.advertised_scopes.as_slice()!=[Scope::MetricsCollect]||self.collector.is_some(){return Err(unavailable())}
        scope.bind_owner_pin(&self.owner_pin)?;
        Ok(Probe{node:self.node,owner_pin:self.owner_pin,actor_revision:self.actor_revision,
            certificate:self.certificate,revoked:self.revoked})
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Claimed {
    pub claim: Signed<Claim>,
    pub claim_digest: Hash,
    pub owner_pin: OwnerPin,
    pub actor_revision: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Committed {
    pub certificate: Certificate,
    pub certificate_digest: Hash,
    pub node_fingerprint: Hash,
    pub owner_pin: OwnerPin,
    pub actor_revision: u64,
    pub revoked: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Failure {
    code: Code,
    effect: Effect,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Code {
    BootstrapUnenrolled,
    TransportUnsupported,
    TransportUnavailable,
    CollectorUnavailable,
    InvalidFrame,
    WrongRequest,
    WrongState,
    ContextChanged,
    Expired,
    Denied,
    Capacity,
    StorageUnavailable,
    UncertainCommit,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Effect {
    NotCommitted,
    Unknown,
}

// Same canonical algorithm as the existing wire codec. This is framing only;
// all statement signatures still use the existing typed wire implementation.
fn append(v: &Value, out: &mut Vec<u8>) -> AppResult<()> {
    match v {
        Value::Object(m) => {
            out.push(b'{');
            let mut entries: Vec<_> = m.iter().collect();
            entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
            for (i, (k, v)) in entries.into_iter().enumerate() {
                if i != 0 {
                    out.push(b',');
                }
                serde_json::to_writer(&mut *out, k).map_err(|_| unavailable())?;
                out.push(b':');
                append(v, out)?;
            }
            out.push(b'}');
        }
        Value::Array(a) => {
            out.push(b'[');
            for (i, v) in a.iter().enumerate() {
                if i != 0 {
                    out.push(b',');
                }
                append(v, out)?;
            }
            out.push(b']');
        }
        Value::Number(n) if !n.is_i64() && !n.is_u64() => return Err(unavailable()),
        _ => serde_json::to_writer(&mut *out, v).map_err(|_| unavailable())?,
    }
    if out.len() >= MAX_FRAME {
        return Err(unavailable());
    }
    Ok(())
}
pub(super) fn canonical(v: &Value) -> AppResult<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::new());
    append(v, &mut out)?;
    out.push(b'\n');
    if !out.is_ascii() || out.len() > MAX_FRAME {
        return Err(unavailable());
    }
    Ok(out)
}
pub(super) fn request(kind: &str, payload: Value) -> AppResult<(Uuid, Zeroizing<Vec<u8>>)> {
    let id = Uuid::new_v4();
    Ok((
        id,
        canonical(
            &serde_json::json!({"format":FORMAT,"requestId":id,"kind":kind,"payload":payload}),
        )?,
    ))
}
/// Token never enters a serde Value/String clone, durable row or Debug body.
/// It is appended only to the owned zeroizing wire buffer in canonical order.
pub(super) fn stage_request(
    revision: u64,
    pin: &OwnerPin,
    offer: &Signed<Offer>,
    token: &str,
) -> AppResult<(Uuid, Zeroizing<Vec<u8>>)> {
    if token.len() != 64
        || !token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(unavailable());
    }
    let id = Uuid::new_v4();
    let p = canonical(&serde_json::to_value(pin).map_err(|_| unavailable())?)?;
    let o = canonical(&serde_json::to_value(offer).map_err(|_| unavailable())?)?;
    let p = std::str::from_utf8(&p[..p.len() - 1]).map_err(|_| unavailable())?;
    let o = std::str::from_utf8(&o[..o.len() - 1]).map_err(|_| unavailable())?;
    let raw=Zeroizing::new(format!("{{\"format\":\"{FORMAT}\",\"kind\":\"stageOffer\",\"payload\":{{\"expectedActorRevision\":{revision},\"expectedOwnerPin\":{p},\"offer\":{o},\"token\":\"{token}\"}},\"requestId\":\"{id}\"}}\n").into_bytes());
    if raw.len() > MAX_FRAME || !raw.is_ascii() {
        return Err(unavailable());
    }
    Ok((id, raw))
}
pub(super) fn reply<T: DeserializeOwned>(raw: &[u8], id: Uuid, kind: &str) -> AppResult<T> {
    if raw.is_empty()
        || raw.len() > MAX_FRAME
        || !raw.is_ascii()
        || raw.contains(&b'\r')
        || raw.last() != Some(&b'\n')
    {
        return Err(unknown());
    }
    let v: Value = serde_json::from_slice(raw).map_err(|_| unknown())?;
    if v.get("requestId").and_then(Value::as_str) != Some(id.to_string().as_str()) {
        return Err(unknown());
    }
    // Duplicate keys, whitespace, floats, escaped IDs and trailing frames cannot
    // equal canonical re-encoding, including nested Signed<T> objects.
    if canonical(&v).map_err(|_| unknown())?.as_slice() != raw {
        return Err(unknown());
    }
    let e: Envelope = serde_json::from_value(v).map_err(|_| unknown())?;
    if e.format != FORMAT || e.request_id != id || id.is_nil() {
        return Err(unknown());
    }
    if e.kind == "error" {
        let f: Failure = serde_json::from_value(e.payload).map_err(|_| unknown())?;
        // Never expose stderr/raw data. No outcome here is a retry permission.
        return Err(match (f.code, f.effect) {
            (Code::BootstrapUnenrolled, Effect::NotCommitted) => unavailable(),
            (Code::TransportUnsupported, Effect::NotCommitted) => AppError::invalid("agent_tunnel_route", "transport_unsupported"),
            (Code::TransportUnavailable, Effect::NotCommitted) => AppError::invalid("agent_tunnel_route", "transport_unavailable"),
            (Code::CollectorUnavailable, Effect::NotCommitted) => AppError::invalid("agent_collector", "collector_unavailable"),
            _ => unknown(),
        });
    }
    if e.kind != kind {
        return Err(unknown());
    }
    serde_json::from_value(e.payload).map_err(|_| unknown())
}
pub(super) fn advance(pin: &OwnerPin, steps: u64) -> AppResult<OwnerPin> {
    let mut next = pin.clone();
    next.checkpoint = pin.checkpoint.checked_add(steps).ok_or_else(unknown)?;
    Ok(next)
}
pub(super) fn initial(scope: &OriginalScope, p: &Probe) -> AppResult<()> {
    scope.bind_owner_pin(&p.owner_pin)?;
    p.node.public_key.validate().map_err(|_| unavailable())?;
    if p.node.format != 1
        || p.node.key_epoch != 1
        || p.certificate.is_some()
        || p.revoked
        || p.node.public_key == p.owner_pin.key
        || p.node.key_fingerprint != p.node.public_key.fingerprint()
        || p.actor_revision.checked_add(3).is_none()
        || p.owner_pin.checkpoint.checked_add(2).is_none()
    {
        return Err(unavailable());
    }
    Ok(())
}
pub(super) fn claimed(p: &Probe, offer: &Signed<Offer>, c: &Claimed, now: Now) -> AppResult<()> {
    claimed_for(AssociationPurpose::Tunnel,p,offer,c,now)
}
pub(super) fn claimed_for(purpose:AssociationPurpose,p:&Probe,offer:&Signed<Offer>,c:&Claimed,now:Now)->AppResult<()> {
    verify_statement(offer, p.owner_pin.key).map_err(|_| unknown())?;
    verify_statement(&c.claim, p.node.public_key).map_err(|_| unknown())?;
    let o = &offer.body;
    let b = &c.claim.body;
    check_recent(b.issued_at, now).map_err(|_| unknown())?;
    check_live(o.issued_at, o.expires_at, now).map_err(|_| unknown())?;
    if o.agent_id != p.node.agent_id
        || o.agent_key != p.node.public_key
        || o.header.context != p.owner_pin.context
        || o.header.owner_epoch != p.owner_pin.epoch
        || o.header.checkpoint != advance(&p.owner_pin, 1)?.checkpoint
        || b.header != o.header
        || b.invitation_id != o.invitation_id
        || b.offer_hash != o.digest().map_err(|_| unknown())?
        || b.agent_id != o.agent_id
        || b.agent_key != o.agent_key
        || o.scopes.0.as_slice() != [purpose.scope()]
        || b.capabilities.version != 1
        || b.capabilities.scopes.0.as_slice() != [purpose.scope()]
        || c.claim_digest != b.digest().map_err(|_| unknown())?
        || c.owner_pin != advance(&p.owner_pin, 1)?
        || c.actor_revision != p.actor_revision.checked_add(2).ok_or_else(unknown)?
    {
        return Err(unknown());
    }
    Ok(())
}
pub(super) fn committed(
    p: &Probe,
    cert: &Signed<Certificate>,
    c: &Committed,
    now: Now,
) -> AppResult<()> {
    verify_statement(cert, p.owner_pin.key).map_err(|_| unknown())?;
    check_live(cert.body.not_before, cert.body.expires_at, now).map_err(|_| unknown())?;
    if c.revoked
        || c.certificate != cert.body
        || c.certificate_digest != cert.body.digest().map_err(|_| unknown())?
        || c.node_fingerprint != p.node.key_fingerprint
        || c.owner_pin != advance(&p.owner_pin, 2)?
        || c.actor_revision != p.actor_revision.checked_add(3).ok_or_else(unknown)?
    {
        return Err(unknown());
    }
    Ok(())
}

/// Public inert candidate from the SAME fixed authenticated Agent role.
#[derive(Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct RotationCandidate {
    pub format:u32,pub agent_id:AgentId,pub rotation_id:String,pub from_key_epoch:u64,pub proposed_key_epoch:u64,
    pub old_key_fingerprint:Hash,pub new_key_fingerprint:Hash,pub new_public_key:PublicKey,
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RotationProofs {
    pub certificate:Signed<Certificate>,pub old:Signed<RotationProof>,pub new:Signed<NewKeyProof>,
}
pub(super) fn rotation_candidate(previous:&Certificate,candidate:&RotationCandidate)->AppResult<()> {
    candidate.new_public_key.validate().map_err(|_|unavailable())?;
    if candidate.format!=1 || candidate.agent_id!=previous.agent_id || candidate.from_key_epoch!=1
        || candidate.proposed_key_epoch!=2 || previous.key_epoch!=1 || candidate.rotation_id.len()!=32
        || !candidate.rotation_id.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))
        || candidate.old_key_fingerprint!=previous.agent_key.fingerprint()
        || candidate.new_key_fingerprint!=candidate.new_public_key.fingerprint()
        || candidate.new_public_key==previous.agent_key {return Err(unavailable())}
    Ok(())
}
/// Historical signature/binding validation ONLY. Fresh new proofs also need an
/// explicit check_recent at their actual receipt before dispatching Actor.rotate.
pub(super) fn historical_rotation(previous:&Signed<Certificate>,candidate:&RotationCandidate,
    proofs:&RotationProofs,owner:PublicKey)->AppResult<()> {
    verify_statement(previous,owner).map_err(|_|unavailable())?;
    verify_statement(&proofs.certificate,owner).map_err(|_|unavailable())?;
    rotation_candidate(&previous.body,candidate)?;
    let old=&previous.body;let new=&proofs.certificate.body;
    if old.scopes.0.as_slice()!=[Scope::TunnelOpen] || old.capabilities.scopes!=old.scopes
        || new.scopes!=old.scopes || new.capabilities!=old.capabilities || new.key_epoch!=2
        || new.agent_id!=old.agent_id || new.agent_key!=candidate.new_public_key
        || new.agent_key==owner || new.header.context!=old.header.context
        || new.header.owner_epoch!=old.header.owner_epoch || new.header.checkpoint<=old.header.checkpoint
        || new.origin!=(CertificateOrigin::Rotation {previous_certificate:old.digest().map_err(|_|unavailable())?,previous_key_epoch:1})
        || new.not_before<old.not_before || new.expires_at>old.expires_at
        || proofs.old.body!=proofs.new.body.0 || proofs.old.body.header!=new.header
        || proofs.old.body.agent_id!=new.agent_id || proofs.old.body.certificate_hash!=new.digest().map_err(|_|unavailable())? {
        return Err(unavailable())
    }
    verify_statement(&proofs.old,old.agent_key).map_err(|_|unavailable())?;
    verify_statement(&proofs.new,new.agent_key).map_err(|_|unavailable())?;
    Ok(())
}

#[cfg(test)]
mod public_transport_error_tests {
    use super::*;
    #[test]
    fn tunnel_transport_prerequisite_errors_are_closed_and_never_commit_acknowledgements() {
        let id=Uuid::new_v4();
        for (code,reason) in [("transportUnsupported","transport_unsupported"),("transportUnavailable","transport_unavailable")] {
            let raw=canonical(&serde_json::json!({"format":FORMAT,"requestId":id,"kind":"error",
                "payload":{"code":code,"effect":"notCommitted"}})).unwrap();
            assert_eq!(reply::<Value>(&raw,id,"probeTunnelTransport").unwrap_err(),
                AppError::invalid("agent_tunnel_route",reason));
            let raw=canonical(&serde_json::json!({"format":FORMAT,"requestId":id,"kind":"error",
                "payload":{"code":code,"effect":"unknown"}})).unwrap();
            assert_eq!(reply::<Value>(&raw,id,"probeTunnelTransport").unwrap_err(),unknown());
        }
        let raw=canonical(&serde_json::json!({"format":FORMAT,"requestId":id,"kind":"error",
            "payload":{"code":"arbitraryWorkerMessage","effect":"notCommitted"}})).unwrap();
        assert_eq!(reply::<Value>(&raw,id,"probeTunnelTransport").unwrap_err(),unknown());
    }
}


#[cfg(test)]
mod metrics_error_tests {
    use super::*;
    #[test]
    fn collector_unavailable_is_fixed_and_only_not_committed() {
        let id=Uuid::new_v4();
        for kind in ["probeMetrics","startCollector"] {
            let raw=canonical(&serde_json::json!({"format":FORMAT,"requestId":id,"kind":"error",
                "payload":{"code":"collectorUnavailable","effect":"notCommitted"}})).unwrap();
            assert_eq!(reply::<Value>(&raw,id,kind).unwrap_err(),AppError::invalid("agent_collector","collector_unavailable"));
            let raw=canonical(&serde_json::json!({"format":FORMAT,"requestId":id,"kind":"error",
                "payload":{"code":"collectorUnavailable","effect":"unknown"}})).unwrap();
            assert_eq!(reply::<Value>(&raw,id,kind).unwrap_err(),unknown());
            let raw=canonical(&serde_json::json!({"format":FORMAT,"requestId":id,"kind":"error",
                "payload":{"code":"collectorUnavailable","effect":"notCommitted","stderr":"untrusted"}})).unwrap();
            assert_eq!(reply::<Value>(&raw,id,kind).unwrap_err(),unknown());
        }
    }
}

#[path = "protocol_exporter.rs"]
mod exporter;
pub(super) use exporter::{ExporterCatalog, ExporterProbe};
