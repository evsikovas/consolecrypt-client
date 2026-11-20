use super::*;

// Current Python/Native structural summary uses null for non-0..255 codes.
// The independent joined-process exit remains signed and must retain -15.
fn evidence() -> Value {
    json!({"version":1,"job_id":Uuid::new_v4(),"manifest_digest":"a".repeat(64),"state":"unknown",
        "sequence":12,"output_bytes":100,"output_truncated":false,"accepted_at":"2026-11-14T12:00:00Z","updated_at":"2026-11-14T12:00:01Z",
        "result":{"exit_code":-15,"timed_out":false,"cancel_requested":true,"remote_changes_may_be_partial":true,"elapsed_ms":1000,
            "runner_execution":{"version":1,"clockSource":"runner-wall","scope":"runner-process","spawnObservedUtc":"2026-11-14T12:00:01Z","exitObservedUtc":"2026-11-14T12:00:00Z","elapsedMonotonicMs":1000},
            "summary":{"status":"failed","exit_code":null,"stats":{"dark":{"owned":1}}}}})
}
fn aliases() -> BTreeSet<String> {
    ["owned".to_string()].into()
}

#[test]
fn actual_unknown_exit_flags_cursor_and_reversed_interval_are_unchanged() {
    let raw = evidence();
    assert_eq!(project(&raw, &aliases()), raw);
    let nested = json!({"reason":"native_operation_not_acknowledged","runner_result":raw});
    assert_eq!(project(&nested, &aliases()), nested);
    assert_eq!(project(&nested, &aliases()).as_object().unwrap().len(), 2);
}

#[test]
fn arbitrary_outer_result_environment_and_statistics_material_is_closed() {
    let original = evidence();
    let mut raw = original.clone();
    raw["token"] = json!("synthetic-secret");
    raw["result"]["stdout"] = json!("synthetic-secret");
    raw["result"]["summary"]["stats"]["dark"]["synthetic-secret"] = json!(1);
    raw["environment"] = json!({"version":1,"environment_digest":"b".repeat(64),"secret":"synthetic-secret","identity":{"system":"Linux","machine":"x86_64","python":"3.12.3","helper":"0.1.0","token":"synthetic-secret","packages":{"ansible-core":{"actual":"2.20.11","extra":"synthetic-secret"},"synthetic-secret":{"actual":"synthetic-secret"}}}});
    let visible = project(&raw, &aliases());
    assert!(!visible.to_string().contains("synthetic-secret"));
    assert_eq!(visible["state"], original["state"]);
    assert_eq!(visible["result"], original["result"]);
    assert_eq!(
        visible["environment"]["identity"]["packages"]["ansible-core"],
        json!({"actual":"2.20.11"})
    );
    assert_eq!(project(&visible, &aliases()), visible);
}

#[test]
fn invalid_and_future_interval_remains_present_invalid_never_a_success_interval() {
    for interval in [
        json!({"version":1,"exit_observed":true,"token":"synthetic-secret"}),
        json!({"version":2}),
        json!("synthetic-secret"),
    ] {
        let mut raw = evidence();
        raw["result"]["runner_execution"] = interval;
        let visible = project(&raw, &aliases());
        assert_eq!(
            visible["result"]["runner_execution"],
            json!({"redacted":true})
        );
        assert_eq!(visible["state"], "unknown");
        assert_eq!(visible["result"]["exit_code"], -15);
        assert_eq!(project(&visible, &aliases()), visible);
    }
}

#[test]
fn control_loss_and_null_actual_flags_do_not_turn_into_success_or_false() {
    let raw = json!({"reason":"control_channel_or_storage_lost","control_failure":"rpc_closed","remote_changes_may_be_partial":true,"raw":"synthetic-secret"});
    assert_eq!(
        project(&raw, &aliases()),
        json!({"reason":"control_channel_or_storage_lost","control_failure":"rpc_closed","remote_changes_may_be_partial":true})
    );
    let mut raw = evidence();
    raw["result"]["cancel_requested"] = Value::Null;
    raw["result"]["timed_out"] = Value::Null;
    assert_eq!(project(&raw, &aliases()), raw);
    assert_eq!(
        project(&json!("synthetic-secret"), &aliases()),
        json!({"redacted":true})
    );
}

#[test]
fn unsupported_extra_keys_are_removed_without_reinterpreting_fixed_wrapper() {
    let raw = json!({"reason":"native_operation_not_acknowledged","runner_result":evidence(),"token":"synthetic-secret"});
    let visible = project(&raw, &aliases());
    assert_eq!(visible.as_object().unwrap().len(), 2);
    assert_eq!(visible["runner_result"]["state"], "unknown");
    assert!(!visible.to_string().contains("synthetic-secret"));
}

#[test]
fn local_quiescence_keeps_original_unknown_time_hash_without_kernel_or_path_dump() {
    let mut raw = evidence();
    raw["local_quiescence"] = json!({"version":1,"original_state":"unknown","original_updated_at":"2026-11-14T12:00:00Z","original_result_sha256":"e".repeat(64),"daemon_pid":123,"identity_inode":456,"source":"systemdPidfdCgroupV2","arbitrary":"synthetic-secret"});
    let visible = project(&raw, &aliases());
    assert_eq!(
        visible["local_quiescence"],
        json!({"version":1,"original_state":"unknown","original_updated_at":"2026-11-14T12:00:00Z","original_result_sha256":"e".repeat(64)})
    );
    assert_eq!(visible["state"], "unknown");
    assert!(!visible.to_string().contains("synthetic-secret"));
    assert_eq!(project(&visible, &aliases()), visible);
}

#[test]
fn unsupported_summary_code_is_redacted_without_changing_actual_process_evidence() {
    let mut raw = evidence();
    raw["result"]["summary"]["exit_code"] = json!(-15);
    let mut expected = raw.clone();
    expected["result"]["summary"]["exit_code"] = Value::Null;
    assert_eq!(project(&raw, &aliases()), expected);
    assert_eq!(expected["state"], "unknown");
    assert_eq!(expected["result"]["exit_code"], -15);
    assert_eq!(expected["result"]["remote_changes_may_be_partial"], true);
    assert_eq!(project(&expected, &aliases()), expected);
}
