//! Kubernetes inventory and native, single-use import previews. Kubeconfig
//! bytes are inbound only; previews and saved metadata contain no credentials.
use crate::api::error::BridgeError;
use crate::state::{from_json, to_json, with_core};
use cc_app_core::kubernetes_import::KubeImportChoice;
use zeroize::Zeroizing;

pub async fn kube_rollout_history(
    session_id: String,
    reference_json: String,
) -> Result<String, BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        to_json(&core.kube_rollout_history(session_id, reference).await?)
    })
    .await
}
pub async fn kube_rollout_diff(
    session_id: String,
    reference_json: String,
    before: u64,
    after: u64,
) -> Result<String, BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        to_json(
            &core
                .kube_rollout_diff(session_id, reference, before, after)
                .await?,
        )
    })
    .await
}

pub async fn kube_ai_history(context_id: String) -> Result<Vec<u8>, BridgeError> {
    with_core(move |core| async move {
        Ok(to_json(&core.kube_ai_history(context_id).await?)?.into_bytes())
    })
    .await
}
pub async fn kube_ai_delete_conversation(conversation_id: String) -> Result<(), BridgeError> {
    with_core(
        move |core| async move { Ok(core.kube_ai_delete_conversation(conversation_id).await?) },
    )
    .await
}

pub async fn kube_secret_reveal(
    session_id: String,
    reference_json: String,
) -> Result<Vec<u8>, BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_secret_reveal(session_id, reference)
            .await?
            .to_vec())
    })
    .await
}

pub async fn kube_history_status(session_id: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_history_status(session_id).await?) })
        .await
}
pub async fn kube_event_history(
    context_id: String,
    namespace: Option<String>,
    warning_only: bool,
    before_ms: Option<i64>,
    before_uid: Option<String>,
    limit: u32,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        to_json(
            &core
                .kube_event_history(
                    context_id,
                    namespace,
                    warning_only,
                    before_ms,
                    before_uid,
                    limit as usize,
                )
                .await?,
        )
    })
    .await
}
pub async fn kube_event_history_clear(context_id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_event_history_clear(context_id).await?) }).await
}

pub async fn kube_ai_prepare(
    session_id: String,
    question_json: Vec<u8>,
) -> Result<String, BridgeError> {
    let question_json = Zeroizing::new(question_json);
    let text = std::str::from_utf8(&question_json)
        .map_err(|_| BridgeError::invalid("question", "invalid_utf8"))?;
    let question = from_json("kube_ai_question", text)?;
    with_core(
        move |core| async move { to_json(&core.kube_ai_prepare(session_id, question).await?) },
    )
    .await
}
pub async fn kube_ai_start(
    session_id: String,
    ticket: String,
    approve_external: bool,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        Ok(core
            .kube_ai_start(session_id, ticket, approve_external)
            .await?)
    })
    .await
}
pub async fn kube_ai_poll(session_id: String, request_id: String) -> Result<Vec<u8>, BridgeError> {
    with_core(move |core| async move {
        Ok(to_json(&core.kube_ai_poll(session_id, request_id).await?)?.into_bytes())
    })
    .await
}
pub async fn kube_ai_cancel(session_id: String, id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_ai_cancel(session_id, id).await?) }).await
}
pub async fn kube_ai_revoke(session_id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_ai_revoke(session_id).await?) }).await
}
pub async fn kube_scan_config_files(directory: String) -> Result<Vec<String>, BridgeError> {
    with_core(move |core| async move { Ok(core.kube_scan_config_files(directory).await?) }).await
}
pub async fn kube_export_context(
    context_id: String,
    path: String,
    include_credentials: bool,
) -> Result<(), BridgeError> {
    with_core(move |core| async move {
        Ok(core
            .kube_export_context(context_id, path, include_credentials)
            .await?)
    })
    .await
}
pub async fn kube_context_update(
    context_id: String,
    update_json: String,
) -> Result<(), BridgeError> {
    let update = from_json("kube_context_update", &update_json)?;
    with_core(move |core| async move {
        core.kube_context_update(context_id, update).await?;
        Ok(())
    })
    .await
}

pub async fn kube_inventory() -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_inventory().await?) }).await
}

pub async fn kube_clusters_list() -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.list_kube_clusters().await?) }).await
}

