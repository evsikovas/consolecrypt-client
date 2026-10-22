//! Selective sharing. Only unlocked AppCore operations release plaintext;
//! directory lookup never establishes trust, and publication is explicit.
use crate::api::error::BridgeError;
use crate::state::{from_json, to_json, with_core};
use cc_app_core::sharing_dto::SharingGrantDto;
use cc_app_core::sharing_projection::SharedProjection;
use zeroize::Zeroizing;

pub async fn sharing_status() -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_status().await?) }).await
}
pub async fn sharing_identity() -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_identity().await?) }).await
}
pub async fn sharing_discover(email: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_discover(email).await?) }).await
}
pub async fn sharing_list(refresh: bool) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sharing_list_report(refresh).await?) }).await
}
pub async fn sharing_inspect(id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_inspect(id).await?) }).await
}
pub async fn sharing_accept(
    id: String,
    confirmed_owner_code: String,
) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_accept(id, confirmed_owner_code).await?) }).await
}
pub async fn sharing_preview_host(id: String, include_notes: bool) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sharing_preview_host(id, include_notes).await?) })
        .await
}
pub async fn sharing_preview_snippet(id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_preview_snippet(id).await?) }).await
}
pub async fn sharing_preview_kube_context(
    id: String,
    include_credentials: bool,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.sharing_preview_kube_context(id, include_credentials)
                .await?,
        )
    })
    .await
}
pub async fn sharing_publish_kube_context(
    context_id: String,
    include_credentials: bool,
    expected_stamp: String,
    grants_json: String,
) -> Result<String, BridgeError> {
    let grants = from_json("sharing_grants", &grants_json)?;
    with_core(move |c| async move {
        to_json(
            &c.sharing_publish_kube_context(
                context_id,
                include_credentials,
                expected_stamp,
                grants,
            )
            .await?,
        )
    })
    .await
}
pub async fn sharing_copy_kube_context(
    share_id: String,
    route_json: String,
    include_credentials: bool,
) -> Result<String, BridgeError> {
    let route: cc_models::kubernetes::KubeRoute = from_json("kube_route", &route_json)?;
    with_core(move |c| async move {
        to_json(
            &c.sharing_copy_kube_context(share_id, route, include_credentials)
                .await?,
        )
    })
    .await
}
pub async fn sharing_preview_db_connection(
    id: String,
    include_credentials: bool,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.sharing_preview_db_connection(id, include_credentials)
                .await?,
        )
    })
    .await
}
pub async fn sharing_publish_db_connection(
    connection_id: String,
    include_credentials: bool,
    expected_stamp: String,
    grants_json: String,
) -> Result<String, BridgeError> {
    let grants = from_json("sharing_grants", &grants_json)?;
    with_core(move |c| async move {
        to_json(
            &c.sharing_publish_db_connection(
                connection_id,
                include_credentials,
                expected_stamp,
                grants,
            )
            .await?,
        )
    })
    .await
}
pub async fn sharing_copy_db_connection(
    share_id: String,
    route_json: String,
    include_credentials: bool,
) -> Result<String, BridgeError> {
    let route: cc_models::database::DbRoute = from_json("db_route", &route_json)?;
    with_core(move |c| async move {
        to_json(
            &c.sharing_copy_db_connection(share_id, route, include_credentials)
                .await?,
        )
    })
    .await
}
pub async fn sharing_publish(
    projection_json: String,
    grants_json: String,
) -> Result<String, BridgeError> {
    let raw = Zeroizing::new(projection_json);
    let projection: SharedProjection = from_json("shared_projection", &raw)?;
    let grants: Vec<SharingGrantDto> = from_json("sharing_grants", &grants_json)?;
    with_core(|c| async move { to_json(&c.sharing_publish(projection, grants).await?) }).await
}
pub async fn sharing_edit(id: String, projection_json: String) -> Result<String, BridgeError> {
    let raw = Zeroizing::new(projection_json);
    let projection: SharedProjection = from_json("shared_projection", &raw)?;
    with_core(|c| async move { to_json(&c.sharing_edit(id, projection).await?) }).await
}
pub async fn sharing_rotate(id: String, grants_json: String) -> Result<String, BridgeError> {
    let grants: Vec<SharingGrantDto> = from_json("sharing_grants", &grants_json)?;
    with_core(|c| async move { to_json(&c.sharing_rotate(id, grants).await?) }).await
}
pub async fn sharing_delete(id: String) -> Result<(), BridgeError> {
    with_core(|c| async move {
        c.sharing_delete(id).await?;
        Ok(())
    })
    .await
}
/// Protocol 1.8: this account leaves a received item on all its devices.
pub async fn sharing_leave(id: String) -> Result<(), BridgeError> {
    with_core(|c| async move {
        c.sharing_leave(id).await?;
        Ok(())
    })
    .await
}
/// Protocol 1.8: owner-deleted items that are still restorable.
pub async fn sharing_deleted() -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_deleted().await?) }).await
}
/// Protocol 1.8: republish a deleted item; call `sharing_flush` afterwards.
pub async fn sharing_restore(id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_restore(id).await?) }).await
}
pub async fn sharing_outbox() -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_outbox().await?) }).await
}
pub async fn sharing_flush() -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_flush().await?) }).await
}

