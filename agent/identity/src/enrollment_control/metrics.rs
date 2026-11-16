//! Fixed MetricsOnly enrollment/collection. Owner authority is the actual Actor
//! Permit retained here; catalog/advertisement/child observations grant nothing.
use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::{Child, Command, Stdio};
#[path = "metrics/disk.rs"]
mod disk;
#[path="metrics/runtime.rs"]
mod runtime;
const CATALOG: &str = "/etc/consolecrypt-agent/collector-volatile.json";
const ADAPTER: &str =
    "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/volatile_unit_adapter.py";
const ENTRY: &str =
    "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/volatile_unit_entry.py";
const SPEC: &str = "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/unit_spec.py";
const PYTHON: &str = "/usr/bin/python3.12";
const SESSION_ROOT: &str = "/run/consolecrypt-agent/volatile";
const VMAGENT: &str =
    "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/vmagent-prod";
const VMAGENT_SHA: &str = "2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6";
fn digest(raw: &[u8]) -> Hash {
    Hash::from_bytes(Sha256::digest(raw).into())
}
// Only four SOURCE-FIXED installed runtime leaves, never a caller path.
fn root_digest(path: &Path) -> Result<Hash> {
    let cap = if path == Path::new(VMAGENT) {
        32 * 1024 * 1024
    } else if path == Path::new(PYTHON) {
        16 * 1024 * 1024
    } else if [ADAPTER, ENTRY, SPEC].iter().any(|p| path == Path::new(p)) {
        65536
    } else if let Some(cap)=runtime::extra_cap(path) {
        cap
    } else {
        return Err(Error::ScopeDenied);
    };
    for parent in path.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(|_| Error::Storage)?;
        if !m.is_dir() || m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err(Error::Storage);
        }
    }
    let before = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.uid() != 0
        || before.nlink() != 1
        || before.mode() & 0o7022 != 0
        || before.len() == 0
        || before.len() > cap
    {
        return Err(Error::Storage);
    }
    let identity = |m: &fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.uid(),
            m.mode(),
            m.nlink(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC)
        .open(path)
        .map_err(|_| Error::Storage)?;
    if identity(&before) != identity(&f.metadata().map_err(|_| Error::Storage)?) {
        return Err(Error::Storage);
    }
    let mut hasher = Sha256::new();
    let mut chunk = [0u8; 65536];
    let mut n = 0u64;
    loop {
        let got = f.read(&mut chunk).map_err(|_| Error::Storage)?;
        if got == 0 {
            break;
        }
        n += got as u64;
        if n > cap {
            return Err(Error::Quota);
        }
        hasher.update(&chunk[..got]);
    }
    if n != before.len()
        || identity(&before) != identity(&f.metadata().map_err(|_| Error::Storage)?)
        || identity(&before) != identity(&fs::symlink_metadata(path).map_err(|_| Error::Storage)?)
    {
        return Err(Error::Storage);
    }
    Ok(Hash::from_bytes(hasher.finalize().into()))
}

