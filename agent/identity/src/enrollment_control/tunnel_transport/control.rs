//! One explicit original pinned-SSH credential bootstrap, never admission.
//! Missing/partial/existing private state is not provisioned/adopted/rebound.
use super::*;
use std::io::{Seek, SeekFrom};
use std::os::fd::AsRawFd;

const CONTROL: &str = "/var/lib/consolecrypt-agent/tunnel/control";
const RECORD: &str = "/var/lib/consolecrypt-agent/tunnel/control/bootstrap.json";
const DIRECTORY: i32 = rustix::fs::OFlags::DIRECTORY.bits() as i32; // Linux O_DIRECTORY; same fixed Linux worker.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Bootstrap {
    expected_rev: u64,
    expected_pin: OwnerPin,
    certificate_digest: Hash,
    control_id: String,
    #[serde(deserialize_with = "secret_string")]
    agent_token: Zeroizing<String>,
    expires_at: Timestamp,
}

fn control_id(value: &str) -> Result<()> {
    let id = uuid::Uuid::parse_str(value).map_err(|_| Error::InvalidWire)?;
    if id.is_nil() || id.to_string() != value { return Err(Error::InvalidWire); }
    Ok(())
}

fn token(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    if bytes.len() != 48 || !bytes.starts_with(b"ccrc_")
        || !bytes[5..].iter().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(b))
        || !b"AEIMQUYcgkosw048".contains(&bytes[47]) {
        return Err(Error::InvalidToken);
    }
    Ok(())
}

fn expiry(expires: Timestamp, owner: Timestamp, certificate: Timestamp, now: Now) -> Result<()> {
    if expires <= now.0 || expires > owner || expires > certificate
        || expires > now.0.checked_add(86_400)? {
        return Err(Error::Expired);
    }
    Ok(())
}

fn directory_identity(meta: &fs::Metadata) -> (u64, u64, u32, u32) {
    (meta.dev(), meta.ino(), meta.uid(), meta.mode())
}

fn held_control() -> Result<(File, (u64, u64, u32, u32))> {
    let uid = current_uid()?;
    if uid == 0 { return Err(Error::Storage); }
    let path = Path::new(CONTROL);
    private_root(path, uid)?; // Explicitly pre-created; no mkdir/provision here.
    let before = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    let held = OpenOptions::new().read(true).custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC | DIRECTORY)
        .open(path).map_err(|_| Error::Storage)?;
    if directory_identity(&before) != directory_identity(&held.metadata().map_err(|_| Error::Storage)?) {
        return Err(Error::Storage);
    }
    Ok((held, directory_identity(&before)))
}

fn check_directory(held: &File, captured: (u64, u64, u32, u32)) -> Result<()> {
    private_root(Path::new(CONTROL), current_uid()?)?;
    if directory_identity(&held.metadata().map_err(|_| Error::Storage)?) != captured
        || directory_identity(&fs::symlink_metadata(CONTROL).map_err(|_| Error::Storage)?) != captured {
        return Err(Error::Storage);
    }
    Ok(())
}

fn same_state_owner(original: &Original, uid: u32) -> Result<()> {
    for path in [&original.config.node_dir, &original.config.db_dir, &original.config.anchor_dir] {
        private_root(path, uid)?;
        let meta = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
        if meta.uid() != uid || meta.mode() & 0o777 != 0o700 { return Err(Error::Storage); }
    }
    Ok(())
}

fn file_identity(meta: &fs::Metadata) -> (u64, u64, u32, u32, u64) {
    (meta.dev(), meta.ino(), meta.uid(), meta.mode(), meta.nlink())
}

fn create_record(correlation: &mut Option<String>, anchored: &Path) -> Result<File> {
    // The mutating syscall itself may have an uncertain result. Mark Unknown
    // BEFORE it, including an error return; no filesystem error proves no effect.
    *correlation = None;
    OpenOptions::new().read(true).write(true).create_new(true).mode(0o600)
        .custom_flags(NOFOLLOW | NONBLOCK | CLOEXEC).open(anchored).map_err(|_| Error::Storage)
}

