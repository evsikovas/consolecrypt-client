// DRAFT/uncompiled: real Actor/custody seams, no SSH transport or install proof.
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::sync::{Arc,Mutex};

fn id() -> AgentId { AgentId::new("44444444-4444-4444-8444-444444444444").unwrap() }
fn stamp(n:u64)->Timestamp { Timestamp::new(n).unwrap() }
fn context()->Context { Context { tenant:Tenant::LocalProfile {
    id:LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap() },
    deployment_id:DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap() } }
fn signing()->SigningKey { SigningKey::from_bytes(&[11;32]) }
fn public(key:&SigningKey)->PublicKey { PublicKey::from_bytes(key.verifying_key().to_bytes()) }
fn signed<T:Statement>(body:T,key:&SigningKey)->Signed<T> {
    let signature=SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed {body,signature}
}
fn header(cp:u64)->Header {Header {version:1,context:context(),owner_epoch:1,checkpoint:cp}}
fn frame(n:u8,kind:&str,payload:Value)->Envelope {
    parse(&canonical(&json!({"format":FORMAT,"requestId":format!("55555555-5555-4555-8555-{n:012x}"),
        "kind":kind,"payload":payload})).unwrap()).unwrap()
}
struct Directory(PathBuf);
impl Directory {
    fn new()->Self {
        use std::os::unix::fs::DirBuilderExt;
        let mut bytes=[0u8;16];getrandom::fill(&mut bytes).unwrap();
        let path=std::env::temp_dir().join(format!("cc-enroll-{}",hex::encode(bytes)));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if self.0.parent()==Some(std::env::temp_dir().as_path())
            && self.0.file_name().unwrap().to_string_lossy().starts_with("cc-enroll-")
            && !fs::symlink_metadata(&self.0).unwrap().file_type().is_symlink() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
#[derive(Clone)]
struct Memory(Arc<Mutex<State>>);
impl CompareAndSwap for Memory {
    fn load(&self)->Result<State> {Ok(self.0.lock().unwrap().clone())}
    fn compare_and_swap(&mut self,expected:u64,next:State)->Result<()> {
        let mut state=self.0.lock().unwrap();
        if state.revision()!=expected {return Err(Error::Conflict)}
        *state=next;Ok(())
    }
}
struct Fixture { directory:Directory, session:Session<Memory>, clock:Clock, owner:SigningKey, pin:OwnerPin, now:u64 }
impl Fixture {
    fn new()->Self {
        let directory=Directory::new();let node_dir=directory.0.join("node");
        let node=provision_node(&node_dir,id()).unwrap();
        let clock=Clock::start().unwrap();let now=clock.last.seconds();let owner=signing();
        let pin=OwnerPin {context:context(),key:public(&owner),epoch:1,checkpoint:0,expires_at:stamp(now+300)};
        let store=Memory(Arc::new(Mutex::new(State::bootstrap(pin.clone(),Now(stamp(now))).unwrap())));
        let session=Session {store:Some(store),node_dir,agent:id(),node,phase:Phase::New,seen:BTreeSet::new(),
            expected_pin:pin.clone(),expected_rev:0,claim:None,certificate:None,offer_expires:None,effects:false};
        Self {directory,session,clock,owner,pin,now}
    }
    fn offer(&self,n:u8)->Signed<Offer> {
        let token=InvitationToken::from_bytes([n;32]).unwrap();
        signed(Offer {header:header(1),invitation_id:InvitationId::new(&format!("33333333-3333-4333-8333-{n:012x}")).unwrap(),
            agent_id:id(),agent_key:self.session.node.public_key,token_hash:token.digest(),
            scopes:Scopes(vec![Scope::TunnelOpen]),issued_at:stamp(self.now),expires_at:stamp(self.now+120)},&self.owner)
    }
    fn probe(&mut self) {self.session.request(frame(1,"probe",json!({})),&mut self.clock).unwrap();}
    fn stage(&mut self)->Value {
        let offer=self.offer(33);let pin=self.pin.clone();
        self.session.request(frame(2,"stageOffer",json!({"expectedActorRevision":0,
            "expectedOwnerPin":pin,"offer":offer,"token":hex::encode([33;32])})),&mut self.clock).unwrap()
    }
}

#[test]
fn enroll_schema_exact_canonical_uuid_no_float_or_duplicate() {
    let good=canonical(&json!({"format":FORMAT,"requestId":"55555555-5555-4555-8555-555555555555",
        "kind":"probe","payload":{}})).unwrap();
    assert!(parse(&good).is_ok());
    for raw in [b"{}\n".as_slice(),b"{}".as_slice(),b"{}\r\n".as_slice(),
        b"{\"a\":1,\"a\":1}\n".as_slice()] {assert!(parse(raw).is_err());}
    for patch in [json!({"kind":"probe","format":FORMAT,"requestId":"00000000-0000-0000-0000-000000000000","payload":{}}),
        json!({"kind":"probe","format":FORMAT,"requestId":"55555555-5555-4555-8555-555555555555","payload":{},"joined":true})] {
        assert!(parse(&canonical(&patch).unwrap()).is_err());
    }
    assert!(canonical(&json!({"payload":{"now":1.0}})).is_err());
    assert!(parse(&vec![b'a';MAX_WIRE_BYTES+1]).is_err());
}
#[test]
fn enroll_payload_cannot_supply_claim_paths_clock_or_pin_replacement() {
    let mut f=Fixture::new();
    assert!(f.session.request(frame(1,"probe",json!({"path":"/tmp/other"})),&mut f.clock).is_err());
    let input=json!({"expectedActorRevision":0,"expectedOwnerPin":f.pin,"offer":f.offer(33),
        "token":hex::encode([33;32]),"claim":{"signature":"forged"}});
    assert!(decode::<StageOffer>(input).is_err());
    assert!(decode::<Empty>(json!({"now":1})).is_err());
}
#[test]
fn enroll_token_is_checked_before_any_offer_mutation() {
    let mut f=Fixture::new();f.probe();let offer=f.offer(33);
    assert!(f.session.request(frame(2,"stageOffer",json!({"expectedActorRevision":0,
        "expectedOwnerPin":f.pin,"offer":offer,"token":hex::encode([34;32])})),&mut f.clock).is_err());
    assert_eq!(f.session.snapshot().unwrap().revision(),0);assert!(!f.session.effects);
}
#[test]
fn enroll_private_revision_fence_blocks_actor_reload_of_interleaved_revision() {
    let f=Fixture::new();let shared=f.session.store.as_ref().unwrap().clone();
    let mut concurrent=Actor::new(shared.clone()).unwrap();concurrent.offer(f.offer(33),Now(stamp(f.now))).unwrap();
    let mut fenced=Actor::new(RevisionFence {inner:shared.clone(),expected:0}).unwrap();
    // A valid non-owner-head change would otherwise commit from reloaded revision1.
    assert_eq!(fenced.expire(InvitationId::new("33333333-3333-4333-8333-000000000021").unwrap(),
        Now(stamp(f.now+121))),Err(Error::Conflict));
    assert_eq!(shared.load().unwrap().revision(),1);
}
#[test]
fn enroll_inner_cas_still_rejects_race_after_load() {
    let f=Fixture::new();let mut store=f.session.store.as_ref().unwrap().clone();
    let stale=store.load().unwrap();let mut concurrent=Actor::new(store.clone()).unwrap();
    concurrent.offer(f.offer(33),Now(stamp(f.now))).unwrap();
    let mut fence=RevisionFence {inner:store.clone(),expected:0};
    assert_eq!(fence.compare_and_swap(0,stale),Err(Error::Conflict));
    assert_eq!(store.load().unwrap().revision(),1);
}
#[test]
fn enroll_real_custody_claim_two_commits_then_certificate_readback() {
    let mut f=Fixture::new();f.probe();let staged=f.stage();
    assert_eq!(staged["payload"]["actorRevision"],2);assert_eq!(staged["payload"]["ownerPin"]["checkpoint"],1);
    let claim:Signed<Claim>=decode(staged["payload"]["claim"].clone()).unwrap();
    claim.verify(f.session.node.public_key).unwrap();
    let hash=claim.body.digest().unwrap();assert_eq!(Some(hash),f.session.claim);
    let cert=signed(Certificate {header:header(2),agent_id:id(),key_epoch:1,agent_key:f.session.node.public_key,
        scopes:Scopes(vec![Scope::TunnelOpen]),capabilities:claim.body.capabilities.clone(),
        origin:CertificateOrigin::Enrollment {invitation_id:claim.body.invitation_id,claim_hash:hash,
            key_fingerprint:f.session.node.key_fingerprint},not_before:stamp(f.now),expires_at:stamp(f.now+100)},&f.owner);
    let expected=f.session.expected_pin.clone();let digest=cert.body.digest().unwrap();
    let applied=f.session.request(frame(3,"applyCertificate",json!({"expectedRev":2,"expectedPin":expected,
        "claimDigest":hash,"certificate":cert})),&mut f.clock).unwrap();
    assert_eq!(applied["payload"]["actorRevision"],3);assert_eq!(applied["payload"]["ownerPin"]["checkpoint"],2);
    assert_eq!(applied["payload"]["certificateDigest"],json!(digest));
    let expected=f.session.expected_pin.clone();
    let observed=f.session.request(frame(4,"observeCertificate",json!({"expectedRev":3,"expectedPin":expected,
        "agentId":id(),"certificateDigest":digest})),&mut f.clock).unwrap();
    assert_eq!(applied["payload"],observed["payload"]);assert!(f.session.phase==Phase::Closed);
}
#[test]
fn enroll_expected_pin_and_request_replay_never_adopt_trust() {
    let mut f=Fixture::new();f.probe();let original=f.session.expected_pin.clone();
    let mut foreign=original.clone();foreign.checkpoint=9;
    assert!(f.session.request(frame(2,"stageOffer",json!({"expectedActorRevision":0,
        "expectedOwnerPin":foreign,"offer":f.offer(33),"token":hex::encode([33;32])})),&mut f.clock).is_err());
    assert_eq!(f.session.expected_pin,original);assert_eq!(f.session.snapshot().unwrap().revision(),0);
    assert!(f.session.request(frame(1,"probe",json!({})),&mut f.clock).is_err());
}
#[test]
fn enroll_probe_never_creates_missing_node_or_guard() {
    let root=Directory::new();let missing=root.0.join("node");
    assert!(inspect_existing_node_identity(&missing,id()).is_err());assert!(!missing.exists());
    fs::create_dir(&missing).unwrap();
    assert!(inspect_existing_node_identity(&missing,id()).is_err());
    assert_eq!(fs::read_dir(&missing).unwrap().count(),0);
}
#[test]
fn enroll_probe_refuses_unknown_rotation_state_without_repair() {
    let f=Fixture::new();let path=f.session.node_dir.join("node-activation.intent.v3");
    fs::write(&path,b"partial").unwrap();
    let before=fs::read(&path).unwrap();
    assert!(inspect_existing_node_identity(&f.session.node_dir,id()).is_err());
    assert_eq!(fs::read(&path).unwrap(),before);
}
#[test]
fn enroll_deadline_or_terminal_phase_cannot_restart_or_replay() {
    let mut f=Fixture::new();f.clock.end=Instant::now();
    assert!(f.session.request(frame(1,"probe",json!({})),&mut f.clock).is_err());
    let mut f=Fixture::new();f.session.phase=Phase::Closed;
    assert!(f.session.request(frame(1,"probe",json!({})),&mut f.clock).is_err());
}

#[test]
fn enroll_custody_uses_nonzero_preoffer_current_pin_not_genesis_or_postoffer() {
    let f=Fixture::new();let mut pre=f.pin.clone();pre.checkpoint=7;
    let mut offer=f.offer(33);offer.body.header.checkpoint=8;offer=signed(offer.body,&f.owner);
    let claim=create_tunnel_node_claim(&f.session.node_dir,id(),&pre,offer.clone(),Now(stamp(f.now))).unwrap();
    claim.verify(f.session.node.public_key).unwrap();assert_eq!(claim.body.header.checkpoint,8);
    assert!(create_tunnel_node_claim(&f.session.node_dir,id(),&f.pin,offer.clone(),Now(stamp(f.now))).is_err());
    let mut post=pre;post.checkpoint=8;
    assert!(create_tunnel_node_claim(&f.session.node_dir,id(),&post,offer,Now(stamp(f.now))).is_err());
}

#[test]
fn enroll_sqlite_open_retains_genesis_after_owner_checkpoint_advances() {
    let f=Fixture::new();let db=f.directory.0.join("db");let anchor=f.directory.0.join("anchor");
    let store=SqliteJournalStore::create(&db,&anchor,f.pin.clone(),Now(stamp(f.now)),
        SqliteStorageBudget::default()).unwrap();
    let mut actor=Actor::new(store).unwrap();actor.offer(f.offer(33),Now(stamp(f.now))).unwrap();
    let current=actor.snapshot().unwrap().owner().clone();assert_eq!(current.checkpoint,1);drop(actor);
    assert!(SqliteJournalStore::open(&db,&anchor,current,SqliteStorageBudget::default()).is_err());
    let reopened=SqliteJournalStore::open(&db,&anchor,f.pin.clone(),SqliteStorageBudget::default()).unwrap();
    assert_eq!(reopened.load().unwrap().owner().checkpoint,1);
}

#[test]
fn enroll_wrong_certificate_claim_is_denied_without_third_commit() {
    let mut f=Fixture::new();f.probe();let staged=f.stage();
    let claim:Signed<Claim>=decode(staged["payload"]["claim"].clone()).unwrap();
    let wrong=Hash::from_bytes([9;32]);
    let cert=signed(Certificate {header:header(2),agent_id:id(),key_epoch:1,agent_key:f.session.node.public_key,
        scopes:Scopes(vec![Scope::TunnelOpen]),capabilities:claim.body.capabilities,
        origin:CertificateOrigin::Enrollment {invitation_id:claim.body.invitation_id,claim_hash:wrong,
            key_fingerprint:f.session.node.key_fingerprint},not_before:stamp(f.now),expires_at:stamp(f.now+100)},&f.owner);
    let expected=f.session.expected_pin.clone();let actual=f.session.claim.unwrap();
    assert!(f.session.request(frame(3,"applyCertificate",json!({"expectedRev":2,"expectedPin":expected,
        "claimDigest":actual,"certificate":cert})),&mut f.clock).is_err());
    assert_eq!(f.session.snapshot().unwrap().revision(),2);
    assert!(f.session.snapshot().unwrap().certificate(id()).is_none());
}

// Only the current owned test ELF is spawned; this is not the installed SSH
// role, a generic executor or an authenticated enrollment ACK fixture.
fn actual_fifo_child(mode:&str,close:bool)->String {
    use std::io::{BufRead,BufReader};
    use std::process::{Command,Stdio};
    use std::sync::mpsc;
    let mut child=Command::new(std::env::current_exe().unwrap())
        .args(["--exact","enrollment_control::tests::owned_enrollment_fifo_child","--ignored","--nocapture"])
        .env_clear().env("CC_ENROLL_FIFO_MODE",mode).env("TMPDIR",std::env::temp_dir())
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let stdout=child.stdout.take().unwrap();let (ready_tx,ready_rx)=mpsc::sync_channel(1);
    let reader=std::thread::spawn(move || {
        let mut input=BufReader::new(stdout);let mut output=String::new();
        loop {
            let mut line=String::new();let n=Read::by_ref(&mut input).take(16385).read_line(&mut line).unwrap();
            if n==0 {break}
            assert!(n<=16384 && output.len()+n<=32768);
            if line.trim_end()=="CC_ENROLL_FIFO_READY" {ready_tx.try_send(()).ok();}
            output.push_str(&line);
        }
        output
    });
    let ready=ready_rx.recv_timeout(Duration::from_secs(3)).is_ok();
    let start=Instant::now();
    let sent=if ready {
        let input=child.stdin.as_mut().unwrap();
        match mode {
            "complete"=>input.write_all(&canonical(&json!({"format":FORMAT,
                "requestId":"55555555-5555-4555-8555-555555555555","kind":"probe","payload":{}})).unwrap()),
            "partial"=>input.write_all(b"{"),
            "partial-eof"=>input.write_all(b"{\"format\":"),
            _=>Ok(()),
        }.is_ok()
    }else{false};
    // Additional bytes arrive after the first-byte deadline is already armed.
    // They must NOT renew its five seconds.
    if ready && sent && mode=="partial" {
        std::thread::sleep(Duration::from_secs(2));
        let _=child.stdin.as_mut().unwrap().write_all(b"\"format\":");
    }
    if close {drop(child.stdin.take());}
    let end=start+Duration::from_secs(8);
    let status=loop {
        if let Some(status)=child.try_wait().unwrap(){break Some(status)}
        if Instant::now()>=end {child.kill().ok();child.wait().unwrap();break None}
        std::thread::sleep(Duration::from_millis(5));
    };
    drop(child.stdin.take());
    let end=Instant::now()+Duration::from_secs(1);
    while !reader.is_finished() && Instant::now()<end {std::thread::sleep(Duration::from_millis(2));}
    assert!(reader.is_finished(),"owned reader must see EOF after exact child wait");
    let output=reader.join().unwrap();
    assert!(ready && sent && status.is_some_and(|s|s.success()),"owned child did not complete its assertions");
    output
}

#[test]
fn enroll_actual_fifo_complete_frame_is_bounded_component_echo_only() {
    let output=actual_fifo_child("complete",false);
    assert!(output.contains("\"kind\":\"componentObserved\""));
    assert!(!output.contains("certificateCommitted"));
}
#[test]
fn enroll_actual_partial_fifo_uses_original_five_second_deadline() {
    let output=actual_fifo_child("partial",false);
    assert!(output.contains("CC_ENROLL_PARTIAL_EXPIRED"));
}
#[test]
fn enroll_actual_empty_and_partial_fifo_eof_never_become_valid_frame() {
    for mode in ["eof","partial-eof"] {
        assert!(actual_fifo_child(mode,true).contains("CC_ENROLL_EOF_UNKNOWN"));
    }
}
#[test]
fn enroll_actual_fifo_wait_is_clamped_by_original_session_deadline() {
    assert!(actual_fifo_child("session",false).contains("CC_ENROLL_SESSION_EXPIRED"));
}
#[test]
fn enroll_actual_handler_two_cas_then_pipe_eof_has_no_certificate_ack() {
    let output=actual_fifo_child("post-commit-eof",true);
    assert!(output.contains("CC_ENROLL_COMMIT_RETAINED_REPLY_UNKNOWN"));
    assert!(!output.contains("certificateCommitted"));
}

#[test]
#[ignore="private owned current-test-ELF component, not installed SSH authentication"]
fn owned_enrollment_fifo_child() {
    let mode=std::env::var("CC_ENROLL_FIFO_MODE").expect("explicit closed component mode");
    assert!(matches!(mode.as_str(),"complete"|"partial"|"eof"|"partial-eof"|"session"|"post-commit-eof"));
    let mut pipe=Pipe::open().unwrap();let mut clock=Clock::start().unwrap();
    let mut retained=None;
    let mut owner=None;
    if mode=="post-commit-eof" {
        let f=Fixture::new();
        let store=SqliteJournalStore::create(f.directory.0.join("fifo-db"),f.directory.0.join("fifo-anchor"),
            f.pin.clone(),Now(stamp(f.now)),SqliteStorageBudget::default()).unwrap();
        let mut session=Session {store:Some(store),node_dir:f.session.node_dir.clone(),agent:id(),node:f.session.node.clone(),
            phase:Phase::New,seen:BTreeSet::new(),expected_pin:f.pin.clone(),expected_rev:0,
            claim:None,certificate:None,offer_expires:None,effects:false};
        session.request(frame(1,"probe",json!({})),&mut clock).unwrap();
        let offer=f.offer(33);
        session.request(frame(2,"stageOffer",json!({"expectedActorRevision":0,"expectedOwnerPin":f.pin,
            "offer":offer,"token":hex::encode([33;32])})),&mut clock).unwrap();
        assert_eq!(session.snapshot().unwrap().revision(),2);
        retained=Some(session);owner=Some(f); // Keep actual private root alive until readback.
    }
    if mode=="session" {clock.end=Instant::now()+Duration::from_millis(200);}
    println!("\nCC_ENROLL_FIFO_READY");std::io::stdout().flush().unwrap();
    let started=Instant::now();let result=pipe.read(&mut clock);
    match mode.as_str() {
        "complete"=>{
            let envelope=parse(&result.unwrap()).unwrap();assert_eq!(envelope.kind,"probe");
            let _:Empty=decode(envelope.payload).unwrap();
            pipe.write(&json!({"format":FORMAT,"requestId":envelope.request_id,
                "kind":"componentObserved","payload":{}}),&mut clock).unwrap();
        }
        "partial"=>{
            assert!(matches!(result,Err(Error::Expired)));
            assert!(started.elapsed()>=Duration::from_millis(4800) && started.elapsed()<Duration::from_millis(6500));
            println!("\nCC_ENROLL_PARTIAL_EXPIRED");
        }
        "session"=>{
            assert!(matches!(result,Err(Error::Expired)));assert!(started.elapsed()<Duration::from_secs(1));
            println!("\nCC_ENROLL_SESSION_EXPIRED");
        }
        "post-commit-eof"=>{
            assert!(matches!(result,Err(Error::InvalidWire)));
            let session=retained.take().unwrap();let state=session.snapshot().unwrap();
            assert_eq!(state.revision(),2);assert_eq!(state.owner().checkpoint,1);
            assert!(state.certificate(id()).is_none());assert!(session.phase==Phase::Claimed);
            drop(session);drop(owner.take()); // Exact owned private root cleanup before completion.
            println!("\nCC_ENROLL_COMMIT_RETAINED_REPLY_UNKNOWN");
        }
        _=>{
            assert!(matches!(result,Err(Error::InvalidWire)));
            println!("\nCC_ENROLL_EOF_UNKNOWN");
        }
    }
}

#[cfg(test)]
mod golden {
// SOURCE-ONLY ignored fixture. Emits actual handler/CAS bytes, never SSH proof.
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::os::unix::fs::DirBuilderExt;

const AGENT: &str = "44444444-4444-4444-8444-444444444444";
fn fixture_id()->AgentId { AgentId::new(AGENT).unwrap() }
fn sign<T: Statement>(body:T,key:&SigningKey)->Signed<T> {
    body.validate().unwrap();
    let signature=SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed {body,signature}
}
fn digest(bytes:&[u8])->String {hex::encode(Sha256::digest(bytes))}
fn owned_output()->PathBuf {
    let p=PathBuf::from(std::env::var_os("CC_ENROLL_GOLDEN_ROOT").expect("explicit owned output required"));
    assert!(p.is_absolute() && p.file_name().unwrap()=="cc-enrollment-golden");
    for part in p.ancestors() {assert!(!fs::symlink_metadata(part).unwrap().file_type().is_symlink());}
    let m=fs::metadata(&p).unwrap();
    let uid=fs::metadata("/proc/self").unwrap().uid();
    assert!(m.is_dir() && m.uid()==uid && m.mode()&0o777==0o700);
    assert_eq!(fs::read_dir(&p).unwrap().count(),0);
    p
}
struct Private(PathBuf);
impl Private {
    fn create()->Self {
        let mut entropy=[0u8;16];getrandom::fill(&mut entropy).unwrap();
        let p=std::env::temp_dir().join(format!("cc-enroll-golden-private-{}",hex::encode(entropy)));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();Self(p)
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(),Some(std::env::temp_dir().as_path()));
        assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("cc-enroll-golden-private-"));
        assert!(!fs::symlink_metadata(&self.0).unwrap().file_type().is_symlink());
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn save(root:&Path,name:&str,raw:&[u8])->Value {
    assert!(raw.len()<=MAX_WIRE_BYTES && raw.last()==Some(&b'\n'));
    let path=root.join(name);
    let mut f=OpenOptions::new().write(true).create_new(true).mode(0o600)
        .custom_flags(NOFOLLOW|CLOEXEC).open(&path).unwrap();
    f.write_all(raw).unwrap();f.sync_all().unwrap();
    json!({"file":name,"bytes":raw.len(),"sha256":digest(raw)})
}
fn exchange(root:&Path,n:u8,kind:&str,payload:Value,session:&mut Session<SqliteJournalStore>,clock:&mut Clock)->(Value,Value) {
    let request=json!({"format":FORMAT,"requestId":format!("55555555-5555-4555-8555-{n:012x}"),
        "kind":kind,"payload":payload});
    let raw=canonical(&request).unwrap();
    let reply=session.request(parse(&raw).unwrap(),clock).unwrap();
    let response=canonical(&reply).unwrap();
    let req=save(root,&format!("{n:02}-{kind}.request.jsonl"),&raw);
    let res=save(root,&format!("{n:02}-{kind}.reply.jsonl"),&response);
    (reply,json!({"kind":kind,"request":req,"reply":res}))
}

#[test]
#[ignore="owned explicit golden generation only; missing env is failure, not skip"]
fn owned_actual_enrollment_golden() {
    let output=owned_output();let private=Private::create();let private_path=private.0.clone();
    let node_dir=private.0.join("node");let db=private.0.join("db");let anchor=private.0.join("anchor");
    // Existing public fixture owner byte11 DECIMAL; invitation byte0x21 (=33).
    // No runtime override/seed of the actual Node key or custody nonce exists.
    let owner=SigningKey::from_bytes(&[11;32]);
    let owner_public=PublicKey::from_bytes(owner.verifying_key().to_bytes());
    let node=provision_node(&node_dir,fixture_id()).unwrap();
    let mut clock=Clock::start().unwrap();let now=clock.last;
    let context=Context {tenant:Tenant::LocalProfile {
        id:LocalProfileId::new("11111111-1111-4111-8111-111111111111").unwrap()},
        deployment_id:DeploymentId::new("22222222-2222-4222-8222-222222222222").unwrap()};
    let pin=OwnerPin {context:context.clone(),key:owner_public,epoch:1,checkpoint:0,
        expires_at:now.checked_add(300).unwrap()};
    let store=SqliteJournalStore::create(&db,&anchor,pin.clone(),Now(now),SqliteStorageBudget::default()).unwrap();
    let mut session=Session {store:Some(store),node_dir:node_dir.clone(),agent:fixture_id(),node:node.clone(),
        phase:Phase::New,seen:BTreeSet::new(),expected_pin:pin.clone(),expected_rev:0,
        claim:None,certificate:None,offer_expires:None,effects:false};
    clock.cap(pin.expires_at).unwrap();
    let mut pairs=Vec::new();
    let (_,pair)=exchange(&output,1,"probe",json!({}),&mut session,&mut clock);pairs.push(pair);
    let token=InvitationToken::from_bytes([0x21;32]).unwrap();
    let offer=sign(Offer {header:Header {version:1,context:context.clone(),owner_epoch:1,checkpoint:1},
        invitation_id:InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
        agent_id:fixture_id(),agent_key:node.public_key,token_hash:token.digest(),scopes:Scopes(vec![Scope::TunnelOpen]),
        issued_at:now,expires_at:now.checked_add(120).unwrap()},&owner);
    offer.verify(owner_public).unwrap();
    let (staged,pair)=exchange(&output,2,"stageOffer",json!({"expectedActorRevision":0,
        "expectedOwnerPin":pin,"offer":offer,"token":hex::encode([0x21;32])}),&mut session,&mut clock);pairs.push(pair);
    let claim:Signed<Claim>=decode(staged["payload"]["claim"].clone()).unwrap();
    claim.verify(node.public_key).unwrap();let claim_digest=claim.body.digest().unwrap();
    let actual=session.snapshot().unwrap();assert_eq!(actual.revision(),2);assert_eq!(actual.owner().checkpoint,1);
    assert_eq!(staged["payload"]["actorRevision"],2);assert_eq!(staged["payload"]["claimDigest"],json!(claim_digest));
    let certificate=sign(Certificate {header:Header {version:1,context,owner_epoch:1,checkpoint:2},
        agent_id:fixture_id(),agent_key:node.public_key,key_epoch:1,scopes:Scopes(vec![Scope::TunnelOpen]),
        capabilities:claim.body.capabilities.clone(),origin:CertificateOrigin::Enrollment {
            invitation_id:claim.body.invitation_id,claim_hash:claim_digest,key_fingerprint:node.key_fingerprint},
        not_before:now,expires_at:now.checked_add(100).unwrap()},&owner);
    certificate.verify(owner_public).unwrap();let certificate_digest=certificate.body.digest().unwrap();
    let expected=session.expected_pin.clone();
    let (applied,pair)=exchange(&output,3,"applyCertificate",json!({"expectedRev":2,"expectedPin":expected,
        "claimDigest":claim_digest,"certificate":certificate}),&mut session,&mut clock);pairs.push(pair);
    let actual=session.snapshot().unwrap();assert_eq!(actual.revision(),3);assert_eq!(actual.owner().checkpoint,2);
    assert_eq!(applied["payload"]["certificateDigest"],json!(certificate_digest));
    let expected=session.expected_pin.clone();
    let (observed,pair)=exchange(&output,4,"observeCertificate",json!({"expectedRev":3,"expectedPin":expected,
        "agentId":fixture_id(),"certificateDigest":certificate_digest}),&mut session,&mut clock);pairs.push(pair);
    assert_eq!(applied["payload"],observed["payload"]);assert!(session.phase==Phase::Closed);
    drop(session);
    // Reopen SAME real journal using genesis, not the advanced request pin.
    let reopened=SqliteJournalStore::open(&db,&anchor,pin.clone(),SqliteStorageBudget::default()).unwrap();
    let state=reopened.load().unwrap();assert_eq!(state.revision(),3);assert_eq!(state.owner().checkpoint,2);
    assert_eq!(state.certificate(fixture_id()).unwrap().digest().unwrap(),certificate_digest);
    assert!(!state.revoked(fixture_id()));drop(reopened);drop(private);
    assert!(!private_path.exists());
    let manifest=json!({"format":"cc-actual-enrollment-public-golden/v1","syntheticFixtureOnly":true,
        "fixtureOwnerSeedDecimal":11,"fixtureTokenByteHex":"21","nodeKeyAndNonce":"actual OS custody, never exported",
        "observedAt":now,"genesisOwnerPin":pin,"publicNode":node,"pairs":pairs,
        "claimDigest":claim_digest,"certificateDigest":certificate_digest,
        "actualFinalRevision":state.revision(),"actualFinalCheckpoint":state.owner().checkpoint,
        "privateNodeAndJournalRemoved":true,"productSshAuthenticationProved":false,
        "expiresAsRecordedNotLiveAuthority":true});
    let raw=canonical(&manifest).unwrap();save(&output,"GOLDEN_MANIFEST.jsonl",&raw);
    println!("CC_ENROLLMENT_GOLDEN_CREATED"); // No payload/key/token diagnostic log.
}

}

// Real same Actor/custody enrollment followed by a NEW fixed-role session.
// These are component tests, not SSH authentication or Native UI acceptance.
fn enrolled_for_revocation(lifetime:u64)->Fixture {
    let mut f=Fixture::new();f.probe();let staged=f.stage();
    let claim:Signed<Claim>=decode(staged["payload"]["claim"].clone()).unwrap();
    let hash=claim.body.digest().unwrap();let now=Clock::observed().unwrap();
    let cert=signed(Certificate {header:header(2),agent_id:id(),key_epoch:1,agent_key:f.session.node.public_key,
        scopes:Scopes(vec![Scope::TunnelOpen]),capabilities:claim.body.capabilities,
        origin:CertificateOrigin::Enrollment {invitation_id:claim.body.invitation_id,claim_hash:hash,
            key_fingerprint:f.session.node.key_fingerprint},not_before:now,expires_at:now.checked_add(lifetime).unwrap()},&f.owner);
    let digest=cert.body.digest().unwrap();
    f.session.request(frame(3,"applyCertificate",json!({"expectedRev":2,"expectedPin":f.session.expected_pin,
        "claimDigest":hash,"certificate":cert})),&mut f.clock).unwrap();
    f.session.request(frame(4,"observeCertificate",json!({"expectedRev":3,"expectedPin":f.session.expected_pin,
        "agentId":id(),"certificateDigest":digest})),&mut f.clock).unwrap();
    fresh_revocation_session(&mut f);f
}
fn fresh_revocation_session(f:&mut Fixture) {
    let store=f.session.store.take().unwrap();let state=store.load().unwrap();
    f.session=Session {store:Some(store),node_dir:f.session.node_dir.clone(),agent:id(),node:f.session.node.clone(),
        phase:Phase::New,seen:BTreeSet::new(),expected_pin:state.owner().clone(),expected_rev:state.revision(),
        claim:None,certificate:None,offer_expires:None,effects:false};
    f.clock=Clock::start().unwrap();f.clock.cap(state.owner().expires_at).unwrap();
}
fn revocation_probe(f:&mut Fixture) {
    f.session.request(frame(1,"probeRevocation",json!({})),&mut f.clock).unwrap();
}
fn revocation(f:&Fixture)->Signed<Revocation> {
    let state=f.session.snapshot().unwrap();let cert=state.certificate(id()).unwrap();
    signed(Revocation {header:Header {version:1,context:state.owner().context.clone(),owner_epoch:state.owner().epoch,
        checkpoint:state.owner().checkpoint+1},agent_id:id(),key_epoch:cert.key_epoch,
        certificate_hash:cert.digest().unwrap(),issued_at:Clock::observed().unwrap()},&f.owner)
}
fn apply_revocation(f:&mut Fixture)->Value {
    let value=revocation(f);let pin=f.session.expected_pin.clone();let revision=f.session.expected_rev;
    f.session.request(frame(2,"revokeCertificate",json!({"expectedRev":revision,"expectedPin":pin,
        "revocation":value})),&mut f.clock).unwrap()
}
#[test]
fn revoke_real_current_node_commit_then_exact_observation_and_no_key_change() {
    let mut f=enrolled_for_revocation(100);let original=f.session.node.clone();revocation_probe(&mut f);
    let applied=apply_revocation(&mut f);assert_eq!(applied["payload"]["actorRevision"],4);
    assert_eq!(applied["payload"]["ownerPin"]["checkpoint"],3);assert_eq!(applied["payload"]["revoked"],true);
    let digest=f.session.certificate.unwrap();
    let observed=f.session.request(frame(3,"observeRevocation",json!({"expectedRev":4,"expectedPin":f.session.expected_pin,
        "agentId":id(),"keyEpoch":1,"certificateDigest":digest})),&mut f.clock).unwrap();
    assert_eq!(observed["payload"],applied["payload"]);assert!(f.session.phase==Phase::Closed);
    assert_eq!(inspect_existing_node_identity(&f.session.node_dir,id()).unwrap(),original);
}
#[test]
fn revoke_expired_registered_certificate_still_tombstones_with_live_owner() {
    let mut f=enrolled_for_revocation(3);
    let expires=f.session.snapshot().unwrap().certificate(id()).unwrap().expires_at;
    let deadline=Instant::now()+Duration::from_secs(5);
    while Clock::observed().unwrap()<expires {
        assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));
    }
    revocation_probe(&mut f);assert_eq!(apply_revocation(&mut f)["payload"]["revoked"],true);
    assert!(f.session.snapshot().unwrap().revoked(id()));
}
#[test]
fn revoke_wrong_certificate_and_stale_cas_do_not_mutate_current_state() {
    let mut f=enrolled_for_revocation(100);revocation_probe(&mut f);
    let mut body=revocation(&f).body;body.certificate_hash=Hash::from_bytes([9;32]);let wrong=signed(body,&f.owner);
    let before=f.session.snapshot().unwrap().revision();
    assert!(f.session.request(frame(2,"revokeCertificate",json!({"expectedRev":before,"expectedPin":f.session.expected_pin,
        "revocation":wrong})),&mut f.clock).is_err());
    assert_eq!(f.session.snapshot().unwrap().revision(),before);assert!(!f.session.effects);
    fresh_revocation_session(&mut f);revocation_probe(&mut f);let request=revocation(&f);
    let mut other=Actor::new(f.session.store.as_ref().unwrap().clone()).unwrap();
    other.revoke(request.clone(),Now(Clock::observed().unwrap())).unwrap();
    assert_eq!(f.session.request(frame(2,"revokeCertificate",json!({"expectedRev":before,"expectedPin":f.session.expected_pin,
        "revocation":request})),&mut f.clock),Err(Error::Conflict));
    assert_eq!(f.session.snapshot().unwrap().revision(),before+1);assert!(!f.session.effects);
}
#[test]
fn revoke_committed_without_final_observe_survives_new_session_without_retry() {
    let mut f=enrolled_for_revocation(100);revocation_probe(&mut f);apply_revocation(&mut f);
    fresh_revocation_session(&mut f);let reply=f.session.request(frame(1,"probeRevocation",json!({})),&mut f.clock).unwrap();
    assert_eq!(reply["payload"]["revoked"],true);assert_eq!(reply["payload"]["actorRevision"],4);
    let request=revocation(&f);
    assert!(f.session.request(frame(2,"revokeCertificate",json!({"expectedRev":4,"expectedPin":f.session.expected_pin,
        "revocation":request})),&mut f.clock).is_err());
    assert_eq!(f.session.snapshot().unwrap().revision(),4);assert!(!f.session.effects);
}

