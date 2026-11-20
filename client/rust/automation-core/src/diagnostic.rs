//! Pure private diagnostic projection. Never a target operation, credential
//! getter, result/approval transition or new authority. Guest broker cannot call.
use crate::TargetFailure;
use cc_ssh_core::DiagnosticCapture;
use serde::Deserialize;
use serde_json::{Value,json};
use std::{collections::BTreeSet,sync::Arc};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request{event_type:String,payload:Value}
fn denied()->TargetFailure{TargetFailure::UnsupportedOperation}
fn number(value:&Value)->bool{value.as_i64().is_some_and(|n|(0..=1_000_000_000).contains(&n))}
/// Closed non-authoritative outcome DATA for durable job history. Arbitrary
/// guest text/keys never survive, including pre-masking legacy summaries.
pub fn structural_summary(summary:&Value,aliases:&BTreeSet<String>)->Value{
    let old=summary.as_object();
    let status=old.and_then(|v|v.get("status")).and_then(Value::as_str).filter(|v|["successful","failed","timeout","canceled","running","starting","unstarted"].contains(v)).unwrap_or("unknown");
    let code=old.and_then(|v|v.get("exit_code")).and_then(Value::as_i64).filter(|v|(0..=255).contains(v));
    let mut out=serde_json::Map::new();out.insert("status".into(),json!(status));out.insert("exit_code".into(),json!(code));
    if let Some(stats)=old.and_then(|v|v.get("stats")).and_then(Value::as_object){
        let mut safe=serde_json::Map::new();for key in ["ok","changed","failures","dark","skipped","rescued","ignored"]{
            if let Some(values)=stats.get(key).and_then(Value::as_object){let values=aliases.iter().filter_map(|host|values.get(host).filter(|n|number(n)).map(|n|(host.clone(),n.clone()))).collect();safe.insert(key.into(),Value::Object(values));}
        }out.insert("stats".into(),Value::Object(safe));
    }Value::Object(out)
}
pub(crate) fn project(body:Value,masks:&[Arc<DiagnosticCapture>],aliases:&BTreeSet<String>)->Result<Value,TargetFailure>{
    if masks.is_empty()||serde_json::to_vec(&body).map_or(true,|b|b.len()>128*1024){return Err(denied())}
    let input:Request=serde_json::from_value(body).map_err(|_|denied())?;
    if !["runner_on_ok","runner_on_failed","runner_on_unreachable","runner_on_skipped","playbook_on_task_start","playbook_on_play_start","playbook_on_stats","runner_summary","worker_error","worker_diagnostic"].contains(&input.event_type.as_str())||!input.payload.is_object(){return Err(denied())}
    let mut nodes=1024usize;let mut work=2_000_000usize;
    fn text(source:&str,masks:&[Arc<DiagnosticCapture>],work:&mut usize)->Result<String,TargetFailure>{
        if source.ends_with("[TRUNCATED]"){return Ok("[CENSORED]".into())}
        let mut out=source.to_owned();for mask in masks{out=mask.redact_with_budget(&out,work).map_err(|_|denied())?;}Ok(out)
    }
    fn visit(value:&Value,depth:usize,nodes:&mut usize,masks:&[Arc<DiagnosticCapture>],work:&mut usize)->Result<Value,TargetFailure>{
        if *nodes==0||depth>12{return Ok(json!({"censored":true,"reason":"budget"}))}*nodes-=1;
        Ok(match value{
            Value::Object(items)=>{
                if items.get("_ansible_no_log").is_some_and(|v|v!=&Value::Bool(false)&&!v.is_null()){return Ok(json!({"censored":true}))}
                let mut out=serde_json::Map::new();
                for(key,value)in items{
                    if key=="ansible_facts"||key=="invocation"{continue}
                    if out.len()>=256||key.len()>256{return Ok(json!({"censored":true,"reason":"budget"}))}
                    out.insert(text(key,masks,work)?,visit(value,depth+1,nodes,masks,work)?);
                }Value::Object(out)
            },
            Value::Array(items)=>Value::Array(items.iter().take(256).map(|v|visit(v,depth+1,nodes,masks,work)).collect::<Result<_,_>>()?),
            Value::String(value)=>Value::String(text(value,masks,work)?),
            Value::Number(_) | Value::Bool(_)=>{let raw=value.to_string();let masked=text(&raw,masks,work)?;if raw==masked{value.clone()}else{Value::String(masked)}},
            Value::Null=>Value::Null,
        })
    }
    let original=input.payload;let mut projected=visit(&original,0,&mut nodes,masks,&mut work)?;
    // Only closed outcome facts are structural. Arbitrary task/host fields
    // remain masked: a caller cannot smuggle a known value as a UUID/label.
    if let (Some(_),Some(out))=(original.as_object(),projected.as_object_mut()){
        if input.event_type=="runner_summary"{
            for(key,value)in structural_summary(&original,aliases).as_object().expect("closed summary object"){out.insert(key.clone(),value.clone());}
        }
    }
    if serde_json::to_vec(&projected).map_or(true,|v|v.len()>96*1024){return Err(denied())}Ok(projected)
}
#[cfg(test)]mod tests{
    use super::*;
    fn mask()->Arc<DiagnosticCapture>{let m=Arc::new(DiagnosticCapture::new());for secret in ["password42","token-value", "1", "-----BEGIN PRIVATE KEY-----\nsecret-pem-body\n-----END PRIVATE KEY-----"]{m.remember(secret).unwrap();}m}
    #[test]fn actual_projection_masks_known_material_before_output_but_keeps_closed_outcome(){
        let m=mask();let id=uuid::Uuid::new_v4().to_string();let aliases=["cc_owned".to_owned()].into();
        let out=project(json!({"event_type":"runner_on_ok","payload":{"task_uuid":id,"host":"cc_owned","res":{"stdout":"password42 token-value -----BEGIN PRIVATE KEY-----\nsecret-pem-body\n-----END PRIVATE KEY-----","n":1}}}),&[m.clone()],&aliases).unwrap();
        let raw=out.to_string();for secret in ["password42","token-value","secret-pem-body"]{assert!(!raw.contains(secret))}assert_eq!(out["task_uuid"],m.redact(&id).unwrap());assert_eq!(out["res"]["n"],"[REDACTED]");
        let summary=project(json!({"event_type":"runner_summary","payload":{"status":"failed","exit_code":1,"stats":{"failures":{"cc_owned":1}}}}),&[m],&aliases).unwrap();assert_eq!(summary["exit_code"],1);assert_eq!(summary["stats"]["failures"]["cc_owned"],1);
    }
    #[test]fn nested_no_log_facts_input_and_truncated_prefix_cannot_escape(){
        let out=project(json!({"event_type":"runner_on_ok","payload":{"res":{"results":[{"_ansible_no_log":true,"stdout":"arbitrary-secret"}],"ansible_facts":{"secret":"fact"},"invocation":{"token":"input"},"stdout":"known-prefix [TRUNCATED]"}}}),&[mask()],&BTreeSet::new()).unwrap();
        assert_eq!(out["res"]["results"][0],json!({"censored":true}));assert!(out["res"].get("ansible_facts").is_none());assert!(out["res"].get("invocation").is_none());assert_eq!(out["res"]["stdout"],"[CENSORED]");
    }
}
