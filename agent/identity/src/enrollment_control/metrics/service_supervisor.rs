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
fn stop_join<'a>(
    adapter: &mut Adapter,
    files: &Files,
    permit: &'a CollectorServicePermit,
) -> Result<JoinedCollectorService<'a>> {
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
    let mut public_files = Vec::new();
    while Instant::now() < end {
        if !observed {
            if let Ok(Some(value)) = adapter.frame() {
                let v: ServiceEnd = decode(value)?;
                observed = v.format == "cc-collector-service-exit/v1"
                    && v.session == files.session
                    && v.unit == files.unit
                    && v.unit_empty
                    && v.leader_exited
                    && v.unit_stopped
                    && !v.private_namespace_removed
                    && !v.cleanup_unknown
                    && v.handles_closed
                    && matches!(v.outcome.as_str(), "stopped" | "unknown");
                if observed {
                    if v.retained_public_files.len() > 5 {
                        return Err(Error::Quota);
                    }
                    for leaf in &v.retained_public_files {
                        leaf.check()?;
                    }
                    public_files = v.retained_public_files;
                }
            }
        }
        match adapter.child.try_wait().map_err(|_| Error::Storage)? {
            Some(status) => {
                adapter.exited = true;
                if observed && matches!(status.code(), Some(0 | 1)) {
                    return Ok(JoinedCollectorService {
                        permit,
                        public_files,
                    });
                }
                return Err(Error::UncertainCommit);
            }
            None => {}
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // No end token and no quota release on timeout, even if kill is accepted.
    let _ = adapter.child.kill();
    Err(Error::UncertainCommit)
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
        )?;
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
        let snapshot = actor.snapshot()?;
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
        actor.check_collector_service(
            &permit,
            actual_now()?,
            &mut CollectorServiceClockObservation::observe_local,
        )?;
        Ok((digest(&raw), deadline, expires))
    })();
    let (config_hash, deadline, expires) = match setup {
        Ok(value) => value,
        Err(error) => {
            let ended = retire_unlaunched(&mut files, &permit)?;
            actor.finish_unlaunched_collector_service(&permit, ended, actual_now()?)?;
            return Err(error);
        }
    };
    // Registered material is created before any OS child exists. Only now
    // disable Drop cleanup: spawn may create a real child and then fail.
    files.cleanup_allowed = false;
    let mut created = false;
    let mut adapter =
        match Adapter::spawn_fixed_observed(&files, config_hash, ADAPTER_SERVICE, &mut created) {
            Ok(adapter) => adapter,
            Err(error) if !created => {
                let ended = retire_unlaunched(&mut files, &permit)?;
                actor.finish_unlaunched_collector_service(&permit, ended, actual_now()?)?;
                return Err(error);
            }
            Err(error) => return Err(error), // A real Child existed: preserve Unknown.
        };
    let action = (|| -> Result<()> {
        let ready_end = Instant::now() + Duration::from_secs(10);
        loop {
            files.check_service_files()?;
            runtime.check(catalog)?;
            original_config(config_raw, &config.owner_pin, pin_raw)?;
            if inspect_existing_node_identity(&config.node_dir, config.agent_id)? != *node {
                return Err(Error::WrongClaim);
            }
            actor.check_collector_service(
                &permit,
                actual_now()?,
                &mut CollectorServiceClockObservation::observe_local,
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
            if let Some(value) = adapter.frame()? {
                let ready: DiskAdapterReady = decode(value)?;
                if ready.format != "cc-collector-service-ready/v1"
                    || ready.session != files.session
                    || ready.unit != files.unit
                    || ready.prometheus_sha256
                        != hex::encode(
                            catalog
                                .disk
                                .as_ref()
                                .ok_or(Error::CapabilityDenied)?
                                .binary_hash
                                .as_bytes(),
                        )
                {
                    return Err(Error::WrongClaim);
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
            actor.check_collector_service(
                &permit,
                actual_now()?,
                &mut CollectorServiceClockObservation::observe_local,
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
    let ended = stop_join(&mut adapter, &files, &permit)?;
    files.retain_service_public(&ended)?;
    files.cleanup_allowed = true;
    files.cleanup()?;
    actor.finish_joined_collector_service(&permit, ended, actual_now()?)?;
    action
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
    let store = SqliteJournalStore::open(
        &config.db_dir,
        &config.anchor_dir,
        genesis,
        SqliteStorageBudget::default(),
    )?;
    let mut actor = Actor::new(store)?;
    let catalog = Catalog::load()?;
    let runtime = Runtime::load(&catalog)?;
    loop {
        if !launcher_alive(&mut input, &mut pending)? {
            return Ok(());
        }
        runtime.check(&catalog)?;
        original_config(&config_raw, &config.owner_pin, &pin_raw)?;
        if inspect_existing_node_identity(&config.node_dir, config.agent_id)? != node {
            return Err(Error::WrongClaim);
        }
        let original = actor.next_collector_service(
            config.agent_id,
            catalog.hash,
            actual_now()?,
            &mut CollectorServiceClockObservation::observe_local,
        )?;
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
