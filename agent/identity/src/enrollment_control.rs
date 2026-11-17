//! Fixed admin-installed SSH role. Stdio JSON is authenticated ONLY by its
//! original pinned SSH channel; it is not a portable receipt or permission.
use crate::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

mod rotation;
pub(crate) mod metrics;
mod tunnel_transport;
mod exporters;
const FORMAT: &str = "cc-native-node-enroll/v1";
const CONFIG: &str = "/etc/consolecrypt-agent/enrollment-control.json";
const NONBLOCK: i32 = rustix::fs::OFlags::NONBLOCK.bits() as i32;
const CLOEXEC: i32 = rustix::fs::OFlags::CLOEXEC.bits() as i32;
const NOFOLLOW: i32 = rustix::fs::OFlags::NOFOLLOW.bits() as i32;

// Private RPC-revision CAS fence. A mutation which reloaded a newer State must
// fail BEFORE forwarding to the SAME store, without policy/journal changes.
struct RevisionFence<S> { inner: S, expected: u64 }
impl<S: CompareAndSwap> CompareAndSwap for RevisionFence<S> {
    fn load(&self) -> Result<State> { self.inner.load() }
    fn compare_and_swap(&mut self, expected: u64, next: State) -> Result<()> {
        if expected != self.expected { return Err(Error::Conflict); }
        self.inner.compare_and_swap(expected, next)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config { format: String, agent_id: AgentId, node_dir: PathBuf,
    db_dir: PathBuf, anchor_dir: PathBuf, owner_pin: PathBuf }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope { format: String, request_id: String, kind: String, payload: Value }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StageOffer { expected_actor_revision: u64, expected_owner_pin: OwnerPin,
    offer: Signed<Offer>, #[serde(deserialize_with="secret_string")] token: Zeroizing<String> }
fn secret_string<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Zeroizing<String>, D::Error> {
    String::deserialize(d).map(Zeroizing::new)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyCertificate { expected_rev: u64, expected_pin: OwnerPin,
    claim_digest: Hash, certificate: Signed<Certificate> }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObserveCertificate { expected_rev: u64, expected_pin: OwnerPin,
    agent_id: AgentId, certificate_digest: Hash }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevokeCertificate { expected_rev: u64, expected_pin: OwnerPin,
    revocation: Signed<Revocation> }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObserveRevocation { expected_rev: u64, expected_pin: OwnerPin,
    agent_id: AgentId, key_epoch: u64, certificate_digest: Hash }

fn canonical(value: &Value) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    crate::wire::canonical_json(value, &mut raw)?; // Also rejects floats.
    raw.push(b'\n');
    if !raw.is_ascii() || raw.len() > MAX_WIRE_BYTES { return Err(Error::InvalidWire); }
    Ok(raw)
}
fn parse(raw: &[u8]) -> Result<Envelope> {
    if raw.is_empty() || raw.len() > MAX_WIRE_BYTES || raw.contains(&b'\r')
        || !raw.is_ascii() || raw.last() != Some(&b'\n') {
        return Err(Error::InvalidWire);
    }
    let v: Value = serde_json::from_slice(raw).map_err(|_| Error::InvalidWire)?;
    // Duplicate keys cannot equal their re-encoding, including nested Signed<T>.
    let recoded = Zeroizing::new(canonical(&v)?);
    if recoded.as_slice() != raw { return Err(Error::InvalidWire); }
    let frame: Envelope = serde_json::from_value(v).map_err(|_| Error::InvalidWire)?;
    let id = uuid::Uuid::parse_str(&frame.request_id).map_err(|_| Error::InvalidWire)?;
    if frame.format != FORMAT || id.is_nil() || id.to_string() != frame.request_id {
        return Err(Error::InvalidWire);
    }
    Ok(frame)
}
fn decode<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| Error::InvalidWire)
}
fn token(text: &str) -> Result<InvitationToken> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(Error::InvalidToken);
    }
    let mut bytes = Zeroizing::new([0u8; 32]);
    hex::decode_to_slice(text, bytes.as_mut()).map_err(|_| Error::InvalidToken)?;
    InvitationToken::from_bytes(*bytes)
}

