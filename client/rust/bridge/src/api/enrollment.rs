//! Public signed pairing packets; no challenge plaintext or object secrets.
use crate::api::error::BridgeError;
use crate::state::{from_json, to_json, with_core};
use cc_app_core::{EnrollmentGrantCreateDto, SharingRoleDto};

pub async fn enrollment_grants(id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_list_grants(id).await?) }).await
}
pub async fn enrollment_create(id: String, create_json: String) -> Result<String, BridgeError> {
    let create: EnrollmentGrantCreateDto = from_json("enrollment_grant", &create_json)?;
    with_core(|c| async move { to_json(&c.enrollment_create_grant(id, create).await?) }).await
}
pub async fn enrollment_revoke(id: String, grant_id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_revoke_grant(id, grant_id).await?) }).await
}
pub async fn enrollment_export(id: String, grant_id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_export_anchor_bundle(id, grant_id).await?) })
        .await
}
pub async fn enrollment_prepare(bundle: String, editor: bool) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.enrollment_prepare_target(
                bundle,
                if editor {
                    SharingRoleDto::Editor
                } else {
                    SharingRoleDto::Reader
                },
            )
            .await?,
        )
    })
    .await
}
pub async fn enrollment_inspect(bundle: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_inspect_pairing(bundle).await?) }).await
}
pub async fn enrollment_endorse(
    bundle: String,
    confirmed_code: String,
) -> Result<String, BridgeError> {
    with_core(
        |c| async move { to_json(&c.enrollment_endorse_target(bundle, confirmed_code).await?) },
    )
    .await
}
pub async fn enrollment_submit(
    bundle: String,
    confirmed_code: String,
) -> Result<String, BridgeError> {
    with_core(
        |c| async move { to_json(&c.enrollment_submit_target(bundle, confirmed_code).await?) },
    )
    .await
}
pub async fn enrollment_requests(id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_list_requests(id).await?) }).await
}
pub async fn enrollment_pending() -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_pending_requests().await?) }).await
}
pub async fn enrollment_challenge(id: String, request_id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_challenge(id, request_id).await?) }).await
}
pub async fn enrollment_respond(id: String, request_id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_respond(id, request_id).await?) }).await
}
pub async fn enrollment_accept(
    id: String,
    request_id: String,
    confirmed_manual: bool,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.enrollment_accept(id, request_id, confirmed_manual)
                .await?,
        )
    })
    .await
}

pub async fn enrollment_process_automatic() -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_process_automatic().await?) }).await
}

pub async fn enrollment_reconcile(
    id: String,
    confirmed_owner_code: String,
) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.enrollment_reconcile(id, confirmed_owner_code).await?) })
        .await
}

pub async fn enrollment_restore_pairing(
    bundle: String,
    confirmed_code: String,
) -> Result<String, BridgeError> {
    with_core(
        |c| async move { to_json(&c.enrollment_restore_pairing(bundle, confirmed_code).await?) },
    )
    .await
}
