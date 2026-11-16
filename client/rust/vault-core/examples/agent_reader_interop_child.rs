//! TEST ONLY. One in-memory Native fixture identity, never a production profile.
//! No key import/export/storage, account proof, Agent key or general signer.
use cc_crypto_core::agent_reader::*;
use cc_vault_core::DeviceIdentity;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufReader, Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FORMAT: &str = "cc-native-reader-interop-child/v1";
const INPUT_MAX: usize = 32768;
const OUTPUT_MAX: usize = 8192;
const LIFETIME: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug)]
enum Code {
    InvalidWire,
    StateUnavailable,
    Rejected,
    Expired,
    ClockUnavailable,
    OutputUnknown,
}
type Result<T> = std::result::Result<T, Code>;
fn crypto(error: AgentReaderError) -> Code {
    match error {
        AgentReaderError::Expired => Code::Expired,
        AgentReaderError::Entropy => Code::StateUnavailable,
        _ => Code::Rejected,
    }
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Metadata {
    native_profile_id: AgentReaderNativeProfileId,
    native_host_id: AgentReaderNativeHostId,
    native_host_revision: u64,
    native_binding_id: AgentReaderNativeBindingId,
    native_binding_revision: u64,
    native_binding_digest: AgentReaderHash,
    native_source_id: String,
    native_source_revision: u64,
    transport_peer_hash: AgentReaderHash,
}
impl Metadata {
    fn validate(&self) -> Result<()> {
        if self.native_host_revision == 0
            || self.native_binding_revision == 0
            || self.native_source_revision == 0
            || self.native_binding_digest == AgentReaderHash::from_bytes([0; 32])
            || self.transport_peer_hash == AgentReaderHash::from_bytes([0; 32])
            || self.native_source_id.is_empty()
            || self.native_source_id.len() > 128
            || self.native_source_id.chars().any(char::is_control)
        {
            return Err(Code::InvalidWire);
        }
        Ok(())
    }
    fn matches(&self, a: &AgentReaderAssociationV1) -> bool {
        self.native_profile_id == a.native_profile_id
            && self.native_host_id == a.native_host_id
            && self.native_host_revision == a.native_host_revision
            && self.native_binding_id == a.native_binding_id
            && self.native_binding_revision == a.native_binding_revision
            && self.native_binding_digest == a.native_binding_digest
            && self.native_source_id == a.native_source_id
            && self.native_source_revision == a.native_source_revision
            && self.transport_peer_hash == a.transport_peer_hash
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitRequest {
    format: String,
    request_id: String,
    kind: String,
    metadata: Metadata,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Expected {
    format: String,
    issuer_pin: AgentReaderIssuerPinV1,
    current_head: AgentReaderHeadV1,
    association_body: AgentReaderAssociationV1,
    target: AgentReaderTargetDescriptorV1,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignRequest {
    format: String,
    request_id: String,
    kind: String,
    expected: Expected,
    packet: Value,
}

struct Clock {
    deadline: Instant,
    floor: Duration,
}
impl Clock {
    fn new(start: Instant) -> Self {
        Self {
            deadline: start + LIFETIME,
            floor: Duration::ZERO,
        }
    }
    fn observe(&mut self) -> Result<Duration> {
        if Instant::now() >= self.deadline {
            return Err(Code::ClockUnavailable);
        }
        let raw = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Code::ClockUnavailable)?;
        if raw < self.floor || raw.as_secs() > AGENT_READER_MAX_TIMESTAMP {
            return Err(Code::ClockUnavailable);
        }
        self.floor = raw;
        Ok(raw)
    }
    fn timestamp(&mut self) -> Result<cc_protocol::Timestamp> {
        let raw = self.observe()?;
        chrono::DateTime::<chrono::Utc>::from_timestamp(raw.as_secs() as i64, raw.subsec_nanos())
            .ok_or(Code::ClockUnavailable)
    }
    fn publication(&mut self, expires: Option<AgentReaderTimestamp>) -> Result<()> {
        let raw = self.observe()?;
        if expires.is_some_and(|end| raw >= Duration::from_secs(end.seconds())) {
            return Err(Code::Expired);
        }
        Ok(())
    }
}
fn id(value: &str) -> Result<()> {
    AgentReaderOperationId::new(value)
        .map(|_| ())
        .map_err(|_| Code::InvalidWire)
}
fn request_id(bytes: &[u8]) -> Option<String> {
    let value: Value = strict_json(bytes, INPUT_MAX).ok()?;
    let text = value.get("requestId")?.as_str()?;
    id(text).ok()?;
    Some(text.to_owned())
}
fn frame<R: Read>(input: &mut R) -> Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0];
        let got = input.read(&mut byte).map_err(|_| Code::InvalidWire)?;
        if got == 0 {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(Code::InvalidWire)
            };
        }
        bytes.push(byte[0]);
        if bytes.len() > INPUT_MAX || byte[0] == b'\r' {
            return Err(Code::InvalidWire);
        }
        if byte[0] == b'\n' {
            return Ok(Some(bytes));
        }
    }
}
fn exact_eof<R: Read>(input: &mut R) -> Result<()> {
    let mut extra = [0];
    if input.read(&mut extra).map_err(|_| Code::InvalidWire)? != 0 {
        return Err(Code::InvalidWire);
    }
    Ok(())
}
fn encoded(value: &Value) -> Result<Vec<u8>> {
    let mut out = serde_json::to_vec(value).map_err(|_| Code::OutputUnknown)?;
    if out.len() + 1 > OUTPUT_MAX {
        return Err(Code::OutputUnknown);
    }
    out.push(b'\n');
    Ok(out)
}
fn reply<W: Write>(
    out: &mut W,
    value: &Value,
    clock: &mut Clock,
    expires: Option<AgentReaderTimestamp>,
) -> Result<()> {
    let bytes = encoded(value)?;
    // No callback/provider remains after this fresh observation and before IO.
    clock.publication(expires)?;
    out.write_all(&bytes)
        .and_then(|_| out.flush())
        .map_err(|_| Code::OutputUnknown)?;
    // Parent rejects nonzero even if a complete frame escaped before failure.
    clock.publication(expires).map_err(|_| Code::OutputUnknown)
}
struct Initialized {
    identity: Arc<DeviceIdentity>,
    metadata: Metadata,
    request_id: String,
}
impl Initialized {
    fn init(bytes: &[u8], clock: &mut Clock) -> Result<(Self, Value)> {
        let request: InitRequest = strict_json(bytes, INPUT_MAX).map_err(|_| Code::InvalidWire)?;
        if request.format != FORMAT || request.kind != "initPublic" {
            return Err(Code::InvalidWire);
        }
        id(&request.request_id)?;
        request.metadata.validate()?;
        clock.observe()?;
        // Only explicit fixture init generates. No production profile/store API.
        let identity = Arc::new(DeviceIdentity::generate().map_err(|_| Code::StateUnavailable)?);
        let device_id =
            AgentReaderDeviceId::from_device_id(identity.device_id()).map_err(crypto)?;
        let key = AgentReaderPublicKey::from_bytes(identity.public_keys().signing);
        key.validate().map_err(crypto)?;
        clock.observe()?;
        let m = &request.metadata;
        let public = json!({"format":"cc-external-reader-public-device/v1","deviceId":device_id,"deviceEpoch":1,"deviceKey":key,
            "nativeProfileId":m.native_profile_id,"nativeHostId":m.native_host_id,"nativeHostRevision":m.native_host_revision,
            "nativeBindingId":m.native_binding_id,"nativeBindingRevision":m.native_binding_revision,"nativeBindingDigest":m.native_binding_digest,
            "nativeSourceId":m.native_source_id,"nativeSourceRevision":m.native_source_revision,"transportPeerHash":m.transport_peer_hash});
        let response = json!({"format":FORMAT,"requestId":request.request_id,"ok":true,"metadata":{"testOnly":true,"nativeApproval":false,"publicDevice":public}});
        Ok((
            Self {
                identity,
                metadata: request.metadata,
                request_id: request.request_id,
            },
            response,
        ))
    }
    fn sign(self, bytes: &[u8], clock: &mut Clock) -> Result<(Value, AgentReaderTimestamp)> {
        let request: SignRequest = strict_json(bytes, INPUT_MAX).map_err(|_| Code::InvalidWire)?;
        if request.format != FORMAT
            || request.kind != "signPacket"
            || request.request_id == self.request_id
            || request.expected.format != "cc-external-reader-expected/v1"
        {
            return Err(Code::InvalidWire);
        }
        id(&request.request_id)?;
        let e = request.expected;
        let actual =
            AgentReaderDeviceId::from_device_id(self.identity.device_id()).map_err(crypto)?;
        if !self.metadata.matches(&e.association_body)
            || e.association_body.device_id != actual
            || e.association_body.device_epoch != 1
            || e.association_body.device_key.as_bytes() != &self.identity.public_keys().signing
            || e.issuer_pin.native_profile_id != self.metadata.native_profile_id
            || e.target.configured_host_id.as_uuid() == self.metadata.native_host_id.as_uuid()
            || matches!(&e.issuer_pin.context.tenant,AgentReaderTenantV1::LocalProfile{id} if id.as_uuid()==self.metadata.native_profile_id.as_uuid())
        {
            return Err(Code::Rejected);
        }
        let packet_bytes = serde_json::to_vec(&request.packet).map_err(|_| Code::InvalidWire)?;
        if packet_bytes.len() > AGENT_READER_FRAME_BYTES {
            return Err(Code::InvalidWire);
        }
        let packet: AgentReaderSigningPacketV1 =
            strict_json(&packet_bytes, AGENT_READER_FRAME_BYTES).map_err(crypto)?;
        let expires = [
            e.issuer_pin.expires_at,
            e.association_body.expires_at,
            packet.grant.body.expires_at,
            packet.freshness.body.expires_at,
            packet.challenge.expires_at,
        ]
        .into_iter()
        .min()
        .ok_or(Code::Rejected)?;
        let inputs = AgentReaderValidatedInputsV1::from_packet(
            &packet_bytes,
            AgentReaderExpectedV1 {
                issuer_pin: e.issuer_pin,
                current_head: e.current_head,
                association_body: e.association_body,
                target: e.target,
            },
            clock.timestamp()?,
        )
        .map_err(crypto)?;
        clock.publication(Some(expires))?;
        let proof = self
            .identity
            .sign_agent_reader_proof_v1(inputs, clock.timestamp()?)
            .map_err(crypto)?;
        // Public verification is not another signature or a replacement nonce.
        proof
            .verify(AgentReaderPublicKey::from_bytes(
                self.identity.public_keys().signing,
            ))
            .map_err(|_| Code::OutputUnknown)?;
        clock
            .publication(Some(expires))
            .map_err(|_| Code::OutputUnknown)?;
        Ok((
            json!({"format":FORMAT,"requestId":request.request_id,"ok":true,"metadata":{"testOnly":true,"nativeApproval":false,"proof":proof}}),
            expires,
        ))
    }
}
fn deny<W: Write>(out: &mut W, request: Option<String>, code: Code, clock: &mut Clock) -> i32 {
    let value = json!({"format":FORMAT,"requestId":request,"ok":false,"code":format!("{code:?}")});
    if reply(out, &value, clock, None).is_err() {
        4
    } else {
        2
    }
}
fn serve<R: Read, W: Write>(input: &mut R, out: &mut W, clock: &mut Clock) -> i32 {
    let first = match frame(input) {
        Ok(Some(v)) => v,
        _ => return deny(out, None, Code::InvalidWire, clock),
    };
    let init_id = request_id(&first);
    let (initialized, public) = match Initialized::init(&first, clock) {
        Ok(v) => v,
        Err(code) => return deny(out, init_id, code, clock),
    };
    // First reply is emitted without waiting for channel EOF.
    if reply(out, &public, clock, None).is_err() {
        return 4;
    }
    let second = match frame(input) {
        Ok(Some(v)) => v,
        Ok(None) => return 0,
        Err(code) => return deny(out, None, code, clock),
    };
    // Terminal before second-frame validation/signing. A third frame/trailing
    // byte is rejected BEFORE any proof signature; missing EOF waits only until
    // the fixed process-entry watchdog. No recovered/replacement identity.
    if let Err(code) = exact_eof(input) {
        return deny(out, None, code, clock);
    }
    let sign_id = request_id(&second);
    match initialized.sign(&second, clock) {
        Ok((proof, expires)) => {
            if reply(out, &proof, clock, Some(expires)).is_ok() {
                0
            } else {
                4
            }
        }
        Err(code) => deny(out, sign_id, code, clock),
    }
}
fn main() {
    let start = Instant::now();
    let mut clock = Clock::new(start);
    let deadline = clock.deadline;
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--test-only")) || args.next().is_some()
    {
        std::process::exit(2);
    }
    if std::thread::Builder::new()
        .name("reader-interop-fixture-deadline".into())
        .spawn(move || {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            std::process::exit(4);
        })
        .is_err()
    {
        std::process::exit(4);
    }
    let mut input = BufReader::with_capacity(4096, std::io::stdin().lock());
    let mut output = std::io::stdout().lock();
    std::process::exit(serve(&mut input, &mut output, &mut clock));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn vectors() -> Value {
        serde_json::from_str(include_str!(
            "../../crypto-core/src/agent_reader/vectors.json"
        ))
        .unwrap()
    }
    fn decode<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).unwrap()
    }
    fn init() -> Vec<u8> {
        let a: AgentReaderAssociationV1 = decode(vectors()["statements"][0]["body"].clone());
        bytes(
            &json!({"format":FORMAT,"requestId":"11111111-1111-4111-8111-111111111111","kind":"initPublic","metadata":{
            "nativeProfileId":a.native_profile_id,"nativeHostId":a.native_host_id,"nativeHostRevision":a.native_host_revision,
            "nativeBindingId":a.native_binding_id,"nativeBindingRevision":a.native_binding_revision,"nativeBindingDigest":a.native_binding_digest,
            "nativeSourceId":a.native_source_id,"nativeSourceRevision":a.native_source_revision,"transportPeerHash":a.transport_peer_hash}}),
        )
    }
    fn bytes(v: &Value) -> Vec<u8> {
        let mut b = serde_json::to_vec(v).unwrap();
        b.push(b'\n');
        b
    }
    fn sign_request(state: &Initialized, clock: &mut Clock) -> Vec<u8> {
        sign_request_age(state, clock, 0)
    }
    fn sign_request_age(state: &Initialized, clock: &mut Clock, age: u64) -> Vec<u8> {
        let v = vectors();
        // Dedicated PUBLIC RFC8032 issuer test seed. Device private keys remain
        // solely inside actual DeviceIdentity; no DeviceSecretKeys API is used.
        let mut seed = zeroize::Zeroizing::new(b"CCRI\x01".to_vec());
        seed.extend(hex_decode_test_issuer());
        let issuer = ReaderIssuerSecretV1::from_secure_store_bytes(&seed).unwrap();
        let observed = AgentReaderTimestamp::from_native(clock.timestamp().unwrap()).unwrap();
        let now = AgentReaderTimestamp::new(observed.seconds().saturating_sub(age)).unwrap();
        let mut pin: AgentReaderIssuerPinV1 = decode(v["pin"]["body"].clone());
        pin.expires_at = now.checked_add(3600).unwrap();
        let mut a: AgentReaderAssociationV1 = decode(v["statements"][0]["body"].clone());
        a.device_id = AgentReaderDeviceId::from_device_id(state.identity.device_id()).unwrap();
        a.device_key = AgentReaderPublicKey::from_bytes(state.identity.public_keys().signing);
        a.previous_authority_hash = pin.genesis_hash().unwrap();
        a.issued_at = now;
        a.not_before = now;
        a.expires_at = now.checked_add(600).unwrap();
        let genesis = AgentReaderHeadV1 {
            context: pin.context.clone(),
            issuer_id: pin.issuer_id,
            issuer_epoch: 1,
            authority_revision: 0,
            authority_hash: pin.genesis_hash().unwrap(),
        };
        let head = advance_reader_head_v1(&genesis, a.body_digest().unwrap()).unwrap();
        let mut c: AgentReaderChallengeV1 = decode(v["challenge"]["body"].clone());
        c.head = head.clone();
        c.device_id = a.device_id;
        c.device_key_fingerprint = agent_reader_key_fingerprint(a.device_key);
        c.association_hash = a.body_digest().unwrap();
        c.issued_at = now;
        c.expires_at = now.checked_add(20).unwrap();
        let mut g: AgentReaderGrantV1 = decode(v["statements"][1]["body"].clone());
        g.head = head.clone();
        g.device_id = a.device_id;
        g.association_hash = c.association_hash;
        g.challenge_hash = c.challenge_hash().unwrap();
        g.issued_at = now;
        g.expires_at = c.expires_at;
        let mut f: AgentReaderFreshnessV1 = decode(v["statements"][4]["body"].clone());
        f.head = head.clone();
        f.issued_at = now;
        f.expires_at = now.checked_add(30).unwrap();
        bytes(
            &json!({"format":FORMAT,"requestId":"22222222-2222-4222-8222-222222222222","kind":"signPacket",
            "expected":{"format":"cc-external-reader-expected/v1","issuerPin":pin,"currentHead":head,"associationBody":a,"target":v["target"]["body"]},
            "packet":{"association":issuer.sign_association(a.clone(),&pin).unwrap(),"grant":issuer.sign_grant(g,&pin).unwrap(),"freshness":issuer.sign_freshness(f,&pin).unwrap(),"challenge":c}}),
        )
    }
    fn hex_decode_test_issuer() -> [u8; 32] {
        [
            0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda, 0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11,
            0x4e, 0x0f, 0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24, 0xda, 0x8c, 0xf6, 0xed,
            0x4f, 0xb8, 0xa6, 0xfb,
        ]
    }
    #[test]
    fn actual_native_fixture_identity_signs_external_packet_and_public_id_key_match() {
        let mut clock = Clock::new(Instant::now());
        let (state, public) = Initialized::init(&init(), &mut clock).unwrap();
        let key = AgentReaderPublicKey::from_bytes(state.identity.public_keys().signing);
        let id = AgentReaderDeviceId::from_device_id(state.identity.device_id()).unwrap();
        assert_eq!(
            public["metadata"]["publicDevice"]["deviceKey"],
            serde_json::to_value(key).unwrap()
        );
        let packet = sign_request(&state, &mut clock);
        let (reply, expiry) = state.sign(&packet, &mut clock).unwrap();
        let proof: AgentReaderSigned<AgentReaderProofV1> =
            decode(reply["metadata"]["proof"].clone());
        proof.verify(key).unwrap();
        assert_eq!(proof.body.device_id, id);
        assert_eq!(reply["metadata"]["nativeApproval"], false);
        clock.publication(Some(expiry)).unwrap();
    }
    #[test]
    fn strict_frames_bounds_duplicates_and_exact_final_eof_reject_third_bytes() {
        assert!(frame(&mut Cursor::new(vec![b'x'; INPUT_MAX + 1])).is_err());
        assert!(frame(&mut Cursor::new(b"{}\r\n")).is_err());
        assert!(frame(&mut Cursor::new(b"{}")).is_err());
        assert!(exact_eof(&mut Cursor::new(b"{}\n")).is_err());
        assert!(exact_eof(&mut Cursor::new(b" ")).is_err());
        assert!(exact_eof(&mut Cursor::new(b"")).is_ok());
        for raw in [
            b"{\"x\":1,\"x\":2}".as_slice(),
            b"{\"x\":1.0}",
            b"{\"x\":1e309}",
        ] {
            assert!(strict_json::<Value>(raw, INPUT_MAX).is_err());
        }
    }
    #[test]
    fn first_reply_happens_before_eof_and_second_init_is_terminal_denial() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Input {
            first: Cursor<Vec<u8>>,
            wrote: Arc<AtomicBool>,
        }
        impl Read for Input {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                let n = self.first.read(b)?;
                if n == 0 {
                    assert!(self.wrote.load(Ordering::SeqCst));
                }
                Ok(n)
            }
        }
        struct Output {
            data: Vec<u8>,
            wrote: Arc<AtomicBool>,
        }
        impl Write for Output {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.data.extend_from_slice(b);
                self.wrote.store(true, Ordering::SeqCst);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let wrote = Arc::new(AtomicBool::new(false));
        let mut input = Input {
            first: Cursor::new(init()),
            wrote: wrote.clone(),
        };
        let mut out = Output {
            data: vec![],
            wrote,
        };
        assert_eq!(
            serve(&mut input, &mut out, &mut Clock::new(Instant::now())),
            0
        );
        assert_eq!(out.data.iter().filter(|b| **b == b'\n').count(), 1);
        let mut all = init();
        all.extend(init());
        let mut out = Vec::new();
        assert_eq!(
            serve(
                &mut Cursor::new(all),
                &mut out,
                &mut Clock::new(Instant::now())
            ),
            2
        );
        assert_eq!(out.iter().filter(|b| **b == b'\n').count(), 2);
        assert!(!String::from_utf8(out).unwrap().contains("\"proof\""));
    }
    #[test]
    fn saved_metadata_wrong_key_links_and_expiry_fail_without_proof_output() {
        for field in ["nativeBindingRevision", "deviceKey", "nativeProfileId"] {
            let mut clock = Clock::new(Instant::now());
            let (state, _) = Initialized::init(&init(), &mut clock).unwrap();
            let mut request: Value = decode_bytes(&sign_request(&state, &mut clock));
            request["expected"]["associationBody"][field] = match field {
                "nativeBindingRevision" => json!(999),
                "deviceKey" => json!("00".repeat(32)),
                _ => json!("88888888-8888-4888-8888-888888888888"),
            };
            assert!(matches!(
                state.sign(&bytes(&request), &mut clock),
                Err(Code::Rejected)
            ));
        }
        let mut clock = Clock::new(Instant::now());
        let (state, _) = Initialized::init(&init(), &mut clock).unwrap();
        let mut request: Value = decode_bytes(&sign_request(&state, &mut clock));
        request["packet"]["grant"]["body"]["operationId"] =
            json!("88888888-8888-4888-8888-888888888888");
        assert!(matches!(
            state.sign(&bytes(&request), &mut clock),
            Err(Code::Rejected)
        ));
    }
    #[test]
    fn correctly_issuer_signed_but_expired_packet_is_terminal_without_proof() {
        let mut clock = Clock::new(Instant::now());
        let (state, _) = Initialized::init(&init(), &mut clock).unwrap();
        let expired = sign_request_age(&state, &mut clock, 40);
        assert!(matches!(
            state.sign(&expired, &mut clock),
            Err(Code::Expired)
        ));
    }
    fn decode_bytes(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap()
    }
    #[test]
    fn output_failure_never_emits_second_frame_and_clock_budget_starts_at_entry() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
        }
        assert_eq!(
            serve(
                &mut Cursor::new(init()),
                &mut Broken,
                &mut Clock::new(Instant::now())
            ),
            4
        );
        let mut clock = Clock::new(Instant::now() - Duration::from_secs(121));
        assert!(matches!(clock.observe(), Err(Code::ClockUnavailable)));
        let mut clock = Clock::new(Instant::now());
        clock.floor = Duration::from_secs(AGENT_READER_MAX_TIMESTAMP);
        assert!(matches!(clock.observe(), Err(Code::ClockUnavailable)));
    }
}
