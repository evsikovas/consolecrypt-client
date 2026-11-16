//! ExportersOnly original SSH role. The real durable Actor Permit stays here
//! through child readiness, status, revoke/expiry checks, stop and actual join.
use super::*;
mod artifact;
mod runtime;
#[cfg(test)]
mod tests;
include!("exporters/session.rs");
pub(super) fn exec(args:&[String])->Result<()> {runtime::exec(args)}

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Start {expected_rev:u64,expected_pin:OwnerPin,binding:NodeRequestBinding,manifest_sha256:Hash}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Control {manifest_sha256:Hash,lease_hash:Hash}
impl Clock {
    // Sample the monotonic clock BEFORE the wall clock so sampling latency can
    // only shorten custody. Capture once before proof/admission I/O, not after.
    fn cap_exporter(&mut self,expiry:Timestamp)->Result<Instant> {
        let monotonic=Instant::now();
        let wall=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|Error::InvalidTime)?;
        let deadline=self.cap_exporter_at(expiry,monotonic,wall)?;
        self.now()?;
        Ok(deadline)
    }
    // Explicit observations keep the existing Clock directly testable. No
    // injected clock/callback, new authority or post-admission timer reset.
    fn cap_exporter_at(&mut self,expiry:Timestamp,monotonic:Instant,wall:Duration)->Result<Instant> {
        let observed=Timestamp::new(wall.as_secs())?;
        if observed<self.last||monotonic>=self.end{return Err(Error::Expired)}
        let expiry=self.expiry.map_or(expiry,|old|old.min(expiry));
        let remaining=Duration::from_secs(expiry.seconds()).checked_sub(wall).ok_or(Error::Expired)?;
        if remaining.is_zero()||remaining>Duration::from_secs(300){return Err(Error::Expired)}
        let deadline=monotonic.checked_add(remaining).ok_or(Error::InvalidTime)?;
        self.last=observed;
        self.expiry=Some(expiry);
        self.end=self.end.min(deadline);
        Ok(self.end)
    }
}
fn config_unchanged(raw:&[u8],pin:&Path,pin_raw:&[u8])->Result<()> {
    if root_file(Path::new(CONFIG))?!=raw||root_file(pin)?!=pin_raw{return Err(Error::Storage)}Ok(())
}
fn check_permit(session:&mut Session<SqliteJournalStore>,permit:&Permit,clock:&mut Clock)->Result<()> {
    let now=clock.now()?;session.checked(now)?;
    let store=session.store.take().ok_or(Error::WrongState)?;
    let mut actor=Actor::new(store)?;let result=actor.check_permit(permit,now);
    session.store=Some(actor.into_store());result
}
fn reply(pipe:&mut Pipe,clock:&mut Clock,frame:&Envelope,payload:Value)->Result<()> {
    pipe.write(&json!({"format":FORMAT,"requestId":frame.request_id,"kind":frame.kind,"payload":payload}),clock)
}
fn running(policy:&ManagedExporterPolicy,lease_hash:Hash,expiry:Timestamp)->Result<Value> {
    Ok(json!({"format":"cc-agent-managed-exporter-session/v1","phase":"running",
        "manifestSha256":policy.digest()?,"leaseHash":lease_hash,"expiresAt":expiry,
        "agentId":policy.agent_id,"bind":{"ip":policy.bind_ip,"port":policy.bind_port},
        "collectors":policy.collectors,"runningVersion":null,"artifactVersion":"v1.12.1"}))
}
fn manage(mut session:Session<SqliteJournalStore>,mut pipe:Pipe,mut clock:Clock,frame:Envelope,
    config_raw:Vec<u8>,pin_path:PathBuf,pin_raw:Vec<u8>)->Result<()> {
    if session.phase!=Phase::Probed||frame.kind!="startExporter" {return Err(Error::WrongState)}
    if !session.seen.insert(frame.request_id.clone()){return Err(Error::Replay)}
    let input:Start=decode(frame.payload.clone())?;session.expect_request(input.expected_rev,&input.expected_pin)?;
    let catalog=session.catalog.as_ref().ok_or(Error::NotFound)?;
    catalog.check()?;let policy=catalog.policy.clone();
    if policy.agent_id!=session.agent||policy.digest()?!=input.manifest_sha256{return Err(Error::WrongClaim)}
    policy.check_binding(&input.binding)?;config_unchanged(&config_raw,&pin_path,&pin_raw)?;
    runtime::environment()?;
    let lease_hash=input.binding.lease.body.digest()?;let expiry=input.binding.lease.body.expires_at;
    let original_deadline=clock.cap_exporter(expiry)?;
    let proof=create_exporter_node_request(&session.node_dir,session.agent,&session.expected_pin,
        &policy,input.binding.clone(),clock.now()?)?;
    let next=session.expected_rev.checked_add(1).ok_or(Error::Quota)?;
    let mut original_permit=None;
    let binding=input.binding;
    session.mutate(session.expected_rev,|actor| {
        original_permit=Some(actor.admit(binding.lease,proof,Some(&binding.freshness),&binding.operation,clock.now()?)?);
        Ok(())
    })?;
    session.expected_rev=next;
    let permit=original_permit.ok_or(Error::UncertainCommit)?;
    // All errors after admission are possibly committed; NEVER re-admit/retry.
    check_permit(&mut session,&permit,&mut clock)?;
    let catalog=session.catalog.as_ref().ok_or(Error::NotFound)?;
    let mut owned=runtime::Owned::spawn(catalog,expiry,original_deadline)?;
    let action=(||->Result<()> {
        let ready_end=Instant::now()+Duration::from_secs(3);
        loop {
            config_unchanged(&config_raw,&pin_path,&pin_raw)?;
            check_permit(&mut session,&permit,&mut clock)?;
            if owned.check(session.catalog.as_ref().ok_or(Error::NotFound)?)?{break}
            if Instant::now()>=ready_end{return Err(Error::Expired)}
            std::thread::sleep(Duration::from_millis(10));
        }
        check_permit(&mut session,&permit,&mut clock)?;
        reply(&mut pipe,&mut clock,&frame,running(&policy,lease_hash,expiry)?)?;
        let mut pending=Zeroizing::new(Vec::new());let mut block=[0u8;1024];
        loop {
            config_unchanged(&config_raw,&pin_path,&pin_raw)?;
            check_permit(&mut session,&permit,&mut clock)?;
            if !owned.check(session.catalog.as_ref().ok_or(Error::NotFound)?)? {return Err(Error::WrongState)}
            pipe.check()?;
            match pipe.input.read(&mut block) {
                Ok(0)=>return Err(Error::WrongState),
                Ok(n)=>{pending.extend_from_slice(&block[..n]);if pending.len()>MAX_WIRE_BYTES{return Err(Error::Quota)}},
                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>{},
                Err(_)=>return Err(Error::Storage),
            }
            if let Some(end)=pending.iter().position(|b|*b==b'\n') {
                let raw:Vec<_>=pending.drain(..=end).collect();let request=parse(&raw)?;
                if session.seen.len()>=64||!session.seen.insert(request.request_id.clone()){return Err(Error::Replay)}
                let control:Control=decode(request.payload.clone())?;
                if control.manifest_sha256!=policy.digest()?||control.lease_hash!=lease_hash{return Err(Error::WrongClaim)}
                check_permit(&mut session,&permit,&mut clock)?;
                match request.kind.as_str() {
                    "exporterStatus"=>reply(&mut pipe,&mut clock,&request,running(&policy,lease_hash,expiry)?)?,
                    "stopExporter"=>{
                        owned.stop_join()?;
                        // RAW terminal once, after real child join/namespace
                        // cleanup. Returning drops the original stdout for EOF.
                        return pipe.write(&json!({"format":"cc-agent-managed-exporter-session/v1","phase":"stopped",
                            "manifestSha256":policy.digest()?,"leaseHash":lease_hash,"expiresAt":expiry,
                            "agentId":policy.agent_id,"cleanupObserved":true,"leaderJoined":true,
                            "remoteDrainObserved":false}),&mut clock)
                    },
                    _=>return Err(Error::WrongState),
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })();
    let joined=owned.stop_join(); // Also on lost reply, invalid frame, expiry/revoke/EOF.
    if joined.is_err(){return Err(Error::UncertainCommit)}action
}

pub(super) fn run(mut pipe:Pipe,mut clock:Clock,probe:Envelope)->Result<()> {
    if !matches!(probe.kind.as_str(),"probeExporters"|"probeExportersRevocation")||probe.payload!=json!({}) {
        return Err(Error::InvalidWire)
    }
    let revoking=probe.kind=="probeExportersRevocation";
    let catalog=if revoking{None}else{
        // Advertise only an actually prepared environment. This prerequisite
        // proves no permission; real Actor admission remains mandatory later.
        // Revocation deliberately needs neither these caps nor the artifact.
        runtime::environment()?;
        Some(artifact::Catalog::load()?)
    };
    let config_raw=root_file(Path::new(CONFIG))?;
    let config:Config=serde_json::from_slice(&config_raw).map_err(|_|Error::InvalidWire)?;
    if config.format!="cc-agent-enroll-config/v1"{return Err(Error::InvalidWire)}
    let paths=[&config.node_dir,&config.db_dir,&config.anchor_dir];
    for(i,path)in paths.iter().enumerate() {
        crate::durable::absolute(path)?;crate::durable::secure_path(path)?;
        if paths.iter().enumerate().any(|(j,other)|i!=j&&(path.starts_with(other)||other.starts_with(path)))
            ||config.owner_pin.starts_with(path)||path.starts_with(runtime::ROOT)||Path::new(runtime::ROOT).starts_with(path)
            ||path.starts_with("/opt/consolecrypt-agent-managed-exporter")
            ||Path::new("/opt/consolecrypt-agent-managed-exporter").starts_with(path) {return Err(Error::Storage)}
    }
    if catalog.as_ref().is_some_and(|c|c.policy.agent_id!=config.agent_id){return Err(Error::WrongClaim)}
    let pin_raw=root_file(&config.owner_pin)?;
    let genesis:OwnerPin=serde_json::from_slice(&pin_raw).map_err(|_|Error::InvalidWire)?;
    let store=SqliteJournalStore::open(&config.db_dir,&config.anchor_dir,genesis,SqliteStorageBudget::default())?;
    let store=Actor::new(store)?.into_store();let snapshot=store.load()?;
    let node=inspect_existing_node_identity(&config.node_dir,config.agent_id)?;
    let mut session=Session{store:Some(store),node_dir:config.node_dir,agent:config.agent_id,node,phase:Phase::New,
        seen:BTreeSet::new(),expected_pin:snapshot.owner().clone(),expected_rev:snapshot.revision(),
        claim:None,certificate:None,offer_expires:None,effects:false,catalog};
    clock.cap(session.expected_pin.expires_at)?;
    let mut first=Some(probe);
    for _ in 0..4 {
        config_unchanged(&config_raw,&config.owner_pin,&pin_raw)?;
        let frame=match first.take(){Some(v)=>v,None=>parse(&pipe.read(&mut clock)?)?};
        config_unchanged(&config_raw,&config.owner_pin,&pin_raw)?;
        if session.phase==Phase::Probed&&frame.kind=="startExporter" {
            return manage(session,pipe,clock,frame,config_raw,config.owner_pin,pin_raw)
        }
        let id=frame.request_id.clone();
        match session.request(frame,&mut clock) {
            Ok(reply)=>{config_unchanged(&config_raw,&config.owner_pin,&pin_raw)?;session.checked(clock.now()?)?;
                pipe.write(&reply,&mut clock)?;if session.phase==Phase::Closed{return Ok(())}},
            Err(error)=>{
                let code=match error{Error::Expired|Error::InvalidTime=>"expired",Error::Conflict=>"contextChanged",
                    Error::Storage=>"storageUnavailable",Error::UncertainCommit=>"uncertainCommit",
                    Error::Quota=>"capacity",Error::InvalidWire=>"invalidFrame",Error::WrongState=>"wrongState",_=>"denied"};
                let _=pipe.write(&json!({"format":FORMAT,"requestId":id,"kind":"error",
                    "payload":{"code":code,"effect":if session.effects{"unknown"}else{"notCommitted"}}}),&mut clock);
                return Err(error)
            }
        }
    }
    Err(Error::WrongState)
}
