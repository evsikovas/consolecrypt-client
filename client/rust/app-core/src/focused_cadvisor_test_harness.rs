//! Feature-only test facade; absent from ordinary production builds.
//! No assertion body, path selector, grant or signing interface is duplicated.

pub use crate::automation_jobs::__focused_admission_audit::{
    exact_original_admission_is_projected_without_private_fields_or_result_change,
    nested_uncertain_result_is_same_outcome_and_metadata_is_removed,
    foreign_signer_scope_route_or_extra_field_never_becomes_recorded,
    missing_native_provenance_never_uses_the_remote_or_current_identity,
    local_manifest_provenance_or_recorded_projection_changes_are_rechecked,
    production_needs_the_actual_original_proof_digest,
    old_or_unavailable_runner_audit_does_not_change_existing_outcome,
};

pub async fn run_original_owned_cadvisor() {
    crate::monitoring_runtime::__focused_live_cadvisor().await;
}
