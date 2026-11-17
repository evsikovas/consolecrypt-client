//! Closed encrypted/public handoff IO only. Actual Core Actor/current-start
//! checks MUST surround these operations in the fixed SSH handler/supervisor.
//! A directory FD, namespace, signed DTO or successful IO is NOT a Permit.
//! Core pre-registers all three fixed names and alone owns inode adoption,
//! lifetime, positive end/join and cleanup. This module NEVER unlinks.
use super::*;
use crate::{CollectorCredentialScope, CollectorRecipientAttestation, RegisteredServiceNamespace};
use cc_agent_node_wire::Statement as NativeStatement;

const MAX_FRAME: usize = crate::MAX_WIRE_BYTES;
const DIRECTORY: i32 = rustix::fs::OFlags::DIRECTORY.bits() as i32;
type LeafIdentity = (u64, u64, u32, u32, u64, u64, i64, i64, i64, i64);
fn identity(m: &fs::Metadata) -> LeafIdentity {
    (m.dev(), m.ino(), m.mode(), m.uid(), m.nlink(), m.len(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec())
}
fn namespace(value: &str) -> Result<()> {
    if value.len() != 32 || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || value.bytes().all(|b| b == b'0') { return Err(Error::InvalidWire); }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RecipientReference {
    version: u8,
    namespace_id: String,
    attestation: Signed<CollectorRecipientAttestation>,
}
impl RecipientReference {
    /// Shape only. Core supplies the genuinely signed statement from its SAME
    /// private-recipient/Permit owner; no signing method exists in SSH IO.
    pub(super) fn new(namespace_id: String, attestation: Signed<CollectorRecipientAttestation>) -> Result<Self> {
        let value = Self { version: 1, namespace_id, attestation };
        value.to_json()?;
        Ok(value)
    }
    fn structure(&self) -> Result<()> {
        namespace(&self.namespace_id)?;
        if self.version != 1 { return Err(Error::InvalidWire); }
        self.attestation.body.validate()
    }
    pub(super) fn namespace_id(&self) -> &str { &self.namespace_id }
    pub(super) fn attestation(&self) -> &Signed<CollectorRecipientAttestation> { &self.attestation }
    pub(super) fn to_json(&self) -> Result<Vec<u8>> {
        self.structure()?;
        json_body(&serde_json::to_value(self).map_err(|_| Error::InvalidWire)?)
    }
    pub(super) fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.is_empty() || raw.len() > MAX_FRAME { return Err(Error::Quota); }
        let value: Self = serde_json::from_slice(raw).map_err(|_| Error::InvalidWire)?;
        if value.to_json()? != raw { return Err(Error::InvalidWire); }
        Ok(value)
    }
    /// Metadata/signature check against a CURRENT original Actor readback.
    /// SSH must ALSO compare observed active-start == attestation.start BEFORE
    /// and AFTER IO using Core's actual active-start observer, never a bool.
    pub(super) fn check_metadata_against(&self, base: &CollectorServiceDelegation, now: Now) -> Result<()> {
        self.structure()?;
        base.validate()?;
        if base.credential_targets.is_empty() { return Err(Error::CapabilityDenied); }
        self.attestation.verify(base.recipient)?;
        let r = &self.attestation.body;
        if r.header != base.header || r.agent_id != base.agent_id || r.key_epoch != base.key_epoch
            || r.certificate_hash != base.certificate_hash || r.service_id != base.service_id
            || r.base_delegation_hash != base.digest()? || r.manifest_sha256 != base.manifest_sha256
            || r.expires_at > base.expires_at || r.issued_at < base.issued_at
            || now.0 < r.issued_at || now.0 >= r.expires_at
        { return Err(Error::WrongClaim); }
        Ok(())
    }
    /// This equality does not check liveness; Core MUST hold/recheck its actual
    /// Access across IO. It cannot be invoked by supplying a JSON service ID.
    pub(super) fn matches_original(&self, permit: &CollectorServicePermit) -> Result<()> {
        let r = &self.attestation.body;
        if permit.service_id() != r.service_id || permit.delegation_hash() != r.base_delegation_hash
            || permit.start_id() != r.original_start_nonce { return Err(Error::WrongClaim); }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EncryptedHandoffFrame {
    version: u8,
    namespace_id: String,
    slot: u8,
    envelope: cc_crypto_core::CollectorCredentialEnvelope,
}
impl EncryptedHandoffFrame {
    pub(super) fn new(namespace_id: String, slot: u8, envelope: cc_crypto_core::CollectorCredentialEnvelope) -> Result<Self> {
        let value = Self { version: 1, namespace_id, slot, envelope };
        value.to_json()?;
        Ok(value)
    }
    pub(super) fn to_json(&self) -> Result<Vec<u8>> {
        namespace(&self.namespace_id)?;
        encrypted_leaf(self.slot)?;
        if self.version != 1 { return Err(Error::InvalidWire); }
        // Existing capsule <=8192 and inner envelope <=12288. Entire closed
        // frame still passes the ORIGINAL full16384 canonical JSON bound.
        self.envelope.to_json().map_err(|_| Error::InvalidWire)?;
        json_body(&serde_json::to_value(self).map_err(|_| Error::InvalidWire)?)
    }
    pub(super) fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.is_empty() || raw.len() > MAX_FRAME { return Err(Error::Quota); }
        let value: Self = serde_json::from_slice(raw).map_err(|_| Error::InvalidWire)?;
        if value.to_json()? != raw { return Err(Error::InvalidWire); }
        Ok(value)
    }
    /// Exact correlation with the ACTUAL retained original Node-signed proof.
    /// Scope signature/AEAD/material policy and actual Actor Access remain
    /// mandatory separate checks before Core adopts a leaf or starts a child.
    pub(super) fn check_correlation(&self, original: &RecipientReference) -> Result<()> {
        self.to_json()?;
        original.structure()?;
        if self.namespace_id != original.namespace_id { return Err(Error::WrongContext); }
        let raw = serde_json::to_vec(&self.envelope.scope()).map_err(|_| Error::InvalidWire)?;
        let scope: Signed<CollectorCredentialScope> = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
        // A concrete public metadata codec only; never convert Actor/Permit.
        if scope.body.signing_bytes()? != self.envelope.scope().body.signing_bytes().map_err(|_| Error::InvalidWire)?
            || scope.signature.as_bytes() != self.envelope.scope().signature.as_bytes()
        { return Err(Error::WrongClaim); }
        let s = &scope.body;
        let r = &original.attestation.body;
        if s.header != r.header || s.agent_id != r.agent_id || s.key_epoch != r.key_epoch
            || s.certificate_hash != r.certificate_hash || s.service_id != r.service_id
            || s.base_delegation_hash != r.base_delegation_hash || s.manifest_sha256 != r.manifest_sha256
            || s.original_start_nonce != r.original_start_nonce || s.recipient_challenge != r.challenge
            || s.recipient_attestation_digest != r.digest()? || s.recipient_id != r.recipient_id
            || s.recipient_key_sha256 != digest(r.recipient_public_key.as_bytes())
            || s.recipient_key_epoch != r.recipient_key_epoch
            || s.recipient_certificate_epoch != r.recipient_certificate_epoch
        { return Err(Error::WrongClaim); }
        Ok(())
    }
    /// Public preflight before either accepting or consuming an encrypted leaf.
    /// Core supplies its CURRENT original Actor pin/base and physically checked
    /// catalog, then independently rechecks actual active-start after IO. This
    /// check never decrypts material or admits an operation.
    pub(super) fn check_owner_metadata(&self, original: &RecipientReference,
        pin: &OwnerPin, base: &CollectorServiceDelegation, manifest: &Value, now: Now) -> Result<()> {
        self.check_correlation(original)?;
        original.check_metadata_against(base, now)?;
        base.check_credential_targets(manifest)?;
        if base.header.context != pin.context || base.header.owner_epoch != pin.epoch
            || base.header.checkpoint != pin.checkpoint || base.expires_at > pin.expires_at
        { return Err(Error::WrongContext); }
        let raw = serde_json::to_vec(self.envelope.scope()).map_err(|_| Error::InvalidWire)?;
        let scope: Signed<CollectorCredentialScope> = serde_json::from_slice(&raw).map_err(|_| Error::InvalidWire)?;
        scope.verify(pin.key)?;
        scope.body.check_base_and_recipient(base, &original.attestation.body)?;
        scope.body.check_catalog(manifest)?;
        let metadata = serde_json::to_value(pin).map_err(|_| Error::InvalidWire)?;
        let native_pin: cc_agent_node_wire::OwnerPin = serde_json::from_value(metadata.clone()).map_err(|_| Error::InvalidWire)?;
        if serde_json::to_value(&native_pin).map_err(|_| Error::InvalidWire)? != metadata { return Err(Error::WrongClaim); }
        let native_now = cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(now.0.seconds()).map_err(|_| Error::InvalidTime)?);
        self.envelope.check_scope_and_capsule_metadata_only(&native_pin, native_now).map_err(|_| Error::WrongClaim)
    }
    pub(super) fn into_envelope(self) -> cc_crypto_core::CollectorCredentialEnvelope { self.envelope }
}

fn encrypted_leaf(slot: u8) -> Result<&'static str> {
    match slot { 0 => Ok("credential-0.capsule.json"), 1 => Ok("credential-1.capsule.json"), _ => Err(Error::Quota) }
}
fn registered_leaf_name(index: u8) -> Result<&'static str> {
    match index {
        0 => Ok("service-recipient.json"),
        1 => encrypted_leaf(0),
        2 => encrypted_leaf(1),
        _ => Err(Error::Quota),
    }
}
/// The supervisor receives ONLY the borrow of its actual Files owner. Compare
/// the registered original path before/after opening a kernel-held directory;
/// a caller namespace string cannot construct this Core borrow.
fn open_registered_namespace(original: &RegisteredServiceNamespace<'_>, index: u8) -> Result<File> {
    let expected_directory = Path::new(disk::ROOT).join(original.session_id());
    let expected_leaf = expected_directory.join(registered_leaf_name(index)?);
    if original.checked_directory()? != expected_directory.as_path()
        || original.registered_leaf(index)? != expected_leaf { return Err(Error::WrongContext); }
    let directory = open_physical_namespace(original.session_id())?;
    original.check_held_directory(&directory)?;
    if original.checked_directory()? != expected_directory.as_path()
        || original.registered_leaf(index)? != expected_leaf { return Err(Error::WrongContext); }
    check_directory(&directory, original.session_id())?;
    original.check_held_directory(&directory)?;
    Ok(directory)
}
fn proc_leaf(directory: &File, leaf: &str) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(leaf)
}
fn check_directory(directory: &File, namespace_id: &str) -> Result<()> {
    namespace(namespace_id)?;
    tmpfs_root_at(disk::ROOT)?;
    let held = directory.metadata().map_err(|_| Error::Storage)?;
    let named = fs::symlink_metadata(Path::new(disk::ROOT).join(namespace_id)).map_err(|_| Error::Storage)?;
    if !held.is_dir() || !named.is_dir() || named.file_type().is_symlink()
        || held.uid() != service_uid()? || held.mode() & 0o7777 != 0o700
        || named.uid() != held.uid() || named.mode() & 0o7777 != 0o700
        || (held.dev(), held.ino()) != (named.dev(), named.ino()) { return Err(Error::Storage); }
    Ok(())
}
/// Physical resolution only. Caller independently compares CURRENT original
/// Actor active-start/proof before/after; no lock acquisition or new owner.
pub(super) fn open_physical_namespace(namespace_id: &str) -> Result<File> {
    namespace(namespace_id)?;
    tmpfs_root_at(disk::ROOT)?;
    let directory = OpenOptions::new().read(true).custom_flags(DIRECTORY | NOFOLLOW | NONBLOCK | CLOEXEC)
        .open(Path::new(disk::ROOT).join(namespace_id)).map_err(|_| Error::Storage)?;
    check_directory(&directory, namespace_id)?;
    Ok(directory)
}
fn check_leaf(m: &fs::Metadata, cap: usize) -> Result<()> {
    if !m.is_file() || m.file_type().is_symlink() || m.uid() != service_uid()? || m.nlink() != 1
        || m.mode() & 0o7777 != 0o400 || m.len() == 0 || m.len() > cap as u64 { return Err(Error::Storage); }
    Ok(())
}
/// Held FD + encrypted/public bytes, not proof of creation, authority or safe
/// cleanup. Only Core may adopt its checked original inode AFTER validation.
pub(super) struct HeldHandoffLeaf {
    file: File,
    identity: LeafIdentity,
    leaf: &'static str,
    raw: Vec<u8>,
}
impl HeldHandoffLeaf {
    pub(super) fn recheck_physical(&self, directory: &File, namespace_id: &str) -> Result<()> {
        check_directory(directory, namespace_id)?;
        let held = self.file.metadata().map_err(|_| Error::Storage)?;
        let named = fs::symlink_metadata(proc_leaf(directory, self.leaf)).map_err(|_| Error::Storage)?;
        check_leaf(&held, MAX_FRAME)?; check_leaf(&named, MAX_FRAME)?;
        if identity(&held) != self.identity || identity(&named) != self.identity { return Err(Error::Storage); }
        Ok(())
    }
    pub(super) fn original_inode_metadata(&self) -> (u64, u64) { (self.identity.0, self.identity.1) }
    /// Core's original Files owner may inspect this actual retained FD AFTER
    /// current-start/cipher verification; metadata tuples are never adoption.
    pub(super) fn held_file(&self) -> &File { &self.file }
    pub(super) fn recheck_registered(&self, original: &RegisteredServiceNamespace<'_>, index: u8) -> Result<()> {
        if self.leaf != registered_leaf_name(index)? { return Err(Error::WrongClaim); }
        let directory = open_registered_namespace(original, index)?;
        self.recheck_physical(&directory, original.session_id())?;
        original.registered_leaf(index)?;
        original.check_held_directory(&directory)?;
        Ok(())
    }
    pub(super) fn recipient_reference(&self) -> Result<RecipientReference> {
        if self.leaf != "service-recipient.json" { return Err(Error::WrongState); }
        RecipientReference::from_json(&self.raw)
    }
    pub(super) fn encrypted_frame(&self) -> Result<EncryptedHandoffFrame> {
        if self.leaf == "service-recipient.json" { return Err(Error::WrongState); }
        let frame = EncryptedHandoffFrame::from_json(&self.raw)?;
        if encrypted_leaf(frame.slot)? != self.leaf { return Err(Error::WrongClaim); }
        Ok(frame)
    }
}
fn read_leaf(directory: &File, namespace_id: &str, leaf: &'static str) -> Result<HeldHandoffLeaf> {
    check_directory(directory, namespace_id)?;
    let before = fs::symlink_metadata(proc_leaf(directory, leaf)).map_err(|_| Error::Storage)?;
    check_leaf(&before, MAX_FRAME)?;
    let mut file = OpenOptions::new().read(true).custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC)
        .open(proc_leaf(directory, leaf)).map_err(|_| Error::Storage)?;
    let expected = identity(&before);
    if identity(&file.metadata().map_err(|_| Error::Storage)?) != expected { return Err(Error::Storage); }
    let mut raw = Vec::new();
    Read::by_ref(&mut file).take(MAX_FRAME as u64 + 1).read_to_end(&mut raw).map_err(|_| Error::Storage)?;
    if raw.len() > MAX_FRAME || raw.len() as u64 != before.len() { return Err(Error::Storage); }
    let held = HeldHandoffLeaf { file, identity: expected, leaf, raw };
    held.recheck_physical(directory, namespace_id)?;
    Ok(held)
}
fn write_leaf(directory: &File, namespace_id: &str, leaf: &'static str, raw: Vec<u8>) -> Result<HeldHandoffLeaf> {
    check_directory(directory, namespace_id)?;
    if raw.is_empty() || raw.len() > MAX_FRAME { return Err(Error::Quota); }
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o400)
        .custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC).open(proc_leaf(directory, leaf))
        .map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { Error::Conflict } else { Error::UncertainCommit })?;
    // From O_EXCL onwards every failure is unknown. No deletion, overwrite,
    // retry or caller-created receipt is manufactured on any error path.
    let result = (|| -> Result<HeldHandoffLeaf> {
        let initial = file.metadata().map_err(|_| Error::Storage)?;
        if !initial.is_file() || initial.uid() != service_uid()? || initial.nlink() != 1
            || initial.mode() & 0o7777 != 0o400 || initial.len() != 0 { return Err(Error::Storage); }
        let named = fs::symlink_metadata(proc_leaf(directory, leaf)).map_err(|_| Error::Storage)?;
        if identity(&initial) != identity(&named) { return Err(Error::Storage); }
        check_directory(directory, namespace_id)?;
        file.write_all(&raw).map_err(|_| Error::Storage)?;
        file.sync_all().map_err(|_| Error::Storage)?;
        directory.sync_all().map_err(|_| Error::Storage)?;
        let expected = identity(&file.metadata().map_err(|_| Error::Storage)?);
        let held = HeldHandoffLeaf { file, identity: expected, leaf, raw };
        held.recheck_physical(directory, namespace_id)?;
        Ok(held)
    })();
    result.map_err(|_| Error::UncertainCommit)
}
fn write_original_reference_physical(directory: &File, reference: &RecipientReference) -> Result<HeldHandoffLeaf> {
    write_leaf(directory, &reference.namespace_id, "service-recipient.json", reference.to_json()?)
}
/// Original supervisor only. Core must call writer.recheck before borrowing
/// handoff_namespace(), DROP that borrow after IO, then writer.recheck again.
/// This API cannot manufacture or refresh Actor access from a path/proof.
pub(super) fn write_registered_reference_physical(original: &RegisteredServiceNamespace<'_>,
    reference: &RecipientReference, permit: &CollectorServicePermit,
    base: &CollectorServiceDelegation, now: Now) -> Result<HeldHandoffLeaf> {
    if reference.namespace_id != original.session_id() { return Err(Error::WrongContext); }
    reference.matches_original(permit)?;
    reference.check_metadata_against(base, now)?;
    let directory = open_registered_namespace(original, 0)?;
    original.check_held_directory(&directory)?;
    let held = write_original_reference_physical(&directory, reference)?;
    // Creation has happened: subsequent failures are unknown, never retried or
    // removed by this helper. The original Core Files owner retains custody.
    original.check_held_directory(&directory).map_err(|_| Error::UncertainCommit)?;
    held.recheck_registered(original, 0).map_err(|_| Error::UncertainCommit)?;
    Ok(held)
}
pub(super) fn read_registered_reference_physical(original: &RegisteredServiceNamespace<'_>,
    permit: &CollectorServicePermit, base: &CollectorServiceDelegation, now: Now) -> Result<HeldHandoffLeaf> {
    let directory = open_registered_namespace(original, 0)?;
    original.check_held_directory(&directory)?;
    let held = read_original_reference_physical(&directory, original.session_id())?;
    let reference = held.recipient_reference()?;
    reference.matches_original(permit)?;
    reference.check_metadata_against(base, now)?;
    held.recheck_registered(original, 0)?;
    original.check_held_directory(&directory)?;
    Ok(held)
}
pub(super) fn read_registered_encrypted_frame_physical(original: &RegisteredServiceNamespace<'_>,
    permit: &CollectorServicePermit, reference: &RecipientReference, slot: u8,
    pin: &OwnerPin, base: &CollectorServiceDelegation, manifest: &Value, now: Now) -> Result<HeldHandoffLeaf> {
    if reference.namespace_id != original.session_id() { return Err(Error::WrongContext); }
    reference.matches_original(permit)?;
    let index = match slot { 0 => 1, 1 => 2, _ => return Err(Error::Quota) };
    let directory = open_registered_namespace(original, index)?;
    original.check_held_directory(&directory)?;
    let held = read_encrypted_frame_physical(&directory, reference, slot, pin, base, manifest, now)?;
    held.recheck_registered(original, index)?;
    original.check_held_directory(&directory)?;
    Ok(held)
}
pub(super) fn read_original_reference_physical(directory: &File, namespace_id: &str) -> Result<HeldHandoffLeaf> {
    let held = read_leaf(directory, namespace_id, "service-recipient.json")?;
    if held.recipient_reference()?.namespace_id != namespace_id { return Err(Error::WrongContext); }
    Ok(held)
}
pub(super) fn write_encrypted_frame_physical(directory: &File, frame: &EncryptedHandoffFrame,
    original: &RecipientReference, pin: &OwnerPin, base: &CollectorServiceDelegation,
    manifest: &Value, now: Now) -> Result<HeldHandoffLeaf> {
    frame.check_owner_metadata(original, pin, base, manifest, now)?;
    write_leaf(directory, &frame.namespace_id, encrypted_leaf(frame.slot)?, frame.to_json()?)
}
pub(super) fn read_encrypted_frame_physical(directory: &File, original: &RecipientReference, slot: u8,
    pin: &OwnerPin, base: &CollectorServiceDelegation, manifest: &Value, now: Now) -> Result<HeldHandoffLeaf> {
    let held = read_leaf(directory, &original.namespace_id, encrypted_leaf(slot)?)?;
    held.encrypted_frame()?.check_owner_metadata(original, pin, base, manifest, now)?;
    Ok(held)
}
