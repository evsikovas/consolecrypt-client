//! Profile-scoped projects; JSON DTOs are versioned in app-core.
use crate::api::error::BridgeError;
use crate::state::{to_json, with_core};

/// Issue explicitly selected, credential-free static DATA for offline export.
pub async fn automation_inventory_export_issue(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.automation_inventory_export_issue(request_json).await?) },
    )
    .await
}

pub async fn automation_runner_cleanup_scan(request_json: String) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.automation_runner_cleanup_scan(request_json).await?) },
    )
    .await
}
pub async fn automation_runner_cleanup_plan(request_json: String) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.automation_runner_cleanup_plan(request_json).await?) },
    )
    .await
}
pub async fn automation_runner_cleanup_commit(request_json: String) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.automation_runner_cleanup_commit(request_json).await?) },
    )
    .await
}

pub async fn automation_project_create_role(
    id: String,
    name: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_create_role(id, name).await?) })
        .await
}

pub async fn automation_runner_prerequisite_terminal(
    operation_id: String,
    target_json: String,
    reviewed_command: String,
) -> Result<String, BridgeError> {
    if target_json.len() > 1024 || reviewed_command.len() > 512 {
        return Err(BridgeError::invalid(
            "automation_runner",
            "prerequisite plan too large",
        ));
    }
    let target = crate::state::from_json("automation_runner", &target_json)?;
    with_core(move |c| async move {
        Ok(
            c.automation_runner_prerequisite_terminal(operation_id, target, reviewed_command)
                .await?,
        )
    })
    .await
}

pub async fn automation_inventory_select(selection_json: String) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.automation_inventory_select(selection_json).await?) },
    )
    .await
}

pub async fn automation_runner_setup(
    operation_id: String,
    target_json: String,
    install: bool,
) -> Result<String, BridgeError> {
    if target_json.len() > 1024 {
        return Err(BridgeError::invalid(
            "automation_runner",
            "setup target too large",
        ));
    }
    let target = crate::state::from_json("automation_runner", &target_json)?;
    with_core(move |c| async move {
        to_json(
            &c.automation_runner_setup(operation_id, target, install)
                .await?,
        )
    })
    .await
}

pub async fn automation_runner_cancel_setup(operation_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_runner_cancel_setup(operation_id).await?) })
        .await
}

pub async fn automation_prepare_exit() -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_prepare_exit().await?) }).await
}

pub async fn automation_abort_exit(token: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_abort_exit(token).await?) }).await
}

pub async fn automation_commit_exit(token: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_commit_exit(token).await?) }).await
}

pub async fn automation_answer_transition(
    request_id: String,
    decision: String,
) -> Result<(), BridgeError> {
    with_core(
        move |c| async move { Ok(c.automation_answer_transition(request_id, decision).await?) },
    )
    .await
}

pub async fn automation_project_open_editor(
    id: String,
    path: String,
    expected_sha256: String,
    editor_json: Option<String>,
) -> Result<(), BridgeError> {
    let editor = editor_json
        .map(|value| {
            if value.len() > 8192 {
                return Err(BridgeError::invalid(
                    "automation_editor",
                    "application reference too large",
                ));
            }
            crate::state::from_json("automation_editor", &value)
        })
        .transpose()?;
    with_core(move |c| async move {
        Ok(
            c.automation_project_open_editor(id, path, expected_sha256, editor)
                .await?,
        )
    })
    .await
}

pub async fn automation_project_diff(
    id: String,
    current_digest: Option<String>,
    offset: u32,
    limit: u32,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_project_diff(id, current_digest, offset, limit)
                .await?,
        )
    })
    .await
}
pub async fn automation_project_file_diff(
    id: String,
    path: String,
    current_digest: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_project_file_diff(id, path, current_digest)
                .await?,
        )
    })
    .await
}

pub async fn automation_inventory_start(
    project_id: String,
    runner_id: String,
    sources_json: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_inventory_start(project_id, runner_id, sources_json)
                .await?,
        )
    })
    .await
}
pub async fn automation_inventory_page(
    id: String,
    query: String,
    offset: u32,
    limit: u16,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_inventory_page(id, query, offset, limit)
                .await?,
        )
    })
    .await
}
pub async fn automation_inventory_discard(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_inventory_discard(id).await?) }).await
}

pub async fn automation_inventory_commit(
    id: String,
    configuration_json: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_inventory_commit(id, configuration_json)
                .await?,
        )
    })
    .await
}
pub async fn automation_inventory_check(
    id: String,
    configuration_json: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(&c.automation_inventory_check(id, configuration_json).await?)
    })
    .await
}
pub async fn automation_inventory_configuration(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_inventory_configuration(id).await?) })
        .await
}
pub async fn automation_inventory_clear(
    id: String,
    expected_revision: u64,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(&c.automation_inventory_clear(id, expected_revision).await?)
    })
    .await
}

