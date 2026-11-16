//! Narrow trusted subprocess adapter; never an HTTP server or generic executor.
use cc_agent_identity_policy::*;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use zeroize::Zeroizing;
#[cfg(target_os = "linux")]
#[path = "cc-agent-identity/custody_worker.rs"]
mod custody_worker;
#[cfg(target_os = "linux")]
#[path = "cc-agent-identity/reader_worker.rs"]
mod reader_worker;
#[cfg(target_os = "linux")]
#[path = "cc-agent-identity/worker.rs"]
mod sqlite_worker;
#[cfg(target_os = "linux")]
#[path = "cc-agent-identity/tunnel_worker.rs"]
mod tunnel_worker;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    now: Timestamp,
    lease: Signed<Lease>,
    proof: Signed<AgentRequest>,
    freshness: Signed<Freshness>,
    operation: Operation,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    agent_id: AgentId,
    operation: Operation,
    expires_at: Timestamp,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationRequest {
    now: Timestamp,
    mutation: Mutation,
}
#[derive(Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum Mutation {
    Offer(Signed<Offer>),
    Claim {
        token: String,
        signed: Signed<Claim>,
    },
    Approve(Signed<Certificate>),
    Reject(Signed<Rejection>),
    Expire(InvitationId),
    Revoke(Signed<Revocation>),
    Rotate {
        certificate: Box<Signed<Certificate>>,
        old: Signed<RotationProof>,
        new: Signed<NewKeyProof>,
    },
    RotateOwner {
        old: Signed<OwnerRotation>,
        new: Signed<NewOwnerProof>,
    },
}
// Cap the entire command bundle, not only individual Statement::from_json
// calls: nested Signed<T> are deserialized by serde and would otherwise bypass
// that standalone frame-size gate through whitespace padding.
fn input() -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::new());
    std::io::stdin()
        .take(MAX_WIRE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::Quota);
    }
    Ok(bytes)
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|v|v=="worker-managed-exporter-exec") {
        #[cfg(target_os="linux")]
        { return run_managed_exporter_exec(&args); }
        #[cfg(not(target_os="linux"))]
        { return Err(Error::CapabilityDenied); }
    }
    #[cfg(all(target_os="linux", feature="owned-lifetime-protocol-proof"))]
    if args.first().is_some_and(|v|v=="owned-lifetime-protocol-owner") {
        return run_owned_lifetime_protocol_owner(&args);
    }
    if args.first().is_some_and(|v|v=="worker-collector-service-sqlite") {
        #[cfg(target_os="linux")] {return run_collector_service_worker(&args);}
        #[cfg(not(target_os="linux"))] {return Err(Error::CapabilityDenied);}
    }
    if args.first().is_some_and(|v| v == "worker-node-enroll-sqlite") {
        #[cfg(target_os = "linux")]
        { return run_enrollment_control_worker(&args); }
        #[cfg(not(target_os = "linux"))]
        { return Err(Error::CapabilityDenied); }
    }
    if args.first().is_some_and(|v| v == "worker-tunnel-supervised-sqlite") {
        #[cfg(target_os = "linux")]
        { return run_tunnel_supervised_worker(&args); }
        #[cfg(not(target_os = "linux"))]
        { return Err(Error::CapabilityDenied); }
    }
    if args.first().is_some_and(|v| v == "worker-tunnel-sqlite") {
        #[cfg(target_os = "linux")]
        {
            return tunnel_worker::run(&args);
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err(Error::CapabilityDenied);
        }
    }
    if args.first().is_some_and(|v| v == "worker-custody-sqlite") {
        #[cfg(target_os = "linux")]
        {
            return custody_worker::run(&args);
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err(Error::CapabilityDenied);
        }
    }
    if args
        .first()
        .is_some_and(|v| v == "worker-sqlite" || v == "init-sqlite")
    {
        #[cfg(target_os = "linux")]
        {
            return sqlite_worker::run(&args, args[0] == "init-sqlite");
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err(Error::CapabilityDenied);
        }
    }
    if args.first().is_some_and(|v| v == "sign-request") {
        return node_request_command(&args);
    }
    if args.first().is_some_and(|v| v == "sign-prepared-rotation") {
        return prepared_rotation_command(&args);
    }
    if args
        .first()
        .is_some_and(|v| v == "activate-prepared-rotation")
    {
        return activation_command(&args);
    }
    if args.first().is_some_and(|v| v == "inspect-node-state") {
        return inspection_command(&args);
    }
    if args.first().is_some_and(|v| v == "prepare-node-rotation") {
        if args.len() != 7
            || args[1] != "--state-dir"
            || args[3] != "--agent-id"
            || args[5] != "--expected-active-fingerprint"
        {
            return Err(Error::InvalidWire);
        }
        let fingerprint: Hash = serde_json::from_value(serde_json::Value::String(args[6].clone()))
            .map_err(|_| Error::InvalidWire)?;
        let public = prepare_node_rotation(
            &PathBuf::from(&args[2]),
            AgentId::new(&args[4])?,
            fingerprint,
        )?;
        println!(
            "{}",
            serde_json::to_string(&public).map_err(|_| Error::Storage)?
        );
        return Ok(());
    }
    if args
        .first()
        .is_some_and(|v| v == "claim-offer" || v == "claim-tunnel-offer")
    {
        return claim_command(&args);
    }
    if args.first().is_some_and(|v| v == "provision-node") {
        return provision_command(&args);
    }
    if args.first().is_some_and(|v| v == "verify-release") {
        return release_command(&args);
    }
    if args.len() != 7 || args[1] != "--store" || args[3] != "--anchor" || args[5] != "--owner-pin"
    {
        return Err(Error::InvalidWire);
    }
    let data = PathBuf::from(&args[2]);
    let anchor = PathBuf::from(&args[4]);
    let pin_path = PathBuf::from(&args[6]);
    if !pin_path.is_absolute() {
        return Err(Error::Storage);
    }
    let meta = std::fs::symlink_metadata(&pin_path).map_err(|_| Error::Storage)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_WIRE_BYTES as u64 {
        return Err(Error::Storage);
    }
    let pin: OwnerPin =
        serde_json::from_slice(&std::fs::read(pin_path).map_err(|_| Error::Storage)?)
            .map_err(|_| Error::InvalidWire)?;
    if args[0] == "status" {
        let status = FileStore::open(data, anchor, pin)?.status()?;
        println!(
            "{}",
            serde_json::to_string(&status).map_err(|_| Error::Storage)?
        );
        return Ok(());
    }
    let bytes = input()?;
    match args[0].as_str() {
        "init" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Init {
                now: Timestamp,
            }
            let init: Init = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
            FileStore::create(data, anchor, pin, Now(init.now))?;
            println!("{{\"initialized\":true}}");
        }
        "verify-request" => {
            let request: Request =
                serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
            // Only the two read-only resource handlers use this bridge. Their
            // trusted adapter maps exact HTTP paths to resource IDs and supplies
            // operation/now; this proof does not bind arbitrary HTTP payloads.
            if !matches!(request.operation, Operation::TelemetryRead { .. }) {
                return Err(Error::ScopeDenied);
            }
            let mut actor = Actor::new(FileStore::open(data, anchor, pin)?)?;
            let permit = actor.admit(
                request.lease,
                request.proof,
                Some(&request.freshness),
                &request.operation,
                Now(request.now),
            )?;
            actor.check_permit(&permit, Now(request.now))?;
            let response = Response {
                agent_id: permit.agent_id(),
                operation: permit.operation().clone(),
                expires_at: permit.expires_at(),
            };
            serde_json::to_writer(std::io::stdout().lock(), &response)
                .map_err(|_| Error::Storage)?;
            std::io::stdout()
                .write_all(b"\n")
                .map_err(|_| Error::Storage)?;
        }
        "mutate" => {
            let request: MutationRequest =
                serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
            let mut actor = Actor::new(FileStore::open(data, anchor, pin)?)?;
            let now = Now(request.now);
            apply_mutation(&mut actor, request.mutation, now)?;
            println!("{{\"revision\":{}}}", actor.snapshot()?.revision());
        }
        _ => return Err(Error::InvalidWire),
    }
    Ok(())
}
fn main() {
    let reader_args: Vec<_> = std::env::args().skip(1).collect();
    if reader_args.first().is_some_and(|command| {
        matches!(
            command.as_str(),
            "init-reader-sqlite" | "apply-reader" | "worker-reader-sqlite"
        )
    }) {
        #[cfg(target_os = "linux")]
        let result = reader_worker::run(&reader_args);
        #[cfg(not(target_os = "linux"))]
        let result: reader::ReaderResult<()> = Err(reader::ReaderError::CapabilityDenied);
        if let Err(error) = result {
            eprintln!(
                "{}",
                serde_json::json!({"event":"readerDenied","code":error.to_string()})
            );
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        eprintln!(
            "{}",
            serde_json::json!({"event":"identityDenied","code":error.to_string()})
        );
        std::process::exit(1);
    }
}

fn release_command(args: &[String]) -> Result<()> {
    if args.len() != 13
        || args[1] != "--public-key"
        || args[3] != "--manifest"
        || args[5] != "--artifact"
        || args[7] != "--platform"
        || args[9] != "--protocol"
        || args[11] != "--current-version"
    {
        return Err(Error::InvalidWire);
    }
    fn small(path: &str) -> Result<Vec<u8>> {
        let path = PathBuf::from(path);
        let meta = std::fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
        if !path.is_absolute()
            || !meta.is_file()
            || meta.file_type().is_symlink()
            || meta.len() > MAX_WIRE_BYTES as u64
        {
            return Err(Error::Storage);
        }
        std::fs::read(path).map_err(|_| Error::Storage)
    }
    let pin: ReleaseSignerPin =
        serde_json::from_slice(&small(&args[2])?).map_err(|_| Error::InvalidWire)?;
    let signed = Signed::<ReleaseManifest>::from_json(&small(&args[4])?)?;
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::InvalidTime)?
        .as_secs();
    let verified = verify_release(
        signed,
        &pin,
        &PathBuf::from(&args[6]),
        &args[8],
        args[10].parse().map_err(|_| Error::InvalidWire)?,
        args[12].parse().map_err(|_| Error::InvalidWire)?,
        Now(Timestamp::new(seconds)?),
    )?;
    let manifest = verified.manifest();
    let response = serde_json::json!({"version":manifest.version,"displayVersion":manifest.display_version,"protocol":manifest.protocol,"platform":manifest.platform,"artifactSha256":manifest.artifact_sha256,"artifactBytes":manifest.artifact_bytes});
    println!(
        "{}",
        serde_json::to_string(&response).map_err(|_| Error::Storage)?
    );
    Ok(())
}