fn root_file(path: &Path) -> Result<Vec<u8>> {
    root_file_with_cap(path, MAX_WIRE_BYTES)
}
// Physical installed scripts are not protocol frames. Only these four signed
// runtime paths may use the same 64KiB cap as the shipping provisioner.
fn root_service_script(path: &Path) -> Result<Vec<u8>> {
    match path.to_str() {
        Some("/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_unit_adapter.py"
            | "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_unit_entry.py"
            | "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_unit_spec.py"
            | "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_binding.py") => root_file_with_cap(path, 65536),
        _ => Err(Error::ScopeDenied),
    }
}
fn root_file_body(reader: &mut impl Read, cap: usize) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    reader.take(cap as u64 + 1).read_to_end(&mut raw).map_err(|_| Error::Storage)?;
    if raw.len() > cap { return Err(Error::Storage); }
    Ok(raw)
}
fn root_file_with_cap(path: &Path, cap: usize) -> Result<Vec<u8>> {
    crate::durable::absolute(path)?;
    for p in path.ancestors() {
        let m = fs::symlink_metadata(p).map_err(|_| Error::Storage)?;
        if m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err(Error::Storage);
        }
    }
    let mut f = OpenOptions::new().read(true).custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC)
        .open(path).map_err(|_| Error::Storage)?;
    let m = f.metadata().map_err(|_| Error::Storage)?;
    if !m.is_file() || m.nlink() != 1 || m.len() > cap as u64
        || m.uid() != 0 || m.mode() & 0o022 != 0 { return Err(Error::Storage); }
    let raw = root_file_body(&mut f, cap)?;
    let after = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    let held = f.metadata().map_err(|_| Error::Storage)?;
    let identity = |x: &fs::Metadata| (x.dev(), x.ino(), x.len(), x.nlink(), x.uid(), x.mode(),
        x.mtime(), x.mtime_nsec(), x.ctime(), x.ctime_nsec());
    if raw.len() > cap || identity(&m) != identity(&after)
        || identity(&m) != identity(&held) { return Err(Error::Storage); }
    Ok(raw)
}