pub async fn kube_contexts_list() -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.list_kube_contexts().await?) }).await
}

pub async fn kube_import_preview(
    yaml: Vec<u8>,
    source_file: Option<String>,
) -> Result<String, BridgeError> {
    // Also wipe the input if the bridge has not been initialized.
    let yaml = Zeroizing::new(yaml);
    with_core(
        move |core| async move { to_json(&core.kube_import_preview(yaml, source_file).await?) },
    )
    .await
}

pub async fn kube_import_preview_file(path: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_import_preview_file(path).await?) }).await
}

pub async fn kube_import_commit(
    ticket: String,
    choices_json: String,
) -> Result<String, BridgeError> {
    let choices: Vec<KubeImportChoice> = from_json("kube_import_choices", &choices_json)?;
    with_core(move |core| async move { to_json(&core.kube_import_commit(ticket, choices).await?) })
        .await
}

pub async fn kube_import_discard(ticket: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_import_discard(ticket).await?) }).await
}

pub async fn kube_context_delete(id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.delete_kube_context(id).await?) }).await
}

pub async fn kube_connect(context_id: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_connect(context_id).await?) }).await
}

pub async fn kube_disconnect(session_id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_disconnect(session_id).await?) }).await
}

pub async fn kube_discover(session_id: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_discover(session_id).await?) }).await
}

pub async fn kube_namespaces(session_id: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_namespaces(session_id).await?) }).await
}

pub async fn kube_resources_list(
    session_id: String,
    kind_json: String,
    query_json: String,
) -> Result<String, BridgeError> {
    let kind = from_json("kube_kind", &kind_json)?;
    let query = from_json("kube_query", &query_json)?;
    with_core(move |core| async move {
        to_json(&core.kube_resources_list(session_id, kind, query).await?)
    })
    .await
}

pub async fn kube_resource_get(
    session_id: String,
    reference_json: String,
) -> Result<String, BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(
        move |core| async move { to_json(&core.kube_resource_get(session_id, reference).await?) },
    )
    .await
}

pub async fn kube_logs(
    session_id: String,
    namespace: String,
    pod: String,
    query_json: String,
) -> Result<String, BridgeError> {
    let query = from_json("kube_logs", &query_json)?;
    with_core(move |core| async move {
        to_json(&core.kube_logs(session_id, namespace, pod, query).await?)
    })
    .await
}

pub async fn kube_watch_start(
    session_id: String,
    kind_json: String,
    query_json: String,
    resource_version: Option<String>,
) -> Result<String, BridgeError> {
    let kind = from_json("kube_kind", &kind_json)?;
    let query = from_json("kube_query", &query_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_watch_start(session_id, kind, query, resource_version)
            .await?)
    })
    .await
}

pub async fn kube_watch_poll(session_id: String, watch_id: String) -> Result<String, BridgeError> {
    with_core(
        move |core| async move { to_json(&core.kube_watch_poll(session_id, watch_id).await?) },
    )
    .await
}

pub async fn kube_watch_stop(session_id: String, watch_id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_watch_stop(session_id, watch_id).await?) })
        .await
}

pub async fn kube_write_access(session_id: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_write_access(session_id).await?) }).await
}

pub async fn kube_request_write_window(
    session_id: String,
    typed_context_name: String,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        core.kube_open_write_window(session_id.clone(), typed_context_name)
            .await?;
        to_json(&core.kube_write_access(session_id).await?)
    })
    .await
}

pub async fn kube_close_write_window(session_id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_close_write_window(session_id).await?) }).await
}

pub async fn kube_apply_yaml(
    session_id: String,
    reference_json: String,
    manifest: Vec<u8>,
    dry_run: bool,
    force: bool,
) -> Result<String, BridgeError> {
    let manifest = Zeroizing::new(manifest);
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        to_json(
            &core
                .kube_apply_yaml(session_id, reference, manifest, dry_run, force)
                .await?,
        )
    })
    .await
}

pub async fn kube_resource_delete(
    session_id: String,
    reference_json: String,
    uid: String,
    resource_version: String,
) -> Result<(), BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_resource_delete(session_id, reference, uid, resource_version)
            .await?)
    })
    .await
}