fn provision_command(args: &[String]) -> Result<()> {
    if args.len() != 5 || args[1] != "--state-dir" || args[3] != "--agent-id" {
        return Err(Error::InvalidWire);
    }
    let public = provision_node(&PathBuf::from(&args[2]), AgentId::new(&args[4])?)?;
    println!(
        "{}",
        serde_json::to_string(&public).map_err(|_| Error::Storage)?
    );
    Ok(())
}

fn claim_command(args: &[String]) -> Result<()> {
    if args.len() != 7
        || args[1] != "--state-dir"
        || args[3] != "--agent-id"
        || args[5] != "--owner-pin"
    {
        return Err(Error::InvalidWire);
    }
    let pin_path = PathBuf::from(&args[6]);
    let meta = std::fs::symlink_metadata(&pin_path).map_err(|_| Error::Storage)?;
    if !pin_path.is_absolute()
        || !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() > MAX_WIRE_BYTES as u64
    {
        return Err(Error::Storage);
    }
    let mut pin_bytes = Vec::new();
    std::fs::File::open(pin_path)
        .map_err(|_| Error::Storage)?
        .take(MAX_WIRE_BYTES as u64 + 1)
        .read_to_end(&mut pin_bytes)
        .map_err(|_| Error::Storage)?;
    if pin_bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::Quota);
    }
    let pin: OwnerPin = serde_json::from_slice(&pin_bytes).map_err(|_| Error::InvalidWire)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ClaimInput {
        offer: Signed<Offer>,
    }
    let request: ClaimInput = serde_json::from_slice(&input()?).map_err(|_| Error::InvalidWire)?;
    // Observation time belongs to the local OS, never to the offered JSON.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::InvalidTime)?
        .as_secs();
    let claim_function = if args[0] == "claim-tunnel-offer" {
        create_tunnel_node_claim
    } else {
        create_node_claim
    };
    let claim = claim_function(
        &PathBuf::from(&args[2]),
        AgentId::new(&args[4])?,
        &pin,
        request.offer,
        Now(Timestamp::new(seconds)?),
    )?;
    println!(
        "{}",
        serde_json::to_string(&claim).map_err(|_| Error::Storage)?
    );
    Ok(())
}

