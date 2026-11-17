//! One real public API fixture; no caller sign/transport authority API.
#[cfg(all(feature = "test-harness", target_os = "linux"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires exact owned real Relay/SSHD/installed current Agent and public bootstrap"]
async fn actual_personal_public_route_payload_close_revoke_and_encrypted_audit() {
    cc_app_core::__test_harness_owned_public_tunnel_route().await;
}

#[cfg(all(feature="test-harness",target_os="linux"))]
#[tokio::test]
async fn original_revoke_blocked_then_eof_requires_verified_completion() {
    cc_app_core::__test_harness_revocation_pending_verified().await;
}
#[cfg(all(feature="test-harness",target_os="linux"))]
#[tokio::test]
async fn original_revoke_failed_or_lost_completion_stays_unknown() {
    cc_app_core::__test_harness_revocation_failed_completion().await;
}
#[cfg(all(feature="test-harness",target_os="linux"))]
#[tokio::test]
async fn original_revoke_completion_cannot_extend_deadline_or_cross_unlock() {
    cc_app_core::__test_harness_revocation_deadline_generation().await;
}

#[cfg(all(feature="test-harness",target_os="linux"))]
#[tokio::test]
async fn saved_agent_destination_save_retires_old_scope_without_minting_grant() {
    cc_app_core::__test_harness_saved_destination_source_scope().await;
}

#[cfg(all(feature="test-harness",target_os="linux"))]
#[tokio::test]
async fn original_pump_joins_when_control_proof_is_unknown() {
    cc_app_core::__test_harness_local_join_without_control_proof().await;
}

#[cfg(all(feature="test-harness",target_os="linux"))]
#[tokio::test]
async fn cancelled_original_join_restores_same_pending_pump() {
    cc_app_core::__test_harness_cancel_pending_join_preserves_handle().await;
}
#[cfg(all(feature="test-harness",target_os="linux"))]
#[tokio::test]
async fn cancelled_control_wait_preserves_actual_original_join() {
    cc_app_core::__test_harness_cancel_control_preserves_join_witness().await;
}

#[cfg(all(feature="test-harness",target_os="linux"))]
#[test]
fn four_second_original_with_476ms_approval_keeps_live_fractional_slot_without_extension() {
    cc_app_core::__test_harness_relay_expiry_fractional();
}
#[cfg(all(feature="test-harness",target_os="linux"))]
#[test]
fn expired_and_nonpositive_windows_cannot_mint_transport_time() {
    cc_app_core::__test_harness_relay_expiry_denials();
}

#[cfg(all(feature="test-harness",target_os="linux"))]
#[test]
fn original_outbound_bootstrap_raw_owner_matches_same_canonical_full_url() {
    cc_app_core::__test_harness_outbound_url_canonical_positive();
}
#[cfg(all(feature="test-harness",target_os="linux"))]
#[test]
fn original_outbound_bootstrap_rejects_changed_url_authority_path_or_credentials() {
    cc_app_core::__test_harness_outbound_url_canonical_negative();
}
