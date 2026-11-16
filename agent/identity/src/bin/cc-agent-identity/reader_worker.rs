//! Separate private reader control/data channel. Never mint/sign or Node cast.
use cc_agent_identity_policy::{
    reader::*, Actor, AgentId, Hash, Now, Operation, OwnerPin, SqliteJournalStore,
    SqliteStorageBudget, Timestamp,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::{Zeroize, Zeroizing};
type Actual = ReaderActor<ReaderSqliteLedger, SqliteJournalStore>;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    #[serde(rename_all = "camelCase")]
    Challenge {
        association_hash: Hash,
        operation_id: ReaderOperationId,
        expected_agent_id: AgentId,
        operation: Operation,
        target_descriptor_hash: Hash,
        freshness: Value,
    },
    #[serde(rename_all = "camelCase")]
    ReaderBegin {
        expected_agent_id: AgentId,
        operation: Operation,
        target_descriptor_hash: Hash,
        bundle: String,
    },
    #[serde(rename_all = "camelCase")]
    ReaderCheck {
        handle: String,
        reader_device_id: ReaderDeviceId,
        target_agent_id: AgentId,
        association_hash: Hash,
        target_descriptor_hash: Hash,
        operation: Operation,
    },
    #[serde(rename_all = "camelCase")]
    ReaderFinalize {
        handle: String,
        reader_device_id: ReaderDeviceId,
        target_agent_id: AgentId,
        association_hash: Hash,
        target_descriptor_hash: Hash,
        operation: Operation,
    },
    #[serde(rename_all = "camelCase")]
    ReaderRetire {
        handle: String,
        reader_device_id: ReaderDeviceId,
        target_agent_id: AgentId,
        association_hash: Hash,
        target_descriptor_hash: Hash,
        operation: Operation,
    },
}
struct Clock {
    last: Option<Duration>,
    failed: bool,
}
impl Clock {
    fn now(&mut self, deadline: Option<Instant>) -> ReaderResult<Now> {
        if self.failed {
            return Err(ReaderError::ClockUnavailable);
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(ReaderError::Deadline);
        }
        let observed = (|| {
            let raw = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| ReaderError::ClockUnavailable)?;
            if raw > Duration::from_secs(253_402_300_799) || self.last.is_some_and(|v| raw < v) {
                return Err(ReaderError::ClockUnavailable);
            }
            Ok((
                raw,
                Now(Timestamp::new(raw.as_secs()).map_err(|_| ReaderError::ClockUnavailable)?),
            ))
        })();
        match observed {
            Ok((raw, now)) => {
                self.last = Some(raw);
                Ok(now)
            }
            Err(e) => {
                self.failed = true;
                Err(e)
            }
        }
    }
}
struct Handle([u8; 32]);
impl Drop for Handle {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl Handle {
    fn from_text(text: &str) -> ReaderResult<Self> {
        if text.len() != 64
            || !text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ReaderError::InvalidWire);
        }
        let mut raw = [0; 32];
        hex::decode_to_slice(text, &mut raw).map_err(|_| ReaderError::InvalidWire)?;
        if raw == [0; 32] {
            return Err(ReaderError::InvalidWire);
        }
        Ok(Self(raw))
    }
    fn same(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;
        bool::from(self.0.ct_eq(&other.0))
    }
}
struct Slot {
    handle: Handle,
    permit: ReaderPermit,
    deadline: Instant,
    checked: bool,
}
impl Slot {
    fn context(
        &self,
        device: ReaderDeviceId,
        agent: AgentId,
        association: Hash,
        target: Hash,
        op: &Operation,
    ) -> ReaderResult<()> {
        if device != self.permit.reader_device_id() {
            return Err(ReaderError::WrongDevice);
        }
        if association != self.permit.association_hash() {
            return Err(ReaderError::WrongAssociation);
        }
        if agent != self.permit.target_agent_id() || target != self.permit.target_descriptor_hash()
        {
            return Err(ReaderError::WrongTarget);
        }
        if op != self.permit.operation() {
            return Err(ReaderError::WrongOperation);
        }
        Ok(())
    }
}
fn output_frame(value: Value) -> ReaderResult<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(serde_json::to_vec(&value).map_err(|_| ReaderError::Storage)?);
    if bytes.len() >= 4096 {
        return Err(ReaderError::Quota);
    }
    bytes.push(b'\n');
    Ok(bytes)
}
fn emit(value: Value) -> ReaderResult<()> {
    let bytes = output_frame(value)?;
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)
        .and_then(|_| out.flush())
        .map_err(|_| ReaderError::Storage)
}
fn decode_frame(line: &[u8]) -> ReaderResult<Command> {
    if line.len() > READER_FRAME_BYTES || !line.ends_with(b"\n") {
        return Err(ReaderError::Protocol);
    }
    strict_json(line, READER_FRAME_BYTES).map_err(|_| ReaderError::Protocol)
}
fn fatal(error: ReaderError) -> bool {
    matches!(
        error,
        ReaderError::ClockUnavailable
            | ReaderError::Storage
            | ReaderError::UncertainCommit
            | ReaderError::Entropy
            | ReaderError::Protocol
    )
}
fn input() -> ReaderResult<Zeroizing<Vec<u8>>> {
    let mut raw = Zeroizing::new(Vec::new());
    std::io::stdin()
        .take(READER_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| ReaderError::Storage)?;
    if raw.len() > READER_FRAME_BYTES {
        return Err(ReaderError::Quota);
    }
    Ok(raw)
}
fn path(value: &str) -> ReaderResult<PathBuf> {
    let p = PathBuf::from(value);
    if !p.is_absolute()
        || p.components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(ReaderError::Storage);
    }
    Ok(p)
}
fn load<T: serde::de::DeserializeOwned>(p: &Path) -> ReaderResult<T> {
    let before = std::fs::symlink_metadata(p).map_err(|_| ReaderError::Storage)?;
    let uid = std::fs::metadata("/proc/self")
        .map_err(|_| ReaderError::Storage)?
        .uid();
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.nlink() != 1
        || before.uid() != uid
        || before.mode() & 0o022 != 0
        || before.len() > READER_FRAME_BYTES as u64
    {
        return Err(ReaderError::Storage);
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x20000)
        .open(p)
        .map_err(|_| ReaderError::Storage)?;
    let after = file.metadata().map_err(|_| ReaderError::Storage)?;
    if before.dev() != after.dev() || before.ino() != after.ino() {
        return Err(ReaderError::Storage);
    }
    let mut raw = Zeroizing::new(Vec::new());
    std::io::Read::by_ref(&mut file)
        .take(READER_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| ReaderError::Storage)?;
    strict_json(&raw, READER_FRAME_BYTES)
}
fn take(slots: &mut Vec<Slot>, handle: &Handle) -> ReaderResult<Slot> {
    let index = slots
        .iter()
        .position(|s| s.handle.same(handle))
        .ok_or(ReaderError::LostHandle)?;
    Ok(slots.swap_remove(index))
}
fn execute(
    actor: &mut Actual,
    clock: &mut Clock,
    slots: &mut Vec<Slot>,
    command: Command,
) -> ReaderResult<Value> {
    let now = clock.now(None)?.0;
    slots.retain(|s| Instant::now() < s.deadline && now < s.permit.expires_at());
    match command {
        Command::Challenge {
            association_hash,
            operation_id,
            expected_agent_id,
            operation,
            target_descriptor_hash,
            freshness,
        } => {
            let fresh = serde_json::from_value(freshness).map_err(|_| ReaderError::InvalidWire)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            let c = actor.challenge(
                association_hash,
                operation_id,
                expected_agent_id,
                &operation,
                target_descriptor_hash,
                fresh,
                &mut || clock.now(Some(deadline)),
            )?;
            Ok(json!({"challengeHash":c.digest()?,"challenge":c}))
        }
        Command::ReaderBegin {
            expected_agent_id,
            operation,
            target_descriptor_hash,
            bundle,
        } => {
            let started = Instant::now();
            let deadline = started + Duration::from_secs(5);
            if slots.len() >= 8 {
                return Err(ReaderError::Quota);
            }
            let encoded = Zeroizing::new(bundle);
            let bytes = Zeroizing::new(reader_decode_base64url(&encoded, READER_BUNDLE_BYTES)?);
            let bundle = ReaderBundle::from_json(&bytes)?;
            let mut entropy = Zeroizing::new([0u8; 32]);
            getrandom::fill(entropy.as_mut()).map_err(|_| ReaderError::Entropy)?;
            let handle = Handle(*entropy);
            if handle.0 == [0; 32] || slots.iter().any(|s| s.handle.same(&handle)) {
                return Err(ReaderError::Entropy);
            }
            let permit = actor.begin(
                bundle,
                expected_agent_id,
                &operation,
                target_descriptor_hash,
                &mut || clock.now(Some(deadline)),
            )?;
            let now = clock.now(Some(deadline))?.0;
            if now >= permit.expires_at() {
                return Err(ReaderError::Expired);
            }
            let response = json!({"readerDeviceId":permit.reader_device_id(),"targetAgentId":permit.target_agent_id(),
                "operation":permit.operation(),"associationHash":permit.association_hash(),"nativeProfileId":permit.native_profile_id(),
                "nativeHostId":permit.native_host_id(),"nativeBindingId":permit.native_binding_id(),"nativeBindingRevision":permit.native_binding_revision(),
                "targetDescriptorHash":permit.target_descriptor_hash(),"expiresAt":permit.expires_at(),"handle":hex::encode(handle.0)});
            slots.push(Slot {
                handle,
                permit,
                deadline,
                checked: false,
            });
            Ok(response)
        }
        Command::ReaderCheck {
            handle,
            reader_device_id,
            target_agent_id,
            association_hash,
            target_descriptor_hash,
            operation,
        } => {
            let raw = Zeroizing::new(handle);
            let mut slot = take(slots, &Handle::from_text(&raw)?)?;
            slot.context(
                reader_device_id,
                target_agent_id,
                association_hash,
                target_descriptor_hash,
                &operation,
            )?;
            if slot.checked {
                return Err(ReaderError::LostHandle);
            }
            actor.check_permit(&slot.permit, &mut || clock.now(Some(slot.deadline)))?;
            if clock.now(Some(slot.deadline))?.0 >= slot.permit.expires_at() {
                return Err(ReaderError::Expired);
            }
            slot.checked = true;
            slots.push(slot);
            Ok(json!({"phase":"checked"}))
        }
        Command::ReaderFinalize {
            handle,
            reader_device_id,
            target_agent_id,
            association_hash,
            target_descriptor_hash,
            operation,
        } => {
            let raw = Zeroizing::new(handle);
            let slot = take(slots, &Handle::from_text(&raw)?)?;
            slot.context(
                reader_device_id,
                target_agent_id,
                association_hash,
                target_descriptor_hash,
                &operation,
            )?;
            if !slot.checked {
                return Err(ReaderError::LostHandle);
            }
            actor.check_permit(&slot.permit, &mut || clock.now(Some(slot.deadline)))?;
            if clock.now(Some(slot.deadline))?.0 >= slot.permit.expires_at() {
                return Err(ReaderError::Expired);
            }
            Ok(json!({"phase":"finalized"}))
        }
        Command::ReaderRetire {
            handle,
            reader_device_id,
            target_agent_id,
            association_hash,
            target_descriptor_hash,
            operation,
        } => {
            let raw = Zeroizing::new(handle);
            match take(slots, &Handle::from_text(&raw)?) {
                Ok(slot) => slot.context(
                    reader_device_id,
                    target_agent_id,
                    association_hash,
                    target_descriptor_hash,
                    &operation,
                )?,
                Err(ReaderError::LostHandle) => (),
                Err(e) => return Err(e),
            }
            Ok(json!({"phase":"retired"}))
        }
    }
}
pub(super) fn run(args: &[String]) -> ReaderResult<()> {
    let flags = [
        "--reader-db-dir",
        "--reader-anchor-dir",
        "--reader-issuer-pin",
        "--node-db-dir",
        "--node-anchor-dir",
        "--node-owner-pin",
        "--target",
    ];
    if args.len() != 15 || flags.iter().enumerate().any(|(i, f)| args[i * 2 + 1] != *f) {
        return Err(ReaderError::InvalidWire);
    }
    let paths: Vec<_> = (0..7)
        .map(|i| path(&args[i * 2 + 2]))
        .collect::<ReaderResult<_>>()?;
    for (i, j) in [(0, 1), (0, 3), (0, 4), (1, 3), (1, 4)] {
        if paths[i].starts_with(&paths[j]) || paths[j].starts_with(&paths[i]) {
            return Err(ReaderError::Storage);
        }
    }
    let pin: ReaderIssuerPin = load(&paths[2])?;
    pin.validate()?;
    let node_pin: OwnerPin = load(&paths[5])?;
    let origin = reader_node_trust_origin(&node_pin)?;
    if pin.node_trust_origin_hash != origin {
        return Err(ReaderError::WrongTarget);
    }
    let targets: ReaderTargetSet = load(&paths[6])?;
    targets.validate()?;
    let mut clock = Clock {
        last: None,
        failed: false,
    };
    let node = Actor::new(SqliteJournalStore::open(
        &paths[3],
        &paths[4],
        node_pin,
        SqliteStorageBudget::default(),
    )?)?;
    let store = if args[0] == "init-reader-sqlite" {
        ReaderSqliteLedger::create(
            &paths[0],
            &paths[1],
            pin,
            clock.now(None)?,
            ReaderStorageBudget::default(),
        )?
    } else {
        ReaderSqliteLedger::open(&paths[0], &paths[1], pin, ReaderStorageBudget::default())?
    };
    let mut actor = ReaderActor::new(store, node, targets, origin)?;
    if args[0] == "init-reader-sqlite" {
        return emit(
            json!({"ok":true,"metadata":{"initialized":true,"provider":"reader-sqlite-journal-v1"}}),
        );
    }
    if args[0] == "apply-reader" {
        actor.administer(strict_json(&input()?, READER_FRAME_BYTES)?, &mut || {
            clock.now(None)
        })?;
        let state = actor.snapshot()?;
        return emit(
            json!({"ok":true,"metadata":{"authorityRevision":state.authority().authority_revision,"authorityHash":state.authority().authority_hash}}),
        );
    }
    if args[0] != "worker-reader-sqlite" {
        return Err(ReaderError::InvalidWire);
    }
    let mut slots = Vec::new();
    let mut stdin = std::io::stdin().lock();
    loop {
        let mut line = Zeroizing::new(Vec::new());
        let count = stdin
            .by_ref()
            .take(READER_FRAME_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|_| ReaderError::Storage)?;
        if count == 0 {
            return Ok(());
        }
        let command = decode_frame(&line);
        let result =
            command.and_then(|command| execute(&mut actor, &mut clock, &mut slots, command));
        match result {
            Ok(metadata) => emit(json!({"ok":true,"metadata":metadata}))?,
            Err(error) => {
                emit(json!({"ok":false,"code":error.to_string()}))?;
                if fatal(error) {
                    return Err(error);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_reader_frame_boundaries_are_terminal_and_inner_signed_data_is_not() {
        let raw = br#"{"action":"readerBegin","expectedAgentId":"11111111-1111-4111-8111-111111111111","operation":{"kind":"TelemetryRead","resource":"22222222-2222-4222-8222-222222222222"},"targetDescriptorHash":"9999999999999999999999999999999999999999999999999999999999999999","bundle":"e30"}"#;
        // Build with the existing Operation serializer; no guessed Node JSON shape.
        let mut value: Value = serde_json::from_slice(raw).unwrap();
        value["operation"] = serde_json::to_value(Operation::TelemetryRead {
            source: cc_agent_identity_policy::ResourceId::new(
                "22222222-2222-4222-8222-222222222222",
            )
            .unwrap(),
        })
        .unwrap();
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        assert!(decode_frame(&bytes).is_ok());
        let Command::ReaderBegin { bundle, .. } = decode_frame(&bytes).unwrap() else {
            panic!("expected reader begin");
        };
        assert!(ReaderBundle::from_json(
            &reader_decode_base64url(&bundle, READER_BUNDLE_BYTES).unwrap()
        )
        .is_err());
        assert!(!fatal(ReaderError::InvalidWire));
        assert!(!fatal(ReaderError::InvalidTime));
        assert!(fatal(ReaderError::ClockUnavailable));
        assert!(decode_frame(&bytes[..bytes.len() - 1]).is_err());
        let mut exact = vec![b' '; READER_FRAME_BYTES - bytes.len()];
        exact.extend(&bytes);
        assert!(decode_frame(&exact).is_ok());
        exact.insert(0, b' ');
        assert!(decode_frame(&exact).is_err());
        assert!(
            decode_frame(b"{\"action\":\"readerBegin\",\"action\":\"readerRetire\"}\n").is_err()
        );
        assert!(decode_frame(b"{\"action\":\"signAnything\"}\n").is_err());
    }
    #[test]
    fn private_reader_output_limit_includes_lf_and_handle_parser_is_strict() {
        // A JSON string uses two quote bytes; the LF occupies the final byte.
        assert_eq!(output_frame(json!("x".repeat(4093))).unwrap().len(), 4096);
        assert!(output_frame(json!("x".repeat(4094))).is_err());
        for value in [
            "00".repeat(32),
            "AA".repeat(32),
            "a".repeat(63),
            "g".repeat(64),
        ] {
            assert!(Handle::from_text(&value).is_err());
        }
        assert!(Handle::from_text(&"ab".repeat(32)).is_ok());
    }
}