pub(super) fn bootstrap(
    pipe: &mut Pipe, clock: &mut Clock, frame: Envelope, original: &Original,
    actor: &Actor<SqliteJournalStore>, installed: &Installed, correlation: &mut Option<String>,
) -> Result<()> {
    let id = frame.request_id;
    let request: Bootstrap = decode(frame.payload)?;
    if request.expected_rev != original.revision || request.expected_pin != original.owner {
        return Err(Error::Conflict);
    }
    if request.certificate_digest != original.certificate.digest()? { return Err(Error::WrongClaim); }
    control_id(&request.control_id)?;
    token(&request.agent_token)?;
    expiry(request.expires_at, original.owner.expires_at, original.certificate.expires_at, clock.now()?)?;
    original.check(actor, clock)?;
    installed.check()?;
    let (directory, captured) = held_control()?;
    same_state_owner(original, current_uid()?)?;
    // Bind the bearer to the exact root-installed Relay routing/trust material.
    // A later root config change must not silently send it to another endpoint.
    let transport_raw = installed.files.iter().find(|(path, _)| path.as_path() == Path::new(TRANSPORT))
        .map(|(_, raw)| raw).ok_or(Error::Storage)?;
    let ca_raw = installed.files.iter().find(|(path, _)| path.as_path() == Path::new(RELAY_CA))
        .map(|(_, raw)| raw).ok_or(Error::Storage)?;
    let mut value = json!({"format":"cc-agent-tunnel-control-bootstrap/v1",
        "controlId":request.control_id,"agentToken":request.agent_token.as_str(),"expiresAt":request.expires_at,
        "node":original.node,"certificateDigest":request.certificate_digest,"ownerPin":original.owner,
        "actorRevision":original.revision,"transportConfigSha256":hash(transport_raw),"relayCaSha256":hash(ca_raw)});
    let serialized = canonical(&value);
    if let Some(Value::String(secret)) = value.get_mut("agentToken") { secret.zeroize(); }
    let raw = Zeroizing::new(serialized?);
    check_directory(&directory, captured)?;
    original.check(actor, clock)?;
    installed.check()?;
    same_state_owner(original, current_uid()?)?;
    expiry(request.expires_at, original.owner.expires_at, original.certificate.expires_at, clock.now()?)?;
    // O_EXCL is the publication boundary: never overwrite/rebind an existing
    // complete OR partial credential. A crashed partial file is manual Unknown.
    // Resolve through our OWN held directory FD, not a second traversal of a
    // replaceable parent. /proc/self/fd is already required by this fixed Linux
    // worker; neither the FD nor basename is a caller input.
    let anchored = PathBuf::from(format!("/proc/self/fd/{}/bootstrap.json", directory.as_raw_fd()));
    let mut file = create_record(correlation, &anchored)?;
    // The attempt already made this operation Unknown, even if open failed.
    // A returned FD permits readback; it does not restore notCommitted custody.
    let published = (|| -> Result<()> {
        let before = file.metadata().map_err(|_| Error::Storage)?;
        if !before.is_file() || before.uid() != current_uid()? || before.nlink() != 1
            || before.mode() & 0o777 != 0o600 || before.len() != 0 { return Err(Error::Storage); }
        file.write_all(&raw).map_err(|_| Error::Storage)?;
        file.sync_all().map_err(|_| Error::Storage)?;
        directory.sync_all().map_err(|_| Error::Storage)?;
        check_directory(&directory, captured)?;
        same_state_owner(original, current_uid()?)?;
        let after = fs::symlink_metadata(RECORD).map_err(|_| Error::Storage)?;
        let held = file.metadata().map_err(|_| Error::Storage)?;
        if file_identity(&before) != file_identity(&after) || file_identity(&before) != file_identity(&held)
            || after.file_type().is_symlink() || held.len() != raw.len() as u64 { return Err(Error::Storage); }
        file.seek(SeekFrom::Start(0)).map_err(|_| Error::Storage)?;
        let mut readback = Zeroizing::new(Vec::new());
        Read::by_ref(&mut file).take(MAX_WIRE_BYTES as u64 + 1).read_to_end(&mut readback)
            .map_err(|_| Error::Storage)?;
        if readback.as_slice() != raw.as_slice() { return Err(Error::Storage); }
        let final_held = file.metadata().map_err(|_| Error::Storage)?;
        let final_path = fs::symlink_metadata(RECORD).map_err(|_| Error::Storage)?;
        if file_identity(&final_held) != file_identity(&held) || file_identity(&final_path) != file_identity(&held)
            || (final_held.len(), final_held.mtime(), final_held.mtime_nsec(), final_held.ctime(), final_held.ctime_nsec())
                != (held.len(), held.mtime(), held.mtime_nsec(), held.ctime(), held.ctime_nsec()) {
            return Err(Error::Storage);
        }
        original.check(actor, clock)?;
        installed.check()?;
        check_directory(&directory, captured)?;
        expiry(request.expires_at, original.owner.expires_at, original.certificate.expires_at, clock.now()?)?;
        Ok(())
    })();
    if let Err(error) = published {
        // No token echo, rollback/unlink, second mutation or receipt adoption.
        let _ = pipe.write(&json!({"format":FORMAT,"requestId":id,"kind":"error",
            "payload":{"code":"transportUnavailable","effect":"unknown"}}), clock);
        return Err(error);
    }
    pipe.write(&json!({"format":FORMAT,"requestId":id,"kind":"bootstrapTunnelControl","payload":{
        "controlId":request.control_id,"expiresAt":request.expires_at,"node":original.node,
        "certificateDigest":request.certificate_digest,"actorRevision":original.revision}}), clock)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Same owned unique/exclusive directory pattern as enrollment_control tests;
    // no extra dev dependency or recursive cleanup of unknown leaves.
    struct TestDirectory {
        path:std::path::PathBuf,
        identity:(u64,u64,u32,u32,u64),
        original:Option<(u64,u64,u32,u32,u64)>,
    }
    impl TestDirectory {
        fn new()->Self {
            use std::os::unix::fs::DirBuilderExt;
            let mut random=[0u8;16];getrandom::fill(&mut random).unwrap();
            let path=std::env::temp_dir().join(format!("cc-control-test-{}",hex::encode(random)));
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            let identity=file_identity(&fs::symlink_metadata(&path).unwrap());
            Self {path,identity,original:None}
        }
        fn path(&self)->&Path {&self.path}
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let Ok(dir)=fs::symlink_metadata(&self.path) else {return};
            if !dir.is_dir() || dir.file_type().is_symlink() || file_identity(&dir)!=self.identity {return}
            let path=self.path.join("bootstrap.json");
            if let Ok(named)=fs::symlink_metadata(&path) {
                if !named.is_file() || named.file_type().is_symlink() || Some(file_identity(&named))!=self.original {return}
                if fs::remove_file(path).is_err(){return}
            }
            let _=fs::remove_dir(&self.path);
        }
    }


    #[test]
    fn control_token_rejects_noncanonical_low_bits_and_other_capability_purposes() {
        let good = format!("ccrc_{}A", "A".repeat(42));
        assert!(token(&good).is_ok());
        for wrong in [good.replace("ccrc_", "ccrl_"), format!("ccrc_{}B", "A".repeat(42)),
            format!("ccrc_{}=", "A".repeat(42)), format!("{good}\n"), "ccrc_".into()] {
            assert!(token(&wrong).is_err());
        }
    }

    #[test]
    fn control_expiry_cannot_extend_original_owner_certificate_or_one_day() {
        let t = |n| Timestamp::new(n).unwrap();
        assert!(expiry(t(130), t(150), t(140), Now(t(100))).is_ok());
        for (end, owner, cert) in [(100,150,140),(151,150,200),(141,200,140),(86_501,90_000,90_000)] {
            assert!(expiry(t(end), t(owner), t(cert), Now(t(100))).is_err());
        }
    }

    #[test]
    fn control_uuid_is_canonical_nonzero_and_never_a_path_or_url() {
        assert!(control_id("e2b9593e-794b-41d8-ae99-1d0663c83916").is_ok());
        for wrong in ["00000000-0000-0000-0000-000000000000", "E2B9593E-794B-41D8-AE99-1D0663C83916",
            "/tmp/bootstrap.json", "https://relay.invalid/control"] { assert!(control_id(wrong).is_err()); }
    }

    #[test]
    fn failed_create_attempt_cannot_emit_not_committed_or_delete_existing_partial() {
        let mut root = TestDirectory::new();
        let path = root.path().join("bootstrap.json");
        fs::write(&path, b"retained partial").unwrap();
        let before = fs::symlink_metadata(&path).unwrap();
        root.original=Some(file_identity(&before));
        let mut correlation = Some("original-request".to_owned());
        assert!(create_record(&mut correlation, &path).is_err());
        assert!(correlation.is_none(), "outer handler cannot report notCommitted after attempt");
        assert_eq!(fs::read(&path).unwrap(), b"retained partial");
        assert_eq!(file_identity(&before), file_identity(&fs::symlink_metadata(&path).unwrap()));
    }
}
