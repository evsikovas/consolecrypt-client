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

pub async fn docker_inspect(session_id: String, container: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.docker_inspect(session_id, container).await?) })
        .await
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

/// Environment with secrets after the vault passphrase (sent as bytes).
pub async fn docker_reveal_environment(
    session_id: String,
    container: String,
    passphrase: Vec<u8>,
) -> Result<String, BridgeError> {
    let passphrase = crate::state::secret_string("passphrase", passphrase)?;
    with_core(move |c| async move {
        to_json(
            &c.docker_reveal_environment(session_id, container, passphrase)
                .await?,
        )
    })
    .await
}