fn node_request_command(args: &[String]) -> Result<()> {
    if args.len() != 7
        || args[1] != "--state-dir"
        || args[3] != "--agent-id"
        || args[5] != "--owner-pin"
    {
        return Err(Error::InvalidWire);
    }
    let pin_path = PathBuf::from(&args[6]);
    let meta = std::fs::symlink_metadata(&pin_path).map_err(|_| Error::Storage)?;
    if !pin_path.is_absolute()
        || !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() > MAX_WIRE_BYTES as u64
    {
        return Err(Error::Storage);
    }
    let mut pin_bytes = Vec::new();
    std::fs::File::open(pin_path)
        .map_err(|_| Error::Storage)?
        .take(MAX_WIRE_BYTES as u64 + 1)
        .read_to_end(&mut pin_bytes)
        .map_err(|_| Error::Storage)?;
    if pin_bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::Quota);
    }
    let pin: OwnerPin = serde_json::from_slice(&pin_bytes).map_err(|_| Error::InvalidWire)?;
    let binding: NodeRequestBinding =
        serde_json::from_slice(&input()?).map_err(|_| Error::InvalidWire)?;
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::InvalidTime)?
        .as_secs();
    let proof = create_node_request(
        &PathBuf::from(&args[2]),
        AgentId::new(&args[4])?,
        &pin,
        binding,
        Now(Timestamp::new(seconds)?),
    )?;
    println!(
        "{}",
        serde_json::to_string(&proof).map_err(|_| Error::Storage)?
    );
    Ok(())
}

