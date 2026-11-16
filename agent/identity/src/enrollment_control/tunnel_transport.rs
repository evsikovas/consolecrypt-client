//! Original pinned SSH supplies reviewed data; only the existing TLS-bound
//! supervisor can admit, debit or privately retire a Node permit.
use super::*;
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use zeroize::Zeroize;

mod control;

const TRANSPORT: &str="/etc/consolecrypt-agent/tunnel-transport.json";
const SERVER_DER: &str="/etc/consolecrypt-agent/tunnel-server.der";
const SERVER_ROOT: &str="/etc/consolecrypt-agent/tunnel-server-root.der";
const RELAY_CA: &str="/etc/consolecrypt-agent/tunnel-relay-ca.pem";
const SERVER_KEY: &str="/var/lib/consolecrypt-agent/tunnel/server.key";
const SESSIONS: &str="/var/lib/consolecrypt-agent/tunnel/sessions";

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct InstalledConfig {format:String,server_name:String,relay_origin:String,relay_dial_ip:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Relay {slot_id:String,#[serde(deserialize_with="secret_string")] capability:Zeroizing<String>,
    expires_at:Timestamp,idle_seconds:u64,max_ciphertext_bytes:u64}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Start {expected_rev:u64,expected_pin:OwnerPin,binding:NodeRequestBinding,
    policy:TunnelRoutePolicyV2,client_certificate_hex:String,relay:Relay}

fn hash(raw:&[u8])->Hash {Hash::from_bytes(Sha256::digest(raw).into())}
fn dns(name:&str)->bool {
    !name.is_empty() && name.len()<=253 && name.is_ascii() && name.split('.').all(|part|
        !part.is_empty() && part.len()<=63 && !part.starts_with('-') && !part.ends_with('-')
        && part.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'))
}
fn origin(value:&str)->Result<()> {
    let authority=value.strip_prefix("wss://").ok_or(Error::ScopeDenied)?;
    if value.len()>256 || !value.is_ascii() || authority.bytes().any(|b|b"/?#@%\\".contains(&b)) {
        return Err(Error::ScopeDenied)
    }
    // Closed first subset: canonical DNS/IPv4 authority; no URI normalizer,
    // redirects, userinfo or endpoint selected by a received capability.
    let host=if let Some((host,port))=authority.rsplit_once(':') {
        let n=port.parse::<u16>().map_err(|_|Error::ScopeDenied)?;
        if n==0 || port!=n.to_string(){return Err(Error::ScopeDenied)} host
    }else{authority};
    if !dns(host){return Err(Error::ScopeDenied)} Ok(())
}
fn public_der(text:&str)->Result<Vec<u8>> {
    if text.len()<128 || text.len()>4096 || text.len()%2!=0
        || !text.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)) {
        return Err(Error::InvalidWire)
    }
    let bytes=hex::decode(text).map_err(|_|Error::InvalidWire)?;
    if bytes.first()!=Some(&0x30){return Err(Error::InvalidWire)}
    // DER PKI parsing/signature/validity/private-key PoP is still OpenSSL's
    // maintained TLS verifier. This is only bounded exact public-byte input.
    Ok(bytes)
}
fn pem(bytes:&[u8])->Vec<u8> {
    const A:&[u8;64]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded=Vec::with_capacity(bytes.len().div_ceil(3)*4);
    for block in bytes.chunks(3) {
        let n=((block[0] as u32)<<16)|((block.get(1).copied().unwrap_or(0) as u32)<<8)
            |block.get(2).copied().unwrap_or(0) as u32;
        encoded.extend_from_slice(&[A[(n>>18) as usize],A[((n>>12)&63) as usize],
            if block.len()>1 {A[((n>>6)&63) as usize]}else{b'='},
            if block.len()>2 {A[(n&63) as usize]}else{b'='}]);
    }
    let mut result=b"-----BEGIN CERTIFICATE-----\n".to_vec();
    for line in encoded.chunks(64){result.extend_from_slice(line);result.push(b'\n')}
    result.extend_from_slice(b"-----END CERTIFICATE-----\n");result
}
struct Installed {config:InstalledConfig,files:Vec<(PathBuf,Vec<u8>)>,server:Vec<u8>,root:Vec<u8>}
impl Installed {
    fn load()->Result<Self> {
        let raw=root_file(Path::new(TRANSPORT))?;
        let config:InstalledConfig=serde_json::from_slice(&raw).map_err(|_|Error::InvalidWire)?;
        if config.format!="cc-agent-installed-tunnel-transport/v1" || !dns(&config.server_name) {
            return Err(Error::ScopeDenied)
        }
        origin(&config.relay_origin)?;
        let ip:IpAddr=config.relay_dial_ip.parse().map_err(|_|Error::ScopeDenied)?;
        if ip.to_string()!=config.relay_dial_ip {return Err(Error::ScopeDenied)}
        let server=root_file(Path::new(SERVER_DER))?;let root=root_file(Path::new(SERVER_ROOT))?;
        if !(64..=2048).contains(&server.len()) || !(64..=2048).contains(&root.len())
            || server.first()!=Some(&0x30) || root.first()!=Some(&0x30) {return Err(Error::InvalidWire)}
        let relay=root_file(Path::new(RELAY_CA))?;
        if relay.is_empty() || relay.len()>16384 {return Err(Error::InvalidWire)}
        private_root(Path::new(SESSIONS),current_uid()?)?;private_key_metadata(current_uid()?)?;
        crate::tunnel_supervisor::check_installed_transport()?;
        Ok(Self {config,files:vec![(PathBuf::from(TRANSPORT),raw),(PathBuf::from(SERVER_DER),server.clone()),
            (PathBuf::from(SERVER_ROOT),root.clone()),(PathBuf::from(RELAY_CA),relay)],server,root})
    }
    fn check(&self)->Result<()> {
        for (path,raw) in &self.files {if root_file(path)?!=*raw{return Err(Error::Storage)}} Ok(())
    }
}
struct Original {
    config:Config,config_raw:Vec<u8>,pin_raw:Vec<u8>,node:PublicNodeIdentity,
    owner:OwnerPin,revision:u64,certificate:Certificate,
}
impl Original {
    fn capture(clock:&mut Clock)->Result<(Self,Actor<SqliteJournalStore>)> {
        let config_raw=root_file(Path::new(CONFIG))?;
        let config:Config=serde_json::from_slice(&config_raw).map_err(|_|Error::InvalidWire)?;
        if config.format!="cc-agent-enroll-config/v1" {return Err(Error::InvalidWire)}
        let paths=[&config.node_dir,&config.db_dir,&config.anchor_dir];
        for(i,path)in paths.iter().enumerate(){
            crate::durable::absolute(path)?;crate::durable::secure_path(path)?;
            if paths.iter().enumerate().any(|(j,p)|i!=j&&(path.starts_with(p)||p.starts_with(path)))
                || config.owner_pin.starts_with(path)
                || path.starts_with("/var/lib/consolecrypt-agent/tunnel")
                || Path::new("/var/lib/consolecrypt-agent/tunnel").starts_with(path) {return Err(Error::Storage)}
        }
        let pin_raw=root_file(&config.owner_pin)?;
        let genesis:OwnerPin=serde_json::from_slice(&pin_raw).map_err(|_|Error::InvalidWire)?;
        let actor=Actor::new(SqliteJournalStore::open(&config.db_dir,&config.anchor_dir,genesis,SqliteStorageBudget::default())?)?;
        let state=actor.snapshot()?;
        let inspected=inspect_node_state(&config.node_dir,config.agent_id)?;
        let node=inspect_existing_node_identity(&config.node_dir,config.agent_id)?;
        if inspected.facts().actual_selected_epoch!=Some(node.key_epoch)
            || !matches!(inspected.facts().selection_state,SelectionState::Legacy1|SelectionState::Prepared1|SelectionState::Selected2) {
            return Err(Error::UncertainCommit)
        }
        inspected.recheck_guard_identity()?;
        let certificate=state.certificate(config.agent_id).ok_or(Error::NotFound)?.clone();
        if state.revoked(config.agent_id) || certificate.agent_key!=node.public_key || certificate.key_epoch!=node.key_epoch
            || certificate.scopes.0.as_slice()!=[Scope::TunnelOpen]
            || certificate.capabilities.version!=1 || certificate.capabilities.scopes.0.as_slice()!=[Scope::TunnelOpen] {
            return Err(Error::CapabilityDenied)
        }
        crate::wire::live(certificate.not_before,certificate.expires_at,clock.now()?)?;
        clock.cap(state.owner().expires_at)?;clock.cap(certificate.expires_at)?;
        let original=Self {config,config_raw,pin_raw,node,owner:state.owner().clone(),revision:state.revision(),certificate};
        Ok((original,actor))
    }
    fn check(&self,actor:&Actor<SqliteJournalStore>,clock:&mut Clock)->Result<()> {
        if root_file(Path::new(CONFIG))?!=self.config_raw || root_file(&self.config.owner_pin)?!=self.pin_raw {
            return Err(Error::Storage)
        }
        let state=actor.snapshot()?;
        if state.owner()!=&self.owner || state.revision()!=self.revision {return Err(Error::Conflict)}
        if state.revoked(self.config.agent_id) || state.certificate(self.config.agent_id)!=Some(&self.certificate)
            || inspect_existing_node_identity(&self.config.node_dir,self.config.agent_id)?!=self.node {
            return Err(Error::WrongClaim)
        }
        crate::wire::live(self.certificate.not_before,self.certificate.expires_at,clock.now()?)?;Ok(())
    }
}
fn check_relay(relay:&Relay,policy:&TunnelRoutePolicyV2,lease_expiry:Timestamp,now:Now)->Result<()> {
    let slot=uuid::Uuid::parse_str(&relay.slot_id).map_err(|_|Error::InvalidWire)?;
    let bytes=relay.capability.as_bytes();
    if slot.is_nil() || slot.to_string()!=relay.slot_id || bytes.len()!=48 || !bytes.starts_with(b"ccrl_")
        || !bytes[5..].iter().all(|b|b.is_ascii_alphanumeric()||b"_-".contains(b))
        // 32 random bytes have two canonical unused low bits in the last URL-safe base64 character.
        || !b"AEIMQUYcgkosw048".contains(&bytes[47])
        || relay.expires_at<=now.0 || relay.expires_at>lease_expiry
        || relay.expires_at.seconds().checked_sub(now.0.seconds()).filter(|v|*v<=300).is_none()
        || relay.idle_seconds==0 || relay.idle_seconds>policy.route.limits.idle_timeout_seconds
        || relay.idle_seconds>30 || relay.max_ciphertext_bytes==0 || relay.max_ciphertext_bytes>8*1024*1024 {
        return Err(Error::ScopeDenied)
    }
    Ok(())
}
fn current_uid()->Result<u32> {
    fs::read_to_string("/proc/self/status").map_err(|_|Error::Storage)?.lines()
        .find_map(|l|l.strip_prefix("Uid:")).and_then(|v|v.split_whitespace().nth(1))
        .ok_or(Error::Storage)?.parse().map_err(|_|Error::Storage)
}
fn private_root(path:&Path,user:u32)->Result<()> {
    for p in path.ancestors(){
        let m=fs::symlink_metadata(p).map_err(|_|Error::Storage)?;
        if !m.is_dir() || m.file_type().is_symlink() || ![0,user].contains(&m.uid()) || m.mode()&0o022!=0 {
            return Err(Error::Storage)
        }
    }
    let m=fs::symlink_metadata(path).map_err(|_|Error::Storage)?;
    if m.uid()!=user || m.mode()&0o777!=0o700 {return Err(Error::Storage)} Ok(())
}
fn private_key_metadata(user:u32)->Result<()> {
    let path=Path::new(SERVER_KEY);let parent=path.parent().ok_or(Error::Storage)?;private_root(parent,user)?;
    let m=fs::symlink_metadata(path).map_err(|_|Error::Storage)?;
    if !m.is_file() || m.file_type().is_symlink() || m.uid()!=user || m.nlink()!=1
        || m.mode()&0o777!=0o600 || m.len()==0 || m.len()>65536 {return Err(Error::Storage)} Ok(())
}
struct SessionFiles {directory:PathBuf,identity:(u64,u64),files:Vec<PathBuf>,_lock:File}
impl SessionFiles {
    fn new()->Result<Self> {
        let user=current_uid()?;let root=Path::new(SESSIONS);private_root(root,user)?;private_key_metadata(user)?;
        if fs::read_dir(root).map_err(|_|Error::Storage)?.take(33).count()>32 {return Err(Error::Quota)}
        let lock_path=root.join(".transport.lock");
        let lock=OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600)
            .custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC).open(&lock_path).map_err(|_|Error::Storage)?;
        let m=lock.metadata().map_err(|_|Error::Storage)?;
        let path_m=fs::symlink_metadata(&lock_path).map_err(|_|Error::Storage)?;
        if !m.is_file() || m.nlink()!=1 || m.uid()!=user || m.mode()&0o777!=0o600
            || (m.dev(),m.ino())!=(path_m.dev(),path_m.ino()) {return Err(Error::Storage)}
        lock.try_lock().map_err(|_|Error::Conflict)?;
        let mut random=[0u8;16];getrandom::fill(&mut random).map_err(|_|Error::Entropy)?;
        if random==[0;16]{return Err(Error::Entropy)}
        let directory=root.join(hex::encode(random));fs::DirBuilder::new().mode(0o700).create(&directory).map_err(|_|Error::Storage)?;
        let m=fs::symlink_metadata(&directory).map_err(|_|Error::Storage)?;
        Ok(Self {directory,identity:(m.dev(),m.ino()),files:Vec::new(),_lock:lock})
    }
    fn write(&mut self,name:&str,bytes:&[u8])->Result<PathBuf> {
        if !matches!(name,"policy.json"|"server.pem"|"client.pem"|"relay.json"|"child.json") || bytes.len()>16384 {
            return Err(Error::Quota)
        }
        let path=self.directory.join(name);
        let mut file=OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(NOFOLLOW|CLOEXEC)
            .open(&path).map_err(|_|Error::Storage)?;
        self.files.push(path.clone());file.write_all(bytes).map_err(|_|Error::Storage)?;
        file.sync_all().map_err(|_|Error::Storage)?;Ok(path)
    }
    fn cleanup(&mut self)->Result<()> {
        if self.files.is_empty() && !self.directory.exists(){return Ok(())}
        let m=fs::symlink_metadata(&self.directory).map_err(|_|Error::Storage)?;
        if !m.is_dir() || m.file_type().is_symlink() || (m.dev(),m.ino())!=self.identity {return Err(Error::Storage)}
        while let Some(path)=self.files.last() {fs::remove_file(path).map_err(|_|Error::Storage)?;self.files.pop();}
        fs::remove_dir(&self.directory).map_err(|_|Error::Storage)
    }
}
impl Drop for SessionFiles {fn drop(&mut self){let _=self.cleanup();}}