pub async fn automation_project_inventory(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_inventory(id).await?) }).await
}
pub async fn automation_project_save_inventory(
    id: String,
    expected_revision: u64,
    inventory_json: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_project_save_inventory(id, expected_revision, inventory_json)
                .await?,
        )
    })
    .await
}

pub async fn automation_project_git_start(
    name: String,
    runner_id: String,
    url: String,
    reference: String,
    ca_pem: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_project_git_start(name, runner_id, url, reference, ca_pem)
                .await?,
        )
    })
    .await
}
pub async fn automation_project_git_cancel(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_project_git_cancel(id).await?) }).await
}

pub async fn automation_ai_prepare(
    provider_id: String,
    task: String,
    path: String,
    selected_text: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_ai_prepare(provider_id, task, path, selected_text)
                .await?,
        )
    })
    .await
}
pub async fn automation_ai_discard(review_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_ai_discard(review_id).await?) }).await
}
pub async fn automation_ai_generate(review_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_ai_generate(review_id).await?) }).await
}

pub async fn automation_prepare_run(request_json: String) -> Result<String, BridgeError> {
    let request = crate::state::from_json("automation_run", &request_json)?;
    with_core(move |c| async move { to_json(&c.automation_prepare_run(request).await?) }).await
}

pub async fn automation_prepare_run_with_inputs(
    request_json: String,
    profile_id: String,
    notification_scope_id: String,
    vault_password: Option<Vec<u8>>,
    become_password: Option<Vec<u8>>,
    use_project_vault_password: bool,
) -> Result<String, BridgeError> {
    fn secret_text(bytes: Option<&[u8]>) -> Result<Option<&str>, BridgeError> {
        bytes
            .map(|value| {
                std::str::from_utf8(value).map_err(|_| {
                    cc_app_core::AppError::InvalidInput {
                        field: "automation_run".into(),
                        reason: "invalid_launch_password".into(),
                    }
                    .into()
                })
            })
            .transpose()
    }

    let vault_password = vault_password.map(zeroize::Zeroizing::new);
    let become_password = become_password.map(zeroize::Zeroizing::new);
    let request = crate::state::from_json("automation_run", &request_json)?;
    with_core(move |c| async move {
        let vault = secret_text(vault_password.as_ref().map(|value| value.as_slice()))?;
        let sudo = secret_text(become_password.as_ref().map(|value| value.as_slice()))?;
        let vault = vault.map(str::to_owned);
        let sudo = sudo.map(str::to_owned);
        drop(vault_password);
        drop(become_password);
        to_json(
            &c.automation_prepare_run_with_inputs(
                request,
                profile_id,
                notification_scope_id,
                vault,
                sudo,
                use_project_vault_password,
            )
            .await?,
        )
    })
    .await
}

pub async fn automation_discard_review(review_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_discard_review(review_id).await?) }).await
}
pub async fn automation_confirm_run(review_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_confirm_run(review_id).await?) }).await
}
/// Additional owner agreement: Native verifies the original review and rejects
/// presence tokens for this passphrase-only production policy.
pub async fn automation_confirm_owner_run(
    review_id: String,
    passphrase: Option<Vec<u8>>,
    presence_token: Option<String>,
) -> Result<String, BridgeError> {
    let proof = crate::state::step_up(passphrase, presence_token)?;
    with_core(move |c| async move { to_json(&c.automation_confirm_owner_run(review_id, proof).await?) }).await
}
pub async fn automation_jobs() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_jobs().await?) }).await
}
pub async fn automation_job_events(
    id: String,
    after: u64,
    limit: u16,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_job_events(id, after, limit).await?) })
        .await
}
pub async fn automation_cancel_job(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_cancel_job(id).await?) }).await
}
pub async fn automation_acknowledge_unknown(
    id: String,
    accept_partial_changes: bool,
) -> Result<(), BridgeError> {
    with_core(move |c| async move {
        Ok(c.automation_acknowledge_unknown(id, accept_partial_changes)
            .await?)
    })
    .await
}

pub async fn automation_runners() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_runners().await?) }).await
}

pub async fn automation_runner_disconnect(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_runner_disconnect(id).await?) }).await
}

pub async fn automation_runner_forget(
    id: String,
    expected_revision: u64,
) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_runner_forget(id, expected_revision).await?) })
        .await
}

