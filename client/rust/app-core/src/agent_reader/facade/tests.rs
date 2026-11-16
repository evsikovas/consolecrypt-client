//! DRAFT, uncompiled/unexecuted. Resource/codec tests are not Native approval,
//! SSH, protected response origin or authority storage acceptance.
use super::*;
use serde_json::json;

fn peer(n: u8) -> PeerKey {
    PeerKey {
        profile: AgentReaderNativeProfileId::new("11111111-1111-4111-8111-111111111111").unwrap(),
        agent: AgentReaderAgentId::new("22222222-2222-4222-8222-222222222222").unwrap(),
        audience: AgentReaderAudienceId::new("33333333-3333-4333-8333-333333333333").unwrap(),
        origin: AgentReaderHash::from_bytes([n; 32]),
        pin: AgentReaderHash::from_bytes([9; 32]),
        owner: AgentReaderHash::from_bytes([7; 32]),
    }
}
fn target(history: bool) -> AgentReaderTargetDescriptorV1 {
    let a = "22222222-2222-4222-8222-222222222222";
    let h = "44444444-4444-4444-8444-444444444444";
    let resource = "55555555-5555-4555-8555-555555555555";
    let dataset = if history {
        json!({"kind":"localAgentHistoryV1","schemaVersion":1,"descriptor":{
        "agent_id":a,"host_id":h,"history_resource_id":resource,"sample_interval_seconds":15,
        "max_bytes":1048576,"max_points":5760,"max_age_seconds":86400,"history_kind":"agent_gauge_history","schema_version":1}})
    } else {
        json!({"kind":"heartbeatV1","schemaVersion":1,"descriptor":{
        "agent_id":a,"host_id":h,"heartbeat_resource_id":resource,"history_kind":"heartbeat","schema_version":1}})
    };
    serde_json::from_value(json!({"format":"cc-reader-target/v1","agentId":a,"configuredHostId":h,
        "action":if history{"historyRead"}else{"heartbeatRead"},"resourceId":resource,"dataset":dataset,
        "maxResponseBytes":if history{262144}else{65536},"maxQueryPoints":if history{1000}else{0}})).unwrap()
}
fn history() -> Value {
    json!({"metrics":["agent_uptime_seconds","agent_collector_running"],
    "points":[{"timestamp":101.25,"agent_uptime_seconds":1.5,"agent_collector_running":null}],
    "truncated":false,"evicted_points":0,"stored_bytes":128,"budget_bytes":1048576,
    "effective_data_budget_bytes":524288,"max_age_seconds":86400,"actual_start":101.25,
    "actual_end":101.25,"storage_state":"ready","available_while_agent_online_only":true})
}
fn heartbeat() -> Value {
    json!({"schema_version":1,"agent_id":"22222222-2222-4222-8222-222222222222",
    "host_id":"44444444-4444-4444-8444-444444444444","host_binding":"configured_not_identity_attested",
    "version":"0.1.0","uptime_seconds":2.25,"timestamp":101.25,
    "collection":{"state":"blocked","last_scrape_success":null,
        "remote_write_receiver_destination_pinned":false,
        "remote_write_receiver_destination_block_reason":"remote_write_receiver_redirect_destination_not_pinned"},
    "capabilities":{"protocol":"cc-agent-local/1","telemetry.read":1,"local_history":1},
    "audit":{"state":"external_sink"},"history_available_while_online_only":true})
}

