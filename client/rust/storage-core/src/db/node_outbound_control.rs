//! Encrypted LocalOnly carrier locator. No bootstrap token, grant or authority.
use super::{Tx,NodeAssociationOwner,NodeAssociationRecord,NodeAssociationPhase};
use crate::{Result,StorageError};
use cc_protocol::{UserId,DeviceId,SessionId};
use rusqlite::{params,OptionalExtension};
use serde::{Serialize,Deserialize};
use uuid::Uuid;
const KEY:&str="cc.native.agent.outbound-control.v1";
const MAX_BYTES:usize=64*1024;
const MAX_ENTRIES:usize=8;
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct NodeOutboundControlReference {
    pub association_id:Uuid,pub control_id:Uuid,pub account_id:UserId,pub device_id:DeviceId,
    pub session_id:SessionId,pub server_url:String,pub descriptor_origin:String,pub expires_at:u64,
}

impl NodeOutboundControlReference {
    /// Full original URL identity only; this observation grants no transport.
    pub fn canonical_server_url_matches(raw_owner:&str,canonical_reference:&str)->bool {
        for raw in [raw_owner,canonical_reference] {
            if raw.is_empty() || raw.len()>2048 || raw.trim()!=raw
                || raw.chars().any(|c|c.is_control()||matches!(c,'@'|'?'|'#'|'\\')) {return false}
        }
        let (Ok(owner),Ok(reference))=(url::Url::parse(raw_owner),url::Url::parse(canonical_reference)) else {return false};
        owner.scheme()=="https" && reference.scheme()=="https" && owner.host().is_some()
            && owner.username().is_empty() && owner.password().is_none() && owner.query().is_none() && owner.fragment().is_none()
            && reference.username().is_empty() && reference.password().is_none() && reference.query().is_none() && reference.fragment().is_none()
            && reference.as_str()==canonical_reference && owner==reference
    }
}

