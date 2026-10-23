//! Docker engines on this computer or saved SSH hosts: containers, logs
//! (polled batches), shells as terminal tabs and port forwards. JSON out.
use crate::api::error::BridgeError;
use crate::state::{to_json, with_core};

pub async fn docker_hosts() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.docker_hosts().await?) }).await
}

/// `{"outcome":"connected", ...session}` or `{"outcome":"failed","failure","detail"}`.
pub async fn docker_connect(host_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.docker_connect(host_id).await?) }).await
}

pub async fn docker_disconnect(session_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.docker_disconnect(session_id).await?) }).await
}

pub async fn docker_info(session_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.docker_info(session_id).await?) }).await
}

pub async fn docker_containers(session_id: String, all: bool) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.docker_containers(session_id, all).await?) }).await
}

/// `docker inspect` with every secret masked natively.
pub async fn docker_inspect(session_id: String, container: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.docker_inspect(session_id, container).await?) })
        .await
}

/// The full `docker inspect` after the vault passphrase (sent as bytes) or
/// a presence token for `docker_reveal` on `<session>/<container>`.
pub async fn docker_inspect_reveal(
    session_id: String,
    container: String,
    passphrase: Option<Vec<u8>>,
    presence_token: Option<String>,
) -> Result<String, BridgeError> {
    let proof = crate::state::step_up(passphrase, presence_token)?;
    with_core(move |c| async move {
        to_json(
            &c.docker_inspect_reveal(session_id, container, proof)
                .await?,
        )
    })
    .await
}

/// Whether `text` holds something the Docker views mask (log selections
/// are then copied as a secret).
#[flutter_rust_bridge::frb(sync)]
pub fn docker_contains_secret(text: String) -> bool {
    cc_app_core::docker_contains_secret(&text)
}

pub async fn docker_action(
    session_id: String,
    container: String,
    action: String,
) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.docker_action(session_id, container, action).await?) })
        .await
}

pub async fn docker_logs_start(
    session_id: String,
    container: String,
    tail: u32,
    follow: bool,
    timestamps: bool,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        Ok(
            c.docker_logs_start(session_id, container, tail, follow, timestamps)
                .await?,
        )
    })
    .await
}

/// A JSON list of lines, or `null` when the stream ended.
pub async fn docker_logs_poll(stream_id: String, wait_ms: u32) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.docker_logs_poll(stream_id, u64::from(wait_ms)).await?) },
    )
    .await
}

pub async fn docker_logs_stop(stream_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.docker_logs_stop(stream_id).await?) }).await
}

pub async fn docker_terminal_open(
    session_id: String,
    container: String,
    name: String,
    command: Vec<String>,
    cols: u32,
    rows: u32,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.docker_terminal_open(session_id, container, name, command, cols, rows)
                .await?,
        )
    })
    .await
}

pub async fn docker_forward_start(
    session_id: String,
    container: String,
    remote_port: u16,
    local_port: u16,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.docker_forward_start(session_id, container, remote_port, local_port)
                .await?,
        )
    })
    .await
}

pub async fn docker_forwards() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.docker_forwards().await?) }).await
}

pub async fn docker_forward_stop(forward_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.docker_forward_stop(forward_id).await?) }).await
}

pub async fn docker_volumes(session_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.docker_volumes(session_id).await?) }).await
}

/// Environment with secrets after the vault passphrase (sent as bytes) or
/// a presence token for `docker_reveal` on `<session>/<container>`.
pub async fn docker_reveal_environment(
    session_id: String,
    container: String,
    passphrase: Option<Vec<u8>>,
    presence_token: Option<String>,
) -> Result<String, BridgeError> {
    let proof = crate::state::step_up(passphrase, presence_token)?;
    with_core(move |c| async move {
        to_json(
            &c.docker_reveal_environment(session_id, container, proof)
                .await?,
        )
    })
    .await
}

/// The compose files of a project, read on the engine host, with secrets
/// masked (`"masked": true` per file where something was hidden).
pub async fn docker_compose_project(
    session_id: String,
    project: String,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.docker_compose_project(session_id, project).await?) },
    )
    .await
}

/// Checks the vault passphrase (sent as bytes) or a presence token for
/// `docker_compose` on `<session>/<project>`; returns a grant to edit the
/// project's files.
pub async fn docker_compose_unlock(
    session_id: String,
    project: String,
    passphrase: Option<Vec<u8>>,
    presence_token: Option<String>,
) -> Result<String, BridgeError> {
    let proof = crate::state::step_up(passphrase, presence_token)?;
    with_core(
        move |c| async move { Ok(c.docker_compose_unlock(session_id, project, proof).await?) },
    )
    .await
}

/// The full compose texts under an edit grant; `null` once it expired.
pub async fn docker_compose_reveal(
    session_id: String,
    project: String,
    grant: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(&c.docker_compose_reveal(session_id, project, grant).await?)
    })
    .await
}

/// Validates and saves edited files (`edits`: UTF-8 JSON
/// `[{"path","content","digest"}]`, zeroized here); with `apply` runs
/// `up -d` as a job. `allow_shared_dirs`: the user agreed to save in
/// directories other users can write to. `{"outcome": "saved" | "invalid"
/// | "tampered" | "unsafe_directory" | ...}`.
pub async fn docker_compose_save(
    session_id: String,
    project: String,
    grant: String,
    edits: Vec<u8>,
    apply: bool,
    allow_shared_dirs: bool,
) -> Result<String, BridgeError> {
    let mut edits = edits;
    let parsed = serde_json::from_slice::<Vec<cc_app_core::ComposeEdit>>(&edits);
    zeroize::Zeroize::zeroize(&mut edits);
    let edits = parsed.map_err(|e| {
        BridgeError::invalid(
            "edits",
            format!("malformed edits ({:?} at line {})", e.classify(), e.line()),
        )
    })?;
    with_core(move |c| async move {
        to_json(
            &c.docker_compose_save(session_id, project, grant, edits, apply, allow_shared_dirs)
                .await?,
        )
    })
    .await
}

/// New output lines of an `up` job and whether it ended.
pub async fn docker_compose_job_poll(job_id: String, wait_ms: u32) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.docker_compose_job_poll(job_id, u64::from(wait_ms))
                .await?,
        )
    })
    .await
}