pub async fn kube_logs_start(
    session_id: String,
    namespace: String,
    pod: String,
    query_json: String,
) -> Result<String, BridgeError> {
    let query = from_json("kube_logs", &query_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_logs_start(session_id, namespace, pod, query)
            .await?)
    })
    .await
}
pub async fn kube_logs_poll(session_id: String, stream_id: String) -> Result<String, BridgeError> {
    with_core(
        move |core| async move { to_json(&core.kube_logs_poll(session_id, stream_id).await?) },
    )
    .await
}
pub async fn kube_logs_stop(session_id: String, stream_id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_logs_stop(session_id, stream_id).await?) })
        .await
}
pub async fn kube_insights(
    session_id: String,
    namespace: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_insights(session_id, namespace).await?) })
        .await
}
pub async fn kube_helm_history(
    session_id: String,
    namespace: Option<String>,
    release_name: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        to_json(
            &core
                .kube_helm_history(session_id, namespace, release_name)
                .await?,
        )
    })
    .await
}
pub async fn kube_helm_release(
    session_id: String,
    namespace: String,
    secret_name: String,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        to_json(
            &core
                .kube_helm_release(session_id, namespace, secret_name)
                .await?,
        )
    })
    .await
}
pub async fn kube_flux_action(
    session_id: String,
    reference_json: String,
    action: String,
    reason: Option<String>,
) -> Result<(), BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_flux_action(session_id, reference, action, reason)
            .await?)
    })
    .await
}
pub async fn kube_argo_action(
    session_id: String,
    reference_json: String,
    action: String,
) -> Result<(), BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(
        move |core| async move { Ok(core.kube_argo_action(session_id, reference, action).await?) },
    )
    .await
}
pub async fn kube_resources_table(
    session_id: String,
    kind_json: String,
    query_json: String,
) -> Result<String, BridgeError> {
    let kind = from_json("kube_kind", &kind_json)?;
    let query = from_json("kube_query", &query_json)?;
    with_core(move |core| async move {
        to_json(&core.kube_resources_table(session_id, kind, query).await?)
    })
    .await
}
pub async fn kube_permissions(
    session_id: String,
    namespace: String,
) -> Result<String, BridgeError> {
    with_core(
        move |core| async move { to_json(&core.kube_permissions(session_id, namespace).await?) },
    )
    .await
}
pub async fn kube_metrics(
    session_id: String,
    pods: bool,
    namespace: Option<String>,
) -> Result<String, BridgeError> {
    with_core(
        move |core| async move { to_json(&core.kube_metrics(session_id, pods, namespace).await?) },
    )
    .await
}
pub async fn kube_exec_approve(ticket: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_exec_approve(ticket).await?) }).await
}
pub async fn kube_auth_cancel(ticket: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_auth_cancel(ticket).await?) }).await
}
pub async fn kube_oidc_complete(
    ticket: String,
    callback_url: Option<String>,
) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_oidc_complete(ticket, callback_url).await?) })
        .await
}
pub async fn kube_exec_review(context_id: String) -> Result<Vec<u8>, BridgeError> {
    with_core(move |core| async move {
        serde_json::to_vec(&core.kube_exec_review(context_id).await?)
            .map_err(|_| BridgeError::internal("cannot encode authentication review"))
    })
    .await
}
pub async fn kube_oidc_begin(context_id: String) -> Result<Vec<u8>, BridgeError> {
    with_core(move |core| async move {
        serde_json::to_vec(&core.kube_oidc_begin(context_id).await?)
            .map_err(|_| BridgeError::internal("cannot encode authentication challenge"))
    })
    .await
}
pub async fn kube_terminal_open(
    session_id: String,
    request_json: String,
    cols: u32,
    rows: u32,
) -> Result<String, BridgeError> {
    let request = from_json("kube_terminal", &request_json)?;
    with_core(move |core| async move {
        to_json(
            &core
                .kube_terminal_open(session_id, request, cols, rows)
                .await?,
        )
    })
    .await
}
pub async fn kube_forward_start(
    session_id: String,
    namespace: String,
    pod: String,
    remote_port: u16,
    local_port: u16,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        to_json(
            &core
                .kube_forward_start(session_id, namespace, pod, remote_port, local_port)
                .await?,
        )
    })
    .await
}
pub async fn kube_forward_start_configured(
    session_id: String,
    request_json: String,
) -> Result<String, BridgeError> {
    let request = from_json("kube_forward", &request_json)?;
    with_core(move |core| async move {
        to_json(
            &core
                .kube_forward_start_configured(session_id, request)
                .await?,
        )
    })
    .await
}
pub async fn kube_forward_list() -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_forward_list().await?) }).await
}
pub async fn kube_forward_stop(session_id: String, forward_id: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_forward_stop(session_id, forward_id).await?) })
        .await
}
pub async fn kube_scale(
    session_id: String,
    reference_json: String,
    resource_version: String,
    replicas: u32,
) -> Result<(), BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_scale(session_id, reference, resource_version, replicas)
            .await?)
    })
    .await
}
pub async fn kube_restart(
    session_id: String,
    reference_json: String,
    resource_version: String,
) -> Result<(), BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_restart(session_id, reference, resource_version)
            .await?)
    })
    .await
}
pub async fn kube_cordon(
    session_id: String,
    node: String,
    resource_version: String,
    unschedulable: bool,
) -> Result<(), BridgeError> {
    with_core(move |core| async move {
        Ok(core
            .kube_cordon(session_id, node, resource_version, unschedulable)
            .await?)
    })
    .await
}
pub async fn kube_drain_plan(session_id: String, node: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_drain_plan(session_id, node).await?) })
        .await
}
pub async fn kube_evict_pod(
    session_id: String,
    reference_json: String,
    uid: String,
    resource_version: String,
) -> Result<(), BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_evict_pod(session_id, reference, uid, resource_version)
            .await?)
    })
    .await
}
pub async fn kube_debug_container(
    session_id: String,
    reference_json: String,
    resource_version: String,
    container_json: String,
) -> Result<(), BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    let container = from_json("kube_debug_container", &container_json)?;
    with_core(move |core| async move {
        Ok(core
            .kube_debug_container(session_id, reference, resource_version, container)
            .await?)
    })
    .await
}
pub async fn kube_workload_pods(
    session_id: String,
    reference_json: String,
) -> Result<String, BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(
        move |core| async move { to_json(&core.kube_workload_pods(session_id, reference).await?) },
    )
    .await
}
pub async fn kube_object_events(
    session_id: String,
    reference_json: String,
    uid: String,
) -> Result<String, BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        to_json(&core.kube_object_events(session_id, reference, uid).await?)
    })
    .await
}
pub async fn kube_schema(session_id: String, kind_json: String) -> Result<String, BridgeError> {
    let kind = from_json("kube_kind", &kind_json)?;
    with_core(move |core| async move { to_json(&core.kube_schema(session_id, kind).await?) }).await
}
pub async fn kube_manifest_diff(
    session_id: String,
    reference_json: String,
    manifest: Vec<u8>,
) -> Result<String, BridgeError> {
    let manifest = Zeroizing::new(manifest);
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        to_json(
            &core
                .kube_manifest_diff(session_id, reference, manifest)
                .await?,
        )
    })
    .await
}
pub async fn kube_create_resource(
    session_id: String,
    reference_json: String,
    manifest: Vec<u8>,
    dry_run: bool,
) -> Result<String, BridgeError> {
    let manifest = Zeroizing::new(manifest);
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        to_json(
            &core
                .kube_create_resource(session_id, reference, manifest, dry_run)
                .await?,
        )
    })
    .await
}