fn prepared_rotation_command(args: &[String]) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        Err(Error::CapabilityDenied)
    }
    #[cfg(target_os = "linux")]
    {
        if args.len() != 9
            || args[1] != "--state-dir"
            || args[3] != "--agent-id"
            || args[5] != "--rotation-id"
            || args[7] != "--owner-pin"
        {
            return Err(Error::InvalidWire);
        }
        let path = PathBuf::from(&args[8]);
        if !path.is_absolute() {
            return Err(Error::Storage);
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_WIRE_BYTES as u64
        {
            return Err(Error::Storage);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| Error::Storage)?
            .take(MAX_WIRE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Storage)?;
        if bytes.len() > MAX_WIRE_BYTES {
            return Err(Error::Quota);
        }
        let pin: OwnerPin = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
        let request: PreparedRotationInput =
            serde_json::from_slice(&input()?).map_err(|_| Error::InvalidWire)?;
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::InvalidTime)?
            .as_secs();
        let proofs = sign_prepared_node_rotation(
            &PathBuf::from(&args[2]),
            AgentId::new(&args[4])?,
            &args[6],
            &pin,
            request,
            Now(Timestamp::new(seconds)?),
        )?;
        let output = serde_json::to_vec(&proofs).map_err(|_| Error::InvalidWire)?;
        if output.len() > MAX_WIRE_BYTES {
            return Err(Error::Quota);
        }
        // Exactly the value of existing {kind:"Rotate",value:...}; no commit ack.
        std::io::stdout()
            .write_all(&output)
            .map_err(|_| Error::Storage)?;
        println!();
        Ok(())
    }
}

fn inspection_command(args: &[String]) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        Err(Error::CapabilityDenied)
    }
    #[cfg(target_os = "linux")]
    {
        if args.len() != 5 || args[1] != "--state-dir" || args[3] != "--agent-id" {
            return Err(Error::InvalidWire);
        }
        let observation = inspect_node_state(&PathBuf::from(&args[2]), AgentId::new(&args[4])?)?;
        let mut output = std::io::stdout().lock();
        emit_inspection(&observation, &mut output)
    }
}
#[cfg(target_os = "linux")]
fn emit_inspection(observation: &NodeInspectionGuard, output: &mut impl Write) -> Result<()> {
    let bytes = serde_json::to_vec(observation.facts()).map_err(|_| Error::Storage)?;
    if bytes.len() > 4096 {
        return Err(Error::Quota);
    }
    observation.recheck_guard_identity()?;
    output.write_all(&bytes).map_err(|_| Error::Storage)?;
    output.write_all(b"\n").map_err(|_| Error::Storage)?;
    output.flush().map_err(|_| Error::Storage)?;
    observation.recheck_guard_identity()
}