fn path_arg(path:&Path)->Result<String> {
    let value=path.to_str().ok_or(Error::Storage)?;
    if !path.is_absolute() || !value.is_ascii() || value.len()>4096 {return Err(Error::Storage)}
    Ok(value.to_owned())
}
fn reply(pipe:&mut Pipe,clock:&mut Clock,frame:&Envelope,payload:Value)->Result<()> {
    pipe.write(&json!({"format":FORMAT,"requestId":frame.request_id,"kind":frame.kind,"payload":payload}),clock)
}
fn prepare(mut pipe:&mut Pipe,mut clock:&mut Clock,first:Envelope,correlation:&mut Option<String>)
    ->Result<Option<(Vec<String>,crate::tunnel_supervisor::PreparedTransport,SessionFiles)>> {
    let _:Empty=decode(first.payload.clone())?;
    if fs::symlink_metadata(TRANSPORT).is_err_and(|e|e.kind()==std::io::ErrorKind::NotFound) {
        return Err(Error::CapabilityDenied)
    }
    let installed=Installed::load()?;
    let (original,mut actor)=Original::capture(&mut clock)?;
    original.check(&actor,&mut clock)?;installed.check()?;
    reply(&mut pipe,&mut clock,&first,json!({"node":original.node,"ownerPin":original.owner,
        "actorRevision":original.revision,"certificate":original.certificate,"certificateDigest":original.certificate.digest()?,
        "revoked":false,"transportFormat":"cc-agent-tunnel-transport/v1","serverCertificateHex":hex::encode(&installed.server),
        "serverRootHex":hex::encode(&installed.root),"serverName":installed.config.server_name,"relayOrigin":installed.config.relay_origin}))?;
    *correlation=None;
    let frame=parse(&pipe.read(&mut clock)?)?;
    *correlation=Some(frame.request_id.clone());
    if frame.kind=="bootstrapTunnelControl" {
        if frame.request_id==first.request_id {return Err(Error::WrongState)}
        original.check(&actor,&mut clock)?;installed.check()?;
        control::bootstrap(&mut pipe,&mut clock,frame,&original,&actor,&installed,correlation)?;
        return Ok(None)
    }
    let mode=crate::tunnel_supervisor::PreparedTransportMode::from_product_verb(&frame.kind)?;
    if frame.request_id==first.request_id {return Err(Error::WrongState)}
    original.check(&actor,&mut clock)?;installed.check()?;
    let reply_id=frame.request_id.clone();let reply_kind=frame.kind.clone();
    let request:Start=decode(frame.payload)?;
    if request.expected_rev!=original.revision || request.expected_pin!=original.owner {return Err(Error::Conflict)}
    let client=public_der(&request.client_certificate_hex)?;
    if hash(&client)!=request.policy.route.client_tls_certificate_sha256
        || hash(&installed.server)!=request.policy.route.agent_tls_certificate_sha256
        || request.policy.route.agent_id!=original.node.agent_id
        || request.policy.route.key_epoch!=original.node.key_epoch
        || request.policy.route.agent_certificate_hash!=original.certificate.digest()? {return Err(Error::WrongClaim)}
    match mode {
        crate::tunnel_supervisor::PreparedTransportMode::Admit => actor.preview_original_tunnel_binding(&request.binding,&request.policy,clock.now()?)?,
        crate::tunnel_supervisor::PreparedTransportMode::Resume => {
            let (expiry,idle)=actor.preview_original_tunnel_resume_binding(&request.binding,&request.policy,clock.now()?)?;
            clock.cap(expiry)?;clock.cap(idle)?;
        }
    }
    check_relay(&request.relay,&request.policy,request.binding.lease.body.expires_at,clock.now()?)?;
    clock.cap(request.binding.lease.body.expires_at)?;clock.cap(request.relay.expires_at)?;
    original.check(&actor,&mut clock)?;installed.check()?;
    let mut files=SessionFiles::new()?;
    let policy=files.write("policy.json",&canonical(&serde_json::to_value(&request.policy).map_err(|_|Error::InvalidWire)?)?)?;
    let certificate=files.write("server.pem",&pem(&installed.server))?;
    let client_ca=files.write("client.pem",&pem(&client))?;
    let mut relay_value=json!({"origin":installed.config.relay_origin,"dialIp":installed.config.relay_dial_ip,
        "slotId":request.relay.slot_id,"capability":request.relay.capability.as_str(),"expiresAt":request.relay.expires_at,
        "idleSeconds":request.relay.idle_seconds,"maxCiphertextBytes":request.relay.max_ciphertext_bytes});
    let serialized_relay=canonical(&relay_value);
    if let Some(Value::String(token))=relay_value.get_mut("capability"){token.zeroize();}
    let relay_raw=Zeroizing::new(serialized_relay?);
    let relay_offer=files.write("relay.json",&relay_raw)?;
    let config=files.write("child.json",&canonical(&json!({"format":"cc-tunnel-supervised-child-config/v1",
        "policy":policy,"certificate":certificate,"private_key":SERVER_KEY,"client_ca":client_ca,"relay_ca":RELAY_CA,
        "relay_offer":relay_offer}))?)?;
    let mut root_files=installed.files.iter().map(|(p,v)|(p.clone(),hash(v))).collect::<Vec<_>>();
    root_files.push((PathBuf::from(CONFIG),hash(&original.config_raw)));
    root_files.push((original.config.owner_pin.clone(),hash(&original.pin_raw)));
    let prepared=crate::tunnel_supervisor::PreparedTransport {binding:request.binding.clone(),revision:original.revision,
        pin:original.owner.clone(),root_files,mode};
    let args=vec!["worker-tunnel-supervised-sqlite".to_owned(),"--db-dir".to_owned(),path_arg(&original.config.db_dir)?,
        "--anchor-dir".to_owned(),path_arg(&original.config.anchor_dir)?,"--owner-pin".to_owned(),
        path_arg(&original.config.owner_pin)?,"--node-dir".to_owned(),path_arg(&original.config.node_dir)?,
        "--policy".to_owned(),path_arg(&policy)?,"--child-config".to_owned(),path_arg(&config)?];
    original.check(&actor,&mut clock)?;installed.check()?;
    pipe.write(&json!({"format":FORMAT,"requestId":reply_id,"kind":reply_kind,"payload":{
        "route":request.policy.route.route_id,"policyHash":request.policy.digest()?,
        "leaseHash":request.binding.lease.body.digest()?,"certificateDigest":original.certificate.digest()?,
        "slotId":request.relay.slot_id,"expiresAt":request.relay.expires_at,"node":original.node}}),&mut clock)?;
    // No Node admission happened above. This SAME SSH Rust process now owns
    // the existing supervisor; no shell/worker path/PID/Permit is supplied.
    drop(actor);
    Ok(Some((args,prepared,files)))
}
pub(super) fn run(mut pipe:Pipe,mut clock:Clock,first:Envelope)->Result<()> {
    let mut correlation=Some(first.request_id.clone());
    let (args,prepared,mut files)=match prepare(&mut pipe,&mut clock,first,&mut correlation) {
        Ok(Some(value))=>value,
        Ok(None)=>return Ok(()),
        Err(error)=>{
            if let Some(id)=correlation {
                let code=match error {
                    Error::Conflict=>"contextChanged",Error::Expired|Error::InvalidTime=>"expired",
                    Error::Storage=>"transportUnavailable",Error::CapabilityDenied=>"transportUnsupported",
                    Error::Quota=>"capacity",_=>"denied",
                };
                let _=pipe.write(&json!({"format":FORMAT,"requestId":id,"kind":"error",
                    "payload":{"code":code,"effect":"notCommitted"}}),&mut clock);
            }
            return Err(error)
        }
    };
    drop(pipe);
    let result=crate::tunnel_supervisor::run_prepared_transport(&args,prepared);
    let cleaned=files.cleanup();result.and(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tunnel_transport_installed_origin_never_accepts_wire_url_features() {
        assert!(origin("wss://relay.example.test:443").is_ok());
        for value in ["https://relay.example.test","wss://user@relay.example.test","wss://relay.example.test/path",
            "wss://relay.example.test?token=x","wss://relay.example.test:0443","wss://relay.example.test#x"] {
            assert!(origin(value).is_err());
        }
    }
    #[test]
    fn tunnel_transport_der_input_is_exact_bounded_public_bytes() {
        assert!(public_der("30").is_err());assert!(public_der(&"30".repeat(2049)).is_err());
        assert!(public_der(&"AA".repeat(64)).is_err());
        let bytes=vec![0x30;64];assert_eq!(public_der(&hex::encode(&bytes)).unwrap(),bytes);
        assert_eq!(pem(b"abc").as_slice(),b"-----BEGIN CERTIFICATE-----\nYWJj\n-----END CERTIFICATE-----\n");
    }
    #[test]
    fn tunnel_transport_start_rejects_any_caller_path_or_clock_field() {
        assert!(decode::<Start>(json!({"path":"/tmp/worker","now":1})).is_err());
        assert!(decode::<InstalledConfig>(json!({"format":"cc-agent-installed-tunnel-transport/v1",
            "serverName":"agent.test","relayOrigin":"wss://relay.test","relayDialIp":"127.0.0.1","command":"sh"})).is_err());
    }
}