#[test]
fn combined_eight_slots_and_known_completion_release() {
    let r = Arc::new(NativeReaderRuntime::default());
    let mut leases = Vec::new();
    for _ in 0..8 {
        leases.push(r.reserve().unwrap());
    }
    assert!(matches!(r.reserve(), Err(Failure::Busy)));
    leases.pop();
    assert!(r.reserve().is_ok());
}
#[test]
fn uncertain_storage_and_unobserved_cleanup_never_release_on_time() {
    let r = Arc::new(NativeReaderRuntime::default());
    let mut lease = r.reserve().unwrap();
    let id = lease.id;
    lease.arm();
    drop(lease);
    assert!(r.state.lock().unwrap().entries[&id].quarantined);
    let mut lease = r.reserve().unwrap();
    let id = lease.id;
    lease.closed(ReaderCleanupObservation::Uncertain);
    lease.closed(ReaderCleanupObservation::ClosedObserved);
    drop(lease);
    assert!(
        r.state.lock().unwrap().entries[&id].quarantined,
        "late close cannot repair earlier unknown I/O"
    );
}
#[test]
fn broker_state_unknown_is_not_an_ordinary_rejection() {
    let r = Arc::new(NativeReaderRuntime::default());
    let mut lease = r.reserve().unwrap();
    let id = lease.id;
    lease.arm();
    let v: Result<()> = lease.transport(Err(ReaderTransportError::BrokerRejected(
        ReaderBrokerError::StateUnknown,
    )));
    assert_eq!(v, Err(Failure::OutcomeUnknown));
    drop(lease);
    assert!(r.state.lock().unwrap().entries[&id].quarantined);
    let mut known = r.reserve().unwrap();
    let id = known.id;
    known.arm();
    let _: Result<()> = known.transport(Err(ReaderTransportError::BrokerRejected(
        ReaderBrokerError::RoleDenied,
    )));
    drop(known);
    assert!(!r.state.lock().unwrap().entries.contains_key(&id));
}
#[test]
fn revoke_cancels_other_same_peer_but_never_itself_or_other_peer() {
    let r = Arc::new(NativeReaderRuntime::default());
    let a = r.reserve().unwrap();
    let b = r.reserve().unwrap();
    let c = r.reserve().unwrap();
    r.bind(a.id, peer(1), Role::Read).unwrap();
    r.bind(b.id, peer(1), Role::Revoke).unwrap();
    r.bind(c.id, peer(2), Role::Read).unwrap();
    r.begin_revoke(b.id).unwrap();
    assert!(a.cancel.is_cancelled());
    assert!(!b.cancel.is_cancelled());
    assert!(!c.cancel.is_cancelled());
    assert!(b.check().is_ok());
    assert!(c.check().is_ok());
    let new = r.reserve().unwrap();
    assert_eq!(
        r.bind(new.id, peer(1), Role::Read),
        Err(Failure::RevokePending)
    );
    b.cancel.cancel();
    assert_eq!(b.check(), Err(Failure::Cancelled));
}
#[test]
fn only_explicit_current_reconcile_can_unblock() {
    let r = Arc::new(NativeReaderRuntime::default());
    let revoker = r.reserve().unwrap();
    r.bind(revoker.id, peer(1), Role::Revoke).unwrap();
    r.begin_revoke(revoker.id).unwrap();
    assert_eq!(
        r.unblock_after_explicit_reconcile(revoker.id),
        Err(Failure::Rejected)
    );
    let rec = r.reserve().unwrap();
    r.bind(rec.id, peer(1), Role::Reconcile).unwrap();
    r.unblock_after_explicit_reconcile(rec.id).unwrap();
    let new = r.reserve().unwrap();
    assert!(r.bind(new.id, peer(1), Role::Read).is_ok());
}
#[test]
fn generation_fence_prevents_data_publication_after_revoke() {
    let r = Arc::new(NativeReaderRuntime::default());
    let read = r.reserve().unwrap();
    r.bind(read.id, peer(1), Role::Read).unwrap();
    assert!(r.fence_locked(read.id, || Ok(())).is_ok());
    let rev = r.reserve().unwrap();
    r.bind(rev.id, peer(1), Role::Revoke).unwrap();
    r.begin_revoke(rev.id).unwrap();
    let mut called = false;
    assert!(r
        .fence_locked(read.id, || {
            called = true;
            Ok(())
        })
        .is_err());
    assert!(!called);
}
#[test]
fn local_only_is_explicit_not_profile_uuid_cast() {
    assert!(local_only(&ProfileKind::Local, false, false).is_ok());
    assert_eq!(
        local_only(&ProfileKind::Synced, false, false),
        Err(Failure::UnsupportedProfile)
    );
    assert!(local_only(&ProfileKind::Local, true, false).is_err());
    assert!(local_only(&ProfileKind::Local, false, true).is_err());
}
#[tokio::test]
async fn dropped_waiter_cancels_without_aborting_owned_cleanup() {
    let token = CancellationToken::new();
    let original = token.clone();
    let (done, finished) = oneshot::channel();
    let operation = launch(token, async move {
        original.cancelled().await;
        let _ = done.send(());
        Ok(())
    });
    drop(operation);
    tokio::time::timeout(Duration::from_secs(1), finished)
        .await
        .unwrap()
        .unwrap();
}
#[test]
fn local_history_uses_real_envelope_and_keeps_unknown_as_null() {
    let t = target(true);
    let v = history();
    let decoded = response::decode(&serde_json::to_vec(&v).unwrap(), &t).unwrap();
    assert!(decoded.value["points"][0]["agent_collector_running"].is_null());
    assert!(decoded.value.get("agent_id").is_none());
    assert_eq!(decoded.target, t.descriptor_hash().unwrap());
    let mut bad = v.clone();
    bad["actual_start"] = json!(100.0);
    assert!(response::decode(&serde_json::to_vec(&bad).unwrap(), &t).is_err());
    let mut bad = v;
    bad["points"][0]["agent_collector_running"] = json!(true);
    assert!(response::decode(&serde_json::to_vec(&bad).unwrap(), &t).is_err());
}
#[test]
fn response_query_cap_does_not_become_retention_cap() {
    let t = target(true);
    let mut v = history();
    v["points"] = Value::Array(vec![v["points"][0].clone(); 1001]);
    assert!(response::decode(&serde_json::to_vec(&v).unwrap(), &t).is_err());
    v["points"] = json!([]);
    v["actual_start"] = Value::Null;
    v["actual_end"] = Value::Null;
    assert!(response::decode(&serde_json::to_vec(&v).unwrap(), &t).is_ok());
}
#[test]
fn heartbeat_exact_ids_and_closed_fields() {
    let t = target(false);
    let v = heartbeat();
    assert!(response::decode(&serde_json::to_vec(&v).unwrap(), &t).is_ok());
    let mut bad = v.clone();
    bad["agent_id"] = json!("66666666-6666-4666-8666-666666666666");
    assert!(response::decode(&serde_json::to_vec(&bad).unwrap(), &t).is_err());
    let mut bad = v;
    bad["collection"]["raw_url"] = json!("CANARY");
    assert!(response::decode(&serde_json::to_vec(&bad).unwrap(), &t).is_err());
}
#[test]
fn duplicate_nonfinite_depth_and_oversize_are_not_private_data() {
    let t = target(true);
    assert!(response::decode(b"{\"points\":[],\"points\":[]}", &t).is_err());
    assert!(response::decode(b"{\"value\":1e309}", &t).is_err());
    let deep = format!("{}0{}", "[".repeat(20), "]".repeat(20));
    assert!(response::decode(deep.as_bytes(), &t).is_err());
    assert!(response::decode(&vec![b' '; 262145], &t).is_err());
}