#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {reference:NodeOutboundControlReference,association:NodeAssociationRecord}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct References {version:u8,owner:NodeAssociationOwner,entries:Vec<Entry>}
fn invalid()->StorageError {StorageError::Invalid("outbound control reference unavailable or changed".into())}
fn valid(reference:&NodeOutboundControlReference,owner:&NodeAssociationOwner)->Result<()> {
    let personal=owner.personal.as_ref().ok_or_else(invalid)?;
    // Preserve the exact raw owner record; compare the SAME full canonical URL.
    // Parsing this LocalOnly locator never constructs or admits a transport.
    let authority=reference.server_url.strip_prefix("https://").and_then(|tail|tail.split('/').next())
        .filter(|value|!value.is_empty()).ok_or_else(invalid)?;
    if reference.association_id.is_nil() || reference.control_id.is_nil() || reference.expires_at==0
        || reference.expires_at>253_402_300_799 || reference.account_id!=personal.user_id
        || reference.device_id!=owner.device_id || reference.server_url.len()>2048
        || reference.descriptor_origin.len()>2048 || !NodeOutboundControlReference::canonical_server_url_matches(&personal.server_url,&reference.server_url)
        || reference.server_url.chars().any(|c|c.is_control()||matches!(c,'@'|'?'|'#'|'\\'))
        || reference.descriptor_origin!=format!("wss://{authority}") {return Err(invalid())}
    Ok(())
}
impl Tx<'_> {
    fn outbound_raw(&self)->Result<Option<Vec<u8>>> {
        let found:Option<(i64,Option<Vec<u8>>)>=self.c().query_row(
            "SELECT length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=?2 THEN CAST(value AS BLOB) ELSE NULL END FROM local_settings WHERE key=?1",
            params![KEY,MAX_BYTES as i64],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        match found {None=>Ok(None),Some((n,Some(raw))) if n>0&&n as usize==raw.len()&&raw.len()<=MAX_BYTES=>Ok(Some(raw)),_=>Err(invalid())}
    }
    fn outbound_refs(&self,owner:&NodeAssociationOwner)->Result<References> {
        let Some(raw)=self.outbound_raw()? else{return Ok(References{version:1,owner:owner.clone(),entries:Vec::new()})};
        let refs:References=serde_json::from_value(super::node_tunnel_preference::unique(&raw)?).map_err(|_|invalid())?;
        if refs.version!=1 || refs.owner!=*owner || refs.entries.len()>MAX_ENTRIES {return Err(invalid())}
        let mut ids=std::collections::BTreeSet::new();let mut controls=std::collections::BTreeSet::new();
        for entry in &refs.entries {
            valid(&entry.reference,owner)?;
            if entry.reference.association_id!=entry.association.public.association_id
                || !ids.insert(entry.reference.association_id) || !controls.insert(entry.reference.control_id) {return Err(invalid())}
        }
        Ok(refs)
    }
    fn outbound_current(&self,owner:&NodeAssociationOwner,association:Uuid)->Result<NodeAssociationRecord> {
        let snapshot=self.node_association_snapshot(owner)?;
        let row=snapshot.records().iter().find(|r|r.public.association_id==association).ok_or_else(invalid)?;
        if row.phase!=NodeAssociationPhase::Associated || row.revocation.is_some() || row.public.certificate.is_none()
            || row.rotation.as_ref().is_some_and(|r|!r.selected_observed) {return Err(invalid())}
        Ok(row.clone())
    }
    /// Reads do not prune, renew, repair or adopt a changed association snapshot.
    pub fn node_outbound_control_reference(&self,owner:&NodeAssociationOwner,association:Uuid)
        ->Result<Option<NodeOutboundControlReference>> {
        let current=self.outbound_current(owner,association)?;let refs=self.outbound_refs(owner)?;
        match refs.entries.into_iter().find(|e|e.reference.association_id==association) {
            None=>Ok(None),Some(entry) if entry.association==current=>Ok(Some(entry.reference)),_=>Err(invalid()),
        }
    }
    /// Create-only, after actual SAME-SSH bootstrap reply+EOF and original scope
    /// validation. First subset does not replace/renew any existing record.
    /// Call inside original Storage::write_node_association, not cloud/outbox.
    pub fn node_outbound_control_store(&self,owner:&NodeAssociationOwner,expected:&NodeAssociationRecord,
        reference:NodeOutboundControlReference)->Result<()> {
        let current=self.outbound_current(owner,reference.association_id)?;
        valid(&reference,owner)?;
        if current!=*expected {return Err(invalid())}
        let mut refs=self.outbound_refs(owner)?;
        if refs.entries.len()>=MAX_ENTRIES || refs.entries.iter().any(|e|
            e.reference.association_id==reference.association_id||e.reference.control_id==reference.control_id) {return Err(invalid())}
        refs.entries.push(Entry{reference,association:current});
        let raw=serde_json::to_vec(&refs)?;if raw.len()>MAX_BYTES {return Err(invalid())}
        self.c().execute("INSERT OR REPLACE INTO local_settings(key,value,updated_at) VALUES(?1,?2,?3)",
            params![KEY,std::str::from_utf8(&raw).map_err(|_|invalid())?,crate::sql::ts(&crate::sql::now())])?;
        if self.outbound_raw()?.as_deref()!=Some(raw.as_slice()) {return Err(invalid())} Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_has_no_bootstrap_token_or_path_field() {
        let id=Uuid::new_v4();
        let raw=serde_json::json!({"associationId":id,"controlId":id,"accountId":id,"deviceId":id,
            "sessionId":id,"serverUrl":"https://example.invalid/","descriptorOrigin":"wss://example.invalid","expiresAt":100});
        assert!(serde_json::from_value::<NodeOutboundControlReference>(raw.clone()).is_ok());
        for key in ["agentToken","path","approved","permit"] {
            let mut extra=raw.clone();extra[key]=serde_json::json!("untrusted");
            assert!(serde_json::from_value::<NodeOutboundControlReference>(extra).is_err());
        }
        let mut fractional=raw;fractional["expiresAt"]=serde_json::json!(100.5);
        assert!(serde_json::from_value::<NodeOutboundControlReference>(fractional).is_err());
    }
}
