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