#[test]
fn revoke_sqlite_same_journal_readback_reopens_genesis_with_exact_r4c3_tombstone() {
    let f=Fixture::new();let db=f.directory.0.join("revoke-db");let anchor=f.directory.0.join("revoke-anchor");
    let store=SqliteJournalStore::create(&db,&anchor,f.pin.clone(),Now(stamp(f.now)),SqliteStorageBudget::default()).unwrap();
    let mut session=Session {store:Some(store),node_dir:f.session.node_dir.clone(),agent:id(),node:f.session.node.clone(),
        phase:Phase::New,seen:BTreeSet::new(),expected_pin:f.pin.clone(),expected_rev:0,claim:None,certificate:None,
        offer_expires:None,effects:false};let mut clock=Clock::start().unwrap();clock.cap(f.pin.expires_at).unwrap();
    session.request(frame(1,"probe",json!({})),&mut clock).unwrap();
    let staged=session.request(frame(2,"stageOffer",json!({"expectedActorRevision":0,"expectedOwnerPin":f.pin,
        "offer":f.offer(33),"token":hex::encode([33;32])})),&mut clock).unwrap();
    let claim:Signed<Claim>=decode(staged["payload"]["claim"].clone()).unwrap();let hash=claim.body.digest().unwrap();
    let now=Clock::observed().unwrap();let cert=signed(Certificate {header:header(2),agent_id:id(),key_epoch:1,
        agent_key:session.node.public_key,scopes:Scopes(vec![Scope::TunnelOpen]),capabilities:claim.body.capabilities,
        origin:CertificateOrigin::Enrollment {invitation_id:claim.body.invitation_id,claim_hash:hash,
            key_fingerprint:session.node.key_fingerprint},not_before:now,expires_at:now.checked_add(100).unwrap()},&f.owner);
    let digest=cert.body.digest().unwrap();
    session.request(frame(3,"applyCertificate",json!({"expectedRev":2,"expectedPin":session.expected_pin,
        "claimDigest":hash,"certificate":cert})),&mut clock).unwrap();
    session.request(frame(4,"observeCertificate",json!({"expectedRev":3,"expectedPin":session.expected_pin,
        "agentId":id(),"certificateDigest":digest})),&mut clock).unwrap();
    drop(session);
    let store=SqliteJournalStore::open(&db,&anchor,f.pin.clone(),SqliteStorageBudget::default()).unwrap();
    let state=store.load().unwrap();let mut session=Session {store:Some(store),node_dir:f.session.node_dir.clone(),agent:id(),
        node:f.session.node.clone(),phase:Phase::New,seen:BTreeSet::new(),expected_pin:state.owner().clone(),
        expected_rev:state.revision(),claim:None,certificate:None,offer_expires:None,effects:false};
    session.request(frame(1,"probeRevocation",json!({})),&mut clock).unwrap();
    let signed=signed(Revocation {header:header(3),agent_id:id(),key_epoch:1,certificate_hash:digest,
        issued_at:Clock::observed().unwrap()},&f.owner);
    session.request(frame(2,"revokeCertificate",json!({"expectedRev":3,"expectedPin":session.expected_pin,
        "revocation":signed})),&mut clock).unwrap();
    let observed=session.request(frame(3,"observeRevocation",json!({"expectedRev":4,"expectedPin":session.expected_pin,
        "agentId":id(),"keyEpoch":1,"certificateDigest":digest})),&mut clock).unwrap();
    assert_eq!(observed["payload"]["revoked"],true);assert!(session.phase==Phase::Closed);drop(session);
    let reopened=SqliteJournalStore::open(&db,&anchor,f.pin.clone(),SqliteStorageBudget::default()).unwrap();
    let state=reopened.load().unwrap();assert_eq!(state.revision(),4);assert_eq!(state.owner().checkpoint,3);
    assert!(state.revoked(id()));assert_eq!(state.certificate(id()).unwrap().digest().unwrap(),digest);
}


