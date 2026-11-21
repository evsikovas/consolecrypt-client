//! Profile-scoped lease around the imported backend's process registries.
use crate::{
    api::error::BridgeError,
    state::{from_json, to_json},
};
use cc_app_core::{AppCore, AppEvent};
use cc_wslc_core::{
    api::desk,
    models::{AppError, DeskSettings},
    wire,
};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

struct Scope {
    id: String,
    profile: String,
    cancel: CancellationToken,
    operations: RwLock<()>,
}
static SCOPE: OnceLock<Mutex<Option<Arc<Scope>>>> = OnceLock::new();
fn active() -> &'static Mutex<Option<Arc<Scope>>> {
    SCOPE.get_or_init(|| Mutex::new(None))
}
fn expired() -> BridgeError {
    BridgeError::invalid("wslc", "session_expired")
}

async fn profile(core: &AppCore) -> Result<String, BridgeError> {
    if !core.is_unlocked().await {
        return Err(cc_app_core::AppError::VaultLocked.into());
    }
    core.active_profile()
        .await?
        .map(|p| p.id)
        .ok_or_else(|| cc_app_core::AppError::NoActiveProfile.into())
}
async fn valid(core: &AppCore, scope: &Scope) -> Result<(), BridgeError> {
    if scope.cancel.is_cancelled() || profile(core).await? != scope.profile {
        Err(expired())
    } else {
        Ok(())
    }
}
async fn session(core: &AppCore, id: &str) -> Result<Arc<Scope>, BridgeError> {
    let scope = active()
        .lock()
        .await
        .as_ref()
        .filter(|s| s.id == id)
        .cloned()
        .ok_or_else(expired)?;
    valid(core, &scope).await?;
    Ok(scope)
}
fn envelope(result: Result<Value, AppError>) -> Result<String, BridgeError> {
    match result {
        Ok(value) => to_json(&serde_json::json!({"ok":value})),
        Err(mut error) => {
            // CLI diagnostics can include arguments/environment; never export them.
            error.diagnostics = None;
            to_json(&serde_json::json!({"error":error}))
        }
    }
}

pub(crate) async fn open(core: AppCore, expected_profile: String) -> Result<String, BridgeError> {
    let mut events = core.subscribe_events();
    let owner = profile(&core).await?;
    if owner != expected_profile {
        return Err(expired());
    }
    let mut current = active().lock().await;
    if let Some(old) = current.take() {
        old.cancel.cancel();
        let _exclusive = old.operations.write().await;
        desk::disconnect().await;
    }
    if profile(&core).await? != owner {
        return Err(expired());
    }
    let scope = Arc::new(Scope {
        id: uuid::Uuid::new_v4().to_string(),
        profile: owner,
        cancel: CancellationToken::new(),
        operations: RwLock::new(()),
    });
    *current = Some(scope.clone());
    drop(current);
    let watcher = scope.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = watcher.cancel.cancelled() => break,
                event = events.recv() => match event {
                    Ok(AppEvent::VaultLocked {..} | AppEvent::VaultUnlocked {..} | AppEvent::AccountDisabled) | Err(_) => break,
                    Ok(AppEvent::ProfilesChanged) if profile(&core).await.ok().as_deref() != Some(&watcher.profile) => break,
                    _ => {}
                }
            }
        }
        watcher.cancel.cancel();
        close(&watcher.id).await;
    });
    to_json(&serde_json::json!({"session_id":scope.id,"profile_id":scope.profile}))
}

pub(crate) async fn close(id: &str) {
    let mut current = active().lock().await;
    if current.as_ref().is_some_and(|scope| scope.id == id) {
        let scope = current.take().unwrap();
        scope.cancel.cancel();
        let _exclusive = scope.operations.write().await;
        desk::disconnect().await;
    }
}

