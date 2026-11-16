//! Dedicated private TunnelOpenBound custody. Never the Reader/legacy worker.
use super::sqlite_worker::StrictJson;
use cc_agent_identity_policy::*;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{Metadata, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

#[derive(PartialEq, Eq)]
struct FileIdentity(u64, u64, u64, u32, u32, i64, i64, i64, i64);
fn identity(m: &Metadata) -> FileIdentity {
    FileIdentity(
        m.dev(),
        m.ino(),
        m.len(),
        m.uid(),
        m.mode(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
fn read_public(path: &Path, uid: u32) -> Result<(Zeroizing<Vec<u8>>, FileIdentity)> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(Error::Storage);
    }
    for parent in path.ancestors().skip(1) {
        let m = std::fs::symlink_metadata(parent).map_err(|_| Error::Storage)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(Error::Storage);
        }
    }
    let before = std::fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.nlink() != 1
        || before.uid() != uid
        || before.mode() & 0o777 != 0o600
        || before.len() == 0
        || before.len() > MAX_WIRE_BYTES as u64
    {
        return Err(Error::Storage);
    }
    // Linux only: nofollow+nonblock prevents replaced symlink/FIFO blocking.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(0x20000 | 0x800)
        .open(path)
        .map_err(|_| Error::Storage)?;
    if identity(&before) != identity(&file.metadata().map_err(|_| Error::Storage)?) {
        return Err(Error::Storage);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    Read::by_ref(&mut file)
        .take(MAX_WIRE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() as u64 != before.len()
        || identity(&before) != identity(&file.metadata().map_err(|_| Error::Storage)?)
        || identity(&before)
            != identity(&std::fs::symlink_metadata(path).map_err(|_| Error::Storage)?)
    {
        return Err(Error::Storage);
    }
    Ok((bytes, identity(&before)))
}

struct FixedInput {
    path: PathBuf,
    uid: u32,
    identity: FileIdentity,
    hash: [u8; 32],
}
impl FixedInput {
    fn load<T: serde::de::DeserializeOwned>(path: PathBuf, uid: u32) -> Result<(T, Self)> {
        let (raw, identity) = read_public(&path, uid)?;
        let StrictJson(value) = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
        let value = serde_json::from_value(value).map_err(|_| Error::InvalidWire)?;
        let hash = Sha256::digest(raw.as_slice()).into();
        Ok((
            value,
            Self {
                path,
                uid,
                identity,
                hash,
            },
        ))
    }
    fn check(&self) -> Result<()> {
        let (raw, identity) = read_public(&self.path, self.uid)?;
        let hash: [u8; 32] = Sha256::digest(raw.as_slice()).into();
        if identity != self.identity || hash != self.hash {
            return Err(Error::Storage);
        }
        Ok(())
    }
}

struct Handle([u8; 32]);
impl Drop for Handle {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl Handle {
    fn parse(text: &str) -> Result<Self> {
        if text.len() != 64
            || !text
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(Error::InvalidToken);
        }
        let mut value = [0; 32];
        hex::decode_to_slice(text, &mut value).map_err(|_| Error::InvalidToken)?;
        if value == [0; 32] {
            return Err(Error::InvalidToken);
        }
        Ok(Self(value))
    }
    fn same(&self, other: &Self) -> bool {
        bool::from(self.0.ct_eq(&other.0))
    }
}
struct Entry {
    handle: Handle,
    permit: Permit,
    started: Instant,
}
#[derive(Clone, Copy)]
struct Sample {
    raw: Duration,
    mono: Instant,
    now: Now,
}
struct Worker {
    actor: Actor<SqliteJournalStore>,
    policy: TunnelRoutePolicy,
    node: PathBuf,
    fixed: Vec<FixedInput>,
    entries: Vec<Entry>,
    last: Option<Sample>,
    clock_failed: bool,
}
impl Worker {
    fn sample(&mut self) -> Result<Sample> {
        let value = (|| {
            if self.clock_failed {
                return Err(Error::InvalidTime);
            }
            let raw = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::InvalidTime)?;
            let mono = Instant::now();
            if raw > Duration::from_secs(253_402_300_799)
                || self.last.is_some_and(|x| raw < x.raw || mono < x.mono)
            {
                return Err(Error::InvalidTime);
            }
            Ok(Sample {
                raw,
                mono,
                now: Now(Timestamp::new(raw.as_secs())?),
            })
        })();
        match value {
            Ok(value) => {
                self.last = Some(value);
                Ok(value)
            }
            Err(error) => {
                self.clock_failed = true;
                self.entries.clear();
                Err(error)
            }
        }
    }
    fn live(&self, entry: &Entry, sample: Sample) -> Result<()> {
        if sample.raw >= Duration::from_secs(entry.permit.expires_at().seconds())
            || sample
                .mono
                .checked_duration_since(entry.started)
                .ok_or(Error::InvalidTime)?
                >= Duration::from_secs(self.policy.limits.max_ttl_seconds)
        {
            return Err(Error::Expired);
        }
        Ok(())
    }
    fn context(&self) -> Result<NodeInspectionGuard> {
        for fixed in &self.fixed {
            fixed.check()?;
        }
        let guard = inspect_node_state(&self.node, self.policy.agent_id)?;
        if guard.facts().actual_selected_epoch != Some(self.policy.key_epoch) {
            return Err(Error::WrongEpoch);
        }
        Ok(guard)
    }
    fn handle(&self) -> Result<Handle> {
        let mut bytes = Zeroizing::new([0; 32]);
        getrandom::fill(bytes.as_mut()).map_err(|_| Error::Entropy)?;
        let handle = Handle(*bytes);
        if handle.0 == [0; 32] || self.entries.iter().any(|e| e.handle.same(&handle)) {
            return Err(Error::Entropy);
        }
        Ok(handle)
    }
    fn begin(&mut self, binding: NodeRequestBinding, actual_peer: Hash) -> Result<Value> {
        let before = self.sample()?;
        self.entries.retain(|e| {
            before.raw < Duration::from_secs(e.permit.expires_at().seconds())
                && before.mono.duration_since(e.started)
                    < Duration::from_secs(self.policy.limits.max_ttl_seconds)
        });
        self.policy.check_binding(&binding)?;
        if actual_peer != self.policy.client_tls_certificate_sha256 {
            return Err(Error::ScopeDenied);
        }
        // ONE actual route Permit; all <=4 data channels share it and one
        // original byte/TTL generation in the Agent controller, never re-admit.
        if !self.entries.is_empty() {
            return Err(Error::Quota);
        }
        let handle = self.handle()?; // Capacity+entropy before any one-use commit.
        let node_guard = self.context()?;
        // The file pin is the independent GENESIS trust root, not a current
        // checkpoint. Obtain the actual verified current pin from this store.
        let current_pin = self.actor.snapshot()?.owner().clone();
        let sign_time = self.sample()?;
        if sign_time.mono.duration_since(before.mono)
            >= Duration::from_secs(self.policy.limits.max_ttl_seconds)
        {
            return Err(Error::Expired);
        }
        let proof = create_tunnel_node_request(
            &self.node,
            self.policy.agent_id,
            &current_pin,
            binding.clone(),
            &self.policy,
            sign_time.now,
        )?;
        let ready = self.sample()?;
        if ready.mono.duration_since(before.mono)
            >= Duration::from_secs(self.policy.limits.max_ttl_seconds)
            || ready.raw >= Duration::from_secs(binding.lease.body.expires_at.seconds())
        {
            return Err(Error::Expired);
        }
        let permit = self.actor.admit(
            binding.lease,
            proof.clone(),
            Some(&binding.freshness),
            &self.policy.operation()?,
            ready.now,
        )?;
        let entry = Entry {
            handle,
            permit,
            started: before.mono,
        };
        let after = self.sample()?;
        self.live(&entry, after)?;
        node_guard.recheck_guard_identity()?;
        for fixed in &self.fixed {
            fixed.check()?;
        }
        let checked = self.sample()?;
        self.live(&entry, checked)?;
        self.actor.check_permit(&entry.permit, checked.now)?;
        let final_time = self.sample()?;
        self.live(&entry, final_time)?;
        let encoded = Zeroizing::new(hex::encode(entry.handle.0));
        let metadata = json!({"handle":encoded.as_str(),"agentId":self.policy.agent_id,
            "route":self.policy.route_id,"policyHash":self.policy.digest()?,
            "expiresAt":entry.permit.expires_at(),"proof":proof});
        self.entries.push(entry);
        Ok(metadata)
    }
    fn check(&mut self, handle: Handle) -> Result<Value> {
        let before = self.sample()?;
        let index = self
            .entries
            .iter()
            .position(|e| e.handle.same(&handle))
            .ok_or(Error::InvalidToken)?;
        let entry = self.entries.remove(index); // Errors retire; never revive a failed handle.
        self.live(&entry, before)?;
        let node_guard = self.context()?;
        self.actor.check_permit(&entry.permit, before.now)?;
        node_guard.recheck_guard_identity()?;
        for fixed in &self.fixed {
            fixed.check()?;
        }
        let after = self.sample()?;
        self.live(&entry, after)?;
        self.entries.push(entry);
        Ok(json!({"phase":"live"}))
    }
    fn close(&mut self, handle: Handle) -> Value {
        self.entries.retain(|entry| !entry.handle.same(&handle));
        json!({"phase":"closed"}) // Drop-only idempotence, never an authorization.
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    #[serde(rename_all = "camelCase")]
    Begin {
        binding: Box<Value>,
        client_tls_certificate_sha256: Hash,
    },
    Check {
        handle: String,
    },
    Close {
        handle: String,
    },
}
fn emit(value: Value) -> Result<()> {
    let mut raw = Zeroizing::new(serde_json::to_vec(&value).map_err(|_| Error::Storage)?);
    if raw.len() >= 4096 {
        return Err(Error::Quota);
    }
    raw.push(b'\n');
    let mut out = std::io::stdout().lock();
    out.write_all(&raw)
        .and_then(|_| out.flush())
        .map_err(|_| Error::Storage)
}
pub(super) fn run(args: &[String]) -> Result<()> {
    if args.len() != 11
        || args[1] != "--db-dir"
        || args[3] != "--anchor-dir"
        || args[5] != "--owner-pin"
        || args[7] != "--node-dir"
        || args[9] != "--policy"
    {
        return Err(Error::InvalidWire);
    }
    let node = PathBuf::from(&args[8]);
    let uid = std::fs::symlink_metadata(&node)
        .map_err(|_| Error::Storage)?
        .uid();
    let (pin, pin_file): (OwnerPin, _) = FixedInput::load(PathBuf::from(&args[6]), uid)?;
    let (policy, policy_file): (TunnelRoutePolicy, _) =
        FixedInput::load(PathBuf::from(&args[10]), uid)?;
    policy.validate()?;
    if policy.context != pin.context {
        return Err(Error::WrongContext);
    }
    let actor = Actor::new(SqliteJournalStore::open(
        &args[2],
        &args[4],
        pin.clone(),
        SqliteStorageBudget::default(),
    )?)?;
    let mut worker = Worker {
        actor,
        policy,
        node,
        fixed: vec![pin_file, policy_file],
        entries: Vec::new(),
        last: None,
        clock_failed: false,
    };
    let mut input = std::io::stdin().lock();
    loop {
        let mut raw = Zeroizing::new(Vec::new());
        let size = input
            .by_ref()
            .take(MAX_WIRE_BYTES as u64 + 1)
            .read_until(b'\n', &mut raw)
            .map_err(|_| Error::Storage)?;
        if size == 0 {
            return Ok(());
        }
        if size > MAX_WIRE_BYTES || !raw.ends_with(b"\n") {
            return Err(Error::InvalidWire);
        }
        let StrictJson(value) = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
        let command: Command = serde_json::from_value(value).map_err(|_| Error::InvalidWire)?;
        let result = match command {
            Command::Begin {
                binding,
                client_tls_certificate_sha256,
            } => serde_json::from_value(*binding)
                .map_err(|_| Error::InvalidWire)
                .and_then(|binding| worker.begin(binding, client_tls_certificate_sha256)),
            Command::Check { handle } => {
                worker.check(Handle::parse(Zeroizing::new(handle).as_str())?)
            }
            Command::Close { handle } => {
                Ok(worker.close(Handle::parse(Zeroizing::new(handle).as_str())?))
            }
        };
        match result {
            Ok(metadata) => emit(json!({"ok":true,"metadata":metadata}))?,
            Err(error) => {
                emit(json!({"ok":false,"code":error.to_string()}))?;
                if worker.clock_failed
                    || matches!(
                        error,
                        Error::Storage | Error::UncertainCommit | Error::Entropy
                    )
                {
                    return Err(error);
                }
            }
        }
    }
}