// Collection bodies are exact captures from the actual AGT07 Go ELF -> Python
// AgentRuntime heartbeat proof. They assert codec compatibility, not live health.
fn native_queue_captures() -> Vec<Value> {
    serde_json::from_str(include_str!("native_queue_captures.json")).unwrap()
}
fn native_queue_heartbeat(capture: &Value) -> Value {
    let mut value = heartbeat();
    value["collection"] = capture["collection"].clone();
    value
}
fn native_queue_decodes(value: &Value) -> bool {
    response::decode(&serde_json::to_vec(value).unwrap(), &target(false)).is_ok()
}
#[test]
fn native_queue_actual_captures_pass_full_heartbeat_decoder() {
    for capture in native_queue_captures() {
        let value = native_queue_heartbeat(&capture);
        let decoded = response::decode(&serde_json::to_vec(&value).unwrap(), &target(false)).unwrap();
        assert_eq!(decoded.value["collection"], capture["collection"]);
    }
}
#[test]
fn native_queue_unsupported_delivery_facts_remain_null() {
    let value = native_queue_heartbeat(&native_queue_captures()[0]);
    for key in ["confirmed_accepted_requests_total", "confirmed_accepted_samples_total",
        "receiver_permanent_rejected_samples_total", "unacknowledged_samples_lost_total",
        "unacknowledged_delivery_lag_seconds"] {
        let mut bad = value.clone();
        bad["collection"]["native_queue"][key] = json!(0);
        assert!(!native_queue_decodes(&bad), "{key}");
    }
}
#[test]
fn native_queue_rejects_unknown_missing_and_hybrid_shapes() {
    let value = native_queue_heartbeat(&native_queue_captures()[0]);
    let mut bad = value.clone(); bad["collection"]["backend"] = json!("other");
    assert!(!native_queue_decodes(&bad));
    let mut bad = value.clone(); bad["collection"]["last_scrape_success"] = json!(true);
    assert!(!native_queue_decodes(&bad));
    let mut bad = value.clone(); bad["collection"]["native_queue"]["extra"] = Value::Null;
    assert!(!native_queue_decodes(&bad));
    for key in value["collection"]["native_queue"].as_object().unwrap().keys() {
        let mut bad = value.clone();
        bad["collection"]["native_queue"].as_object_mut().unwrap().remove(key);
        assert!(!native_queue_decodes(&bad), "{key}");
    }
}
#[test]
fn native_queue_enforces_counter_boolean_and_numeric_bounds() {
    let value = native_queue_heartbeat(&native_queue_captures()[0]);
    for key in ["sample_send_attempts_total", "last_validated_root_charge_bytes",
        "system_disk_low_available_rejections_total"] {
        for invalid in [json!(true), json!(1.5), json!(-1), json!(1_u64 << 53), json!("1")] {
            let mut bad = value.clone(); bad["collection"]["native_queue"][key] = invalid;
            assert!(!native_queue_decodes(&bad), "{key}");
        }
    }
    for key in ["source_max_enqueued_timestamp_seconds", "source_max_reported_sent_timestamp_seconds",
        "oldest_retained_segment_residence_seconds"] {
        for invalid in [json!(true), json!(-1), json!(1_u64 << 53), json!("1")] {
            let mut bad = value.clone(); bad["collection"]["native_queue"][key] = invalid;
            assert!(!native_queue_decodes(&bad), "{key}");
        }
    }
    for key in ["global_unacknowledged_loss_unknown", "sticky_unavailable"] {
        let mut bad = value.clone(); bad["collection"]["native_queue"][key] = json!(0);
        assert!(!native_queue_decodes(&bad));
    }
}
#[test]
fn native_queue_cannot_report_healthy_after_sticky_failure_or_excess_charge() {
    let mut value = native_queue_heartbeat(&native_queue_captures()[0]);
    value["collection"]["native_queue"]["sticky_unavailable"] = json!(true);
    assert!(!native_queue_decodes(&value));
    value["collection"]["state"] = json!("degraded");
    assert!(native_queue_decodes(&value));
    value["collection"]["native_queue"]["last_validated_root_charge_bytes"] = json!(512*1024*1024_u64+1);
    assert!(!native_queue_decodes(&value));
    value["collection"]["state"] = json!("blocked");
    assert!(native_queue_decodes(&value));
    value["collection"]["state"] = json!("stopped");
    assert!(native_queue_decodes(&value));
    let mut empty = native_queue_heartbeat(&native_queue_captures()[2]);
    empty["collection"]["state"] = json!("running");
    assert!(!native_queue_decodes(&empty));
}

