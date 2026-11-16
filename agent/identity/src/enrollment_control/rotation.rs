//! Fixed TunnelOnly rotation control. No caller paths/clock/key material.
use super::*;
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Expected { expected_rev:u64, expected_pin:OwnerPin, certificate_digest:Hash }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Prove { expected_rev:u64, expected_pin:OwnerPin, rotation_id:String,
    previous_certificate:Signed<Certificate>, certificate:Signed<Certificate> }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Apply { expected_rev:u64, expected_pin:OwnerPin, rotation_id:String, certificate_digest:Hash }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Reconcile { expected_rev:u64, expected_pin:OwnerPin, rotation_id:String,
    previous_certificate:Signed<Certificate>, proofs:PreparedRotationProofs }
#[derive(Clone,PartialEq,Eq)]
struct Local {
    old:PublicNodeIdentity, prepared:Option<PreparedNodeKeyPublic>,
    selected:Option<PublicNodeIdentity>, phase:&'static str,
}
fn local(directory:&Path,agent:AgentId)->Result<Local> {
    // The public inspection guard validates the CLOSED whole private layout.
    let inspected=crate::inspect_node_state(directory,agent)?;
    inspected.recheck_guard_identity()?;
    let guard=crate::node_prepare::inspection_read_lock(directory)?;
    let old=crate::provision::load_existing_under_guard(&guard,agent)?;
    let prepared=crate::node_prepare::check(&guard,Some(&old))?;
    let phase=match inspected.facts().selection_state {
        crate::SelectionState::Legacy1=>"legacy1",
        crate::SelectionState::Prepared1=>"prepared1",
        crate::SelectionState::ActivationPending=>"activationPending",
        crate::SelectionState::Selected2=>"selected2",
    };
    let selected=if phase=="selected2" {
        let value=crate::node_activation::selected(&guard,&old)?.ok_or(Error::UncertainCommit)?;
        Some(PublicNodeIdentity {format:3,agent_id:agent,key_epoch:2,
            public_key:value.prepared.new_public_key,key_fingerprint:value.prepared.new_key_fingerprint})
    }else if phase=="activationPending" {None}else{Some(old.clone())};
    crate::node_prepare::guard_inode(&guard)?;inspected.recheck_guard_identity()?;
    Ok(Local {old,prepared,selected,phase})
}
fn tunnel(cert:&Certificate)->Result<()> {
    cert.validate()?;
    if cert.scopes.0.as_slice()!=[Scope::TunnelOpen] || cert.capabilities.version!=1
        || cert.capabilities.scopes!=cert.scopes {return Err(Error::CapabilityDenied)}
    Ok(())
}
struct Session {
    store:Option<SqliteJournalStore>, node_dir:PathBuf, agent:AgentId,
    pin:OwnerPin, revision:u64, original:Local, current:Certificate,
    prepared:Option<PreparedNodeKeyPublic>, previous:Option<Certificate>, proofs:Option<PreparedRotationProofs>,
    mode_reconcile:bool, stage:u8, seen:BTreeSet<String>, effects:bool,
}
impl Session {
    fn checked(&self,clock:&mut Clock)->Result<State> {
        let state=self.store.as_ref().ok_or(Error::WrongState)?.load()?;
        if state.owner()!=&self.pin || state.revision()!=self.revision {return Err(Error::Conflict)}
        let now=clock.now()?;
        if state.revoked(self.agent) || state.certificate(self.agent)!=Some(&self.current) {return Err(Error::WrongClaim)}
        crate::wire::live(self.current.not_before,self.current.expires_at,now)?;
        tunnel(&self.current)?;
        if local(&self.node_dir,self.agent)?.old!=self.original.old {return Err(Error::WrongClaim)}
        Ok(state)
    }
    fn expect(&self,rev:u64,pin:&OwnerPin)->Result<()> {
        if rev!=self.revision || pin!=&self.pin {return Err(Error::Conflict)}Ok(())
    }
    fn observed(&self,clock:&mut Clock)->Result<Value> {
        let state=self.checked(clock)?;let observed=local(&self.node_dir,self.agent)?;
        let prepared=self.prepared.as_ref().ok_or(Error::WrongState)?;
        if observed.phase!="selected2" || observed.prepared.as_ref()!=Some(prepared) {return Err(Error::UncertainCommit)}
        let node=observed.selected.ok_or(Error::UncertainCommit)?;
        if node.public_key!=self.current.agent_key || node.key_epoch!=self.current.key_epoch
            || node.key_fingerprint!=prepared.new_key_fingerprint {return Err(Error::WrongClaim)}
        self.checked(clock)?;
        Ok(json!({"node":node,"ownerPin":state.owner(),"actorRevision":state.revision(),
            "certificate":self.current,"certificateDigest":self.current.digest()?,"revoked":false,
            "rotationId":prepared.rotation_id,"selectionState":"selected2","requiredReaderSchema":3}))
    }
    fn activate(&mut self,previous:Certificate,clock:&mut Clock)->Result<Value> {
        self.checked(clock)?;
        let prepared=self.prepared.as_ref().ok_or(Error::WrongState)?.clone();
        let store=self.store.take().ok_or(Error::WrongState)?;
        let mut actor=Actor::new(store)?;
        // Only this concrete Actor + held private directory can produce the
        // activation acknowledgement; no caller/Native boolean is accepted.
        let result=activate_prepared_tunnel_rotation(&self.node_dir,&mut actor,self.agent,&prepared.rotation_id,
            crate::LocalActivationInput {previous_certificate:previous,committed_certificate:self.current.clone()});
        self.store=Some(actor.into_store());result?;
        self.checked(clock)?;
        self.observed(clock)
    }
    fn request(&mut self,frame:Envelope,clock:&mut Clock)->Result<Value> {
        let max=if self.mode_reconcile {3}else{5};
        if self.seen.len()>=max || !self.seen.insert(frame.request_id.clone()) {return Err(Error::Replay)}
        self.checked(clock)?;
        let payload=match (self.mode_reconcile,self.stage,frame.kind.as_str()) {
            (false,0,"probeRotation") | (true,0,"probeRotationReconcile")=>{
                let _:Empty=decode(frame.payload)?;let observed=local(&self.node_dir,self.agent)?;
                self.stage=1;
                json!({"ownerPin":self.pin,"actorRevision":self.revision,"certificate":self.current,
                    "revoked":false,"oldNode":observed.old,"node":observed.selected,
                    "candidate":observed.prepared,"selectionState":observed.phase})
            }
            (false,1,"prepareRotationCandidate")=>{
                let input:Expected=decode(frame.payload)?;self.expect(input.expected_rev,&input.expected_pin)?;
                if input.certificate_digest!=self.current.digest()? || self.current.key_epoch!=1
                    || self.original.old.public_key!=self.current.agent_key {return Err(Error::WrongClaim)}
                self.effects=true;
                let prepared=prepare_node_rotation(&self.node_dir,self.agent,self.original.old.key_fingerprint)?;
                self.checked(clock)?;
                if prepared.from_key_epoch!=1 || prepared.proposed_key_epoch!=2 {return Err(Error::WrongEpoch)}
                self.prepared=Some(prepared.clone());self.stage=2;
                json!({"candidate":prepared,"ownerPin":self.pin,"actorRevision":self.revision,
                    "certificateDigest":self.current.digest()?,"selectedKeyEpoch":1})
            }
            (false,2,"proveRotation")=>{
                let input:Prove=decode(frame.payload)?;self.expect(input.expected_rev,&input.expected_pin)?;
                let prepared=self.prepared.as_ref().ok_or(Error::WrongState)?;
                if input.rotation_id!=prepared.rotation_id || input.previous_certificate.body!=self.current {
                    return Err(Error::WrongClaim)
                }
                let result=sign_prepared_tunnel_node_rotation(&self.node_dir,self.agent,&input.rotation_id,&self.pin,
                    PreparedRotationInput {previous_certificate:input.previous_certificate,certificate:input.certificate},clock.now()?)?;
                self.checked(clock)?;self.previous=Some(self.current.clone());self.proofs=Some(result.clone());self.stage=3;
                serde_json::to_value(result).map_err(|_|Error::InvalidWire)?
            }
            (false,3,"applyRotation")=>{
                let input:Apply=decode(frame.payload)?;self.expect(input.expected_rev,&input.expected_pin)?;
                let prepared=self.prepared.as_ref().ok_or(Error::WrongState)?;
                let proofs=self.proofs.as_ref().ok_or(Error::WrongState)?.clone();
                if input.rotation_id!=prepared.rotation_id || input.certificate_digest!=proofs.certificate.body.digest()? {
                    return Err(Error::WrongClaim)
                }
                self.effects=true;
                let store=self.store.take().ok_or(Error::WrongState)?;
                let mut actor=Actor::new(RevisionFence {inner:store,expected:self.revision})?;
                let result=actor.rotate(proofs.certificate.clone(),proofs.old,proofs.new,clock.now()?);
                self.store=Some(actor.into_store().inner);result?;
                self.revision=self.revision.checked_add(1).ok_or(Error::Quota)?;
                self.pin.checkpoint=self.pin.checkpoint.checked_add(1).ok_or(Error::Quota)?;
                self.current=proofs.certificate.body;clock.cap(self.current.expires_at)?;
                let previous=self.previous.clone().ok_or(Error::WrongState)?;
                let observed=self.activate(previous,clock)?;self.stage=4;observed
            }
            (true,1,"reconcileRotation")=>{
                let input:Reconcile=decode(frame.payload)?;self.expect(input.expected_rev,&input.expected_pin)?;
                let candidate=self.prepared.as_ref().ok_or(Error::WrongState)?;
                let proof=&input.proofs;
                if input.rotation_id!=candidate.rotation_id || proof.certificate.body!=self.current
                    || proof.old.body!=proof.new.body.0 || proof.old.body.certificate_hash!=self.current.digest()?
                    || proof.old.body.header!=self.current.header || proof.old.body.agent_id!=self.agent {
                    return Err(Error::WrongClaim)
                }
                input.previous_certificate.verify(self.pin.key)?;proof.certificate.verify(self.pin.key)?;
                proof.old.verify(input.previous_certificate.body.agent_key)?;proof.new.verify(self.current.agent_key)?;
                if self.current.origin!=(CertificateOrigin::Rotation {
                    previous_certificate:input.previous_certificate.body.digest()?,previous_key_epoch:1}) {
                    return Err(Error::WrongClaim)
                }
                // Historical accepted proofs only. No signing/recent() refresh,
                // Actor.rotate, entropy, proposal or certificate renewal here.
                self.effects=true;
                let observed=self.activate(input.previous_certificate.body,clock)?;self.stage=4;observed
            }
            (_,4,"observeRotation")=>{
                let input:Apply=decode(frame.payload)?;self.expect(input.expected_rev,&input.expected_pin)?;
                if input.rotation_id!=self.prepared.as_ref().ok_or(Error::WrongState)?.rotation_id
                    || input.certificate_digest!=self.current.digest()? {return Err(Error::WrongClaim)}
                let observed=self.observed(clock)?;self.stage=5;observed
            }
            _=>return Err(Error::WrongState),
        };
        self.checked(clock)?;
        Ok(json!({"format":FORMAT,"requestId":frame.request_id,"kind":frame.kind,"payload":payload}))
    }
}
pub(super) fn run(mut pipe:Pipe,mut clock:Clock,first:Envelope)->Result<()> {
    let reconcile=first.kind=="probeRotationReconcile";
    let config_raw=root_file(Path::new(CONFIG))?;
    let config:Config=serde_json::from_slice(&config_raw).map_err(|_|Error::InvalidWire)?;
    if config.format!="cc-agent-enroll-config/v1" {return Err(Error::InvalidWire)}
    let paths=[&config.node_dir,&config.db_dir,&config.anchor_dir];
    for(i,p)in paths.iter().enumerate(){
        crate::durable::absolute(p)?;crate::durable::secure_path(p)?;
        if paths.iter().enumerate().any(|(j,q)|i!=j&&(p.starts_with(q)||q.starts_with(p)))
            || config.owner_pin.starts_with(p){return Err(Error::Storage)}
    }
    let pin_raw=root_file(&config.owner_pin)?;
    let genesis:OwnerPin=serde_json::from_slice(&pin_raw).map_err(|_|Error::InvalidWire)?;
    let store=SqliteJournalStore::open(&config.db_dir,&config.anchor_dir,genesis,SqliteStorageBudget::default())?;
    let store=Actor::new(store)?.into_store();let state=store.load()?;
    let current=state.certificate(config.agent_id).ok_or(Error::NotFound)?.clone();tunnel(&current)?;
    if state.revoked(config.agent_id) || current.header.owner_epoch!=state.owner().epoch {return Err(Error::Revoked)}
    clock.cap(state.owner().expires_at)?;clock.cap(current.expires_at)?;
    let original=local(&config.node_dir,config.agent_id)?;
    if current.agent_id!=original.old.agent_id || if reconcile {current.key_epoch!=2}else{current.key_epoch!=1} {
        return Err(Error::WrongEpoch)
    }
    if !reconcile && (current.agent_key!=original.old.public_key || original.phase=="activationPending" || original.phase=="selected2") {
        return Err(Error::WrongClaim)
    }
    let mut session=Session {store:Some(store),node_dir:config.node_dir.clone(),agent:config.agent_id,pin:state.owner().clone(),
        revision:state.revision(),prepared:original.prepared.clone(),original,current,previous:None,proofs:None,
        mode_reconcile:reconcile,stage:0,seen:BTreeSet::new(),effects:false};
    let mut first=Some(first);
    for _ in 0..(if reconcile {3}else{5}) {
        if root_file(Path::new(CONFIG))?!=config_raw || root_file(&config.owner_pin)?!=pin_raw {return Err(Error::Storage)}
        let frame=match first.take(){Some(v)=>v,None=>parse(&pipe.read(&mut clock)?)?};
        if root_file(Path::new(CONFIG))?!=config_raw || root_file(&config.owner_pin)?!=pin_raw {return Err(Error::Storage)}
        let id=frame.request_id.clone();
        match session.request(frame,&mut clock){
            Ok(value)=>{pipe.write(&value,&mut clock)?;if session.stage==5 {return Ok(())}},
            Err(error)=>{
                let _=pipe.write(&json!({"format":FORMAT,"requestId":id,"kind":"error","payload":{
                    "code":if error==Error::Conflict {"contextChanged"}else{"denied"},
                    "effect":if session.effects {"unknown"}else{"notCommitted"}}}),&mut clock);
                return Err(error)
            }
        }
    }
    Err(Error::WrongState)
}
#[cfg(test)]
mod tests;
