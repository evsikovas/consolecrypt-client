//! Closed display projections. No stored row becomes a signer, grant or route.
use super::*;
pub(super) fn utc(t: Timestamp) -> AppResult<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(
        t.seconds().try_into().map_err(|_| unavailable())?,
        0,
    )
    .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    .ok_or_else(unavailable)
}
fn id<T: serde::Serialize>(v: &T) -> AppResult<String> {
    json(v)?.as_str().map(str::to_owned).ok_or_else(unavailable)
}
pub(super) fn node(value: &protocol::NodeIdentity) -> Value {
    json!({"agentId":value.agent_id,"keyEpoch":value.key_epoch.to_string(),"publicKey":value.public_key,"keyFingerprint":value.key_fingerprint})
}
pub(super) fn node_context(value: &cc_agent_node_wire::Context) -> AppResult<Value> {
    let (kind, owner) = match &value.tenant {
        Tenant::LocalProfile { id: value } => ("localProfile", id(value)?),
        Tenant::PersonalAccount { id: value } => ("personalAccount", id(value)?),
        Tenant::Team { .. } => return Err(unavailable()),
    };
    Ok(json!({"kind":kind,"id":owner,"deploymentId":value.deployment_id}))
}
pub(super) fn view(context: &Context, host: ObjectId, revision: i64, owner: Value) -> Value {
    view_for(context,host,revision,owner,AssociationPurpose::Tunnel)
}
pub(super) fn view_for(context:&Context,host:ObjectId,revision:i64,owner:Value,purpose:AssociationPurpose)->Value {
    let mut view=json!({"purpose":purpose.label(),"scope":purpose.scope_name(),"scopes":[purpose.scope_name()],
        "scopeSource":"explicitFactoryRequest","advertisedScopes":null,"collector":null,"version":1,"profileId":context.frame.profile_id,"notificationScopeId":context.frame.notification_scope_id,
        "phase":"idle","hostId":host,"hostRevision":revision.to_string(),"remainingMs":0,
        "node":null,"nodeContext":null,"owner":owner,"ownerEpoch":null,"checkpoint":null,"actorRevision":null,
        "clientIdentity":null,"associationId":null,"claimDigest":null,"certificateDigest":null,
        "maxCertificateLifetimeSeconds":0,"certificateExpiresUtc":null,"actorAdmission":false,"cleanupState":"notStarted"});
    if purpose==AssociationPurpose::Exporters {view["exporter"]=Value::Null;}
    view
}
pub(super) fn offer(view: &mut Value, r: &NodeAssociationReview) -> AppResult<()> {
    r.live()?;
    view["phase"] = json!("offerReview");
    view["purpose"]=json!(r.purpose.label());view["scope"]=json!(r.purpose.scope_name());view["scopes"]=json!([r.purpose.scope_name()]);
    view["collector"]=json!(r.metrics.as_ref().map(|m|m.public()));
    if r.purpose==AssociationPurpose::Metrics {view["advertisedScopes"]=json!(["metrics.collect"]);view["scopeSource"]=json!("actualAdvertisedIntersection");}
    if r.purpose==AssociationPurpose::Exporters {
        view["exporter"]=json!(r.exporter.as_ref().map(|e|e.public()));
        view["advertisedScopes"]=json!(["exporters.manage"]);
        view["scopeSource"]=json!("actualAdvertisedIntersection");
    }
    view["node"] = node(&r.probe.node);
    view["nodeContext"] = node_context(&r.probe.owner_pin.context)?;
    view["ownerEpoch"] = json!(r.probe.owner_pin.epoch.to_string());
    view["checkpoint"] = json!(r.probe.owner_pin.checkpoint.to_string());
    view["actorRevision"] = json!(r.probe.actor_revision.to_string());
    view["clientIdentity"] =
        json!({"identityId":r.tls.identity_id,"certificateSha256":r.tls.certificate_sha256});
    Ok(())
}
pub(super) fn max_lifetime(r: &NodeClaimReview) -> AppResult<u32> {
    r.original.live()?;
    let expiry = r
        .original
        .probe
        .owner_pin
        .expires_at
        .min(super::super::client_expiry(&r.original.tls)?);
    let seconds = r
        .original
        .scope
        .remaining_until(expiry)?
        .as_secs()
        .min(CERTIFICATE_TTL);
    if seconds == 0 {
        return Err(unavailable());
    }
    seconds.try_into().map_err(|_| unavailable())
}
pub(super) fn claim(view: &mut Value, r: &NodeClaimReview) -> AppResult<()> {
    protocol::claimed_for(
        r.original.purpose,&r.original.probe,
        &r.offer,
        &r.claim,
        r.original.scope.now()?,
    )?;
    view["phase"] = json!("claimReview");
    view["associationId"] = json!(r.association_id);
    view["claimDigest"] = json!(r.claim.claim_digest);
    view["checkpoint"] = json!(r.claim.owner_pin.checkpoint.to_string());
    view["actorRevision"] = json!(r.claim.actor_revision.to_string());
    view["maxCertificateLifetimeSeconds"] = json!(max_lifetime(r)?);
    Ok(())
}
pub(super) fn associated(view: &mut Value, r: &AssociatedNode) -> AppResult<()> {
    r.scope.bind_owner_pin(&r.pin)?;
    verify_statement(&r.certificate, r.pin.key).map_err(|_| unavailable())?;
    r.purpose.check_scopes(&r.certificate.body.scopes)?;
    view["purpose"]=json!(r.purpose.label());view["scope"]=json!(r.purpose.scope_name());view["scopes"]=json!([r.purpose.scope_name()]);
    view["phase"] = json!("associated");
    view["associationId"] = json!(r.association_id);
    view["certificateDigest"] = json!(r.certificate.body.digest().map_err(|_| unavailable())?);
    view["certificateExpiresUtc"] = json!(utc(r.certificate.body.expires_at)?);
    view["checkpoint"] = json!(r.pin.checkpoint.to_string());
    view["actorRevision"] = json!(r.actor_revision.to_string());
    view["maxCertificateLifetimeSeconds"] = json!(0);
    Ok(())
}
pub(super) fn association(
    r: &cc_storage_core::NodeAssociationRecord,
    key: PublicKey,
) -> AppResult<Value> {
    let p = &r.public;
    let offer =
        serde_json::from_slice::<Signed<Offer>>(&p.signed_offer_json).map_err(|_| unavailable())?;
    verify_statement(&offer, key).map_err(|_| unavailable())?;
    let purpose=match offer.body.scopes.0.as_slice(){[Scope::TunnelOpen]=>AssociationPurpose::Tunnel,
        [Scope::MetricsCollect]=>AssociationPurpose::Metrics,[Scope::ExportersManage]=>AssociationPurpose::Exporters,_=>return Err(unavailable())};
    if matches!(purpose,AssociationPurpose::Metrics|AssociationPurpose::Exporters)&&r.rotation.is_some(){return Err(unavailable())}
    if id(&offer.body.agent_id)? != p.agent_id
        || hex::encode(offer.body.agent_key.fingerprint().as_bytes()) != p.node_key_fingerprint
        || hex::encode(offer.body.digest().map_err(|_| unavailable())?.as_bytes()) != p.offer_digest
        || offer.body.header.owner_epoch != p.owner_epoch
        || offer.body.header.checkpoint != p.checkpoint.checked_add(1).ok_or_else(unavailable)?
    {
        return Err(unavailable());
    }
    let (mut digest, mut expiry) = if let Some(c) = &p.certificate {
        let cert = serde_json::from_slice::<Signed<Certificate>>(&c.signed_certificate_json)
            .map_err(|_| unavailable())?;
        verify_statement(&cert, key).map_err(|_| unavailable())?;
        purpose.check_scopes(&cert.body.scopes)?;purpose.check_scopes(&cert.body.capabilities.scopes)?;
        if cert.body.header.context != offer.body.header.context
            || cert.body.header.owner_epoch != p.owner_epoch
            || cert.body.header.checkpoint != p.checkpoint.checked_add(2).ok_or_else(unavailable)?
            || cert.body.agent_id != offer.body.agent_id
            || cert.body.agent_key != offer.body.agent_key
            || cert.body.key_epoch != p.key_epoch
            || hex::encode(cert.body.digest().map_err(|_| unavailable())?.as_bytes())
                != c.certificate_digest
        {
            return Err(unavailable());
        }
        (
            Some(c.certificate_digest.clone()),
            Some(utc(cert.body.expires_at)?),
        )
    } else {
        (None, None)
    };
    let current=if p.certificate.is_some(){Some(ledger::current_certificate(r,key)?)}else{None};
    let current_pin=if p.certificate.is_some(){Some(ledger::current_pin(r,key)?)}else{None};
    let fingerprint=current.as_ref().map(|c|hex::encode(c.body.agent_key.fingerprint().as_bytes()))
        .unwrap_or_else(||p.node_key_fingerprint.clone());
    let key_epoch=current.as_ref().map_or(p.key_epoch,|c|c.body.key_epoch);
    if let Some(current)=&current {digest=Some(hex::encode(current.body.digest().map_err(|_|unavailable())?.as_bytes()));
        expiry=Some(utc(current.body.expires_at)?);}
    let observed_revoked = r.phase == cc_storage_core::NodeAssociationPhase::Revoked;
    if let Some(intent) = &r.revocation {
        let revoke: Signed<Revocation> = serde_json::from_slice(&intent.signed_revocation_json).map_err(|_| unavailable())?;
        let pin: OwnerPin = serde_json::from_slice(&intent.committed_owner_pin_json).map_err(|_| unavailable())?;
        let certificate = p.certificate.as_ref().ok_or_else(unavailable)?;
        let minimum_revision=r.rotation.as_ref().filter(|r|r.selected_observed)
            .and_then(|r|r.observed_actor_revision).unwrap_or(certificate.expected_actor_revision);
        let cert=current.as_ref().ok_or_else(unavailable)?;
        let previous_pin=current_pin.as_ref().ok_or_else(unavailable)?.clone();
        verify_statement(&revoke, key).map_err(|_| unavailable())?;
        let mut expected_pin = previous_pin.clone(); expected_pin.checkpoint = revoke.body.header.checkpoint;
        if revoke.body.agent_id != cert.body.agent_id || revoke.body.key_epoch != cert.body.key_epoch
            || revoke.body.certificate_hash != cert.body.digest().map_err(|_| unavailable())?
            || revoke.body.header.context != cert.body.header.context
            || revoke.body.header.owner_epoch != cert.body.header.owner_epoch
            || revoke.body.header.checkpoint <= cert.body.header.checkpoint
            || pin != expected_pin || pin.key != key
            || intent.expected_actor_revision <= minimum_revision {
            return Err(unavailable());
        }
    } else if observed_revoked { return Err(unavailable()); }
    let phase = match r.phase {
        cc_storage_core::NodeAssociationPhase::Pending => "pending",
        cc_storage_core::NodeAssociationPhase::Unknown => "unknown",
        cc_storage_core::NodeAssociationPhase::Associated => "associated",
        cc_storage_core::NodeAssociationPhase::Blocked => "blocked",
        cc_storage_core::NodeAssociationPhase::Revoked => "revoked",
    };
    if phase == "associated" && digest.is_none() {
        return Err(unavailable());
    }
    Ok(
        json!({"purpose":purpose.label(),"scope":purpose.scope_name(),"scopes":[purpose.scope_name()],"associationId":p.association_id,"hostId":p.saved_host_id,"hostRevision":p.saved_host_revision.to_string(),"agentId":p.agent_id,
        "nodeKeyFingerprint":fingerprint,"keyEpoch":key_epoch.to_string(),"certificateDigest":digest,"certificateExpiresUtc":expiry,"state":phase,"remoteRevocationObserved":observed_revoked,
        "rotationState":if r.rotation.as_ref().is_some_and(|r|r.selected_observed){"selectedObserved"}else if r.rotation.is_some(){"unknown"}else{"notRequested"},
        "revocationState":if observed_revoked {"observedRevoked"} else if r.revocation.is_some() {"unknown"} else {"notRequested"}}),
    )
}