pub async fn automation_runner_update(
    id: String,
    expected_revision: u64,
    name: String,
    configuration_json: String,
) -> Result<String, BridgeError> {
    if configuration_json.len() > 8192 {
        return Err(BridgeError::invalid(
            "automation_runner",
            "configuration too large",
        ));
    }
    let configuration = crate::state::from_json("automation_runner", &configuration_json)?;
    with_core(move |c| async move {
        to_json(
            &c.automation_runner_update(id, expected_revision, name, configuration)
                .await?,
        )
    })
    .await
}

pub async fn automation_runner_create(
    name: String,
    configuration_json: String,
) -> Result<String, BridgeError> {
    let configuration = crate::state::from_json("runner_configuration", &configuration_json)?;
    with_core(
        move |c| async move { to_json(&c.automation_runner_create(name, configuration).await?) },
    )
    .await
}

pub async fn automation_runner_connect(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_runner_connect(id).await?) }).await
}

pub async fn automation_projects() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_projects().await?) }).await
}

pub async fn automation_project_create(
    name: String,
    directory: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_create(name, directory).await?) })
        .await
}

pub async fn automation_project_rename(
    id: String,
    expected_revision: u64,
    name: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_project_rename(id, expected_revision, name)
                .await?,
        )
    })
    .await
}

pub async fn automation_project_scaffold(
    name: String,
    directory: Option<String>,
    role: Option<String>,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(&c.automation_project_scaffold(name, directory, role).await?)
    }).await
}

pub async fn automation_project_files(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_files(id).await?) }).await
}

pub async fn automation_project_file(id: String, path: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_file(id, path).await?) }).await
}

pub async fn automation_project_save(
    id: String,
    path: String,
    expected_sha256: Option<String>,
    text: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_project_save(id, path, expected_sha256, text)
                .await?,
        )
    })
    .await
}

pub async fn automation_project_move_file(
    id: String,
    path: String,
    directory: String,
    expected_sha256: String,
) -> Result<(), BridgeError> {
    with_core(move |c| async move {
        c.automation_project_move_file(id, path, directory, expected_sha256).await?;
        Ok(())
    }).await
}

/// Profile-local named Ansible group to existing ConsoleCrypt folder DATA.
/// Native re-resolves proposals; this call never selects or approves targets.
pub async fn automation_project_folder_binding_json(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.automation_project_folder_binding_json(request_json)
                .await?,
        )
    })
    .await
}

/// Explicit bounded project dependency actions. Source/network review and
/// one-use accepted-candidate publication are enforced by the Native owner.
/// No caller command, privilege/approval flag or credential resolver exists.
pub async fn automation_dependency_action_json(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_dependency_action_json(request_json).await?) })
        .await
}

/// Reference-only, original-profile/source freshness; never a credential grant.
pub async fn automation_project_vault_context_json(
    project_id: String,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { Ok(c.automation_project_vault_context_json(project_id).await?) },
    )
    .await
}
/// Fixed Native reference-file CAS; password values never cross this boundary.
pub async fn automation_project_vault_select_json(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { Ok(c.automation_project_vault_select_json(request_json).await?) },
    )
    .await
}

/// Fixed bounded observation only; original Native profile/project scope is
/// validated before reading a host or the already connected Runner.
pub async fn automation_ai_context_capture_json(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_ai_context_capture_json(request_json).await?) }).await
}
pub async fn automation_ai_context_cancel(operation_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.automation_ai_context_cancel(operation_id).await?) }).await
}
/// Reference-only selected context. Native resolves facts/group/tag references;
/// this endpoint accepts no caller-authored verified facts or execution grant.
pub async fn automation_ai_prepare_context_json(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.automation_ai_prepare_context_json(request_json).await?) }).await
}

/// Fixed read-only Windows metadata. No command/install/elevation parameters.
/// The original Core is captured once; cancellation uses the existing setup API.
pub async fn automation_windows_prerequisites_json(
    request_json: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        Ok(c.automation_windows_prerequisites_json(&request_json).await?)
    })
    .await
}

/// Original-frame project-local SSH credential references. No secret bytes.
pub async fn automation_project_credentials_context(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_credentials_context(id).await?) }).await
}
pub async fn automation_project_credentials_save(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_credentials_save(request_json).await?) }).await
}

/// Closed metadata/reference DATA. Native owns original frame and preservation admission.
pub async fn automation_project_metadata_context(id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_metadata_context(id).await?) }).await
}
pub async fn automation_project_metadata_catalog() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_metadata_catalog().await?) }).await
}
pub async fn automation_project_metadata_publish(request_json: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.automation_project_metadata_publish(request_json).await?) }).await
}