#[test]
fn linux_target_directory_flags_open_directory_and_refuse_regular_file() {
    let directory=Directory::new();
    let flags=NOFOLLOW|NONBLOCK|CLOEXEC|rustix::fs::OFlags::DIRECTORY.bits() as i32;
    let held=OpenOptions::new().read(true).custom_flags(flags).open(&directory.0).unwrap();
    assert!(held.metadata().unwrap().is_dir());
    let file=directory.0.join("regular");fs::write(&file,b"owned").unwrap();
    assert!(OpenOptions::new().read(true).custom_flags(flags).open(file).is_err());
}

#[test]
fn linux_target_nofollow_flags_refuse_directory_and_file_symlinks() {
    use std::os::unix::fs::symlink;
    let directory=Directory::new();
    let link=directory.0.join("directory-link");symlink(&directory.0,&link).unwrap();
    assert!(OpenOptions::new().read(true)
        .custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC|rustix::fs::OFlags::DIRECTORY.bits() as i32).open(link).is_err());
    let file=directory.0.join("regular");fs::write(&file,b"owned").unwrap();
    let link=directory.0.join("file-link");symlink(&file,&link).unwrap();
    assert!(OpenOptions::new().read(true).custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC).open(link).is_err());
}

#[test]
fn linux_target_file_flags_keep_original_nonblock_and_cloexec() {
    use std::os::fd::AsRawFd;
    let directory=Directory::new();let file=directory.0.join("regular");fs::write(&file,b"owned").unwrap();
    let held=OpenOptions::new().read(true).custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC).open(file).unwrap();
    let info=fs::read_to_string(format!("/proc/self/fdinfo/{}",held.as_raw_fd())).unwrap();
    let flags=info.lines().find_map(|line|line.strip_prefix("flags:\t")).unwrap();
    let flags=u64::from_str_radix(flags.trim(),8).unwrap();
    assert_ne!(flags&rustix::fs::OFlags::CLOEXEC.bits() as u64,0);
    assert_ne!(flags&rustix::fs::OFlags::NONBLOCK.bits() as u64,0);
}
