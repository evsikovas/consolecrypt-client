//! Fixed installed V2 worker. Actual Child/target/pipes/threads remain Rust-owned.
//! Linux/Debian12 system Python3.11 subset ONLY; not Ubuntu24.04/Python3.12.
use crate::*;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;
#[path = "tunnel_supervisor/installed.rs"]
mod installed;
#[path = "tunnel_supervisor/pipe.rs"]
mod pipe;
use installed::{InstalledChild, Pin};
use pipe::{ParentControl, Pipes};
#[path = "tunnel_supervisor/retirement.rs"]
mod retirement;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildConfig {
    format: String,
    policy: PathBuf,
    certificate: PathBuf,
    private_key: PathBuf,
    client_ca: PathBuf,
    relay_ca: PathBuf,
    relay_offer: PathBuf,
}
struct Fixed {
    pins: Vec<Pin>,
    config: PathBuf,
    node: PathBuf,
    node_facts: LocalNodeInspection,
    policy: TunnelRoutePolicyV2,
}
fn fixed_path_overlap(file: &Path, root: &Path) -> bool {
    file == root || file.starts_with(root) || root.starts_with(file)
}
fn uid() -> Result<u32> {
    let status = fs::read_to_string("/proc/self/status").map_err(|_| Error::Storage)?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .ok_or(Error::Storage)?
        .parse()
        .map_err(|_| Error::Storage)
}
fn load<T: serde::de::DeserializeOwned>(path: &Path, cap: u64) -> Result<(T, Pin)> {
    let (raw, pin) = installed::read_file(path, cap, false)?;
    let value = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
    Ok((value, pin))
}
impl Fixed {
    fn check(&self) -> Result<()> {
        for p in &self.pins {
            p.check()?;
        }
        self.node_check()
    }
    fn node_check(&self) -> Result<()> {
        let node = inspect_node_state(&self.node, self.policy.route.agent_id)?;
        if node.facts() != &self.node_facts
            || node.facts().actual_selected_epoch != Some(self.policy.route.key_epoch)
        {
            return Err(Error::WrongEpoch);
        }
        node.recheck_guard_identity()
    }
}
struct Clock {
    wall: Duration,
    deadline: Instant,
    idle: Option<Instant>,
    idle_expires: Option<Timestamp>,
    origin_wall: Duration,
    origin_mono: Instant,
}
impl Clock {
    fn new(ttl: u64) -> Result<Self> {
        let origin = Instant::now();
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidTime)?;
        Ok(Self {
            wall,
            deadline: origin
                .checked_add(Duration::from_secs(ttl))
                .ok_or(Error::InvalidTime)?,
            idle: None,
            idle_expires: None,
            origin_wall: wall,
            origin_mono: origin,
        })
    }
    fn now(&mut self) -> Result<Now> {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidTime)?;
        if wall < self.wall || Instant::now() >= self.deadline {
            return Err(Error::Expired);
        }
        self.wall = wall;
        Ok(Now(Timestamp::new(wall.as_secs())?))
    }
    fn bind(&mut self, expires: Timestamp) -> Result<()> {
        self.now()?;
        let end = Duration::from_secs(expires.seconds())
            .checked_sub(self.origin_wall)
            .ok_or(Error::Expired)?;
        if end.is_zero() {
            return Err(Error::Expired);
        }
        if self.wall >= Duration::from_secs(expires.seconds()) {
            return Err(Error::Expired);
        }
        self.deadline = self.deadline.min(
            self.origin_mono
                .checked_add(end)
                .ok_or(Error::InvalidTime)?,
        );
        Ok(())
    }
    fn bind_idle(&mut self, expires: Timestamp) -> Result<()> {
        self.now()?;
        let bound = Duration::from_secs(expires.seconds());
        if self.wall >= bound {
            return Err(Error::Expired);
        }
        let end = self
            .origin_mono
            .checked_add(bound.checked_sub(self.origin_wall).ok_or(Error::Expired)?)
            .ok_or(Error::InvalidTime)?;
        if Instant::now() >= end {
            return Err(Error::Expired);
        }
        // Same Actor timestamp maps to the SAME monotonic deadline. Control
        // observations cannot renew it; only committed DATA can change it.
        self.idle = Some(end.min(self.deadline));
        self.idle_expires = Some(expires);
        Ok(())
    }
    fn idle_expires(&self) -> Result<Timestamp> {
        self.idle_expires.ok_or(Error::WrongState)
    }
    fn io(&self) -> Instant {
        self.deadline
            .min(self.idle.unwrap_or(self.deadline))
            .min(Instant::now() + Duration::from_secs(5))
    }
}
fn check_live<S:CompareAndSwap>(
    actor: &mut Actor<S>,
    permit: &Permit,
    fixed: &Fixed,
    installed: &InstalledChild,
    clock: &mut Clock,
) -> Result<()> {
    installed.check()?;
    fixed.check()?;
    let (expires, idle) = actor.original_tunnel_window(permit, &fixed.policy, clock.now()?)?;
    clock.bind(expires)?;
    clock.bind_idle(idle)?;
    clock.now()?;
    Ok(())
}
fn wait_child(child: &mut Child, end: Instant) -> Result<bool> {
    while Instant::now() < end {
        if child.try_wait().map_err(|_| Error::Storage)?.is_some() {
            return Ok(child.wait().map_err(|_| Error::Storage)?.success());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Err(Error::WrongState)
}
struct ChildOwner(Child);
impl std::ops::Deref for ChildOwner {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for ChildOwner {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            kill_wait(&mut self.0);
        }
    }
}
fn kill_wait(child: &mut Child) -> bool {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    // A kill request is never clean-drain evidence, regardless of wait result.
    wait_child(child, Instant::now() + Duration::from_secs(3)).is_ok()
}
fn data(value: &Value, cap: usize) -> Result<Zeroizing<Vec<u8>>> {
    let text = value["hex"].as_str().ok_or(Error::InvalidWire)?;
    if text.is_empty()
        || text.len() % 2 != 0
        || text.len() > cap * 2
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Quota);
    }
    Ok(Zeroizing::new(
        hex::decode(text).map_err(|_| Error::InvalidWire)?,
    ))
}
// Private metering boundary shared by production and the actual TCP unit probe.
// No caller length or fake Permit: read count comes ONLY from this TcpStream.
pub(crate) fn debit_target<S: CompareAndSwap>(
    actor: &mut Actor<S>,
    permit: &Permit,
    policy: &TunnelRoutePolicyV2,
    target: &mut TcpStream,
    buffer: &mut [u8],
    now: Now,
) -> Result<Option<usize>> {
    actor.original_tunnel_window(permit, policy, now)?;
    if buffer.is_empty()
        || buffer.len() > pipe::DATA
        || buffer.len() as u64 > policy.route.limits.max_frame_bytes
    {
        return Err(Error::Quota);
    }
    match target.read(buffer) {
        Ok(0) => Err(Error::WrongState),
        Ok(n) => {
            actor.debit_tunnel(permit, policy, &buffer[..n], now)?;
            Ok(Some(n))
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(_) => Err(Error::Storage),
    }
}
fn write_target<S:CompareAndSwap>(
    target: &mut TcpStream,
    bytes: &[u8],
    actor: &mut Actor<S>,
    permit: &Permit,
    fixed: &Fixed,
    installed: &InstalledChild,
    clock: &mut Clock,
    control: &ParentControl,
) -> Result<()> {
    let end = clock.io();
    let mut offset = 0;
    while offset < bytes.len() {
        if control.flag.load(Ordering::SeqCst) != 0 {
            return Err(Error::WrongState);
        }
        check_live(actor, permit, fixed, installed, clock)?;
        if Instant::now() >= end {
            return Err(Error::Expired);
        }
        match target.write(&bytes[offset..]) {
            Ok(0) => return Err(Error::WrongState),
            Ok(n) => {
                offset += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(_) => return Err(Error::Storage),
        }
    }
    Ok(())
}
fn verify_echo(
    echoed: &Value,
    pending: &Signed<AgentRequest>,
    policy_hash: Hash,
    device: &str,
    peer: &Value,
) -> Result<()> {
    if echoed["nonce"]
        != serde_json::to_value(pending.body.nonce).map_err(|_| Error::InvalidWire)?
        || echoed["policyHash"]
            != serde_json::to_value(policy_hash).map_err(|_| Error::InvalidWire)?
        || echoed["leaseHash"]
            != serde_json::to_value(pending.body.lease_hash).map_err(|_| Error::InvalidWire)?
        || echoed["deviceId"] != device
        || &echoed["peerSha256"] != peer
    {
        return Err(Error::WrongClaim);
    }
    Ok(())
}
fn emit(cleanup: bool, drained: bool) -> Result<()> {
    let value = json!({"format":"cc-tunnel-supervised-result/v1","outcome":if drained{"stopped"}else{"unknown"},
        "cleanupObserved":cleanup,"drainRecorded":drained});
    let raw = pipe::canonical(&value)?;
    if raw.len() > 4096 {
        return Err(Error::Quota);
    }
    std::io::stdout()
        .lock()
        .write_all(&raw)
        .map_err(|_| Error::Storage)
}

/// Internal, source-fixed enrollment producer handoff. These values are not a
/// Permit or a child-claimed authorization and have no serde constructor.
#[derive(Clone,Copy,PartialEq,Eq)]
pub(crate) enum PreparedTransportMode { Admit, Resume }
impl PreparedTransportMode {
    pub(crate) fn from_product_verb(verb:&str)->Result<Self> {
        match verb {"startTunnelTransport"=>Ok(Self::Admit),"resumeTunnelTransport"=>Ok(Self::Resume),_=>Err(Error::WrongState)}
    }
    fn matches(self,mode:&str)->bool {
        matches!((self,mode),(Self::Admit,"admit")|(Self::Resume,"resume"))
    }
}
pub(crate) struct PreparedTransport {
    pub(crate) binding:NodeRequestBinding,
    pub(crate) revision:u64,
    pub(crate) pin:OwnerPin,
    pub(crate) root_files:Vec<(PathBuf,Hash)>,
    pub(crate) mode:PreparedTransportMode,
}
struct StartStore<S> {inner:S,first_revision:Option<u64>}
impl<S:CompareAndSwap> CompareAndSwap for StartStore<S> {
    fn load(&self)->Result<State>{self.inner.load()}
    fn compare_and_swap(&mut self,revision:u64,next:State)->Result<()> {
        if self.first_revision.is_some_and(|expected|expected!=revision){return Err(Error::Conflict)}
        self.inner.compare_and_swap(revision,next)?;
        self.first_revision=None;Ok(())
    }
}
pub(crate) fn check_installed_transport()->Result<()> {InstalledChild::capture()?.check()}
pub(crate) fn run_prepared_transport(args:&[String],expected:PreparedTransport)->Result<()> {
    run_fixed(args,Some(expected))
}

/// ONLY fixed CLI command, no executor/command/Actor/Permit injection parameter.
/// V1 worker dispatch and wire remain untouched. No normal success is possible
/// from source checkouts, uninstalled binaries or an absent runtime descriptor.
pub fn run_tunnel_supervised_worker(args: &[String]) -> Result<()> {
    run_fixed(args,None)
}
fn run_fixed(args:&[String],expected:Option<PreparedTransport>)->Result<()> {
    if args.len() != 13
        || args[0] != "worker-tunnel-supervised-sqlite"
        || args[1] != "--db-dir"
        || args[3] != "--anchor-dir"
        || args[5] != "--owner-pin"
        || args[7] != "--node-dir"
        || args[9] != "--policy"
        || args[11] != "--child-config"
    {
        return Err(Error::InvalidWire);
    }
    pipe::fifo(0)?;
    pipe::fifo(1)?;
    pipe::audit_inherited_fds()?;
    let installed = InstalledChild::capture()?;
    let user = uid()?;
    let node = crate::durable::absolute(Path::new(&args[8]))?;
    let protected = [
        crate::durable::absolute(Path::new(&args[2]))?,
        crate::durable::absolute(Path::new(&args[4]))?,
        node.clone(),
    ];
    for path in &protected {
        crate::durable::secure_path(path)?;
    }
    for raw in [&args[6], &args[10], &args[12]] {
        let path = crate::durable::absolute(Path::new(raw))?;
        if protected.iter().any(|p| fixed_path_overlap(&path, p)) {
            return Err(Error::ScopeDenied);
        }
    }
    let (pin, pin_file): (OwnerPin, Pin) = load(Path::new(&args[6]), 16384)?;
    let (policy, policy_file): (TunnelRoutePolicyV2, Pin) = load(Path::new(&args[10]), 16384)?;
    policy.validate()?;
    if policy.route.context != pin.context {
        return Err(Error::WrongContext);
    }
    let config = PathBuf::from(&args[12]);
    let config_pin = Pin::metadata(&config, user, true)?;
    let (child_config, child_pin): (ChildConfig, Pin) = load(&config, 16384)?;
    if child_config.format != "cc-tunnel-supervised-child-config/v1"
        || child_config.policy != PathBuf::from(&args[10])
    {
        return Err(Error::ScopeDenied);
    }
    let mut pins = vec![pin_file, policy_file, config_pin, child_pin];
    if protected.iter().any(|p| fixed_path_overlap(&config, p)) {
        return Err(Error::ScopeDenied);
    }
    let child_files = [
        &child_config.policy,
        &child_config.certificate,
        &child_config.private_key,
        &child_config.client_ca,
        &child_config.relay_ca,
        &child_config.relay_offer,
    ];
    let unique = child_files
        .iter()
        .map(|p| p.as_path())
        .collect::<std::collections::BTreeSet<_>>();
    if unique.len() != child_files.len() {
        return Err(Error::ScopeDenied);
    }
    for path in child_files {
        if fixed_path_overlap(path, &config)
            || protected.iter().any(|p| fixed_path_overlap(path, p))
        {
            return Err(Error::ScopeDenied);
        }
        pins.push(Pin::metadata(
            path,
            user,
            path == &child_config.private_key || path == &child_config.relay_offer,
        )?);
    }
    // Retain public inspection facts and metadata identities, never Node key
    // bytes outside its existing purpose signer. Any selected-key/file change
    // invalidates this original owner; no silent rebase during channel I/O.
    let inspected = inspect_node_state(&node, policy.route.agent_id)?;
    let node_facts = inspected.facts().clone();
    for (count, item) in fs::read_dir(&node).map_err(|_| Error::Storage)?.enumerate() {
        if count >= 7 {
            return Err(Error::Storage);
        }
        pins.push(Pin::metadata(
            &item.map_err(|_| Error::Storage)?.path(),
            user,
            true,
        )?);
    }
    inspected.recheck_guard_identity()?;
    drop(inspected);
    if let Some(start)=&expected {
        for (path,digest) in &start.root_files {
            let (bytes,pin)=installed::read_file(path,16384,true)?;
            if Hash::from_bytes(Sha256::digest(&bytes).into())!=*digest {return Err(Error::Storage)}
            pins.push(pin);
        }
        policy.check_binding(&start.binding)?;
    }
    let fixed = Fixed {
        pins,
        config,
        node,
        node_facts,
        policy,
    };
    fixed.check()?;
    let mut clock = Clock::new(fixed.policy.route.limits.max_ttl_seconds)?;
    let store=SqliteJournalStore::open(&args[2],&args[4],pin,SqliteStorageBudget::default())?;
    let mut actor=Actor::new(StartStore {inner:store,first_revision:expected.as_ref().map(|s|s.revision)})?;
    if let Some(start)=&expected {
        let state=actor.snapshot()?;
        if state.owner()!=&start.pin || state.revision()!=start.revision {return Err(Error::Conflict)}
        match start.mode {
            PreparedTransportMode::Admit=>actor.preview_original_tunnel_binding(&start.binding,&fixed.policy,clock.now()?)?,
            PreparedTransportMode::Resume=>{
                let (expiry,idle)=actor.preview_original_tunnel_resume_binding(&start.binding,&fixed.policy,clock.now()?)?;
                clock.bind(expiry)?;clock.bind_idle(idle)?;
            }
        }
        clock.bind(start.binding.lease.body.expires_at)?;
    }
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|_| Error::Entropy)?;
    if random == [0; 16] {
        return Err(Error::Entropy);
    }
    let session = hex::encode(random);
    installed.check()?;
    fixed.check()?;
    pipe::audit_inherited_fds()?;
    // Same process group as the original Rust leader. No shell/PATH/caller argv,
    // env or extra descriptor is inherited. Parent0/1 are replaced by NEW pipes.
    let mut child = ChildOwner(
        Command::new(installed.python)
            .args(["-I", "-S", "-B"])
            .arg(&installed.script)
            .env_clear()
            .env("LANG", "C.UTF-8")
            .current_dir(&installed.release)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::Storage)?,
    );
    let input = child.stdin.take().ok_or(Error::Storage)?;
    let output = child.stdout.take().ok_or(Error::Storage)?;
    let mut pipes = match Pipes::new(input, output, session) {
        Ok(v) => v,
        Err(e) => {
            kill_wait(&mut child);
            return Err(e);
        }
    };
    let mut control = match ParentControl::start() {
        Ok(v) => v,
        Err(e) => {
            pipes.close_writer();
            kill_wait(&mut child);
            pipes.joins(Instant::now() + Duration::from_secs(2));
            return Err(e);
        }
    };
    let mut remote_intent = false;
    let mut loss_request = None;
    let mut permit = None;
    let mut target = None;
    let workflow = (|| -> Result<()> {
        let opened=pipes.call("start",json!({"configPath":fixed.config,"inputPipeInode":pipes.input_inode,"outputPipeInode":pipes.output_inode}),
            &[("opened",&["peerSha256","mode","binding"])],clock.io(),&control)?;
        fixed.check()?;
        installed.check()?;
        let binding: NodeRequestBinding =
            serde_json::from_value(opened["binding"].clone()).map_err(|_| Error::InvalidWire)?;
        fixed.policy.check_binding(&binding)?;
        if let Some(start)=&expected {
            if serde_json::to_value(&binding).map_err(|_|Error::InvalidWire)?
                !=serde_json::to_value(&start.binding).map_err(|_|Error::InvalidWire)? {
                return Err(Error::WrongClaim)
            }
        }
        clock.bind(binding.lease.body.expires_at)?;
        let peer: Hash =
            serde_json::from_value(opened["peerSha256"].clone()).map_err(|_| Error::InvalidWire)?;
        if peer != fixed.policy.route.client_tls_certificate_sha256 {
            return Err(Error::ScopeDenied);
        }
        let mode = opened["mode"].as_str().ok_or(Error::InvalidWire)?;
        if !matches!(mode, "admit" | "resume") {
            return Err(Error::InvalidWire);
        }
        if expected.as_ref().is_some_and(|start|!start.mode.matches(mode)) {return Err(Error::ScopeDenied)}
        // Deny a non-opted-in Resume before any new Node proof/nonce.
        if mode == "resume" && !fixed.policy.resume_original {
            return Err(Error::ScopeDenied);
        }
        if control.flag.load(Ordering::SeqCst) != 0 {
            return Err(Error::WrongState);
        }
        // ONE original pending proof. Never generate another after challenge.
        let current = actor.snapshot()?.owner().clone();
        let pending = create_original_tunnel_node_request(
            &fixed.node,
            fixed.policy.route.agent_id,
            &current,
            binding.clone(),
            &fixed.policy,
            clock.now()?,
        )?;
        let policy_hash = fixed.policy.digest()?;
        let echoed=pipes.call("challenge",json!({"proof":pending,"policyHash":policy_hash,"expiresAt":binding.lease.body.expires_at}),
            &[("echo",&["nonce","policyHash","leaseHash","deviceId","peerSha256"])],clock.io(),&control)?;
        verify_echo(
            &echoed,
            &pending,
            policy_hash,
            &fixed.policy.route.client_device_id,
            &opened["peerSha256"],
        )?;
        fixed.check()?;
        installed.check()?;
        if control.flag.load(Ordering::SeqCst) != 0 {
            return Err(Error::WrongState);
        }

        let granted = if mode == "admit" {
            actor.admit(
                binding.lease.clone(),
                pending.clone(),
                Some(&binding.freshness),
                &fixed.policy.operation()?,
                clock.now()?,
            )?
        } else {
            actor.resume_tunnel(
                binding.lease.clone(),
                pending.clone(),
                &binding.freshness,
                &fixed.policy,
                clock.now()?,
            )?
        };
        permit = Some(granted);
        if control.flag.load(Ordering::SeqCst) != 0 {
            return Err(Error::WrongState);
        }
        let owned = permit.as_ref().ok_or(Error::WrongState)?;
        check_live(&mut actor, owned, &fixed, &installed, &mut clock)?;
        let destination: SocketAddr = format!(
            "{}:{}",
            if fixed.policy.route.destination.ip == "::1" {
                "[::1]"
            } else {
                "127.0.0.1"
            },
            fixed.policy.route.destination.port
        )
        .parse()
        .map_err(|_| Error::InvalidWire)?;
        let socket = TcpStream::connect_timeout(
            &destination,
            clock.io().saturating_duration_since(Instant::now()),
        )
        .map_err(|_| Error::Storage)?;
        if socket.peer_addr().map_err(|_| Error::Storage)? != destination {
            return Err(Error::ScopeDenied);
        }
        socket.set_nonblocking(true).map_err(|_| Error::Storage)?;
        target = Some(socket);
        check_live(&mut actor, owned, &fixed, &installed, &mut clock)?;
        pipes.call(
            "accepted",
            json!({"proof":pending,"policyHash":policy_hash,"expiresAt":owned.expires_at(),"idleExpiresAt":clock.idle_expires()?}),
            &[("accepted", &[])],
            clock.io(),
            &control,
        )?;
        let cap = (fixed.policy.route.limits.max_frame_bytes as usize).min(pipe::DATA);
        let mut buffer = Zeroizing::new(vec![0u8; cap]);
        loop {
            check_live(&mut actor, owned, &fixed, &installed, &mut clock)?;
            if control.flag.load(Ordering::SeqCst) == 2 {
                return Err(Error::WrongState);
            }
            if control.stopped() {
                return Ok(());
            }
            let bound_fields=&["nonce", "policyHash", "leaseHash", "deviceId", "peerSha256"][..];
            let mut replies=vec![("data", &["hex"][..]), ("idle", &[][..]), ("eof", &[][..]),
                ("stop_requested", bound_fields)];
            if expected.is_some() && fixed.policy.resume_original {replies.push(("loss_request",bound_fields));}
            let incoming = pipes.call(
                "poll",
                json!({"idleExpiresAt":clock.idle_expires()?}),
                &replies,
                clock.io(),
                &control,
            )?;
            if incoming["kind"] == "eof" {
                return Err(Error::WrongState);
            }
            if incoming["kind"] == "loss_request" {
                check_live(&mut actor, owned, &fixed, &installed, &mut clock)?;
                loss_request=Some(retirement::loss_request(&incoming,&pending,policy_hash,
                    &opened["peerSha256"],&fixed,expected.as_ref(),&control)?);
                return Ok(());
            }
            if incoming["kind"] == "stop_requested" {
                // Private owned-child observation of authenticated close_notify
                // on the SAME active SSLObject, bound to this ORIGINAL proof.
                verify_echo(&incoming, &pending, policy_hash,
                    &fixed.policy.route.client_device_id, &opened["peerSha256"])?;
                check_live(&mut actor, owned, &fixed, &installed, &mut clock)?;
                control.retire_for_remote()?;
                remote_intent = true;
                return Ok(());
            }
            if incoming["kind"] == "data" {
                let bytes = data(&incoming, cap)?;
                check_live(&mut actor, owned, &fixed, &installed, &mut clock)?;
                actor.debit_tunnel(owned, &fixed.policy, &bytes, clock.now()?)?;
                if control.stopped() {
                    return Ok(());
                } // charged discarded bytes are NEVER refunded.
                write_target(
                    target.as_mut().ok_or(Error::WrongState)?,
                    &bytes,
                    &mut actor,
                    owned,
                    &fixed,
                    &installed,
                    &mut clock,
                    &control,
                )?;
            }
            check_live(&mut actor, owned, &fixed, &installed, &mut clock)?;
            if control.stopped() {
                return Ok(());
            }
            match debit_target(
                &mut actor,
                owned,
                &fixed.policy,
                target.as_mut().ok_or(Error::WrongState)?,
                &mut buffer,
                clock.now()?,
            )? {
                Some(n) => {
                    if control.stopped() {
                        return Ok(());
                    }
                    check_live(&mut actor, owned, &fixed, &installed, &mut clock)?;
                    let done=pipes.call("deliver",json!({"hex":hex::encode(&buffer[..n]),"idleExpiresAt":clock.idle_expires()?}),&[("delivered",&["bytes"])],clock.io(),&control)?;
                    if done["bytes"].as_u64() != Some(n as u64) {
                        return Err(Error::WrongState);
                    }
                }
                None => thread::sleep(Duration::from_millis(5)),
            }
        }
    })();
    // Product-only typed network-loss request. EOF/Err/panic never enters
    // this path: it requires an exact successful active private frame above.
    if workflow.is_ok() { if let Some(request)=loss_request.take() {
        let retired=(|| -> Result<()> {
            let target=retirement::target(&mut target)?;
            let old=permit.take().ok_or(Error::WrongState)?;
            check_live(&mut actor,&old,&fixed,&installed,&mut clock)?;
            control.retire_for_transport_loss()?;
            pipes.call("stop",json!({}),&[("stopped",&[])],clock.io(),&control)?;
            pipes.close_writer();
            let child=retirement::child(&mut child,clock.io().min(Instant::now()+Duration::from_secs(3)))?;
            let pipes=pipes.retire_after_transport_loss(clock.io().min(Instant::now()+Duration::from_secs(2)))?;
            let control=control.join_transport_loss(clock.io().min(Instant::now()+Duration::from_secs(1)))?;
            retirement::OwnedRetirement::observe(request,target,child,pipes,control)
                .commit(&mut actor,old,&fixed,&installed,&mut clock)?;
            retirement::emit(&mut actor,&fixed,&installed,expected.as_ref().ok_or(Error::WrongState)?,&mut clock)
        })();
        if retired.is_ok() {return Ok(())}
        // Separate bounded cleanup, never converts failure/forced kill into
        // retirement evidence or retries the original CAS.
        control.abort();
        if let Some(socket)=target.take(){let _=socket.shutdown(Shutdown::Both);drop(socket);}
        pipes.close_writer();kill_wait(&mut child);
        let _=pipes.joins(Instant::now()+Duration::from_secs(2));
        let _=control.join(Instant::now()+Duration::from_secs(1));
        emit(false,false)?;return Err(Error::WrongState)
    } }
    // A verified TLS alert is REQUEST only, never child-claimed drain. Raw EOF,
    // error, cancellation and timeout remain UNKNOWN and cannot be revived.
    let intentional = workflow.is_ok() && (control.stopped() || remote_intent) && permit.is_some();
    if !intentional { control.abort(); }
    let shutdown = target
        .as_ref()
        .is_some_and(|t| t.shutdown(Shutdown::Both).is_ok());
    drop(target);
    let child_ack = if intentional && shutdown {
        pipes
            .call("stop", json!({}), &[("stopped", &[])], clock.io(), &control)
            .is_ok()
    } else {
        false
    };
    pipes.close_writer();
    let exited = if child_ack {
        wait_child(&mut child, Instant::now() + Duration::from_secs(3)).unwrap_or(false)
    } else {
        false
    };
    let forced = if !exited {
        kill_wait(&mut child);
        true
    } else {
        false
    };
    let joined = pipes.joins(Instant::now() + Duration::from_secs(2));
    let parent_joined = control.join(Instant::now() + Duration::from_secs(1));
    let cleanup = shutdown && exited && joined && parent_joined && !forced
        && (control.stopped() || remote_intent && control.remote_retired());
    let drained = if intentional && cleanup {
        let old = permit.take().ok_or(Error::WrongState)?;
        check_live(&mut actor, &old, &fixed, &installed, &mut clock)
            .and_then(|_| actor.retire_tunnel_after_owned_drain(old, clock.now()?))
            .is_ok()
    } else {
        false
    };
    emit(cleanup, drained)?;
    if drained {
        Ok(())
    } else {
        Err(Error::WrongState)
    }
}

#[cfg(test)]
#[path = "tunnel_supervisor/tests.rs"]
mod tests;
