//! Reuse original assertions without building the monolithic AppCore libtest.
#![cfg(feature = "test-harness")]

mod automation_jobs {
    mod admission_audit {
        mod tests {
            #[test]
            fn exact_original_admission_is_projected_without_private_fields_or_result_change() {
                cc_app_core::focused_cadvisor_test_harness::exact_original_admission_is_projected_without_private_fields_or_result_change();
            }
            #[test]
            fn nested_uncertain_result_is_same_outcome_and_metadata_is_removed() {
                cc_app_core::focused_cadvisor_test_harness::nested_uncertain_result_is_same_outcome_and_metadata_is_removed();
            }
            #[test]
            fn foreign_signer_scope_route_or_extra_field_never_becomes_recorded() {
                cc_app_core::focused_cadvisor_test_harness::foreign_signer_scope_route_or_extra_field_never_becomes_recorded();
            }
            #[test]
            fn missing_native_provenance_never_uses_the_remote_or_current_identity() {
                cc_app_core::focused_cadvisor_test_harness::missing_native_provenance_never_uses_the_remote_or_current_identity();
            }
            #[test]
            fn local_manifest_provenance_or_recorded_projection_changes_are_rechecked() {
                cc_app_core::focused_cadvisor_test_harness::local_manifest_provenance_or_recorded_projection_changes_are_rechecked();
            }
            #[test]
            fn production_needs_the_actual_original_proof_digest() {
                cc_app_core::focused_cadvisor_test_harness::production_needs_the_actual_original_proof_digest();
            }
            #[test]
            fn old_or_unavailable_runner_audit_does_not_change_existing_outcome() {
                cc_app_core::focused_cadvisor_test_harness::old_or_unavailable_runner_audit_does_not_change_existing_outcome();
            }
        }
    }
}

mod monitoring_runtime {
    mod exporter_reads {
        mod tests {
            mod live_cadvisor {
                #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
                #[ignore = "requires UUID-owned QEMU systemd target, real pinned Runner and real Prometheus coordinator"]
                async fn owned_cadvisor_install_signed_job_real_source_binding_and_docker_panel() {
                    cc_app_core::focused_cadvisor_test_harness::run_original_owned_cadvisor().await;
                }
            }
        }
    }
}