struct Clock { end: Instant, last: Timestamp, expiry: Option<Timestamp> }
impl Clock {
    fn start() -> Result<Self> {
        let now = Self::observed()?;
        Ok(Self { end: Instant::now() + Duration::from_secs(300), last: now, expiry: None })
    }
    fn observed() -> Result<Timestamp> {
        Timestamp::new(SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidTime)?.as_secs())
    }
    fn now(&mut self) -> Result<Now> {
        let n = Self::observed()?;
        if Instant::now() >= self.end || n < self.last || self.expiry.is_some_and(|v| n>=v) {
            return Err(Error::Expired);
        }
        self.last = n;
        Ok(Now(n))
    }
    fn cap(&mut self, expiry: Timestamp) -> Result<()> {
        self.expiry=Some(self.expiry.map_or(expiry,|old|old.min(expiry)));
        self.now()?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { New, Probed, RevocationProbed, Claimed, Applied, Revoked, Closed }
struct Session<S> {
    store: Option<S>, node_dir: PathBuf, agent: AgentId, node: PublicNodeIdentity,
    phase: Phase, seen: BTreeSet<String>, expected_pin: OwnerPin, expected_rev: u64,
    claim: Option<Hash>, certificate: Option<Hash>, offer_expires: Option<Timestamp>,
    effects: bool,
}
impl<S: CompareAndSwap> Session<S> {
    fn snapshot(&self) -> Result<State> { self.store.as_ref().ok_or(Error::WrongState)?.load() }
    fn checked(&self, now: Now) -> Result<State> {
        let state = self.snapshot()?;
        if state.owner() != &self.expected_pin || state.revision() != self.expected_rev {
            return Err(Error::Conflict);
        }
        if now.0 >= state.owner().expires_at
            || self.offer_expires.is_some_and(|x| now.0 >= x) { return Err(Error::Expired); }
        if inspect_existing_node_identity(&self.node_dir, self.agent)? != self.node {
            return Err(Error::WrongClaim);
        }
        Ok(state)
    }
    fn mutate<F>(&mut self, revision: u64, effect: F) -> Result<()>
    where F: FnOnce(&mut Actor<RevisionFence<S>>) -> Result<()> {
        self.effects = true; // Any ambiguity after this is terminal UNKNOWN.
        let inner = self.store.take().ok_or(Error::WrongState)?;
        let mut actor = Actor::new(RevisionFence { inner, expected: revision })?;
        let result = effect(&mut actor);
        self.store = Some(actor.into_store().inner);
        result
    }
    fn expect_request(&self, revision: u64, pin: &OwnerPin) -> Result<()> {
        if revision != self.expected_rev || pin != &self.expected_pin { return Err(Error::Conflict); }
        Ok(())
    }
    fn readback(&self, now: Now) -> Result<Value> {
        let state = self.checked(now)?;
        let certificate = state.certificate(self.agent).ok_or(Error::NotFound)?;
        if state.revoked(self.agent) || certificate.agent_key != self.node.public_key
            || certificate.key_epoch != self.node.key_epoch || now.0 < certificate.not_before
            || now.0 >= certificate.expires_at
            || Some(certificate.digest()?) != self.certificate { return Err(Error::WrongClaim); }
        Ok(json!({"certificate":certificate,"certificateDigest":certificate.digest()?,
            "nodeFingerprint":self.node.key_fingerprint,"ownerPin":state.owner(),
            "actorRevision":state.revision(),"revoked":false}))
    }
    fn revoked_readback(&self, now: Now) -> Result<Value> {
        let state = self.checked(now)?;
        let cert = state.certificate(self.agent).ok_or(Error::NotFound)?;
        // No certificate-live check: a CURRENT expired key must be tombstonable.
        if !state.revoked(self.agent) || cert.agent_key != self.node.public_key
            || cert.key_epoch != self.node.key_epoch || Some(cert.digest()?) != self.certificate {
            return Err(Error::WrongClaim);
        }
        Ok(json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
            "certificateDigest":cert.digest()?,"revoked":true}))
    }
    fn request(&mut self, frame: Envelope, clock: &mut Clock) -> Result<Value> {
        if self.seen.len() >= 4 || !self.seen.insert(frame.request_id.clone()) {
            return Err(Error::Replay);
        }
        let now = clock.now()?;
        self.checked(now)?;
        let payload = match (self.phase, frame.kind.as_str()) {
            (Phase::New, "probe") => {
                let _: Empty = decode(frame.payload)?;
                let state = self.checked(clock.now()?)?;
                self.phase = Phase::Probed;
                json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
                    "certificate":state.certificate(self.agent),"revoked":state.revoked(self.agent)})
            }
            (Phase::New, "probeRevocation") => {
                let _: Empty = decode(frame.payload)?;
                let state = self.checked(clock.now()?)?;
                self.phase = Phase::RevocationProbed;
                json!({"node":self.node,"ownerPin":state.owner(),"actorRevision":state.revision(),
                    "certificate":state.certificate(self.agent),"revoked":state.revoked(self.agent)})
            }
            (Phase::RevocationProbed, "revokeCertificate") => {
                let input: RevokeCertificate = decode(frame.payload)?;
                self.expect_request(input.expected_rev, &input.expected_pin)?;
                let state = self.checked(clock.now()?)?;
                let current = state.certificate(self.agent).ok_or(Error::NotFound)?;
                let revoke = &input.revocation.body;
                if state.revoked(self.agent) || current.agent_key != self.node.public_key
                    || current.key_epoch != self.node.key_epoch
                    || current.scopes.0.as_slice() != [Scope::TunnelOpen]
                    || current.capabilities.scopes != current.scopes
                    || current.header.owner_epoch != state.owner().epoch
                    || revoke.agent_id != self.agent || revoke.key_epoch != current.key_epoch
                    || revoke.certificate_hash != current.digest()? { return Err(Error::WrongClaim); }
                let digest = current.digest()?;
                let revision = self.expected_rev.checked_add(1).ok_or(Error::Quota)?;
                let checkpoint = self.expected_pin.checkpoint.checked_add(1).ok_or(Error::Quota)?;
                self.mutate(self.expected_rev, |actor| actor.revoke(input.revocation, clock.now()?))?;
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
                if input.agent_id != self.agent || input.key_epoch != self.node.key_epoch
                    || Some(input.certificate_digest) != self.certificate { return Err(Error::WrongClaim); }
                let value = self.revoked_readback(clock.now()?)?;
                self.phase = Phase::Closed;
                value
            }
            (Phase::Probed, "stageOffer") => {
                let input: StageOffer = decode(frame.payload)?;
                self.expect_request(input.expected_actor_revision, &input.expected_owner_pin)?;
                let state = self.checked(clock.now()?)?;
                if state.certificate(self.agent).is_some() || self.node.key_epoch != 1
                    || input.offer.body.agent_id != self.agent
                    || input.offer.body.agent_key != self.node.public_key
                    || input.offer.body.scopes.0.as_slice() != [Scope::TunnelOpen] {
                    return Err(Error::WrongClaim);
                }
                let secret = token(input.token.as_str())?;
                if !bool::from(secret.digest().as_bytes().ct_eq(input.offer.body.token_hash.as_bytes())) {
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
                let claim = create_tunnel_node_claim(&self.node_dir, self.agent, &pre_pin,
                    offer.clone(), clock.now()?)?;
                let digest = claim.body.digest()?;
                self.checked(clock.now()?)?;
                let consumed = claim.clone();
                self.mutate(offered_rev, |a| {
                    if a.consume(&secret, consumed, clock.now()?)? != digest { return Err(Error::WrongClaim); }
                    Ok(())
                })?;
                self.expected_rev = claimed_rev;
                let state = self.checked(clock.now()?)?;
                if state.invitation_status(offer.body.invitation_id) != Some(InvitationStatus::Claimed) {
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
                if self.claim != Some(input.claim_digest) || cert.agent_id != self.agent
                    || cert.agent_key != self.node.public_key || cert.key_epoch != 1
                    || !matches!(&cert.origin, CertificateOrigin::Enrollment { claim_hash, .. }
                        if Some(*claim_hash) == self.claim) { return Err(Error::WrongClaim); }
                let digest = cert.digest()?;
                let revision = self.expected_rev.checked_add(1).ok_or(Error::Quota)?;
                let checkpoint = self.expected_pin.checkpoint.checked_add(1).ok_or(Error::Quota)?;
                self.checked(clock.now()?)?;
                self.mutate(self.expected_rev, |a| a.approve(input.certificate, clock.now()?))?;
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
                if input.agent_id != self.agent || self.certificate != Some(input.certificate_digest) {
                    return Err(Error::WrongClaim);
                }
                let v = self.readback(clock.now()?)?;
                self.phase = Phase::Closed;
                v
            }
            _ => return Err(Error::WrongState),
        };
        self.checked(clock.now()?)?;
        Ok(json!({"format":FORMAT,"requestId":frame.request_id,"kind":frame.kind,"payload":payload}))
    }
}

