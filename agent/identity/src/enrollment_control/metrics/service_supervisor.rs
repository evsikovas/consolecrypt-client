//! Installed fixed daemon; all execution authority comes from the SAME signed
//! Actor journal. Stdio is only its original owning launcher's stop lifetime.
use super::*;
const ADAPTER_SERVICE: &str =
    "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_unit_adapter.py";
const ENTRY_SERVICE: &str =
    "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_unit_entry.py";
const SPEC_SERVICE: &str =
    "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_unit_spec.py";
const BINDING_SERVICE: &str =
    "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_binding.py";
const CATALOG_SERVICE: &str = "/etc/consolecrypt-agent/collector-service.json";
type Physical = (u64, u64, u32, u32, u64, u64, i64, i64, i64, i64);
// Only closed stage/error names leave this boundary. No config, identifier,
// endpoint, credential, or underlying I/O error enters the diagnostic.
#[derive(Clone, Copy)]
pub(super) enum FailureStage {
    LifetimeValidate, LifetimeDecode, LifetimeOpen, LifetimeProjection,
    Material, Session, Spawn, ReadyFrame, ReadyDecode, Runtime, Retire, Cleanup,
}
impl FailureStage {
    fn name(self) -> &'static str {
        match self {
            Self::LifetimeValidate => "lifetimeValidate", Self::LifetimeDecode => "lifetimeDecode",
            Self::LifetimeOpen => "lifetimeOpen", Self::LifetimeProjection => "lifetimeProjection",
            Self::Material => "material", Self::Session => "session", Self::Spawn => "spawn",
            Self::ReadyFrame => "readyFrame", Self::ReadyDecode => "readyDecode",
            Self::Runtime => "runtime", Self::Retire => "retire", Self::Cleanup => "cleanup",
        }
    }
}
fn write_failure(stage: FailureStage, error: Error, sink: &mut impl Write) -> Error {
    // Error is the existing fieldless enum. A closed/broken stderr must never
    // replace the original failure or prevent its mandatory retirement path.
    let _ = writeln!(sink, "{{\"event\":\"collectorServiceFailure\",\"stage\":\"{}\",\"code\":\"{:?}\"}}", stage.name(), error);
    error
}
pub(super) fn failure(stage: FailureStage, error: Error) -> Error {
    write_failure(stage, error, &mut std::io::stderr().lock())
}
fn physical(m: &fs::Metadata) -> Physical {
    (
        m.dev(),
        m.ino(),
        m.mode(),
        m.uid(),
        m.nlink(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
struct Runtime {
    raw: Vec<u8>,
    pins: Vec<(PathBuf, Physical)>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Descriptor {
    format: String,
    manifest_sha256: Hash,
    adapter_sha256: Hash,
    entry_sha256: Hash,
    spec_sha256: Hash,
    binding_sha256: Hash,
}
impl Runtime {
    fn load(catalog: &Catalog) -> Result<Self> {
        catalog.check()?;
        if catalog.disk.is_none() {
            return Err(Error::CapabilityDenied);
        }
        let raw = root_file(Path::new(CATALOG_SERVICE))?;
        let value: Value = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
        if canonical(&value)? != raw {
            return Err(Error::InvalidWire);
        }
        let descriptor: Descriptor = decode(value)?;
        if descriptor.format != "cc-agent-finite-collector-service/v1"
            || descriptor.manifest_sha256 != catalog.hash
        {
            return Err(Error::WrongClaim);
        }
        let mut pins = Vec::new();
        for (name, expected) in [
            (ADAPTER_SERVICE, descriptor.adapter_sha256),
            (ENTRY_SERVICE, descriptor.entry_sha256),
            (SPEC_SERVICE, descriptor.spec_sha256),
            (BINDING_SERVICE, descriptor.binding_sha256),
        ] {
            let path = PathBuf::from(name);
            let before = fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
            if before.mode() & 0o777 != 0o644
                || !before.is_file()
                || before.file_type().is_symlink()
                || before.uid() != 0
                || before.nlink() != 1
            {
                return Err(Error::Storage);
            }
            let body = root_service_script(&path)?;
            if digest(&body) != expected
                || physical(&before)
                    != physical(&fs::symlink_metadata(&path).map_err(|_| Error::Storage)?)
            {
                return Err(Error::Storage);
            }
            pins.push((path, physical(&before)));
        }
        Ok(Self { raw, pins })
    }
    fn check(&self, catalog: &Catalog) -> Result<()> {
        catalog.check()?;
        if root_file(Path::new(CATALOG_SERVICE))? != self.raw {
            return Err(Error::Storage);
        }
        for (path, expected) in &self.pins {
            for parent in path.ancestors().skip(1) {
                let m = fs::symlink_metadata(parent).map_err(|_| Error::Storage)?;
                if !m.is_dir()
                    || m.file_type().is_symlink()
                    || m.uid() != 0
                    || m.mode() & 0o022 != 0
                {
                    return Err(Error::Storage);
                }
            }
            if physical(&fs::symlink_metadata(path).map_err(|_| Error::Storage)?) != *expected {
                return Err(Error::Storage);
            }
        }
        Ok(())
    }
}
/// No constructor/Serde. The sole constructor below observes the actual fixed
/// adapter terminal AND its original OS Child exit before this value exists.
pub(crate) struct JoinedCollectorService<'a> {
    permit: &'a CollectorServicePermit,
    public_files: Vec<PublicFile>,
}
impl JoinedCollectorService<'_> {
    pub(crate) fn matches_original(&self, permit: &CollectorServicePermit) -> bool {
        std::ptr::eq(self.permit, permit)
    }
}
pub(crate) struct UnlaunchedCollectorService<'a> {
    permit: &'a CollectorServicePermit,
}
impl UnlaunchedCollectorService<'_> {
    pub(crate) fn matches_original(&self, permit: &CollectorServicePermit) -> bool {
        std::ptr::eq(self.permit, permit)
    }
}
fn retire_unlaunched<'a>(
    files: &mut Files,
    permit: &'a CollectorServicePermit,
) -> Result<UnlaunchedCollectorService<'a>> {
    // Called ONLY in run_one's pre-spawn branch. There is no Adapter/Child in
    // this typestate. Actual original namespace cleanup must finish first.
    files.cleanup_allowed = false;
    files.cleanup()?;
    Ok(UnlaunchedCollectorService { permit })
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceEnd {
    format: String,
    session: String,
    unit: String,
    unit_empty: bool,
    leader_exited: bool,
    unit_stopped: bool,
    private_namespace_removed: bool,
    cleanup_unknown: bool,
    handles_closed: bool,
    outcome: String,
    retained_public_files: Vec<PublicFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicFile {
    name: String,
    dev: u64,
    ino: u64,
    mode: u32,
}
impl PublicFile {
    fn check(&self) -> Result<()> {
        let expected = match self.name.as_str() {
            "launch-owner.json" | "stop.request" => 0o400,
            "preexec.json" | "prometheus-exec.json" | "prometheus-exit.json" => 0o600,
            _ => return Err(Error::WrongClaim),
        };
        if self.ino == 0 || self.mode != expected {
            return Err(Error::WrongClaim);
        }
        Ok(())
    }
}
impl Files {
    fn check_service_files(&self) -> Result<()> {
        for (path, expected) in &self.paths {
            match (expected, fs::symlink_metadata(path)) {
                (None, Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
                (Some(expected), Ok(m))
                    if m.is_file()
                        && !m.file_type().is_symlink()
                        && m.uid() == service_uid()?
                        && m.nlink() == 1
                        && m.mode() & 0o7777 == expected.identity.2
                        && (m.dev(), m.ino()) == (expected.identity.0, expected.identity.1) => {
                    let held=expected.file.metadata().map_err(|_|Error::Storage)?;
                    if (held.dev(),held.ino(),held.mode()&0o777)!=expected.identity{return Err(Error::Storage);}
                }
                _ => return Err(Error::UncertainCommit),
            }
        }
        Ok(())
    }
    fn retain_service_public(&mut self, end: &JoinedCollectorService<'_>) -> Result<()> {
        if end.public_files.len() > 5 {
            return Err(Error::Quota);
        }
        let mut names = BTreeSet::new();
        for leaf in &end.public_files {
            leaf.check()?;
            if !names.insert(leaf.name.clone()) {
                return Err(Error::WrongClaim);
            }
            let path = self.directory.join(&leaf.name);
            // Never replace/adopt a pre-existing Rust credential/session slot.
            if self.paths.iter().any(|(old, _)| *old == path) {
                return Err(Error::WrongState);
            }
            let actual = fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
            if !actual.is_file()
                || actual.file_type().is_symlink()
                || actual.uid() != service_uid()?
                || actual.nlink() != 1
                || actual.mode() & 0o777 != leaf.mode
                || (actual.dev(), actual.ino()) != (leaf.dev, leaf.ino)
            {
                return Err(Error::UncertainCommit);
            }
        }
        for leaf in &end.public_files {
            let path=self.directory.join(&leaf.name);
            let file=OpenOptions::new().read(true).custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC).open(&path).map_err(|_|Error::Storage)?;
            let retained=RetainedFile::capture(&file)?;
            if retained.identity!=(leaf.dev,leaf.ino,leaf.mode){return Err(Error::UncertainCommit);}
            self.paths.push((path,Some(retained)));
        }
        Ok(())
    }
}
fn check_service_ready(
    ready: &DiskAdapterReady, session: &str, unit: &str, binary_hash: Option<Hash>,
) -> Result<()> {
    if ready.format != "cc-collector-service-ready/v1" || ready.session != session || ready.unit != unit
        || ready.prometheus_sha256 != hex::encode(binary_hash.ok_or(Error::CapabilityDenied)?.as_bytes()) {
        return Err(Error::WrongClaim);
    }
    Ok(())
}
// Protocol retirement is separate from the non-Serde Permit-bound join token.
// A late READY is observation only; terminal, real stdout EOF and original Child
// exit are all still required under the SAME original ten-second stop end.
fn stop_join_frames(
    adapter: &mut Adapter, session: &str, unit: &str, binary_hash: Option<Hash>,
    mut ready_seen: bool,
) -> Result<Vec<PublicFile>> {
    let end = Instant::now() + Duration::from_secs(10);
    let mut sent = 0;
    while sent < 5 && Instant::now() < end {
        match adapter.input.write(&b"STOP\n"[sent..]) {
            Ok(0) => break,
            Ok(n) => sent += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(_) => break,
        }
    }
    let mut observed = false;
    let mut eof = false;
    let mut status_code = None;
    let mut public_files = Vec::new();
    while Instant::now() < end {
        if !eof {
            match adapter.frame_event()? {
                AdapterFrame::Pending => {},
                AdapterFrame::Eof => {
                    eof = true;
                    if !observed { return Err(Error::UncertainCommit); }
                },
                AdapterFrame::Frame(value) => {
                    // Neither duplicate terminal nor any trailing record can be
                    // ignored after the original terminal has been observed.
                    if observed { return Err(Error::WrongState); }
                    if value.get("format").and_then(Value::as_str) == Some("cc-collector-service-ready/v1") {
                        if ready_seen { return Err(Error::WrongState); }
                        let ready: DiskAdapterReady = decode(value)?;
                        check_service_ready(&ready,session,unit,binary_hash)?;
                        ready_seen = true;
                    } else {
                        let v: ServiceEnd = decode(value)?;
                        observed = v.format == "cc-collector-service-exit/v1"
                            && v.session == session && v.unit == unit && v.unit_empty && v.leader_exited
                            && v.unit_stopped && !v.private_namespace_removed && !v.cleanup_unknown
                            && v.handles_closed && matches!(v.outcome.as_str(), "stopped" | "unknown");
                        if !observed { return Err(Error::WrongClaim); }
                        if v.retained_public_files.len() > 5 { return Err(Error::Quota); }
                        for leaf in &v.retained_public_files { leaf.check()?; }
                        public_files = v.retained_public_files;
                    }
                }
            }
        }
        if let Some(status) = adapter.child.try_wait().map_err(|_| Error::Storage)? {
            adapter.exited = true;
            status_code = Some(status.code());
        }
        if observed && eof {
            if let Some(code) = status_code {
                if matches!(code, Some(0 | 1)) { return Ok(public_files); }
                return Err(Error::UncertainCommit);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // No end token and no quota release on timeout, even if kill is accepted.
    let _ = adapter.child.kill();
    Err(Error::UncertainCommit)
}
fn stop_join<'a>(
    adapter: &mut Adapter, files: &Files, permit: &'a CollectorServicePermit,
    binary_hash: Option<Hash>, ready_seen: bool,
) -> Result<JoinedCollectorService<'a>> {
    let public_files = stop_join_frames(adapter,&files.session,&files.unit,binary_hash,ready_seen)?;
    Ok(JoinedCollectorService { permit, public_files })
}
fn control_pipe() -> Result<File> {
    let input = OpenOptions::new()
        .read(true)
        .custom_flags(NONBLOCK | CLOEXEC)
        .open("/proc/self/fd/0")
        .map_err(|_| Error::Storage)?;
    let m = input.metadata().map_err(|_| Error::Storage)?;
    if !m.file_type().is_fifo() || m.uid() != service_uid()? {
        return Err(Error::CapabilityDenied);
    }
    Ok(input)
}
pub(super) fn launcher_alive(input: &mut File, pending: &mut Vec<u8>) -> Result<bool> {
    let mut raw = [0u8; 6];
    match input.read(&mut raw) {
        Ok(0) => Ok(false),
        Ok(n) => {
            pending.extend_from_slice(&raw[..n]);
            if pending.len() > 5 || !b"STOP\n".starts_with(pending) {
                return Err(Error::InvalidWire);
            }
            Ok(pending.as_slice() != b"STOP\n")
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
        Err(_) => Err(Error::Storage),
    }
}
pub(super) fn actual_now() -> Result<Now> {
    Ok(CollectorServiceClockObservation::observe_local()?.now())
}

// A fresh process stamp follows expensive unchanged validation. This never
// refreshes the Node-signed material deadline or expiry.
fn launch_stamp_after_validation(
    validate: impl FnOnce() -> Result<()>,
    clock: impl FnOnce() -> Result<u64>,
    deadline: u64,
) -> Result<u64> {
    validate()?;
    let created = clock()?;
    if created >= deadline { return Err(Error::Expired); }
    Ok(created)
}

fn run_one(
    actor: &mut Actor<SqliteJournalStore>,
    catalog: &Catalog,
    runtime: &Runtime,
    config: &Config,
    config_raw: &[u8],
    pin_raw: &[u8],
    node: &PublicNodeIdentity,
    observed: CollectorServiceStoredObservation,
    input: &mut File,
    pending: &mut Vec<u8>,
) -> Result<()> {
    let policy = &observed.policy.body;
    let checks = ReadChecks { catalog, runtime, config, config_raw, pin_raw, node,
        signed_end: Some((policy.expires_at, observed.claim.body.clock.deadline_boottime_ns)),
        original_stopped: std::cell::Cell::new(false) };
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| Error::Entropy)?;
    nonce[6] = (nonce[6] & 0x0f) | 0x40;
    nonce[8] = (nonce[8] & 0x3f) | 0x80;
    let permit = actor.reserve_collector_service_start(
        policy.service_id,
        Nonce::from_bytes(nonce),
        actual_now()?,
        &mut CollectorServiceClockObservation::observe_local,
    )?;
    let mut files = Files::new_service(&permit)?;
    let setup = (|| -> Result<(Hash, u64, Timestamp)> {
        let material = super::service_material::prepare(
            actor,
            &permit,
            &mut files,
            catalog,
            &config.node_dir,
            input,
            pending,
            &checks,
        ).map_err(|e| failure(FailureStage::Material, e))?;
        let scrape = material.scrape;
        files.write("scrape.json", &scrape)?;
        runtime.check(catalog)?;
        original_config(config_raw, &config.owner_pin, pin_raw)?;
        let deadline = material.deadline;
        let expires = material.expires;
        // This is a fresh PROCESS launch stamp, not a refreshed signed policy
        // deadline. Every start uses the exact original Node-signed BOOTTIME end.
        let created = launch_stamp_after_validation(|| runtime.check(catalog), boottime_ns, deadline)?;
        let boot = boot_id()?;
        if ResourceId::new(&boot)? != observed.claim.body.clock.boot_id || created >= deadline {
            return Err(Error::Expired);
        }
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| Error::Storage)?;
        let port = reserved.local_addr().map_err(|_| Error::Storage)?.port();
        drop(reserved);
        if port < 1024 {
            return Err(Error::Storage);
        }
        let snapshot = actor.snapshot_collector_service_waiting(actual_now()?,
            Instant::now() + Duration::from_secs(3), &mut || checks.clock(input, pending))?;
        let cert = snapshot
            .certificate(config.agent_id)
            .ok_or(Error::NotFound)?;
        let binding = json!({"original":disk::binding(&observed.owner,node,cert,catalog)?,
        "service_id":policy.service_id,"delegation_sha256":policy.digest()?,
        "policy_revision":policy.policy_revision,"revocation_epoch":policy.revocation_epoch,
        "expires_unix_seconds":policy.expires_at.seconds()});
        let mut value = json!({"format":"cc-collector-service-session/v1","session":files.session,"unit":files.unit,
        "bootId":boot,"createdBoottimeNs":created,"deadlineBoottimeNs":deadline,
        "expiresUnixNs":expires.seconds().checked_mul(1_000_000_000).ok_or(Error::InvalidTime)?,
        "scrapeSha256":digest(&scrape),"receiver":catalog.manifest["receiver"]["url"],"statusPort":port,
        "ownerPid":std::process::id(),"ownerStartTicks":start_ticks()?,"manifestSha256":catalog.hash,
        "binarySha256":catalog.disk.as_ref().ok_or(Error::CapabilityDenied)?.binary_hash,"binding":binding});
        if catalog.manifest["schema_version"]==json!(2) {
            value["format"]=json!("cc-collector-service-session/v2");
            value["runtime"]=catalog.manifest["runtime"].clone();
        }
        let raw = private_value(&value)?;
        files.write("session.json", &raw)?;
        original_config(config_raw, &config.owner_pin, pin_raw)?;
        actor.check_collector_service_waiting(
            &permit, actual_now()?, Instant::now() + Duration::from_secs(3),
            &mut || checks.clock(input, pending),
        )?;
        Ok((digest(&raw), deadline, expires))
    })();
    let (config_hash, deadline, expires) = match setup {
        Ok(value) => value,
        Err(error) => {
            let error = failure(FailureStage::Session, error);
            let ended = retire_unlaunched(&mut files, &permit).map_err(|e| failure(FailureStage::Retire, e))?;
            actor.finish_unlaunched_collector_service(&permit, ended, actual_now()?)?;
            return Err(error);
        }
    };
    // Registered material is created before any OS child exists. Only now
    // disable Drop cleanup: spawn may create a real child and then fail.
    files.cleanup_allowed = false;
    let mut created = false;
    let mut adapter =
        match Adapter::spawn_fixed_observed(&files, config_hash, ADAPTER_SERVICE, &mut created)
            .map_err(|e| failure(FailureStage::Spawn, e)) {
            Ok(adapter) => adapter,
            Err(error) if !created => {
                let ended = retire_unlaunched(&mut files, &permit)?;
                actor.finish_unlaunched_collector_service(&permit, ended, actual_now()?)?;
                return Err(error);
            }
            Err(error) => return Err(error), // A real Child existed: preserve Unknown.
        };
    let mut ready_seen = false;
    let action = (|| -> Result<()> {
        let ready_end = Instant::now() + Duration::from_secs(10);
        loop {
            files.check_service_files()?;
            runtime.check(catalog)?;
            original_config(config_raw, &config.owner_pin, pin_raw)?;
            if inspect_existing_node_identity(&config.node_dir, config.agent_id)? != *node {
                return Err(Error::WrongClaim);
            }
            actor.check_collector_service_waiting(
                &permit, actual_now()?, ready_end.min(Instant::now() + Duration::from_secs(3)),
                &mut || checks.clock_before(input, pending, ready_end, expires),
            )?;
            if boottime_ns()? >= deadline || actual_now()?.0 >= expires {
                return Err(Error::Expired);
            }
            if !launcher_alive(input, pending)? {
                return Ok(());
            }
            if Instant::now() >= ready_end {
                return Err(Error::Expired);
            }
            if let Some(value) = adapter.frame().map_err(|e| failure(FailureStage::ReadyFrame, e))? {
                let ready: DiskAdapterReady = decode(value).map_err(|e| failure(FailureStage::ReadyDecode, e))?;
                check_service_ready(&ready,&files.session,&files.unit,
                    catalog.disk.as_ref().map(|disk|disk.binary_hash))?;
                ready_seen = true;
                break;
            }
            if adapter
                .child
                .try_wait()
                .map_err(|_| Error::Storage)?
                .is_some()
            {
                adapter.exited = true;
                return Err(Error::UncertainCommit);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        loop {
            files.check_service_files()?;
            runtime.check(catalog)?;
            original_config(config_raw, &config.owner_pin, pin_raw)?;
            if inspect_existing_node_identity(&config.node_dir, config.agent_id)? != *node {
                return Err(Error::WrongClaim);
            }
            actor.check_collector_service_waiting(
                &permit, actual_now()?, Instant::now() + Duration::from_secs(3),
                &mut || checks.clock(input, pending),
            )?;
            if boottime_ns()? >= deadline || actual_now()?.0 >= expires {
                return Err(Error::Expired);
            }
            if !launcher_alive(input, pending)? {
                return Ok(());
            }
            if adapter
                .child
                .try_wait()
                .map_err(|_| Error::Storage)?
                .is_some()
            {
                adapter.exited = true;
                return Err(Error::UncertainCommit);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })();
    if let Err(error) = action {
        // The existing owned-STOP path is normalized only AFTER retirement.
        // Do not describe that expected control signal as a runtime failure.
        if error != Error::Expired || !checks.original_stopped.get() {
            failure(FailureStage::Runtime, error);
        }
    }
    let ended = stop_join(&mut adapter, &files, &permit, catalog.disk.as_ref().map(|disk|disk.binary_hash), ready_seen).map_err(|e| failure(FailureStage::Retire, e))?;
    files.retain_service_public(&ended)?;
    files.cleanup_allowed = true;
    files.cleanup().map_err(|e| failure(FailureStage::Cleanup, e))?;
    actor.finish_joined_collector_service(&permit, ended, actual_now()?)?;
    finish_original_launcher_stop(action, checks.original_stopped.get())
}
fn check_original_launcher(input: &mut File, pending: &mut Vec<u8>,
    stopped: &std::cell::Cell<bool>) -> Result<()> {
    if !launcher_alive(input, pending)? {
        stopped.set(true);
        return Err(Error::Expired);
    }
    Ok(())
}
fn observe_before_original_launcher(input: &mut File, pending: &mut Vec<u8>,
    stopped: &std::cell::Cell<bool>,
    observe: impl FnOnce() -> Result<CollectorServiceClockObservation>,
) -> Result<CollectorServiceClockObservation> {
    let observed = observe()?;
    check_original_launcher(input, pending, stopped)?;
    Ok(observed)
}
// Called only after original retirement/join, cleanup and Actor end succeeded.
fn finish_original_launcher_stop(result: Result<()>, stopped: bool) -> Result<()> {
    match result {
        Err(Error::Expired) if stopped => Ok(()),
        other => other,
    }
}
pub(super) struct ReadChecks<'a> {
    catalog: &'a Catalog,
    runtime: &'a Runtime,
    config: &'a Config,
    config_raw: &'a [u8],
    pin_raw: &'a [u8],
    node: &'a PublicNodeIdentity,
    signed_end: Option<(Timestamp, u64)>,
    original_stopped: std::cell::Cell<bool>,
}
impl ReadChecks<'_> {
    fn observe_current(&self) -> Result<CollectorServiceClockObservation> {
        self.runtime.check(self.catalog)?;
        original_config(self.config_raw, &self.config.owner_pin, self.pin_raw)?;
        if inspect_existing_node_identity(&self.config.node_dir, self.config.agent_id)? != *self.node {
            return Err(Error::WrongClaim);
        }
        let observed = CollectorServiceClockObservation::observe_local()?;
        if self.signed_end.is_some_and(|(expires, deadline)|
            observed.now().0 >= expires || observed.boottime_ns() >= deadline) {
            return Err(Error::Expired);
        }
        Ok(observed)
    }
    pub(super) fn clock(&self, input: &mut File, pending: &mut Vec<u8>)
        -> Result<CollectorServiceClockObservation> {
        observe_before_original_launcher(input, pending, &self.original_stopped, || self.observe_current())
    }
    pub(super) fn clock_before(&self, input: &mut File, pending: &mut Vec<u8>,
        end: Instant, expires: Timestamp) -> Result<CollectorServiceClockObservation> {
        observe_before_original_launcher(input, pending, &self.original_stopped, || {
            if Instant::now() >= end { return Err(Error::Expired); }
            let observed = self.observe_current()?;
            if Instant::now() >= end || observed.now().0 >= expires {
                return Err(Error::Expired);
            }
            Ok(observed)
        })
    }
}
pub(crate) fn run_fixed_collector_service(args: &[String]) -> Result<()> {
    if args != ["worker-collector-service-sqlite"] || service_uid()? == 0 {
        return Err(Error::CapabilityDenied);
    }
    let mut input = control_pipe()?;
    let mut pending = Vec::new();
    let config_raw = root_file(Path::new(CONFIG))?;
    let config: Config = serde_json::from_slice(&config_raw).map_err(|_| Error::InvalidWire)?;
    if config.format != "cc-agent-enroll-config/v1" {
        return Err(Error::InvalidWire);
    }
    for path in [&config.node_dir, &config.db_dir, &config.anchor_dir] {
        crate::durable::absolute(path)?;
        crate::durable::secure_path(path)?;
    }
    let pin_raw = root_file(&config.owner_pin)?;
    let genesis: OwnerPin = serde_json::from_slice(&pin_raw).map_err(|_| Error::InvalidWire)?;
    let node = inspect_existing_node_identity(&config.node_dir, config.agent_id)?;
    let catalog = Catalog::load()?;
    let runtime = Runtime::load(&catalog)?;
    let checks = ReadChecks { catalog: &catalog, runtime: &runtime, config: &config,
        config_raw: &config_raw, pin_raw: &pin_raw, node: &node, signed_end: None, original_stopped: std::cell::Cell::new(false) };
    let mut checked_clock = || {
        checks.clock(&mut input, &mut pending)
    };
    let startup = crate::policy::wait_collector_service_read(
        Instant::now() + Duration::from_secs(3), actual_now()?, &mut checked_clock, || {
            let store = SqliteJournalStore::open(
                &config.db_dir, &config.anchor_dir, genesis.clone(),
                SqliteStorageBudget::default(),
            )?;
            Actor::new(store)
        });
    let (mut actor, _) = match startup {
        Ok(value) => value,
        Err(Error::Expired) if checks.original_stopped.get() => return Ok(()),
        Err(error) => return Err(error),
    };
    loop {
        if !launcher_alive(&mut input, &mut pending)? {
            return Ok(());
        }
        runtime.check(&catalog)?;
        original_config(&config_raw, &config.owner_pin, &pin_raw)?;
        if inspect_existing_node_identity(&config.node_dir, config.agent_id)? != node {
            return Err(Error::WrongClaim);
        }
        let original = actor.next_collector_service_waiting(
            config.agent_id, catalog.hash, actual_now()?,
            Instant::now() + Duration::from_secs(3),
            &mut || checks.clock(&mut input, &mut pending),
        );
        let original = match original {
            Ok(value) => value,
            Err(Error::Expired) if checks.original_stopped.get() => return Ok(()),
            Err(error) => return Err(error),
        };
        if let Some(policy) = original {
            run_one(
                &mut actor,
                &catalog,
                &runtime,
                &config,
                &config_raw,
                &pin_raw,
                &node,
                policy,
                &mut input,
                &mut pending,
            )?;
        } else {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

#[cfg(feature="owned-lifetime-protocol-proof")]
#[path="protocol_fixture_owner.rs"]
mod protocol_fixture_owner;
#[cfg(feature="owned-lifetime-protocol-proof")]
pub(crate) use protocol_fixture_owner::run_owned_protocol_component;

#[cfg(test)]
mod launch_stamp_tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn process_launch_stamp_follows_expensive_validation_without_extending_signed_end() {
        let observed=Cell::new(1_000_000_000u64);let deadline=60_000_000_000u64;
        let old_stamp=observed.get();
        let created=launch_stamp_after_validation(|| { observed.set(25_000_000_000);Ok(()) },||Ok(observed.get()),deadline).unwrap();
        let entry_now=observed.get()+500_000_000;
        assert!(entry_now-old_stamp>=10_000_000_000,"old placement spends the entire window on validation");
        assert!(entry_now-created<10_000_000_000,"same ten-second window follows validation");
        assert_eq!(created,25_000_000_000);assert_eq!(deadline,60_000_000_000);
        assert!(created<deadline);assert_eq!(deadline-created,35_000_000_000);
    }
    #[test]
    fn process_launch_stamp_validation_failure_or_original_expiry_cannot_renew() {
        let sampled=Cell::new(false);
        let denied=launch_stamp_after_validation(||Err(Error::Storage),||{sampled.set(true);Ok(1)},60);
        assert!(matches!(denied,Err(Error::Storage)));assert!(!sampled.get());
        for clock in [60,61] {
            assert!(matches!(launch_stamp_after_validation(||Ok(()),||Ok(clock),60),Err(Error::Expired)));
        }
        assert!(matches!(launch_stamp_after_validation(||Ok(()),||Err(Error::InvalidTime),60),Err(Error::InvalidTime)));
    }
}

#[cfg(test)]
mod original_launcher_stop_tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::os::fd::OwnedFd;
    #[test]
    fn collector_failure_diagnostic_preserves_error_when_stderr_is_broken() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        let stages = [FailureStage::LifetimeValidate, FailureStage::LifetimeDecode,
            FailureStage::LifetimeOpen, FailureStage::LifetimeProjection, FailureStage::Material,
            FailureStage::Session, FailureStage::Spawn, FailureStage::ReadyFrame,
            FailureStage::ReadyDecode, FailureStage::Runtime, FailureStage::Retire, FailureStage::Cleanup];
        let errors = [Error::InvalidWire, Error::InvalidKey, Error::InvalidSignature, Error::InvalidTime,
            Error::Expired, Error::WrongContext, Error::WrongEpoch, Error::WrongCheckpoint,
            Error::UnsupportedTeam, Error::Replay, Error::NotFound, Error::WrongState,
            Error::InvalidToken, Error::WrongClaim, Error::ScopeDenied, Error::CapabilityDenied,
            Error::MissingFreshness, Error::Revoked, Error::Quota, Error::Conflict,
            Error::Storage, Error::Entropy, Error::KeyReuse, Error::UncertainCommit];
        for stage in stages {
            for error in errors {
                assert_eq!(write_failure(stage, error, &mut Broken), error);
                let mut raw = Vec::new();
                assert_eq!(write_failure(stage, error, &mut raw), error);
                assert!(raw.len() < 128 && raw.last() == Some(&b'\n'));
                let value: Value = serde_json::from_slice(&raw).unwrap();
                assert_eq!(value.as_object().unwrap().len(), 3);
                assert_eq!(value["event"], "collectorServiceFailure");
                assert_eq!(value["stage"], stage.name());
                assert_eq!(value["code"], format!("{error:?}"));
            }
        }
    }
    #[test]
    fn supervisor_read_original_stop_is_graceful_only_after_verified_retirement() {
        for eof in [true, false] {
            let (reader, mut writer) = UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            let fd: OwnedFd = reader.into();
            let mut input = File::from(fd);
            if eof { drop(writer); } else { writer.write_all(b"STOP\n").unwrap(); }
            let stopped = std::cell::Cell::new(false);
            assert!(matches!(check_original_launcher(&mut input, &mut Vec::new(), &stopped), Err(Error::Expired)));
            assert!(stopped.get());
            assert!(finish_original_launcher_stop(Err(Error::Expired), stopped.get()).is_ok());
            // The caller's mandatory retirement/CAS failure is never translated.
            assert!(matches!(finish_original_launcher_stop(Err(Error::UncertainCommit), stopped.get()), Err(Error::UncertainCommit)));
            assert!(matches!(finish_original_launcher_stop(Err(Error::Revoked), stopped.get()), Err(Error::Revoked)));
        }
        assert!(matches!(finish_original_launcher_stop(Err(Error::Expired), false), Err(Error::Expired)));
    }
    #[test]
    fn supervisor_read_expiry_or_guard_failure_precedes_original_stop_marker() {
        for kind in 0..3 {
            let (reader, writer) = UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap(); drop(writer);
            let fd: OwnedFd = reader.into(); let mut input = File::from(fd);
            let stopped = std::cell::Cell::new(false);
            let value = observe_before_original_launcher(&mut input, &mut Vec::new(), &stopped, || {
                Err(match kind { 0 => Error::Expired, 1 => Error::Revoked, _ => Error::Storage })
            });
            assert!(value.is_err()); assert!(!stopped.get());
        }
    }
    #[test]
    fn supervisor_read_open_or_malformed_launcher_never_marks_owned_stop() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let fd: OwnedFd = reader.into();
        let mut input = File::from(fd);
        let stopped = std::cell::Cell::new(false);
        assert!(check_original_launcher(&mut input, &mut Vec::new(), &stopped).is_ok());
        assert!(!stopped.get());
        writer.write_all(b"BAD\n").unwrap();
        assert!(matches!(check_original_launcher(&mut input, &mut Vec::new(), &stopped), Err(Error::InvalidWire)));
        assert!(!stopped.get());
    }
}

#[cfg(test)]
mod retirement_frame_tests {
    use super::*;

    fn ready() -> Value {
        json!({"format":"cc-collector-service-ready/v1","session":"owned-session",
            "unit":"owned-unit","prometheusSha256":hex::encode([7u8;32])})
    }
    fn terminal() -> Value {
        json!({"format":"cc-collector-service-exit/v1","session":"owned-session",
            "unit":"owned-unit","unitEmpty":true,"leaderExited":true,"unitStopped":true,
            "privateNamespaceRemoved":false,"cleanupUnknown":false,"handlesClosed":true,
            "outcome":"stopped","retainedPublicFiles":[]})
    }
    fn bytes(values: &[Value]) -> Vec<u8> {
        values.iter().flat_map(|value| canonical(value).unwrap()).collect()
    }
    fn run_frames(prefix: Vec<u8>, suffix: Vec<u8>, fragmented: bool,
        exit_delay: f64, code: i32, ready_seen: bool, pending: Vec<u8>) -> (Result<Vec<PublicFile>>, bool, Duration) {
        // Real owned child and original pipe fds; never a Permit, Files, unit,
        // cgroup or joined-authority substitute.
        let program = r#"import os,sys,time
prefix=bytes.fromhex(sys.argv[1]); suffix=bytes.fromhex(sys.argv[2])
os.write(1,prefix)
control=b''
while len(control)<5:
    block=os.read(0,5-len(control))
    if not block:sys.exit(91)
    control+=block
if control!=b'STOP\n':sys.exit(92)
step=7 if sys.argv[3]=='1' else max(1,len(suffix))
for offset in range(0,len(suffix),step):
    os.write(1,suffix[offset:offset+step])
    if sys.argv[3]=='1':time.sleep(.001)
os.close(1)
time.sleep(float(sys.argv[4]))
sys.exit(int(sys.argv[5]))
"#;
        let mut child = Command::new("/usr/bin/python3").args(["-I","-B","-c",program])
            .arg(hex::encode(prefix)).arg(hex::encode(suffix)).arg(if fragmented {"1"} else {"0"})
            .arg(exit_delay.to_string()).arg(code.to_string())
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let original_in=child.stdin.take().unwrap(); let original_out=child.stdout.take().unwrap();
        let input=OpenOptions::new().write(true).custom_flags(NONBLOCK|CLOEXEC)
            .open(format!("/proc/self/fd/{}",original_in.as_raw_fd())).unwrap();
        let output=OpenOptions::new().read(true).custom_flags(NONBLOCK|CLOEXEC)
            .open(format!("/proc/self/fd/{}",original_out.as_raw_fd())).unwrap();
        drop(original_in);drop(original_out);
        let mut adapter=Adapter { child,input,output,pending,exited:false,stopped:false };
        let started=Instant::now();
        let result=stop_join_frames(&mut adapter,"owned-session","owned-unit",Some(Hash::from_bytes([7;32])),ready_seen);
        let elapsed=started.elapsed();let joined=adapter.exited;
        // Join on all negative paths, too, before making any assertion.
        let _=adapter.child.kill();let _=adapter.child.wait();adapter.exited=true;
        (result,joined,elapsed)
    }
    #[test]
    fn retirement_late_ready_is_schema_error_in_old_decoder_but_real_join_now_succeeds() {
        assert!(matches!(decode::<ServiceEnd>(ready()),Err(Error::InvalidWire)));
        let (result,joined,_)=run_frames(bytes(&[ready()]),bytes(&[terminal()]),false,0.0,0,false,vec![]);
        assert!(result.unwrap().is_empty());assert!(joined);
    }
    #[test]
    fn retirement_coalesced_fragmented_and_preconsumed_ready_preserve_original_frames() {
        for fragmented in [false,true] {
            let (result,joined,_)=run_frames(vec![],bytes(&[ready(),terminal()]),fragmented,0.0,0,false,vec![]);
            assert!(result.unwrap().is_empty());assert!(joined);
        }
        let (result,joined,_)=run_frames(vec![],bytes(&[terminal()]),true,0.0,1,true,vec![]);
        assert!(result.unwrap().is_empty());assert!(joined);
        // Buffered record is consumed before a new read, remaining terminal
        // bytes survive until the next call, and original stdout EOF is real.
        let (result,joined,_)=run_frames(vec![],vec![],false,0.0,0,false,bytes(&[ready(),terminal()]));
        assert!(result.unwrap().is_empty());assert!(joined);
    }
    #[test]
    fn retirement_duplicate_foreign_hash_trailing_partial_quota_and_nonzero_deny() {
        let mut foreign=ready();foreign["session"]=json!("foreign");
        let mut wrong_unit=ready();wrong_unit["unit"]=json!("foreign");
        let mut wrong_hash=ready();wrong_hash["prometheusSha256"]=json!(hex::encode([8u8;32]));
        let mut unknown=ready();unknown["format"]=json!("unknown");
        let mut bad_terminal=terminal();bad_terminal["unitEmpty"]=json!(false);
        let invalids=vec![bytes(&[ready(),ready(),terminal()]),bytes(&[terminal(),terminal()]),
            bytes(&[foreign,terminal()]),bytes(&[wrong_unit,terminal()]),bytes(&[wrong_hash,terminal()]),
            bytes(&[unknown,terminal()]),bytes(&[bad_terminal]),bytes(&[terminal(),ready()]),
            [bytes(&[terminal()]),b"x".to_vec()].concat(),b"{\"partial\":".to_vec(),
            b"{\"a\":1, \"b\":2}\n".to_vec(),vec![]];
        for suffix in invalids {
            let (result,_,_)=run_frames(vec![],suffix,false,0.0,0,false,vec![]);
            assert!(result.is_err());
        }
        let (result,_,_)=run_frames(vec![],bytes(&[ready(),terminal()]),false,0.0,0,true,vec![]);
        assert!(matches!(result,Err(Error::WrongState)));
        let (result,_,_)=run_frames(vec![],vec![],false,0.0,0,false,vec![b'x';4097]);
        assert!(matches!(result,Err(Error::Quota)));
        let (result,joined,_)=run_frames(vec![],bytes(&[terminal()]),false,0.0,7,true,vec![]);
        assert!(matches!(result,Err(Error::UncertainCommit)));assert!(joined);
    }
    #[test]
    fn retirement_original_eof_does_not_replace_original_child_exit() {
        let (result,joined,elapsed)=run_frames(vec![],bytes(&[ready(),terminal()]),false,0.25,0,false,vec![]);
        assert!(result.unwrap().is_empty());assert!(joined);assert!(elapsed>=Duration::from_millis(240));
    }
}