pub async fn sharing_reconcile(
    id: String,
    confirmed_owner_code: String,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.sharing_reconcile(id, confirmed_owner_code).await?) },
    )
    .await
}
pub async fn sharing_discard_pending(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sharing_discard_pending(id).await?) }).await
}
pub async fn sharing_preview_secret(id: String, passphrase: bool) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        let projection = c.sharing_prepare_credential(id, passphrase).await?;
        if let SharedProjection::Secret(ref secret) = projection {
            // Metadata only: the secret never enters an FFI preview buffer.
            to_json(&serde_json::json!({"kind":"secret", "data": {
                "name":secret.name, "secret_kind":secret.secret_kind,
            }}))
        } else {
            unreachable!("credential preparation returns only Secret")
        }
    })
    .await
}
pub async fn sharing_preview_group(
    id: String,
    children_json: String,
) -> Result<String, BridgeError> {
    let children = from_json("shared_children", &children_json)?;
    with_core(move |c| async move { to_json(&c.sharing_preview_group(id, children).await?) }).await
}
pub async fn sharing_publish_secret(
    id: String,
    passphrase: bool,
    grants_json: String,
) -> Result<String, BridgeError> {
    let grants = from_json("sharing_grants", &grants_json)?;
    with_core(
        move |c| async move { to_json(&c.sharing_publish_secret(id, passphrase, grants).await?) },
    )
    .await
}
pub async fn sharing_reveal_secret(id: String) -> Result<Vec<u8>, BridgeError> {
    with_core(move |c| async move {
        let mut revealed = c.sharing_reveal_secret(id).await?;
        Ok(revealed.take_bytes())
    })
    .await
}
pub async fn sharing_copy_host(
    id: String,
    credential_id: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sharing_copy_host(id, credential_id).await?) }).await
}
pub async fn sharing_copy_snippet(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sharing_copy_snippet(id).await?) }).await
}
pub async fn sharing_refresh_bound_host(
    id: String,
    confirm_endpoint_change: bool,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.sharing_refresh_bound_host(id, confirm_endpoint_change)
                .await?,
        )
    })
    .await
}
pub async fn sharing_detach_host(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.sharing_detach_host(id).await?) }).await
}

pub async fn sharing_edit_secret(id: String, value: Vec<u8>) -> Result<String, BridgeError> {
    let value = cc_models::secret::SecretValue::new(crate::state::secret_string("secret", value)?);
    with_core(|c| async move { to_json(&c.sharing_edit_secret(id, value).await?) }).await
}
pub async fn sharing_copy_secret_credential(id: String) -> Result<String, BridgeError> {
    with_core(|c| async move { to_json(&c.sharing_copy_secret_credential(id).await?) }).await
}

pub async fn sharing_refresh_bound_host_expected(
    id: String,
    address: String,
    port: u16,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.sharing_refresh_bound_host_expected(id, address, port)
                .await?,
        )
    })
    .await
}