struct Pipe { input: File, output: File, input_id: (u64,u64), output_id: (u64,u64) }
impl Pipe {
    fn identity(fd: u8) -> Result<(u64,u64)> {
        let m = fs::metadata(format!("/proc/self/fd/{fd}")).map_err(|_| Error::Storage)?;
        if !(m.file_type().is_fifo()||m.file_type().is_socket()) || m.ino() == 0 { return Err(Error::Storage); }
        Ok((m.dev(), m.ino()))
    }
    fn open() -> Result<Self> {
        let input_id = Self::identity(0)?; let output_id = Self::identity(1)?;
        let input_socket=fs::metadata("/proc/self/fd/0").map_err(|_|Error::Storage)?.file_type().is_socket();
        let output_socket=fs::metadata("/proc/self/fd/1").map_err(|_|Error::Storage)?.file_type().is_socket();
        if input_socket!=output_socket||input_id==output_id&&!input_socket {return Err(Error::Storage)}
        if input_socket {
            use std::os::fd::AsFd;
            let input=File::from(std::io::stdin().as_fd().try_clone_to_owned().map_err(|_|Error::Storage)?);
            let output=File::from(std::io::stdout().as_fd().try_clone_to_owned().map_err(|_|Error::Storage)?);
            for file in [&input,&output] {
                let flags=rustix::fs::fcntl_getfl(file).map_err(|_|Error::Storage)?;
                rustix::fs::fcntl_setfl(file,flags|rustix::fs::OFlags::NONBLOCK).map_err(|_|Error::Storage)?;
            }
            let pipe=Self{input,output,input_id,output_id};pipe.check()?;return Ok(pipe)
        }
        let input = OpenOptions::new().read(true).custom_flags(NONBLOCK|CLOEXEC)
            .open("/proc/self/fd/0").map_err(|_| Error::Storage)?;
        let output = OpenOptions::new().write(true).custom_flags(NONBLOCK|CLOEXEC)
            .open("/proc/self/fd/1").map_err(|_| Error::Storage)?;
        let pipe = Self { input, output, input_id, output_id }; pipe.check()?; Ok(pipe)
    }
    fn check(&self) -> Result<()> {
        for (fd,file,id) in [(0,&self.input,self.input_id),(1,&self.output,self.output_id)] {
            let m=file.metadata().map_err(|_| Error::Storage)?;
            if Self::identity(fd)? != id || !(m.file_type().is_fifo()||m.file_type().is_socket()) || (m.dev(),m.ino()) != id {
                return Err(Error::Storage);
            }
        }
        Ok(())
    }
    fn read(&mut self, clock: &mut Clock) -> Result<Zeroizing<Vec<u8>>> {
        let mut raw=Zeroizing::new(Vec::new());
        let mut end=Instant::now()+Duration::from_secs(60);
        let mut block=Zeroizing::new([0u8;1024]);
        loop {
            self.check()?; clock.now()?;
            if Instant::now()>=end { return Err(Error::Expired); }
            match self.input.read(block.as_mut()) {
                Ok(0)=>return Err(Error::InvalidWire),
                Ok(n)=>{
                    if raw.is_empty() { end=Instant::now()+Duration::from_secs(5); }
                    raw.extend_from_slice(&block[..n]);
                    if raw.len()>MAX_WIRE_BYTES { return Err(Error::Quota); }
                    if raw.contains(&b'\n') { return Ok(raw); }
                }
                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(2)),
                Err(_)=>return Err(Error::Storage),
            }
        }
    }
    fn write(&mut self, value: &Value, clock: &mut Clock) -> Result<()> {
        let raw=canonical(value)?; let mut offset=0;
        let end=Instant::now()+Duration::from_secs(5);
        while offset<raw.len() {
            self.check()?;clock.now()?;
            if Instant::now()>=end { return Err(Error::Expired); }
            match self.output.write(&raw[offset..]) {
                Ok(0)=>return Err(Error::Storage), Ok(n)=>offset+=n,
                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(2)),
                Err(_)=>return Err(Error::Storage),
            }
        }
        self.check()?;clock.now()?;Ok(())
    }
}