pub async fn kube_flux_discover(manual_path: Option<String>) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_flux_discover(manual_path).await?) })
        .await
}
pub async fn kube_flux_probe(path: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_flux_probe(path).await?) }).await
}
pub async fn kube_flux_review(
    version: String,
    method: String,
    operation: String,
) -> Result<String, BridgeError> {
    let method = serde_json::from_value(serde_json::Value::String(method))
        .map_err(|_| BridgeError::invalid("method", "invalid_method"))?;
    let operation = serde_json::from_value(serde_json::Value::String(operation))
        .map_err(|_| BridgeError::invalid("operation", "invalid_operation"))?;
    with_core(move |core| async move {
        to_json(&core.kube_flux_review(version, method, operation).await?)
    })
    .await
}
pub async fn kube_flux_approve(ticket: String) -> Result<Option<String>, BridgeError> {
    with_core(move |core| async move { Ok(core.kube_flux_approve(ticket).await?) }).await
}
pub async fn kube_flux_cancel(ticket: String) -> Result<(), BridgeError> {
    with_core(move |core| async move { Ok(core.kube_flux_cancel(ticket).await?) }).await
}
pub async fn kube_flux_run(
    session_id: String,
    path: String,
    action_json: String,
) -> Result<Vec<u8>, BridgeError> {
    let action = from_json("flux_action", &action_json)?;
    with_core(move |core| async move { Ok(core.kube_flux_run(session_id, path, action).await?) })
        .await
}
pub async fn kube_flux_terminal(session_id: String, path: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { Ok(core.kube_flux_terminal(session_id, path).await?) }).await
}

