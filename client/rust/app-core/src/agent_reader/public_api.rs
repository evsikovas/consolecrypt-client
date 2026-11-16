//! Public product adapter: original opaque reviews, no provider/signer injection.
use super::{
    authority::{NativeReaderAuthority, ReaderRevokeSelection},
    facade::*,
    node_owner::ReaderNodeOwnerReview,
    scope::{ReaderOriginalContext, ReaderSourceSelection},
    transport::ReaderPeerBinding,
};
use crate::{AppCore, AppError, AppResult};
use cc_agent_node_wire as node;
use cc_crypto_core::agent_reader::*;
use serde::{
    de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor},
    Deserialize,
};
use serde_json::{json, Value};
use std::{
    fmt, fs,
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

fn denied() -> AppError {
    AppError::invalid("agent_reader", "reader_action_rejected")
}
fn failure(error: Failure) -> AppError {
    let code = match error {
        Failure::Busy => "reader_busy",
        Failure::Rejected => "reader_action_rejected",
        Failure::UnsupportedProfile => "reader_profile_unsupported",
        Failure::Unavailable => "reader_unavailable",
        Failure::OutcomeUnknown => "reader_outcome_unknown",
        Failure::Cancelled => "reader_cancelled",
        Failure::Expired => "reader_expired",
        Failure::ResponseInvalid => "reader_response_invalid",
        Failure::RevokePending => "reader_revoke_pending",
    };
    AppError::invalid("agent_reader", code)
}
fn id(text: &str) -> AppResult<Uuid> {
    let value = Uuid::parse_str(text).map_err(|_| denied())?;
    if value.is_nil() || value.to_string() != text {
        return Err(denied());
    }
    Ok(value)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    profile_id: String,
    host_id: String,
    source_id: String,
    source_revision: u64,
    binding_id: String,
    binding_revision: u64,
    known_host_id: String,
}
impl Selection {
    fn native(&self) -> AppResult<NativeReaderSelection> {
        for value in [
            &self.profile_id,
            &self.host_id,
            &self.source_id,
            &self.binding_id,
            &self.known_host_id,
        ] {
            id(value)?;
        }
        if self.source_revision == 0 || self.binding_revision == 0 {
            return Err(denied());
        }
        Ok(NativeReaderSelection {
            original: None,
            source: ReaderSourceSelection {
                profile_id: self.profile_id.clone(),
                host_id: self.host_id.clone(),
                source_id: self.source_id.clone(),
                source_revision: self.source_revision,
                binding_id: self.binding_id.clone(),
                binding_revision: self.binding_revision,
            },
            known_host_id: crate::dto::parse_id("reader_known_host", &self.known_host_id)?,
        })
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Locator {
    peer_digest: AgentReaderHash,
    association_digest: AgentReaderHash,
}
impl Locator {
    fn native(&self) -> AssociationRef {
        AssociationRef::from_public_locator(self.peer_digest, self.association_digest)
    }
}
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Action {
    NodeOffer {
        owner_pin_file: String,
        node_hello_file: String,
        offer_expires_at: u64,
    },
    NodeCert {
        owner_pin_file: String,
        signed_offer_file: String,
        signed_claim_file: String,
        certificate_expires_at: u64,
    },
    IssuerInit {},
    Association {
        selection_file: String,
    },
    Read {
        association: Locator,
    },
    Revoke {
        association: Locator,
        subject: RevokeSubject,
    },
    Reconcile {
        association: Locator,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, Copy)]
enum RevokeSubject {
    Association,
    OriginalDevice,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepare {
    notification_scope_id: String,
    selection: Selection,
    action: Action,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Hello {
    format: u32,
    agent_id: node::AgentId,
    key_epoch: u64,
    public_key: node::PublicKey,
    key_fingerprint: node::Hash,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssociationInput {
    pin: AgentReaderIssuerPinV1,
    target: AgentReaderTargetDescriptorV1,
    binding: ReaderPeerBinding,
    installed_head: AgentReaderHeadV1,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Status {
    notification_scope_id: String,
    selection: Selection,
    association: Option<Locator>,
}

// Complete duplicate-rejecting parse; bounded bytes/depth/strings first.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("closed reader JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Unique, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Unique(n.into()))
                    .ok_or_else(|| E::custom("number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = Vec::new();
                while let Some(Unique(x)) = a.next_element()? {
                    v.push(x);
                }
                Ok(Unique(v.into()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if v.contains_key(&k) {
                        return Err(de::Error::custom("duplicate"));
                    }
                    let Unique(x) = a.next_value()?;
                    v.insert(k, x);
                }
                Ok(Unique(v.into()))
            }
        }
        d.deserialize_any(V)
    }
}
fn bounded(v: &Value, depth: usize) -> AppResult<()> {
    if depth > 16 {
        return Err(denied());
    }
    match v {
        Value::String(s) if s.len() > 4096 => Err(denied()),
        Value::Array(v) => {
            for x in v {
                bounded(x, depth + 1)?;
            }
            Ok(())
        }
        Value::Object(v) => {
            for (k, x) in v {
                if k.len() > 128 {
                    return Err(denied());
                }
                bounded(x, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> AppResult<T> {
    if bytes.is_empty() || bytes.len() > 16384 {
        return Err(denied());
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let Unique(v) = Unique::deserialize(&mut d).map_err(|_| denied())?;
    d.end().map_err(|_| denied())?;
    bounded(&v, 0)?;
    serde_json::from_value(v).map_err(|_| denied())
}
fn witness<T: DeserializeOwned>(name: &str) -> AppResult<T> {
    if name.len() > 1024 || name.contains('\0') {
        return Err(denied());
    }
    let p = Path::new(name);
    if !p.is_absolute() {
        return Err(denied());
    }
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(denied());
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if !matches!(p.components().next(),Some(Component::Prefix(v)) if matches!(v.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
        {
            return Err(denied());
        }
    }

    for item in p.ancestors() {
        let m = fs::symlink_metadata(item).map_err(|_| denied())?;
        if m.file_type().is_symlink() {
            return Err(denied());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(denied());
            }
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let mut f = options.open(p).map_err(|_| denied())?;
    let before = f.metadata().map_err(|_| denied())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if before.file_attributes() & 0x400 != 0 {
            return Err(denied());
        }
    }
    if !before.is_file() || before.len() > 16384 {
        return Err(denied());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.nlink() != 1 {
            return Err(denied());
        }
    }
    let mut bytes = Zeroizing::new(Vec::new());
    (&mut f)
        .take(16385)
        .read_to_end(&mut bytes)
        .map_err(|_| denied())?;
    let after = f.metadata().map_err(|_| denied())?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || bytes.len() as u64 != before.len()
    {
        return Err(denied());
    }
    decode(&bytes)
}
fn stamp() -> AppResult<node::Timestamp> {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| denied())?
        .as_secs();
    node::Timestamp::new(t).map_err(|_| denied())
}
fn next(pin: &node::OwnerPin) -> AppResult<node::Header> {
    Ok(node::Header {
        version: 1,
        context: pin.context.clone(),
        owner_epoch: pin.epoch,
        checkpoint: pin.checkpoint.checked_add(1).ok_or_else(denied)?,
    })
}
fn native_snapshot(v: &super::scope::ReaderContextSnapshot) -> Value {
    json!({"notificationScopeId":v.notification_scope_id,"nativeProfileId":v.native_profile_id,"nativeHostId":v.native_host_id,
    "sourceId":v.native_source_id,"sourceRevision":v.native_source_revision,"bindingId":v.native_binding_id,"bindingRevision":v.native_binding_revision,
    "nativeBindingDigest":v.native_binding_digest,"deviceId":v.device_id})
}

// Lives only in the caller's opaque original handle, never in AppCore::Inner.
enum Prepared {
    Node {
        review: ReaderNodeOwnerReview,
        pin_file: String,
        token: Option<Zeroizing<[u8; 32]>>,
        slot: ReaderOwnerSlot,
    },
    Issuer(NativeReview<IssuerReview>),
    Association(NativeReview<AssociationReview>),
    Read(NativeReview<ReadReview>),
    Revoke(NativeReview<RevokeReview>),
    Reconcile(NativeReview<ReconcileReview>),
}
#[path = "public_api/export.rs"]
mod export;
use export::{NativeExportFile, NativeExports};

enum Held {
    Prepared(Prepared),
    Data(NativePrivateReaderData),
    Export(NativeExports),
}
struct Shared {
    transferring: AtomicBool,
    held: Mutex<Option<Held>>,
    running: Mutex<Option<CancellationToken>>,
    cancelled: CancellationToken,
    deadline: Instant,
    original: Arc<ReaderOriginalContext>,
}
impl Shared {
    fn cancel(&self) {
        self.cancelled.cancel();
        self.original.retire();
        if let Ok(c) = self.running.lock() {
            if let Some(c) = c.as_ref() {
                c.cancel();
            }
        }
        let old = self.held.lock().ok().and_then(|mut s| s.take());
        drop(old);
    }
    fn publish(&self, value: Held) -> AppResult<()> {
        let mut state = self.held.lock().map_err(|_| denied())?;
        self.check()?;
        if state.is_some() {
            return Err(denied());
        }
        *state = Some(value);
        Ok(())
    }
    fn check(&self) -> AppResult<()> {
        if self.cancelled.is_cancelled() || Instant::now() >= self.deadline {
            return Err(denied());
        }
        self.original.now()?;
        Ok(())
    }
}
pub struct ReaderActionHandle {
    core: std::sync::Weak<crate::app::Inner>,
    shared: Arc<Shared>,
    details: String,
    confirmed: AtomicBool,
}
impl fmt::Debug for ReaderActionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReaderActionHandle([redacted])")
    }
}
impl Drop for ReaderActionHandle {
    fn drop(&mut self) {
        self.shared.cancel();
    }
}
impl ReaderActionHandle {
    // Retain only the SAME original root weakly while idle. A bounded call may
    // upgrade it, but must never resolve a replacement/global AppCore.
    fn original_core(&self) -> AppResult<AppCore> {
        self.shared.check()?;
        Ok(AppCore { inner: self.core.upgrade().ok_or_else(denied)? })
    }
    pub fn details_json(&self) -> AppResult<String> {
        let _core = self.original_core()?;
        // Only immutable review metadata is cached. Every call uses the SAME
        // original attempt and absolute deadline; no recapture or renewal.
        self.shared.check()?;
        let mut details: Value = serde_json::from_str(&self.details).map_err(|_| denied())?;
        self.shared.check()?;
        let remaining = self
            .shared
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(denied)?;
        let remaining_ms = u64::try_from(remaining.as_millis()).map_err(|_| denied())?;
        if remaining_ms == 0 {
            return Err(denied());
        }
        details["remainingMilliseconds"] = json!(remaining_ms);
        serde_json::to_string(&details).map_err(|_| denied())
    }
    pub fn cancel(&self) {
        self.shared.cancel();
    }
    async fn operation<T>(&self, op: NativeReaderOperation<T>) -> AppResult<T> {
        let op = op.from_original(&self.shared.original);
        let cancel = op.cancellation();
        *self.shared.running.lock().map_err(|_| denied())? = Some(cancel.clone());
        if self.shared.cancelled.is_cancelled() {
            cancel.cancel();
        }
        let value = op.wait().await.map_err(failure);
        self.shared.running.lock().map_err(|_| denied())?.take();
        value
    }
    pub async fn confirm_json(&self) -> AppResult<String> {
        if self.confirmed.swap(true, Ordering::SeqCst) {
            return Err(denied());
        }
        let result = self.confirm_once().await;
        if result.is_err() {
            self.shared.cancel();
        }
        result
    }
    async fn confirm_once(&self) -> AppResult<String> {
        let core = self.original_core()?;
        self.shared.check()?;
        let held = {
            self.shared
                .held
                .lock()
                .map_err(|_| denied())?
                .take()
                .ok_or_else(denied)?
        };
        let Held::Prepared(prepared) = held else {
            return Err(denied());
        };
        let post_unknown = matches!(
            &prepared,
            Prepared::Issuer(_)
                | Prepared::Association(_)
                | Prepared::Revoke(_)
                | Prepared::Reconcile(_)
        );
        let result = match prepared {
            Prepared::Node {
                review,
                pin_file,
                token,
                slot,
            } => {
                slot.check().map_err(failure)?;
                let token_cancel = slot.cancellation();
                *self.shared.running.lock().map_err(|_| denied())? = Some(token_cancel.clone());
                if self.shared.cancelled.is_cancelled() {
                    token_cancel.cancel();
                }
                let pin: node::OwnerPin = witness(&pin_file)?;
                let signed = review.approve_once(&core, &pin).await?;
                self.shared.check()?;
                slot.check().map_err(failure)?;
                let dir = tempfile::Builder::new()
                    .prefix("reader-export-")
                    .tempdir_in(core.inner.dirs.root())
                    .map_err(|_| denied())?;
                crate::kubernetes_proxy::restrict_directory(dir.path())
                    .await
                    .map_err(|_| denied())?;
                self.shared.check()?;
                slot.check().map_err(failure)?;
                let (kind, filename, expires, bytes, secret) = match signed {
                    node::ReaderNodeOwnerSigned::Offer(signed) => {
                        let secret = token.ok_or_else(denied)?;
                        (
                            "nodeOfferExported",
                            "signed-offer.json",
                            signed.body.expires_at.seconds(),
                            Zeroizing::new(serde_json::to_vec(&signed).map_err(|_| denied())?),
                            Some(secret),
                        )
                    }
                    node::ReaderNodeOwnerSigned::Certificate(signed) => (
                        "nodeCertificateExported",
                        "signed-certificate.json",
                        signed.body.expires_at.seconds(),
                        Zeroizing::new(serde_json::to_vec(&signed).map_err(|_| denied())?),
                        None,
                    ),
                };
                let path = dir.path().join(filename);
                fn write_export(path: &Path, bytes: &[u8]) -> AppResult<fs::File> {
                    let mut o = fs::OpenOptions::new();
                    o.read(true).write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        o.mode(0o600).custom_flags(libc::O_NOFOLLOW);
                    }
                    let mut file = o.open(path).map_err(|_| denied())?;
                    file.write_all(bytes)
                        .and_then(|_| file.sync_all())
                        .map_err(|_| denied())?;
                    Ok(file)
                }
                let public_file = write_export(&path, &bytes)?;
                let (secret_path, invitation_file) = if let Some(token) = secret {
                    let secret_path = dir.path().join("invitation-token.hex");
                    let text = Zeroizing::new(hex::encode(token.as_ref()));
                    let file = write_export(&secret_path, text.as_bytes())?;
                    (
                        Some(secret_path.to_string_lossy().into_owned()),
                        Some(NativeExportFile::created(
                            secret_path,
                            file,
                            text.as_bytes(),
                        )),
                    )
                } else {
                    (None, None)
                };
                self.shared.original.check(&core).await?;
                self.shared.check()?;
                slot.check().map_err(failure)?;
                use sha2::{Digest, Sha256};
                let hash = hex::encode(Sha256::digest(&*bytes));
                let value = json!({"kind":kind,"publicArtifactPath":path.to_string_lossy(),"sha256":hash,
                    "bytes":bytes.len(),"privateInvitationPath":secret_path,"expiresAt":expires,
                    "actorApply":"notObserved","cleanup":"cancelDropOrOriginalDeadline"});
                self.shared.publish(Held::Export(NativeExports::new(
                    dir,
                    slot,
                    NativeExportFile::created(path, public_file, &bytes),
                    invitation_file,
                    expires,
                )?))?;
                self.shared.running.lock().map_err(|_| denied())?.take();
                value
            }
            Prepared::Issuer(review) => {
                let value = self
                    .operation(
                        core
                            .reader_confirm_issuer_initialization_once(review)
                            .map_err(failure)?,
                    )
                    .await?;
                json!({"kind":"issuerReady","issuerId":value.issuer_id,"publicKey":value.issuer_key})
            }
            Prepared::Association(review) => {
                let value = self
                    .operation(
                        core
                            .reader_approve_association_once(review)
                            .map_err(failure)?,
                    )
                    .await?;
                let (p, a) = value.locator();
                json!({"kind":"associationInstalled","peerDigest":p,"associationDigest":a,"ack":"observedThisOperation"})
            }
            Prepared::Read(review) => {
                let value = self
                    .operation(core.reader_read_once(review).map_err(failure)?)
                    .await?;
                if !value.uses_original(&self.shared.original) {
                    return Err(denied());
                }
                let result = json!({"kind":"readReady","targetHash":value.target_hash(),"expiresAt":value.expires_at()});
                self.shared.publish(Held::Data(value))?;
                result
            }
            Prepared::Revoke(review) => {
                self.operation(core.reader_revoke_once(review).map_err(failure)?)
                    .await?;
                json!({"kind":"readerRevocationApplied","ack":"observedThisOperation"})
            }
            Prepared::Reconcile(review) => {
                self.operation(core.reader_reconcile_once(review).map_err(failure)?)
                    .await?;
                json!({"kind":"knownHeadReconciled","newApply":false})
            }
        };
        let post_error = || {
            if post_unknown {
                failure(Failure::OutcomeUnknown)
            } else {
                denied()
            }
        };
        let encoded = serde_json::to_string(&result).map_err(|_| post_error())?;
        self.shared
            .original
            .check(&core)
            .await
            .map_err(|_| post_error())?;
        self.shared.check().map_err(|_| post_error())?;
        Ok(encoded)
    }
    pub async fn consume_read_json(&self) -> AppResult<String> {
        let core = self.original_core()?;
        self.shared.check()?;
        let held = {
            self.shared
                .held
                .lock()
                .map_err(|_| denied())?
                .take()
                .ok_or_else(denied)?
        };
        let Held::Data(data) = held else {
            return Err(denied());
        };
        let body = data.consume_json(&core).await.map_err(failure)?;
        self.shared.check()?;
        let encoded=serde_json::to_string(&json!({"kind":"readerDataset","targetHash":data.target_hash(),"expiresAt":data.expires_at(),"bodyJson":body})).map_err(|_|denied())?;
        self.shared.original.check(&core).await?;
        self.shared.check()?;
        Ok(encoded)
    }
}

struct Original(Option<Arc<ReaderOriginalContext>>);
impl Drop for Original {
    fn drop(&mut self) {
        if let Some(c) = &self.0 {
            c.retire();
        }
    }
}
fn details_for<K: Send + 'static>(kind: &str, review: &NativeReview<K>) -> AppResult<Value> {
    let mut v =
        json!({"kind":kind,"native":native_snapshot(review.original_snapshot().map_err(failure)?)});
    if let Some(s) = review.association_selection() {
        v["target"] = serde_json::to_value(&s.target).map_err(|_| denied())?;
        v["issuerPin"] = serde_json::to_value(&s.pin).map_err(|_| denied())?;
        v["expectedHead"] = serde_json::to_value(&s.installed_head).map_err(|_| denied())?;
        v["transportBinding"] = serde_json::to_value(&s.binding).map_err(|_| denied())?;
    }
    if let Some(s) = review.registered_selection() {
        v["target"] = serde_json::to_value(&s.target).map_err(|_| denied())?;
        v["installedHead"] = serde_json::to_value(&s.installed_head).map_err(|_| denied())?;
        v["associationDigest"] =
            serde_json::to_value(s.association.body.body_digest().map_err(|_| denied())?)
                .map_err(|_| denied())?;
    }
    Ok(v)
}
impl AppCore {
    /// Initial capture only. Every later method uses the returned ORIGINAL handle.
    pub async fn reader_prepare_action_json(&self, input: &str) -> AppResult<ReaderActionHandle> {
        let deadline = Instant::now() + Duration::from_secs(60);
        tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.reader_prepare_action_before(input, deadline),
        )
        .await
        .map_err(|_| failure(Failure::Expired))?
    }
    async fn reader_prepare_action_before(
        &self,
        input: &str,
        entry_deadline: Instant,
    ) -> AppResult<ReaderActionHandle> {
        let request: Prepare = decode(input.as_bytes())?;
        ReaderOriginalContext::check_notification_scope(
            self,
            &request.selection.profile_id,
            &request.notification_scope_id,
        )
        .await?;
        let selection = request.selection.native()?;
        let admission = self
            .inner
            .reader_native
            .reserve_owner_review()
            .map_err(failure)?;
        let admission_deadline = admission.deadline();
        let cancel = CancellationToken::new();
        let mut original = Original(Some(Arc::new(
            ReaderOriginalContext::capture_for_notification(
                self,
                selection.source.clone(),
                &cancel,
                &request.notification_scope_id,
            )
            .await?,
        )));
        let ctx = original.0.as_ref().ok_or_else(denied)?;
        let outer_deadline = ctx.deadline().min(admission_deadline).min(entry_deadline);
        let (prepared, details, deadline) = match request.action {
            Action::NodeOffer {
                owner_pin_file,
                node_hello_file,
                offer_expires_at,
            } => {
                let slot = admission;
                slot.bind_original(ctx).map_err(failure)?;
                let pin: node::OwnerPin = witness(&owner_pin_file)?;
                let hello: Hello = witness(&node_hello_file)?;
                hello.public_key.validate().map_err(|_| denied())?;
                if hello.format != 1
                    || hello.key_epoch != 1
                    || hello.key_fingerprint != hello.public_key.fingerprint()
                {
                    return Err(denied());
                }
                let mut secret = Zeroizing::new([0u8; 32]);
                cc_crypto_core::fill_random(secret.as_mut()).map_err(|_| denied())?;
                let token = node::InvitationToken::from_bytes(*secret).map_err(|_| denied())?;
                let offer = node::Offer {
                    header: next(&pin)?,
                    invitation_id: node::InvitationId::new(&Uuid::new_v4().to_string())
                        .map_err(|_| denied())?,
                    agent_id: hello.agent_id,
                    agent_key: hello.public_key,
                    token_hash: token.digest(),
                    scopes: node::Scopes(vec![node::Scope::TelemetryRead]),
                    issued_at: stamp()?,
                    expires_at: node::Timestamp::new(offer_expires_at).map_err(|_| denied())?,
                };
                let review = ReaderNodeOwnerReview::enrollment_offer_from_original(
                    self,
                    ctx.clone(),
                    &pin,
                    offer,
                )
                .await?;
                if !review.uses_original(ctx) {
                    return Err(denied());
                }
                let detail = json!({"kind":"nodeOffer","native":native_snapshot(review.source_snapshot()),"review":review.details()?,
                    "witness":"operatorImport","actorApply":"notObserved"});
                let deadline = slot.deadline();
                (
                    Prepared::Node {
                        review,
                        pin_file: owner_pin_file,
                        token: Some(secret),
                        slot,
                    },
                    detail,
                    deadline,
                )
            }
            Action::NodeCert {
                owner_pin_file,
                signed_offer_file,
                signed_claim_file,
                certificate_expires_at,
            } => {
                let slot = admission;
                slot.bind_original(ctx).map_err(failure)?;
                let pin: node::OwnerPin = witness(&owner_pin_file)?;
                let offer: node::Signed<node::Offer> = witness(&signed_offer_file)?;
                let claim: node::Signed<node::Claim> = witness(&signed_claim_file)?;
                use node::Statement;
                let cert = node::Certificate {
                    header: next(&pin)?,
                    agent_id: offer.body.agent_id,
                    agent_key: offer.body.agent_key,
                    key_epoch: 1,
                    scopes: node::Scopes(vec![node::Scope::TelemetryRead]),
                    capabilities: claim.body.capabilities.clone(),
                    origin: node::CertificateOrigin::Enrollment {
                        invitation_id: offer.body.invitation_id,
                        claim_hash: claim.body.digest().map_err(|_| denied())?,
                        key_fingerprint: claim.body.agent_key.fingerprint(),
                    },
                    not_before: stamp()?,
                    expires_at: node::Timestamp::new(certificate_expires_at)
                        .map_err(|_| denied())?,
                };
                let review = ReaderNodeOwnerReview::enrollment_certificate_from_original(
                    self,
                    ctx.clone(),
                    &pin,
                    offer,
                    claim,
                    cert,
                )
                .await?;
                if !review.uses_original(ctx) {
                    return Err(denied());
                }
                let detail = json!({"kind":"nodeCert","native":native_snapshot(review.source_snapshot()),"review":review.details()?,
                    "witness":"operatorImport","actorApply":"notObserved"});
                let deadline = slot.deadline();
                (
                    Prepared::Node {
                        review,
                        pin_file: owner_pin_file,
                        token: None,
                        slot,
                    },
                    detail,
                    deadline,
                )
            }
            Action::IssuerInit {} => {
                let r = self
                    .reader_prepare_issuer_initialization_from_original(selection, ctx.clone())
                    .map_err(failure)?
                    .wait()
                    .await
                    .map_err(failure)?;
                if !r.uses_original(ctx) {
                    return Err(denied());
                }
                let detail = details_for("issuerInit", &r)?;
                let d = r.deadline().map_err(failure)?;
                (Prepared::Issuer(r), detail, d)
            }
            Action::Association { selection_file } => {
                let s: AssociationInput = witness(&selection_file)?;
                let r = self
                    .reader_prepare_association_from_original(
                        selection,
                        ctx.clone(),
                        ProvisionedAssociationSelection {
                            pin: s.pin,
                            target: s.target,
                            binding: s.binding,
                            installed_head: s.installed_head,
                        },
                    )
                    .map_err(failure)?
                    .wait()
                    .await
                    .map_err(failure)?;
                if !r.uses_original(ctx) {
                    return Err(denied());
                }
                let detail = details_for("association", &r)?;
                let d = r.deadline().map_err(failure)?;
                (Prepared::Association(r), detail, d)
            }
            Action::Read { association } => {
                let r = self
                    .reader_prepare_read_from_original(selection, ctx.clone(), association.native())
                    .map_err(failure)?
                    .wait()
                    .await
                    .map_err(failure)?;
                if !r.uses_original(ctx) {
                    return Err(denied());
                }
                let detail = details_for("read", &r)?;
                let d = r.deadline().map_err(failure)?;
                (Prepared::Read(r), detail, d)
            }
            Action::Revoke {
                association,
                subject,
            } => {
                let kind = match subject {
                    RevokeSubject::Association => ReaderRevokeSelection::Association,
                    RevokeSubject::OriginalDevice => ReaderRevokeSelection::OriginalDevice,
                };
                let subject = match subject {
                    RevokeSubject::Association => "association",
                    RevokeSubject::OriginalDevice => "originalDevice",
                };
                let locator = json!({"peerDigest":association.peer_digest,"associationDigest":association.association_digest});
                let r = self
                    .reader_prepare_revoke_from_original(
                        selection,
                        ctx.clone(),
                        association.native(),
                        kind,
                    )
                    .map_err(failure)?
                    .wait()
                    .await
                    .map_err(failure)?;
                if !r.uses_original(ctx) {
                    return Err(denied());
                }
                let mut detail = details_for("revoke", &r)?;
                detail["subject"] = json!(subject);
                detail["association"] = locator;
                detail["localCancellationFirst"] = json!(true);
                let d = r.deadline().map_err(failure)?;
                (Prepared::Revoke(r), detail, d)
            }
            Action::Reconcile { association } => {
                let r = self
                    .reader_prepare_reconcile_from_original(
                        selection,
                        ctx.clone(),
                        association.native(),
                    )
                    .map_err(failure)?
                    .wait()
                    .await
                    .map_err(failure)?;
                if !r.uses_original(ctx) {
                    return Err(denied());
                }
                let detail = details_for("reconcileKnownHeadOnly", &r)?;
                let d = r.deadline().map_err(failure)?;
                (Prepared::Reconcile(r), detail, d)
            }
        };
        ctx.check(self).await?;
        ctx.now()?;
        let deadline = deadline.min(outer_deadline);
        if Instant::now() >= deadline {
            return Err(denied());
        }
        let details=serde_json::to_string(&json!({"reviewId":Uuid::new_v4().to_string(),"details":details,
            "remainingMilliseconds":deadline.saturating_duration_since(Instant::now()).as_millis() as u64,
            "humanConfirmationRequired":true,"deadlineRenewable":false})).map_err(|_|denied())?;
        if details.len() > 32768 {
            return Err(denied());
        }
        let shared = Arc::new(Shared {
            transferring: AtomicBool::new(false),
            held: Mutex::new(Some(Held::Prepared(prepared))),
            running: Mutex::new(None),
            cancelled: cancel.clone(),
            deadline,
            original: original.0.take().ok_or_else(denied)?,
        });
        let weak = Arc::downgrade(&shared);
        let scope = shared.original.scope_identity();
        tokio::spawn(async move {
            tokio::select! {_=cancel.cancelled()=>{},_=scope.cancelled()=>{},_=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>{}}
            if let Some(s) = weak.upgrade() {
                s.cancel();
            }
        });
        Ok(ReaderActionHandle {
            core: Arc::downgrade(&self.inner),
            shared,
            details,
            confirmed: AtomicBool::new(false),
        })
    }
    /// Read-only metadata. Persisted records and Probe never imply Actor apply ACK.
    pub async fn reader_status_json(&self, input: &str) -> AppResult<String> {
        let deadline = Instant::now() + Duration::from_secs(60);
        tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            self.reader_status_before(input),
        )
        .await
        .map_err(|_| failure(Failure::Expired))?
    }
    async fn reader_status_before(&self, input: &str) -> AppResult<String> {
        let r: Status = decode(input.as_bytes())?;
        ReaderOriginalContext::check_notification_scope(
            self,
            &r.selection.profile_id,
            &r.notification_scope_id,
        )
        .await?;
        let selection = r.selection.native()?;
        let status_slot = self
            .inner
            .reader_native
            .reserve_owner_review()
            .map_err(failure)?;
        let cancel = CancellationToken::new();
        let original = Original(Some(Arc::new(
            ReaderOriginalContext::capture_for_notification(
                self,
                selection.source.clone(),
                &cancel,
                &r.notification_scope_id,
            )
            .await?,
        )));
        let ctx = original.0.as_ref().ok_or_else(denied)?;
        let authority = NativeReaderAuthority::for_original_context(self, ctx)
            .await
            .map_err(|_| denied())?;
        let issuer = match authority.issuer_public(self, ctx).await {
            Ok(v) => json!({"state":"ready","issuerId":v.issuer_id,"publicKey":v.issuer_key}),
            Err(_) => json!({"state":"unknown","issuerId":null,"publicKey":null}),
        };
        let association = if let Some(locator) = r.association {
            match authority
                .registered_association(self, ctx, locator.peer_digest, locator.association_digest)
                .await
            {
                Ok(v) => {
                    json!({"state":"verifiedLocal","peerDigest":v.peer_digest,"associationDigest":locator.association_digest,
                    "latestSignedHead":v.latest_signed_head,"locallyRecordedInstalledHead":v.installed_head,"target":v.target,
                    "meaning":"verifiedLocalRecordNotCurrentRemoteAck"})
                }
                Err(_) => json!({"state":"unknown"}),
            }
        } else {
            json!({"state":"unknown"})
        };
        let choice_rows = authority.known_associations(self, ctx).await;
        let endpoint = match self
            .reader_observe_endpoint_once_from_original(selection, ctx.clone())
            .map_err(failure)?
            .wait()
            .await
        {
            Ok(value) => {
                if !value.uses_original(ctx) {
                    return Err(denied());
                }
                let e = value.endpoint();
                json!({"state":"observed","agentId":e.agent_id,"nodeContext":e.node_context,
                "agentCertificateHash":e.agent_certificate_hash,"agentKeyEpoch":e.agent_key_epoch,
                "targets":e.targets,"sshKeyDigest":value.ssh_key_digest(),"meaning":"authenticatedMetadataNotIssuerApproval"})
            }
            Err(_) => json!({"state":"unavailable"}),
        };
        ctx.check(self).await?;
        let now = AgentReaderTimestamp::from_native(ctx.now()?).map_err(|_| denied())?;
        let choices = match choice_rows {
            Ok((rows, truncated)) => {
                let mut items = Vec::new();
                for row in rows.into_iter().filter(|r| {
                    r.association.body.not_before <= now && now < r.association.body.expires_at
                }) {
                    items.push(json!({"peerDigest":row.peer_digest,"associationDigest":row.association.body.body_digest().map_err(|_|denied())?,
                        "agentId":row.target.agent_id,"resourceId":row.target.resource_id,"action":row.target.action,
                        "datasetKind":row.target.dataset.kind,"schemaVersion":row.target.dataset.schema_version,
                        "targetHash":row.target.descriptor_hash().map_err(|_|denied())?,"expiresAt":row.association.body.expires_at,
                        "deviceId":row.association.body.device_id,"scope":"telemetry.read"}));
                }
                json!({"state":"available","items":items,"truncated":truncated,"provenance":"verifiedLocalAuthorityNotRemoteAck"})
            }
            Err(_) => {
                json!({"state":"unavailable","items":[],"truncated":false,"provenance":"verifiedLocalAuthorityNotRemoteAck"})
            }
        };
        status_slot.check().map_err(failure)?;
        let (busy, quarantined) = self.inner.reader_native.public_counts().map_err(failure)?;
        let encoded=serde_json::to_string(&json!({"native":native_snapshot(ctx.snapshot()),"issuer":issuer,"association":association,
            "associationChoices":choices,"endpoint":endpoint,"runtime":{"busySlots":busy.saturating_sub(1),"excludesThisStatus":true,"quarantinedSlots":quarantined},"nodeActorApply":"unknown","remoteApplyAck":"notObservedByStatus"})).map_err(|_|denied())?;
        if encoded.len() > 32768 {
            return Err(denied());
        }
        // No await/recapture after final filtering. If a row expires even during
        // bounded encoding, deny publication rather than offer stale metadata.
        let final_now = u64::try_from(ctx.now()?.timestamp()).map_err(|_| denied())?;
        if choices["items"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|r| r["expiresAt"].as_u64().is_none_or(|t| t <= final_now))
        }) {
            return Err(denied());
        }
        Ok(encoded)
    }
}

#[cfg(test)]
#[path = "public_api_tests.rs"]
mod tests;