/// Fixed entrypoint only. Admin configuration is not remote wire authority.
pub fn run_enrollment_control_worker(args: &[String]) -> Result<()> {
    if args.len()!=1 || args[0]!="worker-node-enroll-sqlite" { return Err(Error::InvalidWire); }
    let mut pipe=Pipe::open()?;let mut clock=Clock::start()?;
    let raw=pipe.read(&mut clock)?;
    let probe=parse(&raw)?;
    if matches!(probe.kind.as_str(),"probeExporters"|"probeExportersRevocation") {return exporters::run(pipe,clock,probe)}
    if probe.kind=="probeTunnelTransport" { return tunnel_transport::run(pipe,clock,probe); }
    if matches!(probe.kind.as_str(),"probeMetrics"|"probeMetricsRevocation"|"probeCollectorService" | "probeCollectorServiceLifetime" | "probeCollectorServiceStatus"){return metrics::run(pipe,clock,probe)}
    if matches!(probe.kind.as_str(),"probeRotation"|"probeRotationReconcile") { return rotation::run(pipe,clock,probe); }
    if !matches!(probe.kind.as_str(),"probe"|"probeRevocation") || probe.payload!=json!({}) { return Err(Error::InvalidWire); }
    let setup=(|| -> Result<_> {
    let config_raw=root_file(Path::new(CONFIG))?;
    let config: Config=serde_json::from_slice(&config_raw).map_err(|_| Error::InvalidWire)?;
    if config.format!="cc-agent-enroll-config/v1" { return Err(Error::InvalidWire); }
    let paths=[&config.node_dir,&config.db_dir,&config.anchor_dir];
    for (i,p) in paths.iter().enumerate() {
        crate::durable::absolute(p)?;crate::durable::secure_path(p)?;
        if paths.iter().enumerate().any(|(j,q)|i!=j && (p.starts_with(q)||q.starts_with(p)))
            || config.owner_pin.starts_with(p) { return Err(Error::Storage); }
    }
    let pin_raw=root_file(&config.owner_pin)?;
    let genesis: OwnerPin=serde_json::from_slice(&pin_raw).map_err(|_| Error::InvalidWire)?;
    let store=SqliteJournalStore::open(&config.db_dir,&config.anchor_dir,genesis,SqliteStorageBudget::default())?;
    let state=Actor::new(store)?.into_store(); // Real replay/validation, no create.
    let snapshot=state.load()?;
    let node=inspect_existing_node_identity(&config.node_dir,config.agent_id)?;
    let session=Session { store:Some(state),node_dir:config.node_dir.clone(),agent:config.agent_id,node,
        phase:Phase::New,seen:BTreeSet::new(),expected_pin:snapshot.owner().clone(),expected_rev:snapshot.revision(),
        claim:None,certificate:None,offer_expires:None,effects:false };
    Ok((config,config_raw,pin_raw,session))
    })();
    let (config,config_raw,pin_raw,mut session)=match setup {
        Ok(value)=>value,
        Err(error)=>{
            let _=pipe.write(&json!({"format":FORMAT,"requestId":probe.request_id,
                "kind":"error","payload":{"code":"bootstrapUnenrolled","effect":"notCommitted"}}),&mut clock);
            return Err(error);
        }
    };
    clock.cap(session.expected_pin.expires_at)?;
    let mut first=Some(probe);
    for _ in 0..4 {
        if root_file(Path::new(CONFIG))?!=config_raw || root_file(&config.owner_pin)?!=pin_raw { return Err(Error::Storage); }
        let frame=match first.take() {
            Some(value)=>value,
            None=>parse(&pipe.read(&mut clock)?)?,
        };
        // Waiting for the next human-reviewed request cannot bypass a changed
        // fixed role configuration or installed genesis pin.
        if root_file(Path::new(CONFIG))?!=config_raw || root_file(&config.owner_pin)?!=pin_raw { return Err(Error::Storage); }
        let id=frame.request_id.clone();
        match session.request(frame,&mut clock) {
            Ok(reply)=>{
                if root_file(Path::new(CONFIG))?!=config_raw || root_file(&config.owner_pin)?!=pin_raw { return Err(Error::Storage); }
                session.checked(clock.now()?)?;
                pipe.write(&reply,&mut clock)?;
                if session.phase == Phase::Closed { break; }
            },
            Err(error)=>{
                let code=match error { Error::Expired|Error::InvalidTime=>"expired",Error::Conflict=>"contextChanged",
                    Error::Storage=>"storageUnavailable",Error::UncertainCommit=>"uncertainCommit",Error::Quota=>"capacity",
                    Error::InvalidWire=>"invalidFrame",Error::WrongState=>"wrongState",_=>"denied" };
                let _=pipe.write(&json!({"format":FORMAT,"requestId":id,"kind":"error","payload":{
                    "code":code,"effect":if session.effects {"unknown"}else{"notCommitted"}}}),&mut clock);
                return Err(error);
            }
        }
    }
    if session.phase!=Phase::Closed { return Err(Error::WrongState); }
    Ok(())
}