pub(crate) async fn request(
    core: AppCore,
    id: String,
    operation: String,
    json: String,
    passphrase: Option<Vec<u8>>,
    presence_token: Option<String>,
) -> Result<String, BridgeError> {
    let passphrase = passphrase.map(Zeroizing::new);
    let scope = session(&core, &id).await?;
    if json.len() > 16 * 1024 * 1024 {
        return Err(BridgeError::invalid("wslc", "input_limit"));
    }
    let args: Value = from_json("wslc", &json)?;
    // Re-probing replaces the process backend; exclude all concurrent operations.
    let _exclusive;
    let _shared;
    if operation == "probe_runtime" {
        _exclusive = Some(scope.operations.write().await);
        _shared = None;
    } else {
        _exclusive = None;
        _shared = Some(scope.operations.read().await);
    }
    valid(&core, &scope).await?;
    if operation == "probe_runtime" {
        desk::disconnect().await;
    }
    if wire::requires_reveal(&operation, &args) {
        let proof = crate::state::step_up(passphrase.map(|p| p.to_vec()), presence_token)?;
        let object = format!(
            "wslc:{}/{}",
            scope.id,
            args.get("container_id")
                .or_else(|| args.get("image_id"))
                .and_then(Value::as_str)
                .unwrap_or("environment")
        );
        core.confirm_step_up("docker_reveal".into(), object, proof)
            .await?;
    }
    let work = async {
        match operation.as_str() {
            // Window startup/tray belong to the enclosing app, not its WSLc workspace.
            "autostart_enabled" | "set_autostart" => envelope(Err(AppError::new(
                "unsupported",
                "autostart",
                "Используйте настройки ConsoleCrypt",
            ))),
            "load_settings" => {
                let value = core.wslc_settings_load(&scope.profile).await?;
                let value = if value.is_null() {
                    serde_json::to_value(DeskSettings::default()).unwrap()
                } else {
                    value
                };
                envelope(Ok(value))
            }
            "save_settings" => {
                let settings: DeskSettings =
                    serde_json::from_value(args.get("settings").cloned().unwrap_or(Value::Null))
                        .map_err(|_| BridgeError::invalid("wslc", "invalid_settings"))?;
                core.wslc_settings_save(
                    &scope.profile,
                    serde_json::to_value(settings)
                        .map_err(|_| BridgeError::invalid("wslc", "invalid_settings"))?,
                )
                .await?;
                envelope(Ok(Value::Null))
            }
            _ => envelope(wire::dispatch(&operation, args).await),
        }
    };
    // OS file dialogs/PTY creation use spawn_blocking; drain them before close
    // cleans the registries so a late completion cannot leak a terminal lease.
    let blocking = matches!(
        operation.as_str(),
        "open_terminal_session"
            | "pick_folder"
            | "pick_file"
            | "pick_save_file"
            | "save_loaded_logs"
            | "download_container_file"
            | "read_compose_file"
            | "create_host_folders"
            | "open_host_folder"
            | "parse_compose"
    );
    let result = if blocking {
        work.await
    } else {
        tokio::select! { _ = scope.cancel.cancelled() => Err(expired()), result = work => result }
    };
    valid(&core, &scope).await?;
    result
}

