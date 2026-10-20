//! Kubernetes inventory and native, single-use import previews. Kubeconfig
//! bytes are inbound only; previews and saved metadata contain no credentials.
use crate::api::error::BridgeError;
use crate::state::{from_json, to_json, with_core};
use cc_app_core::kubernetes_import::KubeImportChoice;
use zeroize::Zeroizing;

pub async fn kube_inventory() -> Result<String, BridgeError> {
    with_core(|core| async move { to_json(&core.kube_inventory().await?) }).await
}

pub async fn kube_clusters_list() -> Result<String, BridgeError> {
    with_core(|core| async move { to_json(&core.list_kube_clusters().await?) }).await
}

pub async fn kube_contexts_list() -> Result<String, BridgeError> {
    with_core(|core| async move { to_json(&core.list_kube_contexts().await?) }).await
}

pub async fn kube_import_preview(
    yaml: Vec<u8>,
    source_file: Option<String>,
) -> Result<String, BridgeError> {
    // Also wipe the input if the bridge has not been initialized.
    let yaml = Zeroizing::new(yaml);
    with_core(|core| async move { to_json(&core.kube_import_preview(yaml, source_file).await?) })
        .await
}

pub async fn kube_import_preview_file(path: String) -> Result<String, BridgeError> {
    with_core(|core| async move { to_json(&core.kube_import_preview_file(path).await?) }).await
}

pub async fn kube_import_commit(
    ticket: String,
    choices_json: String,
) -> Result<String, BridgeError> {
    let choices: Vec<KubeImportChoice> = from_json("kube_import_choices", &choices_json)?;
    with_core(|core| async move { to_json(&core.kube_import_commit(ticket, choices).await?) }).await
}

pub async fn kube_import_discard(ticket: String) -> Result<(), BridgeError> {
    with_core(|core| async move { Ok(core.kube_import_discard(ticket).await?) }).await
}

pub async fn kube_context_delete(id: String) -> Result<(), BridgeError> {
    with_core(|core| async move { Ok(core.delete_kube_context(id).await?) }).await
}