fn json_body(value: &Value) -> Result<Vec<u8>> {
    let mut b = canonical(value)?;
    b.pop();
    Ok(b)
}
fn exact(value: &Value, keys: &[&str]) -> Result<()> {
    let o = value.as_object().ok_or(Error::InvalidWire)?;
    if o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)) {
        return Err(Error::InvalidWire);
    }
    Ok(())
}
fn integer(v: &Value, min: u64, max: u64) -> Result<u64> {
    v.as_u64()
        .filter(|n| min <= *n && *n <= max)
        .ok_or(Error::InvalidWire)
}
fn name(v: &Value) -> Result<&str> {
    let s = v.as_str().ok_or(Error::InvalidWire)?;
    let b = s.as_bytes();
    if b.is_empty()
        || b.len() > 32
        || !b[0].is_ascii_alphabetic()
        || !b
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(b))
    {
        return Err(Error::InvalidWire);
    }
    Ok(s)
}
fn endpoint(value: &Value, scrape: bool) -> Result<(String, String, String)> {
    let raw = value.as_str().ok_or(Error::InvalidWire)?;
    if raw.len() > 512 || !raw.is_ascii() {
        return Err(Error::ScopeDenied);
    }
    let (scheme, tail) = if let Some(v) = raw.strip_prefix("http://") {
        ("http", v)
    } else if let Some(v) = raw.strip_prefix("https://") {
        ("https", v)
    } else {
        return Err(Error::ScopeDenied);
    };
    let (auth, path) = tail.split_once('/').ok_or(Error::ScopeDenied)?;
    let addr: SocketAddr = auth.parse().map_err(|_| Error::ScopeDenied)?;
    if !addr.ip().is_loopback()
        || addr.port() == 0
        || addr.to_string() != auth
        || (scrape && path != "metrics")
        || (!scrape && !matches!(path, "api/v1/write" | "api/v1/push"))
    {
        return Err(Error::ScopeDenied);
    }
    if !scrape && (scheme != "http" || !auth.starts_with("127.0.0.1:")) {
        return Err(Error::ScopeDenied);
    }
    Ok((scheme.to_owned(), auth.to_owned(), format!("/{path}")))
}
fn validate_manifest(v: &Value) -> Result<()> {
    if json_body(v)?.len()>8192{return Err(Error::Quota)}
    if v["schema_version"]==json!(2) {
        let host=v["labels"]["cc_host_id"].as_str().ok_or(Error::WrongClaim)?;
        cc_agent_node_wire::validate_metrics_manifest(v,host).map_err(|_|Error::CapabilityDenied)?;
        return Ok(())
    }

    if json_body(v)?.len() > 8192 {
        return Err(Error::Quota);
    }
    exact(
        v,
        &[
            "schema_version",
            "mode",
            "targets",
            "labels",
            "scrape_interval_seconds",
            "receiver",
            "buffer",
        ],
    )?;
    if v["schema_version"] != json!(1) || v["mode"] != "remote_write" {
        return Err(Error::CapabilityDenied);
    }
    exact(&v["labels"], &["cc_host_id", "site", "environment"])?;
    let host = v["labels"]["cc_host_id"]
        .as_str()
        .ok_or(Error::WrongClaim)?;
    let id = uuid::Uuid::parse_str(host).map_err(|_| Error::WrongClaim)?;
    if id.is_nil() || id.to_string() != host {
        return Err(Error::WrongClaim);
    }
    name(&v["labels"]["site"])?;
    name(&v["labels"]["environment"])?;
    integer(&v["scrape_interval_seconds"], 1, 300)?;
    exact(&v["receiver"], &["url"])?;
    endpoint(&v["receiver"]["url"], false)?;
    exact(&v["buffer"], &["max_bytes", "max_age_seconds"])?;
    if v["buffer"]["max_bytes"] != json!(33554432) || v["buffer"]["max_age_seconds"] != json!(300) {
        return Err(Error::ScopeDenied);
    }
    let targets = v["targets"].as_array().ok_or(Error::InvalidWire)?;
    if targets.is_empty() || targets.len() > 16 {
        return Err(Error::Quota);
    }
    let (mut ids, mut urls) = (BTreeSet::new(), BTreeSet::new());
    for t in targets {
        exact(
            t,
            &[
                "id",
                "url",
                "kind",
                "sample_limit",
                "series_limit",
                "response_bytes",
                "label_limit",
            ],
        )?;
        if !ids.insert(name(&t["id"])?.to_owned())
            || !urls.insert(t["url"].as_str().ok_or(Error::InvalidWire)?.to_owned())
        {
            return Err(Error::Replay);
        }
        endpoint(&t["url"], true)?;
        if !matches!(
            t["kind"].as_str(),
            Some("application" | "node_exporter" | "cadvisor")
        ) {
            return Err(Error::CapabilityDenied);
        }
        integer(&t["sample_limit"], 1, 50000)?;
        integer(&t["series_limit"], 1, 50000)?;
        integer(&t["response_bytes"], 256, 8 * 1024 * 1024)?;
        integer(&t["label_limit"], 4, 64)?;
    }
    Ok(())
}
// Same metadata-only UUIDv8 address as Native metrics_collector_id; full
// 256-bit manifest hash remains checked independently in every descriptor/start.
fn collector_id(manifest_hash: Hash) -> Result<ResourceId> {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&manifest_hash.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    ResourceId::new(&uuid::Uuid::from_bytes(bytes).to_string())
}
fn check_collector_id(id: ResourceId, manifest_hash: Hash) -> Result<()> {
    if id != collector_id(manifest_hash)? {
        return Err(Error::WrongClaim);
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogFile {
    format: String,
    collector_id: ResourceId,
    manifest: Value,
    adapter_sha256: Hash,
    entry_sha256: Hash,
    spec_sha256: Hash,
    interpreter_sha256: Hash,
}
#[derive(Clone)]
struct Catalog {
    id: ResourceId,
    manifest: Value,
    hash: Hash,
    raw: Vec<u8>,
    fixed: Vec<(PathBuf, Hash)>,
    disk: Option<disk::Installed>,
}
impl Catalog {
    fn load() -> Result<Self> {
        if service_uid()? == 0 {
            return Err(Error::CapabilityDenied);
        }
        if let Some(value) = disk::load()? {
            return Ok(value);
        }
        disk::no_sessions(disk::ROOT)?;
        let raw = root_file(Path::new(CATALOG))?;
        let parsed: Value = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
        if canonical(&parsed)? != raw {
            return Err(Error::InvalidWire);
        }
        let value: CatalogFile = decode(parsed)?;
        let v2=value.format=="cc-agent-volatile-collector/v2";
        if !(v2&&value.manifest["schema_version"]==json!(2)
            ||value.format=="cc-agent-volatile-collector/v1"&&value.manifest["schema_version"]==json!(1)) {
            return Err(Error::CapabilityDenied);
        }
        validate_manifest(&value.manifest)?;
        // BEFORE descriptor/advertisement: real uname, original selected
        // per-release verifier data, selected+stable held-FD ELF/SHA identity.
        let(vmagent_hash,selected_pins)=if v2 {runtime::selected(&value.manifest)?}
            else {runtime::legacy()?;(decode(json!(VMAGENT_SHA))?,Vec::new())};
        let mut fixed = vec![
            (PathBuf::from(VMAGENT), vmagent_hash),
            (PathBuf::from(ADAPTER), value.adapter_sha256),
            (PathBuf::from(ENTRY), value.entry_sha256),
            (PathBuf::from(SPEC), value.spec_sha256),
            (PathBuf::from(PYTHON), value.interpreter_sha256),
        ];
        fixed.extend(selected_pins);
        for (path, expected) in &fixed {
            if root_digest(path)? != *expected {
                return Err(Error::Storage);
            }
        }
        let hash = digest(&json_body(&value.manifest)?);
        check_collector_id(value.collector_id, hash)?;
        Ok(Self {
            id: value.collector_id,
            manifest: value.manifest,
            hash,
            raw,
            fixed,
            disk: None,
        })
    }
    fn check(&self) -> Result<()> {
        if root_file(Path::new(if self.disk.is_some() {
            disk::CATALOG
        } else {
            CATALOG
        }))? != self.raw
        {
            return Err(Error::Storage);
        }
        for (path, expected) in &self.fixed {
            if root_digest(path)? != *expected {
                return Err(Error::Storage);
            }
        }
        if let Some(disk) = &self.disk {
            return disk.check();
        }
        Ok(())
    }
    fn descriptor(&self) -> Value {
        json!({"collectorId":self.id,"manifestSha256":self.hash,"manifest":self.manifest})
    }
    fn scrape(&self) -> Result<Vec<u8>> {
        if self.disk.is_some() {
            return disk::scrape(self);
        }
        let interval = integer(&self.manifest["scrape_interval_seconds"], 1, 300)?;
        let mut configs = Vec::new();
        for t in self.manifest["targets"]
            .as_array()
            .ok_or(Error::InvalidWire)?
        {
            let (scheme, authority, path) = endpoint(&t["url"], true)?;
            let labels = self.manifest["labels"].clone();
            let relabel = labels
                .as_object()
                .ok_or(Error::InvalidWire)?
                .iter()
                .map(|(k, v)| json!({"target_label":k,"replacement":v}))
                .collect::<Vec<_>>();
            configs.push(json!({"job_name":format!("cc_{}",name(&t["id"])?),"scheme":scheme,"metrics_path":path,
                "scrape_interval":format!("{interval}s"),"scrape_timeout":format!("{}s",10u64.min(interval)),
                "honor_labels":false,"honor_timestamps":true,"follow_redirects":false,
                "sample_limit":t["sample_limit"],"label_limit":t["label_limit"],
                "static_configs":[{"targets":[authority],"labels":labels}],"metric_relabel_configs":relabel,
                "series_limit":t["series_limit"],"max_scrape_size":t["response_bytes"].to_string()}));
        }
        json_body(&json!({"scrape_configs":configs}))
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Probed,
    RevocationProbed,
    Claimed,
    Applied,
    Revoked,
    Closed,
}
struct Session<S> {
    store: Option<S>,
    node_dir: PathBuf,
    agent: AgentId,
    node: PublicNodeIdentity,
    phase: Phase,
    seen: BTreeSet<String>,
    expected_pin: OwnerPin,
    expected_rev: u64,
    claim: Option<Hash>,
    certificate: Option<Hash>,
    offer_expires: Option<Timestamp>,
    effects: bool,
    catalog: Option<Catalog>,
}
impl<S: CompareAndSwap> Session<S> {
    fn snapshot(&self) -> Result<State> {
        self.store.as_ref().ok_or(Error::WrongState)?.load()
    }
    fn checked(&self, now: Now) -> Result<State> {
        if let Some(catalog) = &self.catalog {
            catalog.check()?;
        }
        let state = self.snapshot()?;
        if state.owner() != &self.expected_pin || state.revision() != self.expected_rev {
            return Err(Error::Conflict);
        }
        if now.0 >= state.owner().expires_at || self.offer_expires.is_some_and(|x| now.0 >= x) {
            return Err(Error::Expired);
        }
        if inspect_existing_node_identity(&self.node_dir, self.agent)? != self.node {
            return Err(Error::WrongClaim);
        }
        Ok(state)
    }
    fn mutate<F>(&mut self, revision: u64, effect: F) -> Result<()>
    where
        F: FnOnce(&mut Actor<RevisionFence<S>>) -> Result<()>,
    {
        self.effects = true; // Any ambiguity after this is terminal UNKNOWN.
        let inner = self.store.take().ok_or(Error::WrongState)?;
        let mut actor = Actor::new(RevisionFence {
            inner,
            expected: revision,
        })?;
        let result = effect(&mut actor);
        self.store = Some(actor.into_store().inner);
        result
    }
    fn expect_request(&self, revision: u64, pin: &OwnerPin) -> Result<()> {
        if revision != self.expected_rev || pin != &self.expected_pin {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    fn readback(&self, now: Now) -> Result<Value> {
        let state = self.checked(now)?;
        let certificate = state.certificate(self.agent).ok_or(Error::NotFound)?;
        if state.revoked(self.agent)
            || certificate.agent_key != self.node.public_key
            || certificate.key_epoch != self.node.key_epoch
            || now.0 < certificate.not_before
            || now.0 >= certificate.expires_at
            || Some(certificate.digest()?) != self.certificate
        {
            return Err(Error::WrongClaim);
        }
        Ok(
            json!({"certificate":certificate,"certificateDigest":certificate.digest()?,
            "nodeFingerprint":self.node.key_fingerprint,"ownerPin":state.owner(),
            "actorRevision":state.revision(),"revoked":false}),
        )
    }
    fn revoked_readback(&self, now: Now) -> Result<Value> {
        let state = self.checked(now)?;
        let cert = state.certificate(self.agent).ok_or(Error::NotFound)?;
        // No certificate-live check: a CURRENT expired key must be tombstonable.
        if !state.revoked(self.agent)
            || cert.agent_key != self.node.public_key
            || cert.key_epoch != self.node.key_epoch
            || Some(cert.digest()?) != self.certificate
        {
            return Err(Error::WrongClaim);
        }
        Ok(
            json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
            "certificateDigest":cert.digest()?,"revoked":true}),
        )
    }
    fn request(&mut self, frame: Envelope, clock: &mut Clock) -> Result<Value> {
        if self.seen.len() >= 4 || !self.seen.insert(frame.request_id.clone()) {
            return Err(Error::Replay);
        }
        let now = clock.now()?;
        self.checked(now)?;
        let payload = match (self.phase, frame.kind.as_str()) {
            (Phase::New, "probeMetrics" | "probeCollectorService" | "probeCollectorServiceLifetime") => {
                let _: Empty = decode(frame.payload)?;
                let state = self.checked(clock.now()?)?;
                self.phase = Phase::Probed;
                json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
                    "certificate":state.certificate(self.agent),"revoked":state.revoked(self.agent),
                    "advertisedScopes":[Scope::MetricsCollect],"collector":self.catalog.as_ref().map(|c|c.descriptor()).unwrap_or(Value::Null)})
            }
            (Phase::New, "probeMetricsRevocation" | "probeCollectorServiceStatus") => {
                let _: Empty = decode(frame.payload)?;
                let state = self.checked(clock.now()?)?;
                let cert = state.certificate(self.agent).ok_or(Error::NotFound)?;
                if cert.agent_key != self.node.public_key
                    || cert.key_epoch != self.node.key_epoch
                    || cert.scopes.0.as_slice() != [Scope::MetricsCollect]
                    || cert.capabilities.scopes != cert.scopes
                    || cert.capabilities.version != 1
                    || cert.header.owner_epoch != state.owner().epoch
                {
                    return Err(Error::WrongClaim);
                }
                self.phase = Phase::RevocationProbed;
                json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
                    "certificate":state.certificate(self.agent),"revoked":state.revoked(self.agent),
                    "advertisedScopes":[Scope::MetricsCollect],"collector":self.catalog.as_ref().map(|c|c.descriptor()).unwrap_or(Value::Null)})
            }
            (Phase::RevocationProbed, "revokeCertificate") => {
                let input: RevokeCertificate = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                let state = self.checked(clock.now()?)?;
                let current = state.certificate(self.agent).ok_or(Error::NotFound)?;
                let revoke = &input.revocation.body;
                if state.revoked(self.agent)
                    || current.agent_key != self.node.public_key
                    || current.key_epoch != self.node.key_epoch
                    || current.scopes.0.as_slice() != [Scope::MetricsCollect]
                    || current.capabilities.scopes != current.scopes
                    || current.header.owner_epoch != state.owner().epoch
                    || revoke.agent_id != self.agent
                    || revoke.key_epoch != current.key_epoch
                    || revoke.certificate_hash != current.digest()?
                {
                    return Err(Error::WrongClaim);
                }
                let digest = current.digest()?;
                let revision = self.expected_rev.checked_add(1).ok_or(Error::Quota)?;
                let checkpoint = self
                    .expected_pin
                    .checkpoint
                    .checked_add(1)
                    .ok_or(Error::Quota)?;
                self.mutate(self.expected_rev, |actor| {
                    actor.revoke(input.revocation, clock.now()?)
                })?;
                self.expected_rev = revision;
                self.expected_pin.checkpoint = checkpoint;
                self.certificate = Some(digest);
                let value = self.revoked_readback(clock.now()?)?;
                self.phase = Phase::Revoked;
                value
            }
            (Phase::Revoked, "observeRevocation") => {
                let input: ObserveRevocation = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                if input.agent_id != self.agent
                    || input.key_epoch != self.node.key_epoch
                    || Some(input.certificate_digest) != self.certificate
                {
                    return Err(Error::WrongClaim);
                }
                let value = self.revoked_readback(clock.now()?)?;
                self.phase = Phase::Closed;
                value
            }
            (Phase::Probed, "stageOffer") => {
                let input: StageOffer = decode(frame.payload)?;
                self.expect_request(input.expected_actor_revision, &input.expected_owner_pin)?;
                let state = self.checked(clock.now()?)?;
                if state.certificate(self.agent).is_some()
                    || self.node.key_epoch != 1
                    || input.offer.body.agent_id != self.agent
                    || input.offer.body.agent_key != self.node.public_key
                    || input.offer.body.scopes.0.as_slice() != [Scope::MetricsCollect]
                {
                    return Err(Error::WrongClaim);
                }
                let secret = token(input.token.as_str())?;
                if !bool::from(
                    secret
                        .digest()
                        .as_bytes()
                        .ct_eq(input.offer.body.token_hash.as_bytes()),
                ) {
                    return Err(Error::InvalidToken); // BEFORE first mutation.
                }
                let pre_pin = state.owner().clone();
                let start = state.revision();
                let offered_rev = start.checked_add(1).ok_or(Error::Quota)?;
                let claimed_rev = offered_rev.checked_add(1).ok_or(Error::Quota)?;
                let offered_cp = pre_pin.checkpoint.checked_add(1).ok_or(Error::Quota)?;
                let offer = input.offer;
                let offered = offer.clone();
                self.mutate(start, |a| a.offer(offered, clock.now()?))?;
                self.expected_rev = offered_rev;
                self.expected_pin.checkpoint = offered_cp;
                self.offer_expires = Some(offer.body.expires_at);
                clock.cap(offer.body.expires_at)?;
                self.checked(clock.now()?)?;
                // Custody requires PRE-offer current pin, not genesis/current-post-offer.
                let claim = create_metrics_node_claim(
                    &self.node_dir,
                    self.agent,
                    &pre_pin,
                    offer.clone(),
                    clock.now()?,
                )?;
                let digest = claim.body.digest()?;
                self.checked(clock.now()?)?;
                let consumed = claim.clone();
                self.mutate(offered_rev, |a| {
                    if a.consume(&secret, consumed, clock.now()?)? != digest {
                        return Err(Error::WrongClaim);
                    }
                    Ok(())
                })?;
                self.expected_rev = claimed_rev;
                let state = self.checked(clock.now()?)?;
                if state.invitation_status(offer.body.invitation_id)
                    != Some(InvitationStatus::Claimed)
                {
                    return Err(Error::WrongState);
                }
                self.claim = Some(digest);
                self.phase = Phase::Claimed;
                json!({"claim":claim,"claimDigest":digest,"ownerPin":state.owner(),"actorRevision":state.revision()})
            }
            (Phase::Claimed, "applyCertificate") => {
                let input: ApplyCertificate = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                let cert = &input.certificate.body;
                if self.claim != Some(input.claim_digest)
                    || cert.agent_id != self.agent
                    || cert.agent_key != self.node.public_key
                    || cert.key_epoch != 1
                    || !matches!(&cert.origin, CertificateOrigin::Enrollment { claim_hash, .. }
                        if Some(*claim_hash) == self.claim)
                {
                    return Err(Error::WrongClaim);
                }
                let digest = cert.digest()?;
                let revision = self.expected_rev.checked_add(1).ok_or(Error::Quota)?;
                let checkpoint = self
                    .expected_pin
                    .checkpoint
                    .checked_add(1)
                    .ok_or(Error::Quota)?;
                self.checked(clock.now()?)?;
                self.mutate(self.expected_rev, |a| {
                    a.approve(input.certificate, clock.now()?)
                })?;
                self.expected_rev = revision;
                self.expected_pin.checkpoint = checkpoint;
                self.certificate = Some(digest);
                let v = self.readback(clock.now()?)?;
                self.phase = Phase::Applied;
                v
            }
            (Phase::Applied, "observeCertificate") => {
                let input: ObserveCertificate = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                if input.agent_id != self.agent
                    || self.certificate != Some(input.certificate_digest)
                {
                    return Err(Error::WrongClaim);
                }
                let v = self.readback(clock.now()?)?;
                self.phase = Phase::Closed;
                v
            }
            _ => return Err(Error::WrongState),
        };
        self.checked(clock.now()?)?;
        Ok(
            json!({"format":FORMAT,"requestId":frame.request_id,"kind":frame.kind,"payload":payload}),
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartCollector {
    expected_rev: u64,
    expected_pin: OwnerPin,
    binding: NodeRequestBinding,
    collector_id: ResourceId,
    manifest_sha256: Hash,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopCollector {
    collector_id: ResourceId,
    lease_hash: Hash,
}
fn kernel_text(path: &str) -> Result<String> {
    let cap = match path {
        "/proc/self/status" => 16384,
        "/proc/self/mountinfo" => 1048576,
        "/proc/self/stat" => 4096,
        "/proc/uptime" => 128,
        "/proc/sys/kernel/random/boot_id" => 64,
        _ => return Err(Error::ScopeDenied),
    };
    let mut bytes = Vec::new();
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(NONBLOCK | CLOEXEC)
        .open(path)
        .map_err(|_| Error::Storage)?;
    Read::by_ref(&mut file)
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() as u64 > cap {
        return Err(Error::Quota);
    }
    String::from_utf8(bytes).map_err(|_| Error::Storage)
}
fn service_uid() -> Result<u32> {
    kernel_text("/proc/self/status")?
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|v| v.split_whitespace().nth(1))
        .ok_or(Error::Storage)?
        .parse()
        .map_err(|_| Error::Storage)
}
fn private_directory(path: &Path) -> Result<()> {
    let uid = service_uid()?;
    for p in path.ancestors() {
        let m = fs::symlink_metadata(p).map_err(|_| Error::Storage)?;
        if !m.is_dir()
            || m.file_type().is_symlink()
            || ![0, uid].contains(&m.uid())
            || m.mode() & 0o022 != 0
        {
            return Err(Error::Storage);
        }
    }
    let m = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    if m.uid() != uid || m.mode() & 0o777 != 0o700 {
        return Err(Error::Storage);
    }
    Ok(())
}
fn size_token(text: &str) -> Result<u64> {
    let (digits, mult) = if let Some(n) = text.strip_suffix('k') {
        (n, 1024u64)
    } else if let Some(n) = text.strip_suffix('m') {
        (n, 1024 * 1024)
    } else {
        (text, 1)
    };
    digits
        .parse::<u64>()
        .map_err(|_| Error::Storage)?
        .checked_mul(mult)
        .ok_or(Error::Storage)
}
fn tmpfs_root_at(root: &str) -> Result<()> {
    private_directory(Path::new(root))?;
    let mounts = kernel_text("/proc/self/mountinfo")?;
    if mounts.len() > 1024 * 1024 {
        return Err(Error::Quota);
    }
    let mut matches = 0;
    for line in mounts.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            return Err(Error::Storage);
        };
        let l = left.split_whitespace().collect::<Vec<_>>();
        let rr = right.split_whitespace().collect::<Vec<_>>();
        if l.len() < 6 || rr.len() < 3 {
            return Err(Error::Storage);
        }
        if l[4] != root {
            continue;
        }
        matches += 1;
        let opts = l[5]
            .split(',')
            .chain(rr[2].split(','))
            .collect::<BTreeSet<_>>();
        if rr[0] != "tmpfs"
            || !["rw", "noswap", "nodev", "nosuid", "noexec"]
                .iter()
                .all(|v| opts.contains(v))
            || !opts
                .iter()
                .find_map(|v| v.strip_prefix("size="))
                .is_some_and(|v| size_token(v) == Ok(33554432))
            || !opts
                .iter()
                .find_map(|v| v.strip_prefix("nr_inodes="))
                .is_some_and(|v| size_token(v) == Ok(4096))
        {
            return Err(Error::Storage);
        }
    }
    if matches != 1 {
        return Err(Error::Storage);
    }
    Ok(())
}
fn boottime_ns() -> Result<u64> {
    // /proc/uptime is a kernel BOOTTIME observation including suspend. Its
    // printed fractional truncation makes the derived deadline conservative.
    let raw = kernel_text("/proc/uptime")?;
    let first = raw.split_whitespace().next().ok_or(Error::InvalidTime)?;
    let (sec, frac) = first.split_once('.').ok_or(Error::InvalidTime)?;
    if frac.is_empty() || frac.len() > 9 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::InvalidTime);
    }
    let s = sec.parse::<u64>().map_err(|_| Error::InvalidTime)?;
    let f = frac.parse::<u64>().map_err(|_| Error::InvalidTime)?;
    s.checked_mul(1_000_000_000)
        .and_then(|s| s.checked_add(f * 10u64.pow((9 - frac.len()) as u32)))
        .ok_or(Error::InvalidTime)
}
fn boot_id() -> Result<String> {
    let raw = kernel_text("/proc/sys/kernel/random/boot_id")?;
    let id = raw.trim();
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| Error::Storage)?;
    if parsed.is_nil() || parsed.to_string() != id {
        return Err(Error::Storage);
    }
    Ok(id.to_owned())
}
fn start_ticks() -> Result<u64> {
    let raw = kernel_text("/proc/self/stat")?;
    raw.rsplit_once(')')
        .ok_or(Error::Storage)?
        .1
        .split_whitespace()
        .nth(19)
        .ok_or(Error::Storage)?
        .parse()
        .map_err(|_| Error::Storage)
}
struct RetainedFile {
    file: File,
    identity: (u64, u64, u32),
}
impl RetainedFile {
    fn capture(file: &File) -> Result<Self> {
        let m = file.metadata().map_err(|_| Error::Storage)?;
        Ok(Self {
            file: file.try_clone().map_err(|_| Error::Storage)?,
            identity: (m.dev(), m.ino(), m.mode() & 0o777),
        })
    }
}
struct Files {
    directory: PathBuf,
    identity: (u64, u64),
    directory_fd: File,
    paths: Vec<(PathBuf, Option<RetainedFile>)>,
    session: String,
    unit: String,
    _lock: File,
    cleanup_allowed: bool,
    disk: bool,
}
impl Files {
    fn new(disk: bool) -> Result<Self> {
        Self::create_session(disk, None)
    }
    fn new_service(permit: &CollectorServicePermit) -> Result<Self> {
        Self::create_session(true, Some(hex::encode(permit.start_id().as_bytes())))
    }
    fn create_session(disk: bool, original_session: Option<String>) -> Result<Self> {
        let root = if disk { disk::ROOT } else { SESSION_ROOT };
        tmpfs_root_at(root)?;
        let lock_path = Path::new(root).join(".collector.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC)
            .open(&lock_path)
            .map_err(|_| Error::Storage)?;
        let m = lock.metadata().map_err(|_| Error::Storage)?;
        let pm = fs::symlink_metadata(&lock_path).map_err(|_| Error::Storage)?;
        if !m.is_file()
            || m.nlink() != 1
            || m.uid() != service_uid()?
            || m.mode() & 0o777 != 0o600
            || (m.dev(), m.ino()) != (pm.dev(), pm.ino())
        {
            return Err(Error::Storage);
        }
        lock.try_lock().map_err(|_| Error::Conflict)?;
        // No concurrent collector and no adoption of an uncertain prior unit.
        // A retained orphan requires explicit administrator reconciliation.
        for item in fs::read_dir(root).map_err(|_| Error::Storage)?.take(33) {
            if item.map_err(|_| Error::Storage)?.path() != lock_path {
                return Err(Error::UncertainCommit);
            }
        }
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).map_err(|_| Error::Entropy)?;
        if b == [0; 16] {
            return Err(Error::Entropy);
        }
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        let session = hex::encode(b);
        let session = original_session.unwrap_or(session);
        let parsed = uuid::Uuid::parse_str(&session).map_err(|_| Error::WrongState)?;
        if parsed.get_version_num() != 4 || parsed.simple().to_string() != session {
            return Err(Error::WrongState);
        }
        let unit = format!(
            "cc-agent-{}-{session}.service",
            if disk { "disk" } else { "volatile" }
        );
        let directory = Path::new(root).join(&session);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|_| Error::Storage)?;
        let m = fs::symlink_metadata(&directory).map_err(|_| Error::Storage)?;
        let directory_fd = OpenOptions::new()
            .read(true)
            .custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC | 0x10000)
            .open(&directory)
            .map_err(|_| Error::Storage)?;
        let held = directory_fd.metadata().map_err(|_| Error::Storage)?;
        if !held.is_dir() || (held.dev(), held.ino()) != (m.dev(), m.ino()) {
            return Err(Error::Storage);
        }
        Ok(Self {
            directory,
            directory_fd,
            identity: (m.dev(), m.ino()),
            paths: Vec::new(),
            session,
            unit,
            _lock: lock,
            cleanup_allowed: true,
            disk,
        })
    }
    fn write(&mut self, name: &str, raw: &[u8]) -> Result<()> {
        if !matches!(name, "session.json" | "scrape.json") || raw.len() > 65536 {
            return Err(Error::Quota);
        }
        let path = self.directory.join(name);
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o400)
            .custom_flags(NOFOLLOW | CLOEXEC)
            .open(&path)
            .map_err(|_| Error::Storage)?;
        let m = f.metadata().map_err(|_| Error::Storage)?;
        if !m.is_file() || m.nlink() != 1 || m.uid() != service_uid()? || m.mode() & 0o777 != 0o400
        {
            return Err(Error::Storage);
        }
        self.paths.push((path, Some(RetainedFile::capture(&f)?)));
        f.write_all(raw).map_err(|_| Error::Storage)?;
        f.sync_all().map_err(|_| Error::Storage)
    }
    fn cleanup(&mut self) -> Result<()> {
        let m = match fs::symlink_metadata(&self.directory) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let held = self.directory_fd.metadata().map_err(|_| Error::Storage)?;
                if !held.is_dir() || (held.dev(), held.ino()) != self.identity || held.nlink() != 0 {
                    return Err(Error::UncertainCommit);
                }
                self.paths.clear();
                return Ok(());
            }
            Err(_) => return Err(Error::Storage),
        };
        let held = self.directory_fd.metadata().map_err(|_| Error::Storage)?;
        if !held.is_dir()
            || (held.dev(), held.ino()) != self.identity
            || !m.is_dir()
            || m.file_type().is_symlink()
            || (m.dev(), m.ino()) != self.identity
        {
            return Err(Error::Storage);
        }
        while let Some((path, created)) = self.paths.last() {
            if let Some(expected) = created {
                let held = expected.file.metadata().map_err(|_| Error::Storage)?;
                if (held.dev(), held.ino(), held.mode() & 0o777) != expected.identity {
                    return Err(Error::UncertainCommit);
                }
                let m = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
                if !m.is_file()
                    || m.file_type().is_symlink()
                    || m.nlink() != 1
                    || m.uid() != service_uid()?
                    || m.mode() & 0o777 != expected.identity.2
                    || (m.dev(), m.ino()) != (expected.identity.0, expected.identity.1)
                {
                    return Err(Error::UncertainCommit);
                }
                fs::remove_file(path).map_err(|_| Error::Storage)?;
            }
            self.paths.pop();
        }
        fs::remove_dir(&self.directory).map_err(|_| Error::Storage)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        if self.cleanup_allowed {
            let _ = self.cleanup();
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdapterReady {
    format: String,
    session: String,
    unit: String,
    vmagent_sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiskAdapterReady {
    format: String,
    session: String,
    unit: String,
    prometheus_sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdapterExit {
    format: String,
    session: String,
    unit: String,
    unit_empty: bool,
    leader_exited: bool,
    handles_closed: bool,
    outcome: String,
}
fn audit_child_fds() -> Result<()> {
    for (count, item) in fs::read_dir("/proc/self/fd")
        .map_err(|_| Error::Storage)?
        .enumerate()
    {
        if count >= 256 {
            return Err(Error::Quota);
        }
        let name = item.map_err(|_| Error::Storage)?.file_name();
        let fd: u32 = name
            .to_str()
            .ok_or(Error::Storage)?
            .parse()
            .map_err(|_| Error::Storage)?;
        if fd <= 2 {
            continue;
        }
        let mut f = match File::open(format!("/proc/self/fdinfo/{fd}")) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(Error::Storage),
        };
        let mut raw = String::new();
        Read::by_ref(&mut f)
            .take(4097)
            .read_to_string(&mut raw)
            .map_err(|_| Error::Storage)?;
        if raw.len() > 4096 {
            return Err(Error::Quota);
        }
        let flags = raw
            .lines()
            .find_map(|l| l.strip_prefix("flags:\t"))
            .ok_or(Error::Storage)?;
        let flags = u64::from_str_radix(flags.trim(), 8).map_err(|_| Error::Storage)?;
        if flags & 0o2000000 == 0 {
            return Err(Error::Storage);
        }
    }
    Ok(())
}
struct Adapter {
    child: Child,
    input: File,
    output: File,
    pending: Vec<u8>,
    exited: bool,
    stopped: bool,
}
impl Adapter {
    fn spawn(files: &Files, config_sha: Hash) -> Result<Self> {
        Self::spawn_fixed(
            files,
            config_sha,
            if files.disk { disk::ADAPTER } else { ADAPTER },
        )
    }
    fn spawn_fixed(files: &Files, config_sha: Hash, adapter: &str) -> Result<Self> {
        Self::spawn_fixed_observed(files, config_sha, adapter, &mut false)
    }
    fn spawn_fixed_observed(
        files: &Files,
        config_sha: Hash,
        adapter: &str,
        created: &mut bool,
    ) -> Result<Self> {
        audit_child_fds()?;
        let mut child = Command::new(PYTHON)
            .args(["-I", "-B", adapter])
            .arg(format!("--session={}", files.session))
            .arg(format!(
                "--config-sha256={}",
                hex::encode(config_sha.as_bytes())
            ))
            .env_clear()
            .env("LANG", "C.UTF-8")
            .current_dir(&files.directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::Storage)?;
        *created = true; // Actual OS Child exists; errors afterhere remain UNKNOWN.
        let stdin = child.stdin.take().ok_or(Error::Storage)?;
        let stdout = child.stdout.take().ok_or(Error::Storage)?;
        let open = |fd: i32, write: bool| -> Result<File> {
            let path = format!("/proc/self/fd/{fd}");
            let original = fs::metadata(&path).map_err(|_| Error::Storage)?;
            let f = OpenOptions::new()
                .read(!write)
                .write(write)
                .custom_flags(NONBLOCK | CLOEXEC)
                .open(path)
                .map_err(|_| Error::Storage)?;
            let m = f.metadata().map_err(|_| Error::Storage)?;
            if !m.file_type().is_fifo() || (m.dev(), m.ino()) != (original.dev(), original.ino()) {
                return Err(Error::Storage);
            }
            Ok(f)
        };
        let opened = (|| -> Result<_> {
            Ok((
                open(stdin.as_raw_fd(), true)?,
                open(stdout.as_raw_fd(), false)?,
            ))
        })();
        drop(stdin);
        drop(stdout);
        let (input, output) = match opened {
            Ok(v) => v,
            Err(error) => {
                // Original pipe EOF lets the fixed adapter retire its unit first.
                let end = Instant::now() + Duration::from_secs(10);
                while Instant::now() < end {
                    match child.try_wait() {
                        Ok(Some(_)) => return Err(error),
                        Ok(None) => {}
                        Err(_) => break,
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                let _ = child.kill();
                return Err(error);
            }
        };
        Ok(Self {
            child,
            input,
            output,
            pending: Vec::new(),
            exited: false,
            stopped: false,
        })
    }
    fn frame(&mut self) -> Result<Option<Value>> {
        let mut buf = [0u8; 1024];
        match self.output.read(&mut buf) {
            Ok(0) => return Err(Error::InvalidWire),
            Ok(n) => {
                self.pending.extend_from_slice(&buf[..n]);
                if self.pending.len() > 4096 {
                    return Err(Error::Quota);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(_) => return Err(Error::Storage),
        }
        if !self.pending.contains(&b'\n') {
            return Ok(None);
        }
        let value: Value = serde_json::from_slice(&self.pending).map_err(|_| Error::InvalidWire)?;
        if canonical(&value)? != self.pending {
            return Err(Error::InvalidWire);
        }
        self.pending.clear();
        Ok(Some(value))
    }
    fn stop_join(&mut self, files: &Files) -> bool {
        let end = Instant::now() + Duration::from_secs(10);
        let mut sent = 0;
        while sent < 5 && Instant::now() < end {
            match self.input.write(&b"STOP\n"[sent..]) {
                Ok(0) => break,
                Ok(n) => sent += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(_) => break,
            }
        }
        let mut observed = false;
        while Instant::now() < end {
            if !observed {
                match self.frame() {
                    Ok(Some(v)) => {
                        if let Ok(v) = decode::<AdapterExit>(v) {
                            observed = v.format
                                == (if files.disk {
                                    "cc-disk-collector-exit/v1"
                                } else {
                                    "cc-volatile-collector-exit/v1"
                                })
                                && v.session == files.session
                                && v.unit == files.unit
                                && v.unit_empty
                                && v.leader_exited
                                && v.handles_closed
                                && matches!(v.outcome.as_str(), "stopped" | "unknown");
                            self.stopped = observed && v.outcome == "stopped";
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.exited = true;
                    return observed && matches!(status.code(), Some(0 | 1));
                }
                Ok(None) => {}
                Err(_) => return false,
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = self.child.kill();
        false
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        if !self.exited {
            let _ = self.child.kill();
        }
    }
}

fn reply_for(pipe: &mut Pipe, clock: &mut Clock, frame: &Envelope, payload: Value) -> Result<()> {
    pipe.write(
        &json!({"format":FORMAT,"requestId":frame.request_id,"kind":frame.kind,"payload":payload}),
        clock,
    )
}
fn private_value(value: &Value) -> Result<Vec<u8>> {
    json_body(value)
}
fn original_config(config_raw: &[u8], pin_path: &Path, pin_raw: &[u8]) -> Result<()> {
    if root_file(Path::new(CONFIG))? != config_raw || root_file(pin_path)? != pin_raw {
        return Err(Error::Storage);
    }
    Ok(())
}
fn collect(
    mut session: Session<SqliteJournalStore>,
    mut pipe: Pipe,
    mut clock: Clock,
    frame: Envelope,
    config_raw: Vec<u8>,
    pin_path: PathBuf,
    pin_raw: Vec<u8>,
) -> Result<()> {
    let catalog = session.catalog.clone().ok_or(Error::CapabilityDenied)?;
    let request: StartCollector = decode(frame.payload.clone())?;
    if session.phase != Phase::Probed
        || frame.kind != "startCollector"
        || !session.seen.insert(frame.request_id.clone())
    {
        return Err(Error::WrongState);
    }
    session.expect_request(request.expected_rev, &request.expected_pin)?;
    let state = session.checked(clock.now()?)?;
    let cert = state.certificate(session.agent).ok_or(Error::NotFound)?;
    if state.revoked(session.agent)
        || cert != &request.binding.certificate.body
        || cert.agent_key != session.node.public_key
        || cert.key_epoch != session.node.key_epoch
        || cert.scopes.0.as_slice() != [Scope::MetricsCollect]
        || cert.capabilities.scopes != cert.scopes
        || request.collector_id != catalog.id
        || request.manifest_sha256 != catalog.hash
        || request.binding.operation
            != (Operation::MetricsCollect {
                collector: catalog.id,
            })
    {
        return Err(Error::WrongClaim);
    }
    let max_age = integer(
        &catalog.manifest["buffer"]["max_age_seconds"],
        1,
        if catalog.disk.is_some() { 7200 } else { 300 },
    )?
    .min(300);
    if request
        .binding
        .lease
        .body
        .expires_at
        .seconds()
        .checked_sub(request.binding.lease.body.issued_at.seconds())
        .filter(|v| *v > 0 && *v <= max_age)
        .is_none()
    {
        return Err(Error::Expired);
    }
    clock.cap(request.binding.lease.body.expires_at)?;
    clock.cap(request.binding.freshness.body.expires_at)?;
    clock.cap(cert.expires_at)?;
    clock.cap(state.owner().expires_at)?;
    // Kernel launch stamp BEFORE private startup files/I/O. It never renews the
    // signed authority or original Clock.end captured by the SSH worker.
    let created = boottime_ns()?;
    let boot = boot_id()?;
    let ticks = start_ticks()?;
    let expires = request
        .binding
        .lease
        .body
        .expires_at
        .min(request.binding.freshness.body.expires_at)
        .min(cert.expires_at)
        .min(state.owner().expires_at);
    let expires_ns = expires
        .seconds()
        .checked_mul(1_000_000_000)
        .ok_or(Error::InvalidTime)?;
    let wall_ns = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidTime)?
            .as_nanos(),
    )
    .map_err(|_| Error::InvalidTime)?;
    let remaining = expires_ns.checked_sub(wall_ns).ok_or(Error::Expired)?.min(
        u64::try_from(
            clock
                .end
                .checked_duration_since(Instant::now())
                .ok_or(Error::Expired)?
                .as_nanos(),
        )
        .map_err(|_| Error::InvalidTime)?,
    );
    if remaining < 12_000_000_000 || remaining > 300_000_000_000 {
        return Err(Error::Expired);
    }
    let deadline = created.checked_add(remaining).ok_or(Error::InvalidTime)?;
    let mut files = Files::new(catalog.disk.is_some())?;
    let scrape = catalog.scrape()?;
    files.write("scrape.json", &scrape)?;
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| Error::Storage)?;
    let status_port = reserved.local_addr().map_err(|_| Error::Storage)?.port();
    drop(reserved);
    if status_port < 1024 {
        return Err(Error::Storage);
    }
    let mut cfg_value = json!({"format":"cc-volatile-vmagent-session/v1","session":files.session,"unit":files.unit,
        "bootId":boot,"createdBoottimeNs":created,"deadlineBoottimeNs":deadline,"expiresUnixNs":expires_ns,
        "scrapeSha256":digest(&scrape),"receiver":catalog.manifest["receiver"]["url"],"statusPort":status_port,
        "ownerPid":std::process::id(),"ownerStartTicks":ticks,"manifestSha256":catalog.hash});
    if catalog.disk.is_none()&&catalog.manifest["schema_version"]==json!(2) {
        cfg_value["format"]=json!("cc-volatile-vmagent-session/v2");
        cfg_value["runtime"]=catalog.manifest["runtime"].clone();
    }
    if let Some(disk) = &catalog.disk {
        cfg_value["format"] = json!("cc-disk-prometheus-session/v1");
        cfg_value["binarySha256"] = json!(disk.binary_hash);
        cfg_value["binding"] = disk::binding(&session.expected_pin, &session.node, cert, &catalog)?;
        if catalog.manifest["schema_version"]==json!(2) {
            cfg_value["format"]=json!("cc-disk-prometheus-session/v2");
            cfg_value["runtime"]=catalog.manifest["runtime"].clone();
        }
    }
    let cfg = private_value(&cfg_value)?;
    files.write("session.json", &cfg)?;
    catalog.check()?;
    original_config(&config_raw, &pin_path, &pin_raw)?;
    session.checked(clock.now()?)?;
    let binding = request.binding;
    let operation = Operation::MetricsCollect {
        collector: catalog.id,
    };
    let proof = create_metrics_node_request(
        &session.node_dir,
        session.agent,
        &session.expected_pin,
        catalog.id,
        binding.clone(),
        clock.now()?,
    )?;
    catalog.check()?;
    original_config(&config_raw, &pin_path, &pin_raw)?;
    let store = session.store.take().ok_or(Error::WrongState)?;
    let mut actor = Actor::new(RevisionFence {
        inner: store,
        expected: request.expected_rev,
    })?;
    // The SAME underlying CAS commits real replay/nonce/lease BEFORE Permit.
    // No child or user-unit can start if any admission or pre-CAS check failed.
    let permit = actor.admit(
        binding.lease.clone(),
        proof,
        Some(&binding.freshness),
        &operation,
        clock.now()?,
    )?;
    actor.check_permit(&permit, clock.now()?)?;
    let lease_hash = binding.lease.body.digest()?;
    files.cleanup_allowed = false; // uncertain launched units retain their namespace
    let mut adapter = Adapter::spawn(&files, digest(&cfg))?;
    let launch_end = Instant::now() + Duration::from_secs(10);
    let live =
        |actor: &mut Actor<RevisionFence<SqliteJournalStore>>, clock: &mut Clock| -> Result<()> {
            actor.check_permit(&permit, clock.now()?)?;
            catalog.check()?;
            original_config(&config_raw, &pin_path, &pin_raw)?;
            if inspect_existing_node_identity(&session.node_dir, session.agent)? != session.node {
                return Err(Error::WrongClaim);
            }
            if boottime_ns()? >= deadline {
                return Err(Error::Expired);
            }
            Ok(())
        };
    let action = (|| -> Result<bool> {
        loop {
            live(&mut actor, &mut clock)?;
            pipe.check()?;
            if Instant::now() >= launch_end {
                return Err(Error::Expired);
            }
            if let Some(v) = adapter.frame()? {
                if let Some(disk) = &catalog.disk {
                    let ready: DiskAdapterReady = decode(v)?;
                    if ready.format != "cc-disk-collector-ready/v1"
                        || ready.session != files.session
                        || ready.unit != files.unit
                        || ready.prometheus_sha256 != hex::encode(disk.binary_hash.as_bytes())
                    {
                        return Err(Error::WrongClaim);
                    }
                } else {
                    let ready: AdapterReady = decode(v)?;
                    if ready.format != "cc-volatile-collector-ready/v1"
                        || ready.session != files.session
                        || ready.unit != files.unit
                        || ready.vmagent_sha256 != catalog.manifest.get("runtime").and_then(|v|v["binarySha256"].as_str()).unwrap_or(VMAGENT_SHA)
                    {
                        return Err(Error::WrongClaim);
                    }
                }
                break;
            }
            if adapter
                .child
                .try_wait()
                .map_err(|_| Error::Storage)?
                .is_some()
            {
                adapter.exited = true;
                return Err(Error::WrongState);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        live(&mut actor, &mut clock)?;
        reply_for(
            &mut pipe,
            &mut clock,
            &frame,
            json!({"node":session.node,"actorRevision":actor.snapshot()?.revision(),
            "collectorId":catalog.id,"manifestSha256":catalog.hash,"leaseHash":lease_hash,
            "expiresAt":expires,"state":"running","lossUnknown":true}),
        )?;
        let mut raw = Zeroizing::new(Vec::new());
        let mut partial_end = None;
        let mut check_at = Instant::now();
        loop {
            pipe.check()?;
            clock.now()?;
            if boottime_ns()? >= deadline {
                return Err(Error::Expired);
            }
            if Instant::now() >= check_at {
                live(&mut actor, &mut clock)?;
                check_at = Instant::now() + Duration::from_millis(100);
            }
            if partial_end.is_some_and(|end| Instant::now() >= end) {
                return Err(Error::Expired);
            }
            let mut buf = [0u8; 1024];
            match pipe.input.read(&mut buf) {
                Ok(0) => return Err(Error::InvalidWire),
                Ok(n) => {
                    if raw.is_empty() {
                        partial_end = Some(Instant::now() + Duration::from_secs(5));
                    }
                    raw.extend_from_slice(&buf[..n]);
                    if raw.len() > MAX_WIRE_BYTES {
                        return Err(Error::Quota);
                    }
                    if raw.contains(&b'\n') {
                        let stop = parse(&raw)?;
                        let payload: StopCollector = decode(stop.payload)?;
                        if stop.kind != "stopCollector"
                            || !session.seen.insert(stop.request_id)
                            || payload.collector_id != catalog.id
                            || payload.lease_hash != lease_hash
                        {
                            return Err(Error::WrongClaim);
                        }
                        live(&mut actor, &mut clock)?;
                        return Ok(true);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(Error::Storage),
            }
            if adapter
                .child
                .try_wait()
                .map_err(|_| Error::Storage)?
                .is_some()
            {
                adapter.exited = true;
                return Err(Error::WrongState);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    let cleanup = adapter.stop_join(&files);
    if cleanup {
        files.cleanup_allowed = true;
    }
    let removed = cleanup && files.cleanup().is_ok();
    // Cleanup observation may be emitted after permission expiry; it authorizes
    // no more DATA and cannot re-enable/renew/adopt a collector or Permit.
    let terminal = canonical(
        &json!({"format":"cc-agent-collector-result/v1","collectorId":catalog.id,"leaseHash":lease_hash,
        "state":if action==Ok(true)&&removed&&adapter.stopped {"stopped"}else{"unknown"},"cleanupObserved":removed,"lossUnknown":true}),
    )?;
    pipe.check()?;
    pipe.output
        .write_all(&terminal)
        .map_err(|_| Error::Storage)?;
    if action == Ok(true) && removed && adapter.stopped {
        Ok(())
    } else {
        Err(action.err().unwrap_or(Error::UncertainCommit))
    }
}

pub(super) fn run(mut pipe: Pipe, mut clock: Clock, probe: Envelope) -> Result<()> {
    if !matches!(
        probe.kind.as_str(),
        "probeMetrics"
            | "probeMetricsRevocation"
            | "probeCollectorService"
            | "probeCollectorServiceLifetime"
            | "probeCollectorServiceStatus"
    ) || probe.payload != json!({})
    {
        return Err(Error::InvalidWire);
    }
    let revoking = matches!(
        probe.kind.as_str(),
        "probeMetricsRevocation" | "probeCollectorServiceStatus"
    );
    let catalog = if revoking {
        None
    } else {
        match Catalog::load() {
            Ok(v) => Some(v),
            Err(error) => {
                let _ = pipe.write(
                    &json!({"format":FORMAT,"requestId":probe.request_id,"kind":"error",
            "payload":{"code":"collectorUnavailable","effect":"notCommitted"}}),
                    &mut clock,
                );
                return Err(error);
            }
        }
    };
    let setup = (|| -> Result<_> {
        let config_raw = root_file(Path::new(CONFIG))?;
        let config: Config = serde_json::from_slice(&config_raw).map_err(|_| Error::InvalidWire)?;
        if config.format != "cc-agent-enroll-config/v1" {
            return Err(Error::InvalidWire);
        }
        let paths = [&config.node_dir, &config.db_dir, &config.anchor_dir];
        for (i, p) in paths.iter().enumerate() {
            crate::durable::absolute(p)?;
            crate::durable::secure_path(p)?;
            if paths
                .iter()
                .enumerate()
                .any(|(j, q)| i != j && (p.starts_with(q) || q.starts_with(p)))
                || config.owner_pin.starts_with(p)
                || !revoking
                    && (p.starts_with(SESSION_ROOT)
                        || Path::new(SESSION_ROOT).starts_with(p)
                        || p.starts_with(disk::ROOT)
                        || Path::new(disk::ROOT).starts_with(p)
                        || p.starts_with(disk::STORE)
                        || Path::new(disk::STORE).starts_with(p)
                        || p.starts_with(
                            "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector",
                        )
                        || Path::new(
                            "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector",
                        )
                        .starts_with(p))
            {
                return Err(Error::Storage);
            }
        }
        let pin_raw = root_file(&config.owner_pin)?;
        let genesis: OwnerPin = serde_json::from_slice(&pin_raw).map_err(|_| Error::InvalidWire)?;
        let store = SqliteJournalStore::open(
            &config.db_dir,
            &config.anchor_dir,
            genesis,
            SqliteStorageBudget::default(),
        )?;
        let state = Actor::new(store)?.into_store(); // Real replay/validation, no create.
        let snapshot = state.load()?;
        let node = inspect_existing_node_identity(&config.node_dir, config.agent_id)?;
        let session = Session {
            store: Some(state),
            node_dir: config.node_dir.clone(),
            agent: config.agent_id,
            node,
            phase: Phase::New,
            seen: BTreeSet::new(),
            expected_pin: snapshot.owner().clone(),
            expected_rev: snapshot.revision(),
            claim: None,
            certificate: None,
            offer_expires: None,
            effects: false,
            catalog,
        };
        Ok((config, config_raw, pin_raw, session))
    })();
    let (config, config_raw, pin_raw, mut session) = match setup {
        Ok(value) => value,
        Err(error) => {
            let _ = pipe.write(
                &json!({"format":FORMAT,"requestId":probe.request_id,
                "kind":"error","payload":{"code":"bootstrapUnenrolled","effect":"notCommitted"}}),
                &mut clock,
            );
            return Err(error);
        }
    };
    clock.cap(session.expected_pin.expires_at)?;
    let lifetime_protocol=probe.kind=="probeCollectorServiceLifetime";
    let mut first = Some(probe);
    for _ in 0..4 {
        if root_file(Path::new(CONFIG))? != config_raw || root_file(&config.owner_pin)? != pin_raw {
            return Err(Error::Storage);
        }
        let frame = match first.take() {
            Some(value) => value,
            None => parse(&pipe.read(&mut clock)?)?,
        };
        // Waiting for the next human-reviewed request cannot bypass a changed
        // fixed role configuration or installed genesis pin.
        if root_file(Path::new(CONFIG))? != config_raw || root_file(&config.owner_pin)? != pin_raw {
            return Err(Error::Storage);
        }
        let id = frame.request_id.clone();
        if session.phase == Phase::RevocationProbed
            && matches!(
                frame.kind.as_str(),
                "observeCollectorService" | "revokeCollectorService"
            )
        {
            return service_control::manage(
                session,
                pipe,
                clock,
                frame,
                config_raw,
                config.owner_pin,
                pin_raw,
            );
        }
        if session.phase == Phase::Probed && frame.kind == "configureCollectorService" {
            return service_control::configure(
                session,
                pipe,
                clock,
                frame,
                config_raw,
                config.owner_pin,
                pin_raw,
                lifetime_protocol,
            );
        }
        if session.phase == Phase::Probed && frame.kind == "startCollector" {
            return collect(
                session,
                pipe,
                clock,
                frame,
                config_raw,
                config.owner_pin,
                pin_raw,
            );
        }
        match session.request(frame, &mut clock) {
            Ok(reply) => {
                if root_file(Path::new(CONFIG))? != config_raw
                    || root_file(&config.owner_pin)? != pin_raw
                {
                    return Err(Error::Storage);
                }
                session.checked(clock.now()?)?;
                pipe.write(&reply, &mut clock)?;
                if session.phase == Phase::Closed {
                    break;
                }
            }
            Err(error) => {
                let code = match error {
                    Error::Expired | Error::InvalidTime => "expired",
                    Error::Conflict => "contextChanged",
                    Error::Storage => "storageUnavailable",
                    Error::UncertainCommit => "uncertainCommit",
                    Error::Quota => "capacity",
                    Error::InvalidWire => "invalidFrame",
                    Error::WrongState => "wrongState",
                    _ => "denied",
                };
                let _ = pipe.write(
                    &json!({"format":FORMAT,"requestId":id,"kind":"error","payload":{
                    "code":code,"effect":if session.effects {"unknown"}else{"notCommitted"}}}),
                    &mut clock,
                );
                return Err(error);
            }
        }
    }
    if session.phase != Phase::Closed {
        return Err(Error::WrongState);
    }
    Ok(())
}

#[cfg(test)]
#[path = "metrics/tests.rs"]
mod tests;

#[path = "metrics/service_credentials.rs"]
mod service_credentials;

#[path = "metrics/service_control.rs"]
mod service_control;

pub(crate) use service_credentials::{RegisteredCredentialWriter, RegisteredServiceNamespace};

#[path = "metrics/service_supervisor.rs"]
pub(crate) mod service_supervisor;

#[cfg(test)]
#[path = "metrics/service_files_tests.rs"]
mod service_files_tests;

#[path = "metrics/service_credential_handoff.rs"]
mod service_credential_handoff;

#[path = "metrics/credential_capsules.rs"]
mod credential_capsules;
#[path = "metrics/service_material.rs"]
mod service_material;

#[path = "metrics/service_handoff_control.rs"]
mod service_handoff_control;

#[path="metrics/service_lifetime_control.rs"]
pub(crate) mod service_lifetime_control;
#[path="metrics/service_lifetime_material.rs"]
mod service_lifetime_material;