#[cfg(test)]
#[path="enrollment_control/tests.rs"]
mod tests;

pub(crate) use metrics::{RegisteredCredentialWriter,RegisteredServiceNamespace};
pub fn run_managed_exporter_exec(args:&[String])->Result<()> {exporters::exec(args)}

pub(crate) use metrics::service_supervisor::run_fixed_collector_service;

#[cfg(test)]
mod service_script_cap_tests {
    use super::*;
    #[test]
    fn installed_service_entry_larger_than_wire_uses_only_fixed_script_cap() {
        let entry = include_bytes!("../../collector/service_unit_entry.py");
        assert!(entry.len() > MAX_WIRE_BYTES && entry.len() <= 65536);
        assert!(matches!(root_file_body(&mut std::io::Cursor::new(entry.as_slice()), MAX_WIRE_BYTES), Err(Error::Storage)));
        assert_eq!(root_file_body(&mut std::io::Cursor::new(entry.as_slice()), 65536).unwrap(), entry.to_vec());
        assert!(matches!(root_service_script(Path::new("/etc/consolecrypt-agent/enrollment-control.json")), Err(Error::ScopeDenied)));
    }
    #[test]
    fn script_body_bound_rejects_oversize_and_read_error_without_raising_wire_limit() {
        let exact = vec![b'x'; 65536];
        assert_eq!(root_file_body(&mut std::io::Cursor::new(&exact), 65536).unwrap(), exact);
        assert!(matches!(root_file_body(&mut std::io::Cursor::new(vec![b'x'; 65537]), 65536), Err(Error::Storage)));
        struct Denied;
        impl Read for Denied {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            }
        }
        assert!(matches!(root_file_body(&mut Denied, 65536), Err(Error::Storage)));
        assert_eq!(MAX_WIRE_BYTES, 16384);
    }
}