pub(crate) async fn login(
    core: AppCore,
    id: String,
    server: Option<String>,
    username: String,
    password: Vec<u8>,
) -> Result<String, BridgeError> {
    let password = Zeroizing::new(password);
    let scope = session(&core, &id).await?;
    let _shared = scope.operations.read().await;
    valid(&core, &scope).await?;
    let password = crate::state::secret_string("password", password.to_vec())?;
    let result = tokio::select! {
        _ = scope.cancel.cancelled() => return Err(expired()),
        result = desk::registry_login(server, username, password) => result
    };
    valid(&core, &scope).await?;
    envelope(result.map(|_| Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn call(
        core: &AppCore,
        id: &str,
        operation: &str,
        args: Value,
    ) -> Result<Value, BridgeError> {
        let value = request(
            core.clone(),
            id.into(),
            operation.into(),
            args.to_string(),
            None,
            None,
        )
        .await?;
        Ok(serde_json::from_str(&value).unwrap())
    }
    #[tokio::test]
    async fn workspace_leases_and_encrypted_settings_cannot_cross_profile_or_lock() {
        let root = tempfile::tempdir().unwrap();
        let core = AppCore::new(cc_app_core::AppConfig::for_tests(
            root.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        assert!(open(core.clone(), "nonexistent".into()).await.is_err());
        core.create_local_profile(
            "WSLc test A".into(),
            "synthetic-wslc-test-passphrase".into(),
        )
        .await
        .unwrap();
        let a = core.active_profile().await.unwrap().unwrap().id;
        let response: Value =
            serde_json::from_str(&open(core.clone(), a.clone()).await.unwrap()).unwrap();
        let first = response["session_id"].as_str().unwrap().to_owned();
        let settings = serde_json::json!({"refresh_seconds":5,"templates":[{"name":"fixture","spec":{
            "image":"example.test/demo:1","ports":[],"mounts":[],"env":[{"key":"PASSWORD","value":"SYNTHETIC_WSLC_SECRET"}],
            "command":[],"auto_remove":false,"start":false
        }}]});
        call(
            &core,
            &first,
            "save_settings",
            serde_json::json!({"settings":settings}),
        )
        .await
        .unwrap();
        assert_eq!(
            call(&core, &first, "load_settings", serde_json::json!({}))
                .await
                .unwrap()["ok"]["templates"][0]["spec"]["env"][0]["value"],
            "SYNTHETIC_WSLC_SECRET"
        );
        // Secret reads fail before any runtime lookup when no step-up was given.
        assert!(call(
            &core,
            &first,
            "container_env",
            serde_json::json!({"container_id":"a".repeat(64)})
        )
        .await
        .is_err());
        core.create_local_profile(
            "WSLc test B".into(),
            "synthetic-wslc-test-passphrase".into(),
        )
        .await
        .unwrap();
        let b = core.active_profile().await.unwrap().unwrap().id;
        assert!(call(&core, &first, "load_settings", serde_json::json!({}))
            .await
            .is_err());
        assert!(core.wslc_settings_save(&a, settings.clone()).await.is_err());
        assert!(open(core.clone(), a.clone()).await.is_err());
        let response: Value = serde_json::from_str(&open(core.clone(), b).await.unwrap()).unwrap();
        let second = response["session_id"].as_str().unwrap().to_owned();
        close(&first).await; // a delayed callback from A must not close B.
        assert_eq!(
            call(&core, &second, "load_settings", serde_json::json!({}))
                .await
                .unwrap()["ok"]["templates"],
            serde_json::json!([])
        );
        core.lock().await.unwrap();
        assert!(
            call(&core, &second, "list_containers", serde_json::json!({}))
                .await
                .is_err()
        );
        close(&second).await;
        core.open_profile(a.clone()).await.unwrap();
        core.unlock_with_passphrase("synthetic-wslc-test-passphrase".into())
            .await
            .unwrap();
        assert_eq!(
            core.wslc_settings_load(&a).await.unwrap()["templates"][0]["spec"]["env"][0]["value"],
            "SYNTHETIC_WSLC_SECRET"
        );
        core.lock().await.unwrap();
        fn no_plaintext(path: &std::path::Path) {
            for entry in std::fs::read_dir(path).unwrap().flatten() {
                if entry.path().is_dir() {
                    no_plaintext(&entry.path());
                } else if let Ok(bytes) = std::fs::read(entry.path()) {
                    assert!(!bytes
                        .windows(b"SYNTHETIC_WSLC_SECRET".len())
                        .any(|window| window == b"SYNTHETIC_WSLC_SECRET"));
                }
            }
        }
        no_plaintext(root.path());
    }
}