pub async fn kube_helm_rollback_review(
    session_id: String,
    namespace: String,
    release: String,
    revision: u64,
    manual_path: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        to_json(
            &core
                .kube_helm_rollback_review(session_id, namespace, release, revision, manual_path)
                .await?,
        )
    })
    .await
}
pub async fn kube_helm_rollback_approve(ticket: String) -> Result<Vec<u8>, BridgeError> {
    with_core(move |core| async move { Ok(core.kube_helm_rollback_approve(ticket).await?) }).await
}
pub async fn kube_flux_preferences(session_id: Option<String>) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_flux_preferences(session_id).await?) })
        .await
}
pub async fn kube_flux_preferences_save(preferences_json: String) -> Result<(), BridgeError> {
    let preferences = from_json("flux_preferences", &preferences_json)?;
    with_core(move |core| async move { Ok(core.kube_flux_preferences_save(preferences).await?) })
        .await
}
pub async fn kube_flux_path_add() -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_flux_path_add().await?) }).await
}
pub async fn kube_log_targets(
    session_id: String,
    reference_json: String,
) -> Result<String, BridgeError> {
    let reference = from_json("resource", &reference_json)?;
    with_core(
        move |core| async move { to_json(&core.kube_log_targets(session_id, reference).await?) },
    )
    .await
}
pub async fn kube_export_logs(
    session_id: String,
    path: String,
    contents: Vec<u8>,
    mask: bool,
) -> Result<(), BridgeError> {
    let contents = zeroize::Zeroizing::new(contents);
    with_core(move |core| async move {
        Ok(core
            .kube_export_logs(session_id, path, contents, mask)
            .await?)
    })
    .await
}

/// Metadata-only links validated against API discovery and object identity.
pub async fn kube_related_objects(
    session_id: String,
    reference_json: String,
) -> Result<String, BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        to_json(&core.kube_related_objects(session_id, reference).await?)
    })
    .await
}

pub async fn kube_context_favourites(
    id: String,
    namespaces: Vec<String>,
    kinds: Vec<String>,
) -> Result<(), BridgeError> {
    with_core(
        move |core| async move { Ok(core.kube_context_favourites(id, namespaces, kinds).await?) },
    )
    .await
}
pub async fn kube_cluster_labels(
    context_id: String,
    tags: Vec<String>,
    color: Option<String>,
) -> Result<(), BridgeError> {
    with_core(
        move |core| async move { Ok(core.kube_cluster_labels(context_id, tags, color).await?) },
    )
    .await
}

/// Bounded Pod/port candidates; this does not open a port forward.
pub async fn kube_forward_targets(
    session_id: String,
    reference_json: String,
) -> Result<String, BridgeError> {
    let reference = from_json("kube_reference", &reference_json)?;
    with_core(move |core| async move {
        to_json(&core.kube_forward_targets(session_id, reference).await?)
    })
    .await
}
pub async fn kube_context_terminal_profiles() -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_context_terminal_profiles().await?) })
        .await
}
pub async fn kube_context_terminal(
    session_id: String,
    namespace: String,
    profile_id: String,
) -> Result<String, BridgeError> {
    with_core(move |core| async move {
        Ok(core
            .kube_context_terminal(session_id, namespace, profile_id)
            .await?)
    })
    .await
}

pub async fn kube_view_preferences(context_id: String) -> Result<String, BridgeError> {
    with_core(move |core| async move { to_json(&core.kube_view_preferences(context_id).await?) })
        .await
}
pub async fn kube_view_preferences_update(
    context_id: String,
    patch_json: String,
) -> Result<(), BridgeError> {
    let patch = from_json("view_preferences", &patch_json)?;
    with_core(
        move |core| async move { Ok(core.kube_view_preferences_update(context_id, patch).await?) },
    )
    .await
}
