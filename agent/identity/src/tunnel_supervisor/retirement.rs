//! Private SAME-parent physical ownership observation, never child authority.
use super::*;

pub(super) struct BoundLossRequest { _private: () }
pub(super) struct TargetRetired { _private: () }
pub(super) struct ChildRetired { _private: () }

pub(super) fn loss_request(value:&Value,pending:&Signed<AgentRequest>,policy_hash:Hash,
    peer:&Value,fixed:&Fixed,prepared:Option<&PreparedTransport>,control:&ParentControl)->Result<BoundLossRequest> {
    if prepared.is_none() || !fixed.policy.resume_original || control.flag.load(Ordering::SeqCst)!=0 {
        return Err(Error::WrongState)
    }
    verify_echo(value,pending,policy_hash,&fixed.policy.route.client_device_id,peer)?;
    Ok(BoundLossRequest{_private:()})
}

pub(super) fn target(target:&mut Option<TcpStream>)->Result<TargetRetired> {
    let socket=target.take().ok_or(Error::WrongState)?;
    let result=socket.shutdown(Shutdown::Both).map_err(|_|Error::Storage);
    drop(socket); // Drop the exact unique stream before producing observation.
    result?;
    Ok(TargetRetired{_private:()})
}

pub(super) fn child(child:&mut Child,end:Instant)->Result<ChildRetired> {
    // Kill/try_wait success alone is insufficient. Real exact Child::wait must
    // return successful exit after the requested child/carrier cleanup path.
    if !wait_child(child,end)? {return Err(Error::WrongState)}
    Ok(ChildRetired{_private:()})
}

pub(super) struct OwnedRetirement {
    _request:BoundLossRequest,
    _target:TargetRetired,
    _child:ChildRetired,
    _pipes:pipe::PipesRetired,
    _control:pipe::ControlRetired,
}
impl OwnedRetirement {
    pub(super) fn observe(request:BoundLossRequest,target:TargetRetired,child:ChildRetired,
        pipes:pipe::PipesRetired,control:pipe::ControlRetired)->Self {
        Self{_request:request,_target:target,_child:child,_pipes:pipes,_control:control}
    }
    pub(super) fn commit<S:CompareAndSwap>(self,actor:&mut Actor<S>,permit:Permit,
        fixed:&Fixed,installed:&InstalledChild,clock:&mut Clock)->Result<()> {
        // Pins/selected Node/cert/checkpoint/absolute+idle deadlines are reread
        // AFTER every physical owner joined, immediately before original CAS.
        check_live(actor,&permit,fixed,installed,clock)?;
        actor.retire_tunnel_after_owned_drain(permit,clock.now()?)
    }
}

pub(super) fn emit<S:CompareAndSwap>(actor:&mut Actor<S>,fixed:&Fixed,installed:&InstalledChild,
    start:&PreparedTransport,clock:&mut Clock)->Result<()> {
    fixed.check()?;installed.check()?;
    actor.preview_original_tunnel_resume_binding(&start.binding,&fixed.policy,clock.now()?)?;
    let node=inspect_existing_node_identity(&fixed.node,fixed.policy.route.agent_id)?;
    let value=json!({"format":"cc-tunnel-supervised-result/v1","outcome":"transportRetired",
        "cleanupObserved":true,"drainRecorded":true,"leaseHash":start.binding.lease.body.digest()?,
        "policyHash":fixed.policy.digest()?,"node":node,"expiresAt":start.binding.lease.body.expires_at});
    let raw=pipe::canonical(&value)?;
    if raw.len()>4096 {return Err(Error::Quota)}
    std::io::stdout().lock().write_all(&raw).map_err(|_|Error::Storage)
}
