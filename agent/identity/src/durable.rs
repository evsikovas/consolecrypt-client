//! Bounded signed-transition journal. The independent anchor must not be restored
//! together with data. Losing either side or a torn commit fails closed.
use crate::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_RECORD_BYTES: u64 = 64 * 1024;
const MAX_RECORDS: usize = 10_000;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
pub(crate) enum Transition {
    Offer(Signed<Offer>),
    Consume {
        token_hash: Hash,
        signed: Signed<Claim>,
    },
    Approve(Signed<Certificate>),
    Reject(Signed<Rejection>),
    Expire(InvitationId),
    Revoke(Signed<Revocation>),
    Rotate {
        certificate: Signed<Certificate>,
        old: Signed<RotationProof>,
        new: Signed<NewKeyProof>,
    },
    RotateOwner {
        old: Signed<OwnerRotation>,
        new: Signed<NewOwnerProof>,
    },
    Admit {
        lease: Signed<Lease>,
        proof: Signed<AgentRequest>,
        freshness: Signed<Freshness>,
        operation: Operation,
    },
    ResumeTunnel {
        lease: Signed<Lease>,
        proof: Signed<AgentRequest>,
        freshness: Signed<Freshness>,
        policy: TunnelRoutePolicyV2,
    },
    TunnelDebit {
        lease_id: LeaseId,
        channel: Nonce,
        policy: TunnelRoutePolicyV2,
        bytes: u64,
        prior_total: u64,
        prior_channel: u64,
    },
    TunnelDrained { lease_id: LeaseId, channel: Nonce },
    CollectorServiceAdmit { policy: Signed<CollectorServiceDelegation>, proof: Signed<CollectorServiceClaim> },
    CollectorServiceStart { service_id: ResourceId, delegation_hash: Hash, prior_starts: u32, start: Nonce, observed_boot: ResourceId, observed_boottime_ns: u64 },
    CollectorServiceEnd { service_id: ResourceId, delegation_hash: Hash, start: Nonce, reason:CollectorServiceEndReason },
    CollectorServiceRevoke(Signed<CollectorServiceRevocation>),
    CollectorServiceSingleStartSelected { service_id:ResourceId, delegation_hash:Hash, start:Nonce, observed_boot:ResourceId, observed_boottime_ns:u64 },
    CollectorServiceCredentialSetAdmit { set:StoredCollectorServiceCredentialSet, manifest:serde_json::Value, observed_boot:ResourceId, observed_boottime_ns:u64 },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub at: Timestamp,
    pub transition: Transition,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Genesis {
    pub(crate) version: u32,
    pub(crate) pin: OwnerPin,
    pub(crate) at: Timestamp,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    revision: u64,
    digest: Hash,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    pub revision: u64,
    pub policy: PolicyCapacity,
    pub journal_records: u64,
    pub journal_data_bytes: u64,
    pub max_journal_records: usize,
    pub max_journal_data_bytes: u64,
}

/// Local single-writer durable adapter. Public keys/statements/hashes only; no
/// invitation token or private key is written. Pin is independently supplied.
/// This is not TPM protection: rollback of both directories is undetectable.
pub struct FileStore {
    data: PathBuf,
    anchor: PathBuf,
    pin: OwnerPin,
}
impl FileStore {
    pub fn status(&self) -> Result<StorageStatus> {
        let _lock = self.lock()?;
        let (state, _, bytes) = self.restore()?;
        Ok(StorageStatus {
            revision: state.revision(),
            policy: state.capacity(),
            journal_records: state.revision() + 1,
            journal_data_bytes: bytes,
            max_journal_records: MAX_RECORDS,
            max_journal_data_bytes: MAX_TOTAL_BYTES,
        })
    }
    pub fn create(
        data: impl AsRef<Path>,
        anchor: impl AsRef<Path>,
        pin: OwnerPin,
        now: Now,
    ) -> Result<Self> {
        let state = State::bootstrap(pin.clone(), now)?;
        let data = absolute(data.as_ref())?;
        let anchor = absolute(anchor.as_ref())?;
        separate(&data, &anchor)?;
        secure_new_dir(&data)?;
        secure_new_dir(&anchor)?;
        let store = Self { data, anchor, pin };
        let _lock = store.lock()?;
        let bytes = serde_json::to_vec(&Genesis {
            version: 1,
            pin: state.owner().clone(),
            at: now.0,
        })
        .map_err(|_| Error::Storage)?;
        store.write_record(0, &bytes, Hash::from_bytes([0; 32]))?;
        Ok(store)
    }
    pub fn open(data: impl AsRef<Path>, anchor: impl AsRef<Path>, pin: OwnerPin) -> Result<Self> {
        let data = absolute(data.as_ref())?;
        let anchor = absolute(anchor.as_ref())?;
        separate(&data, &anchor)?;
        let store = Self { data, anchor, pin };
        store.load()?;
        Ok(store)
    }
    fn lock(&self) -> Result<Lock> {
        secure_path(&self.data)?;
        secure_path(&self.anchor)?;
        let path = self.data.join("writer.lock");
        let mut file = create_file(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Conflict
            } else {
                Error::Storage
            }
        })?;
        file.write_all(b"consolecrypt-identity-exclusive-v1\n")
            .map_err(|_| Error::Storage)?;
        file.sync_all().map_err(|_| Error::Storage)?;
        Ok(Lock(path))
    }
    fn restore(&self) -> Result<(State, Hash, u64)> {
        let records = inventory(&self.data, true)?;
        let anchors = inventory(&self.anchor, false)?;
        if records != anchors || records == 0 || records > MAX_RECORDS {
            return Err(Error::WrongCheckpoint);
        }
        let mut previous = Hash::from_bytes([0; 32]);
        let mut state = None;
        let mut total = 0u64;
        for n in 0..records {
            let bytes = read_bounded(&self.data.join(name(n as u64)))?;
            total = total.checked_add(bytes.len() as u64).ok_or(Error::Quota)?;
            if total > MAX_TOTAL_BYTES {
                return Err(Error::Quota);
            }
            let stamp: Anchor =
                serde_json::from_slice(&read_bounded(&self.anchor.join(name(n as u64)))?)
                    .map_err(|_| Error::Storage)?;
            let digest = chained(previous, &bytes);
            if stamp.revision != n as u64 || stamp.digest != digest {
                return Err(Error::WrongCheckpoint);
            }
            if n == 0 {
                let genesis: Genesis =
                    serde_json::from_slice(&bytes).map_err(|_| Error::Storage)?;
                if genesis.version != 1 || genesis.pin != self.pin {
                    return Err(Error::WrongContext);
                }
                state = Some(State::bootstrap(genesis.pin, Now(genesis.at))?);
            } else {
                let entry: Entry = serde_json::from_slice(&bytes).map_err(|_| Error::Storage)?;
                let mut actor = Actor::new(Memory(state.take().ok_or(Error::Storage)?))?;
                replay(&mut actor, entry)?;
                state = Some(actor.into_store().0);
            }
            previous = digest;
        }
        Ok((state.ok_or(Error::Storage)?, previous, total))
    }
    fn write_record(&self, revision: u64, bytes: &[u8], previous: Hash) -> Result<()> {
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(Error::Quota);
        }
        write_new(&self.data.join(name(revision)), bytes)?;
        sync_dir(&self.data)?;
        let stamp = Anchor {
            revision,
            digest: chained(previous, bytes),
        };
        write_new(
            &self.anchor.join(name(revision)),
            &serde_json::to_vec(&stamp).map_err(|_| Error::Storage)?,
        )?;
        sync_dir(&self.anchor)
    }
}
impl CompareAndSwap for FileStore {
    fn load(&self) -> Result<State> {
        let _lock = self.lock()?;
        self.restore().map(|v| v.0)
    }
    fn compare_and_swap(&mut self, expected: u64, next: State) -> Result<()> {
        let _lock = self.lock()?;
        let (current, digest, total) = self.restore()?;
        if current.revision() != expected {
            return Err(Error::Conflict);
        }
        if next.revision() != expected.checked_add(1).ok_or(Error::Quota)?
            || next.owner().context != self.pin.context
        {
            return Err(Error::Storage);
        }
        if next.revision() as usize + 1 >= MAX_RECORDS {
            return Err(Error::Quota);
        }
        // Revalidate the supplied successor transition, rather than accepting an
        // opaque caller snapshot. Replay also verifies every historical signature.
        let entry = next.latest.clone().ok_or(Error::Storage)?;
        let mut actor = Actor::new(Memory(current))?;
        replay(&mut actor, entry.clone())?;
        let checked = actor.into_store().0;
        if checked.revision() != next.revision() {
            return Err(Error::Storage);
        }
        let bytes = serde_json::to_vec(&entry).map_err(|_| Error::Storage)?;
        if total + bytes.len() as u64 > MAX_TOTAL_BYTES {
            return Err(Error::Quota);
        }
        self.write_record(next.revision(), &bytes, digest)
    }
}
pub(crate) struct Memory(pub(crate) State);
impl CompareAndSwap for Memory {
    fn load(&self) -> Result<State> {
        Ok(self.0.clone())
    }
    fn compare_and_swap(&mut self, expected: u64, next: State) -> Result<()> {
        if self.0.revision() != expected {
            return Err(Error::Conflict);
        }
        self.0 = next;
        Ok(())
    }
}
pub(crate) fn replay(actor: &mut Actor<Memory>, entry: Entry) -> Result<()> {
    let now = Now(entry.at);
    match entry.transition {
        Transition::Offer(v) => actor.offer(v, now),
        Transition::Consume { token_hash, signed } => {
            actor.consume_digest(token_hash, signed, now).map(|_| ())
        }
        Transition::Approve(v) => actor.approve(v, now),
        Transition::Reject(v) => actor.reject(v, now),
        Transition::Expire(v) => actor.expire(v, now),
        Transition::Revoke(v) => actor.revoke(v, now),
        Transition::Rotate {
            certificate,
            old,
            new,
        } => actor.rotate(certificate, old, new, now),
        Transition::RotateOwner { old, new } => actor.rotate_owner(old, new, now),
        Transition::Admit {
            lease,
            proof,
            freshness,
            operation,
        } => actor
            .admit(lease, proof, Some(&freshness), &operation, now)
            .map(|_| ()),
        Transition::ResumeTunnel { lease, proof, freshness, policy } =>
            actor.resume_tunnel(lease, proof, &freshness, &policy, now).map(|_| ()),
        Transition::TunnelDebit { lease_id, channel, policy, bytes, prior_total, prior_channel } =>
            actor.replay_tunnel_debit(lease_id, channel, &policy, bytes, prior_total, prior_channel, now),
        Transition::TunnelDrained { lease_id, channel } =>
            actor.replay_tunnel_drained(lease_id, channel, now),
        Transition::CollectorServiceAdmit { policy, proof } => actor.replay_collector_service_admit(policy, proof, now),
        Transition::CollectorServiceStart { service_id, delegation_hash, prior_starts, start, observed_boot, observed_boottime_ns } =>
            actor.replay_collector_service_start(service_id, delegation_hash, prior_starts, start, observed_boot, observed_boottime_ns, now),
        Transition::CollectorServiceEnd { service_id, delegation_hash, start, reason } =>
            actor.replay_collector_service_end(service_id, delegation_hash, start, reason, now),
        Transition::CollectorServiceRevoke(signed) => actor.revoke_collector_service(signed, now),
        Transition::CollectorServiceSingleStartSelected {service_id,delegation_hash,start,observed_boot,observed_boottime_ns} => actor.replay_collector_single_start(service_id,delegation_hash,start,observed_boot,observed_boottime_ns,now),
        Transition::CollectorServiceCredentialSetAdmit {set,manifest,observed_boot,observed_boottime_ns} => actor.replay_collector_service_lifetime(set,manifest,observed_boot,observed_boottime_ns,now),
    }
}
fn name(n: u64) -> String {
    format!("{n:020}.json")
}
pub(crate) fn chained(previous: Hash, bytes: &[u8]) -> Hash {
    let mut bound = previous.as_bytes().to_vec();
    bound.extend(bytes);
    crate::wire::hash(b"consolecrypt.agent.durable-journal.v1", &bound)
}
fn inventory(dir: &Path, allow_lock: bool) -> Result<usize> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|_| Error::Storage)? {
        let entry = entry.map_err(|_| Error::Storage)?;
        let item = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Storage)?;
        if allow_lock && item == "writer.lock" {
            continue;
        }
        names.push(item);
        if names.len() > MAX_RECORDS {
            return Err(Error::Quota);
        }
    }
    names.sort();
    if names.iter().enumerate().any(|(n, v)| *v != name(n as u64)) {
        return Err(Error::WrongCheckpoint);
    }
    Ok(names.len())
}
pub(crate) fn absolute(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute()
        || path.components().any(|v| {
            matches!(
                v,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(Error::Storage);
    }
    Ok(path.to_path_buf())
}
fn separate(a: &Path, b: &Path) -> Result<()> {
    if a.starts_with(b) || b.starts_with(a) {
        Err(Error::Storage)
    } else {
        Ok(())
    }
}
pub(crate) fn secure_path(path: &Path) -> Result<()> {
    for component in path.ancestors() {
        let meta = fs::symlink_metadata(component).map_err(|_| Error::Storage)?;
        if meta.file_type().is_symlink() {
            return Err(Error::Storage);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(Error::Storage);
            }
        }
    }
    if !path.is_dir() {
        return Err(Error::Storage);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path)
            .map_err(|_| Error::Storage)?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(Error::Storage);
        }
    }
    Ok(())
}
pub(crate) fn secure_new_dir(path: &Path) -> Result<()> {
    secure_path(path.parent().ok_or(Error::Storage)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|_| Error::Storage)?;
    }
    #[cfg(not(unix))]
    fs::create_dir(path).map_err(|_| Error::Storage)?;
    secure_path(path)
}
fn create_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = create_file(path).map_err(|_| Error::Storage)?;
    file.write_all(bytes).map_err(|_| Error::Storage)?;
    file.sync_all().map_err(|_| Error::Storage)
}
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_RECORD_BYTES {
        return Err(Error::Storage);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.nlink() != 1 || meta.permissions().mode() & 0o077 != 0 {
            return Err(Error::Storage);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(Error::Storage);
        }
    }
    let file = File::open(path).map_err(|_| Error::Storage)?;
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(Error::Storage);
    }
    Ok(bytes)
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|v| v.sync_all())
            .map_err(|_| Error::Storage)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