#[test]
fn local_history_optional_state_diagnostics_preserve_legacy_and_exact_gauges() {
    let t = target(true);
    assert!(response::decode(&serde_json::to_vec(&history()).unwrap(), &t).is_ok());
    for (state, running) in [
        ("running", Some(1_u64)), ("disabled", Some(0)), ("stopped", Some(0)),
        ("not_running", Some(0)), ("starting", None), ("degraded", None),
        ("blocked", None), ("unknown", None),
    ] {
        let mut v = history();
        v["points"][0]["collection_state"] = json!(state);
        v["points"][0]["collection_state_version"] = json!(1);
        v["points"][0]["agent_collector_running"] = json!(running);
        let decoded = response::decode(&serde_json::to_vec(&v).unwrap(), &t).unwrap();
        assert_eq!(decoded.value["points"][0]["collection_state"], state);
        assert_eq!(decoded.target, t.descriptor_hash().unwrap());
    }
}
#[test]
fn local_history_state_diagnostics_reject_partial_unknown_and_inconsistent_pairs() {
    let t = target(true);
    let mut valid = history();
    valid["points"][0]["collection_state"] = json!("unknown");
    valid["points"][0]["collection_state_version"] = json!(1);
    for field in ["collection_state", "collection_state_version"] {
        let mut bad = valid.clone();
        bad["points"][0].as_object_mut().unwrap().remove(field);
        assert!(response::decode(&serde_json::to_vec(&bad).unwrap(), &t).is_err());
    }
    for (field, value) in [
        ("collection_state_version", json!(2)), ("collection_state_version", json!(true)),
        ("collection_state_version", json!(1.0)), ("collection_state", json!("private text")),
        ("collection_state", json!(null)), ("agent_collector_running", json!(0)),
        ("drained", json!(true)),
    ] {
        let mut bad = valid.clone();
        bad["points"][0][field] = value;
        assert!(response::decode(&serde_json::to_vec(&bad).unwrap(), &t).is_err());
    }
}
