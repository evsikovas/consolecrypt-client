//! One normal Node original scoped-secret read; no default-feature production API.
#[cfg(all(feature="test-harness",any(target_os="linux",target_os="windows")))]
#[tokio::test(flavor="multi_thread",worker_threads=4)]
#[ignore="requires same owned ordinary Node and genuine public saved-ref lifetime admission"]
async fn actual_public_normal_lifetime_receiver_read_original_owner() {
    cc_app_core::__test_harness_owned_normal_lifetime_receiver().await;
}