#[cfg(all(test, target_os = "linux"))]
mod inspection_output_tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;
    struct FailOutput {
        bytes: Vec<u8>,
        limit: usize,
        flush_fails: bool,
    }
    impl Write for FailOutput {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.bytes.len() >= self.limit {
                return Err(std::io::Error::other("owned test write failure"));
            }
            let n = bytes.len().min(self.limit - self.bytes.len());
            self.bytes.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if self.flush_fails {
                Err(std::io::Error::other("owned test flush failure"))
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn inspection_output_failures_return_error_even_after_full_json_and_keep_guard_alive() {
        let path =
            std::env::temp_dir().join(format!("cc-inspector-output-owned-{}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let node = path.join("node");
        let id = AgentId::new("44444444-4444-4444-8444-444444444444").unwrap();
        provision_node(&node, id).unwrap();
        let guard = inspect_node_state(&node, id).unwrap();
        for (limit, flush_fails) in [(0, false), (5, false), (usize::MAX, true)] {
            let mut output = FailOutput {
                bytes: Vec::new(),
                limit,
                flush_fails,
            };
            assert_eq!(emit_inspection(&guard, &mut output), Err(Error::Storage));
            if flush_fails {
                let facts: serde_json::Value = serde_json::from_slice(&output.bytes).unwrap();
                assert_eq!(facts["selectionState"], "legacy1");
            }
            guard.recheck_guard_identity().unwrap();
        }
        drop(guard);
        assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
        assert!(path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("cc-inspector-output-owned-"));
        std::fs::remove_dir_all(path).unwrap();
    }
}

fn activation_command(args: &[String]) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        Err(Error::CapabilityDenied)
    }
    #[cfg(target_os = "linux")]
    {
        if args.len() != 15
            || args[1] != "--provider"
            || args[2] != "sqlite"
            || args[3] != "--state-dir"
            || args[5] != "--agent-id"
            || args[7] != "--rotation-id"
            || args[9] != "--store"
            || args[11] != "--anchor"
            || args[13] != "--owner-pin"
        {
            return Err(Error::InvalidWire);
        }
        let path = PathBuf::from(&args[14]);
        if !path.is_absolute() {
            return Err(Error::Storage);
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_WIRE_BYTES as u64
        {
            return Err(Error::Storage);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| Error::Storage)?
            .take(MAX_WIRE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Storage)?;
        if bytes.len() > MAX_WIRE_BYTES {
            return Err(Error::Quota);
        }
        let pin: OwnerPin = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
        let request: LocalActivationInput =
            serde_json::from_slice(&input()?).map_err(|_| Error::InvalidWire)?;
        let mut actor = Actor::new(SqliteJournalStore::open(
            &args[10],
            &args[12],
            pin,
            SqliteStorageBudget::default(),
        )?)?;
        let output = activate_prepared_rotation(
            &PathBuf::from(&args[4]),
            &mut actor,
            AgentId::new(&args[6])?,
            &args[8],
            request,
        )?;
        let bytes = serde_json::to_vec(&output).map_err(|_| Error::Storage)?;
        if bytes.len() > 4096 {
            return Err(Error::Quota);
        }
        std::io::stdout()
            .write_all(&bytes)
            .map_err(|_| Error::Storage)?;
        println!();
        Ok(())
    }
}

fn apply_mutation<S: CompareAndSwap>(
    actor: &mut Actor<S>,
    mutation: Mutation,
    now: Now,
) -> Result<()> {
    match mutation {
        Mutation::Offer(v) => actor.offer(v, now)?,
        Mutation::Claim { token, signed } => {
            let token = Zeroizing::new(token);
            let mut raw = Zeroizing::new([0u8; 32]);
            if token.len() != 64 {
                return Err(Error::InvalidToken);
            }
            hex::decode_to_slice(token.as_bytes(), raw.as_mut())
                .map_err(|_| Error::InvalidToken)?;
            actor.consume(&InvitationToken::from_bytes(*raw)?, signed, now)?;
        }
        Mutation::Approve(v) => actor.approve(v, now)?,
        Mutation::Reject(v) => actor.reject(v, now)?,
        Mutation::Expire(v) => actor.expire(v, now)?,
        Mutation::Revoke(v) => actor.revoke(v, now)?,
        Mutation::Rotate {
            certificate,
            old,
            new,
        } => actor.rotate(*certificate, old, new, now)?,
        Mutation::RotateOwner { old, new } => actor.rotate_owner(old, new, now)?,
    }
    Ok(())
}
