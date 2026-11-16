//! LocalOnly destination intent, never a grant, automatic start or renewal.
use super::*;
use cc_storage_core::{NodeAssociationOwner, NodeAssociationPhase, NodeAssociationRecord,
    NodeTunnelDestination, Storage};

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct ReadRequest {
    version:u8, profile_id:String, notification_scope_id:String,
    association_id:String, destination_ip:String, destination_port:u16,
}
fn destination(id:Uuid,ip:String,port:u16)->AppResult<NodeTunnelDestination> {
    if id.is_nil() || !matches!(ip.as_str(),"127.0.0.1"|"::1") || port==0 {
        return Err(unavailable())
    }
    Ok(NodeTunnelDestination{association_id:id,destination_ip:ip,destination_port:port})
}
pub(super) async fn original(context:&Context,id:Uuid)->AppResult<(Storage,NodeAssociationOwner,NodeAssociationRecord)> {
    let ledger=ledger::Ledger::capture_original(context.scope.clone()).await?;
    let snapshot=ledger.snapshot_original().await?;
    let row=snapshot.records().iter().find(|r|r.public.association_id==id
        && r.phase==NodeAssociationPhase::Associated && r.revocation.is_none())
        .cloned().ok_or_else(unavailable)?;
    let key=context.scope.with_current_resources(|_,_,id|Ok(PublicKey::from_bytes(id.public_keys().signing)))?;
    let public=data::association(&row,key)?;
    if public["purpose"]!="tunnel" {return Err(unavailable())}
    context.validate_host(row.public.saved_host_id,row.public.saved_host_revision).await?;
    let owner=ledger.audit_owner()?;
    let storage=context.scope.with_current_resources(|session,_,_|Ok(session.storage()?))?;
    context.scope.check_current()?;
    Ok((storage,owner,row))
}
fn same_row(tx:&cc_storage_core::Tx<'_>,owner:&NodeAssociationOwner,expected:&NodeAssociationRecord)->AppResult<()> {
    let snapshot=tx.node_association_snapshot(owner)?;
    if snapshot.records().iter().find(|r|r.public.association_id==expected.public.association_id)!=Some(expected) {
        return Err(unavailable())
    }
    Ok(())
}
pub(super) async fn save(context:&Context,prepared:&producer::Prepared)->AppResult<()> {
    let id=prepared.association;
    let route=&prepared.policy.route.destination;
    if route.protocol!="tcp" {return Err(unavailable())}
    let target=destination(id,route.ip.clone(),route.port)?;
    let(storage,owner,expected)=original(context,id).await?;
    let key=context.scope.with_current_resources(|_,_,id|Ok(PublicKey::from_bytes(id.public_keys().signing)))?;
    let certificate=ledger::current_certificate(&expected,key)?;
    if expected.public.saved_host_id!=prepared.host || expected.public.saved_host_revision!=prepared.host_revision
        || expected.public.route_trust_digest!=prepared.route_digest()
        || certificate.body.agent_id!=prepared.policy.route.agent_id
        || certificate.body.key_epoch!=prepared.policy.route.key_epoch
        || certificate.body.digest().map_err(|_|unavailable())?!=prepared.policy.route.agent_certificate_hash {
        return Err(unavailable())
    }
    let enabled=prepared.policy.resume_original;
    let scope=context.scope.clone();
    storage.write_node_association(move|tx|scope.with_io_current(|_| {
        same_row(tx,&owner,&expected)?;
        tx.node_tunnel_set_resume_preference(&owner,&target,enabled).map_err(AppError::from)
    })).await.map_err(|_|unknown())?;
    context.scope.check_current()
}

impl AppCore {
    /// Read only the original encrypted profile. No SSH or Relay is opened.
    pub async fn agent_tunnel_resume_preference_json(self,raw:&str)->AppResult<String> {
        let request:ReadRequest=parse(raw).map_err(|_|unavailable())?;
        let id=Uuid::parse_str(&request.association_id).map_err(|_|unavailable())?;
        if id.to_string()!=request.association_id {return Err(unavailable())}
        let target=destination(id,request.destination_ip,request.destination_port)?;
        let context=Context::capture(&self,Frame{version:request.version,profile_id:request.profile_id,
            notification_scope_id:request.notification_scope_id}).await?;
        drop(self);
        let scope=context.scope.clone();
        bounded_original(&scope,async move {
            let(storage,owner,expected)=original(&context,id).await?;
            let original=context.scope.clone();
            let selected=NodeTunnelDestination{association_id:target.association_id,
                destination_ip:target.destination_ip.clone(),destination_port:target.destination_port};
            let enabled=storage.read(move|tx|original.with_io_current(|_| {
                same_row(tx,&owner,&expected)?;
                tx.node_tunnel_resume_preference(&owner,&selected).map_err(AppError::from)
            })).await?;
            context.check().await?;
            context.publish(json!({"associationId":id,"destination":{"protocol":"tcp",
                "ip":target.destination_ip,"port":target.destination_port},"resumeOriginal":enabled}),4096)
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preference_destination_has_no_alias_wildcard_protocol_or_scalar_authority() {
        let id=Uuid::new_v4();
        for ip in ["127.0.0.1","::1"] {destination(id,ip.into(),22).unwrap();}
        for ip in ["localhost","0.0.0.0","::","::ffff:127.0.0.1","127.0.0.1%0","10.0.0.1"] {
            assert!(destination(id,ip.into(),22).is_err());
        }
        assert!(destination(id,"127.0.0.1".into(),0).is_err());
        assert!(destination(Uuid::nil(),"127.0.0.1".into(),22).is_err());
        let raw=json!({"version":1,"profileId":"profile","notificationScopeId":"notice",
            "associationId":id,"destinationIp":"127.0.0.1","destinationPort":22});
        for key in ["resumeOriginal","approved","ownerPin","lease","protocol","url","path"] {
            let mut bad=raw.clone();bad[key]=json!(true);
            assert!(serde_json::from_value::<ReadRequest>(bad).is_err());
        }
    }
}
