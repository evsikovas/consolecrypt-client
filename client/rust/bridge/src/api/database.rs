//! Databases: saved connections in the vault, live sessions, guarded queries
//! (polled event batches), structure browsing and query history. JSON in and
//! out; passwords travel as bytes and never return.
use crate::api::error::BridgeError;
use crate::state::{from_json, opt_secret_string, secret_string, to_json, with_core};
use cc_db_core::{DbObjectRef, DbQueryRequest};
use cc_models::{
    database::{DbConnection, DbEngine},
    secret::SecretValue,
};
use cc_ssh_core::secrecy::SecretString;

/// A connection from the editor: a new one has an empty id and no
/// timestamps yet (the core assigns them on save).
fn connection_from_json(json: &str) -> Result<DbConnection, BridgeError> {
    let mut value: serde_json::Value = from_json("connection", json)?;
    if let Some(object) = value.as_object_mut() {
        if object.get("id").and_then(serde_json::Value::as_str) == Some("") {
            object.insert("id".into(), cc_models::ObjectId::NIL.to_string().into());
        }
        let now = serde_json::to_value(chrono::Utc::now())
            .map_err(|_| BridgeError::invalid("connection", "malformed connection"))?;
        for key in ["created_at", "updated_at"] {
            if object.get(key).is_none_or(serde_json::Value::is_null) {
                object.insert(key.into(), now.clone());
            }
        }
    }
    serde_json::from_value(value)
        .map_err(|_| BridgeError::invalid("connection", "malformed connection"))
}

pub async fn db_connections() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_connections().await?) }).await
}

pub async fn db_save_connection(
    connection_json: String,
    password: Option<Vec<u8>>,
    private_key: Option<Vec<u8>>,
) -> Result<String, BridgeError> {
    let connection = connection_from_json(&connection_json)?;
    let password = opt_secret_string("password", password)?.map(SecretValue::new);
    let private_key = opt_secret_string("private_key", private_key)?.map(SecretValue::new);
    with_core(move |c| async move {
        to_json(
            &c.db_save_connection(connection, password, private_key)
                .await?,
        )
    })
    .await
}

/// Creates a new, empty SQLite database file on this computer (the editor's
/// "create a new database"). Refuses an existing file.
pub async fn db_sqlite_create_file(path: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_sqlite_create_file(path).await?) }).await
}

pub async fn db_delete_connection(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_delete_connection(id).await?) }).await
}

pub async fn db_test_connection(
    connection_json: String,
    password: Option<Vec<u8>>,
) -> Result<String, BridgeError> {
    let connection = connection_from_json(&connection_json)?;
    let password = opt_secret_string("password", password)?.map(SecretString::from);
    with_core(move |c| async move { to_json(&c.db_test_connection(connection, password).await?) })
        .await
}

pub async fn db_connect(
    connection_id: String,
    password: Option<Vec<u8>>,
) -> Result<String, BridgeError> {
    let password = opt_secret_string("password", password)?.map(SecretString::from);
    with_core(move |c| async move { to_json(&c.db_connect(connection_id, password).await?) }).await
}

pub async fn db_disconnect(session_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_disconnect(session_id).await?) }).await
}

pub async fn db_set_session_write(session_id: String, allow: bool) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_set_session_write(session_id, allow).await?) })
        .await
}

/// Live write state of a session (production write window included).
pub async fn db_session_state(session_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_session_state(session_id).await?) }).await
}

/// Opens a temporary production write window after the vault passphrase
/// (sent as bytes) checks out.
pub async fn db_unlock_production(
    session_id: String,
    passphrase: Vec<u8>,
    minutes: u32,
) -> Result<String, BridgeError> {
    let passphrase = secret_string("passphrase", passphrase)?;
    with_core(move |c| async move {
        to_json(
            &c.db_unlock_production(session_id, passphrase, minutes)
                .await?,
        )
    })
    .await
}

pub async fn db_lock_production(session_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_lock_production(session_id).await?) }).await
}

pub async fn db_tree(
    session_id: String,
    parent_json: Option<String>,
    cursor: Option<String>,
) -> Result<String, BridgeError> {
    let parent: Option<DbObjectRef> = parent_json
        .map(|json| from_json("parent", &json))
        .transpose()?;
    with_core(move |c| async move { to_json(&c.db_tree(session_id, parent, cursor).await?) }).await
}

pub async fn db_identifier(session_id: String, object_json: String) -> Result<String, BridgeError> {
    let object: DbObjectRef = from_json("object", &object_json)?;
    with_core(move |c| async move { Ok(c.db_identifier(session_id, object).await?) }).await
}

pub async fn db_object_query(
    session_id: String,
    object_json: String,
    limit: u32,
) -> Result<String, BridgeError> {
    let object: DbObjectRef = from_json("object", &object_json)?;
    with_core(move |c| async move { Ok(c.db_object_query(session_id, object, limit).await?) }).await
}

pub async fn db_ddl(session_id: String, object_json: String) -> Result<String, BridgeError> {
    let object: DbObjectRef = from_json("object", &object_json)?;
    with_core(move |c| async move { Ok(c.db_ddl(session_id, object).await?) }).await
}

pub async fn db_explain(
    session_id: String,
    request_json: String,
    analyze: bool,
) -> Result<String, BridgeError> {
    let request: DbQueryRequest = from_json("request", &request_json)?;
    with_core(move |c| async move { to_json(&c.db_explain(session_id, request, analyze).await?) })
        .await
}

pub async fn db_complete(
    session_id: String,
    text: String,
    cursor: u32,
) -> Result<String, BridgeError> {
    with_core(
        move |c| async move { to_json(&c.db_complete(session_id, text, cursor as usize).await?) },
    )
    .await
}

pub async fn db_prepare_query(
    session_id: String,
    text: String,
    mode: String,
    selection_start: u32,
    selection_end: u32,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.db_prepare_query(
                session_id,
                text,
                mode,
                selection_start as usize,
                selection_end as usize,
            )
            .await?,
        )
    })
    .await
}

pub async fn db_format_query(engine: String, text: String) -> Result<String, BridgeError> {
    let engine: DbEngine = from_json("engine", &format!("\"{engine}\""))?;
    with_core(move |c| async move { Ok(c.db_format_query(engine, text).await?) }).await
}

pub async fn db_review(session_id: String, request_json: String) -> Result<String, BridgeError> {
    let request: DbQueryRequest = from_json("request", &request_json)?;
    with_core(move |c| async move { to_json(&c.db_review(session_id, request).await?) }).await
}

pub async fn db_execute(session_id: String, request_json: String) -> Result<String, BridgeError> {
    let request: DbQueryRequest = from_json("request", &request_json)?;
    with_core(move |c| async move { Ok(c.db_execute(session_id, request).await?) }).await
}

pub async fn db_poll(query_id: String, wait_ms: u32) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_poll(query_id, u64::from(wait_ms)).await?) })
        .await
}

pub async fn db_load_more(query_id: String, rows: u32) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_load_more(query_id, rows).await?) }).await
}

pub async fn db_cancel(query_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_cancel(query_id).await?) }).await
}

pub async fn db_release(query_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_release(query_id).await?) }).await
}

pub async fn db_history(
    connection_id: String,
    query: String,
    succeeded: Option<bool>,
    favorites_only: bool,
    cursor: Option<String>,
    limit: u32,
) -> Result<String, BridgeError> {
    with_core(move |c| async move {
        to_json(
            &c.db_history(
                connection_id,
                query,
                succeeded,
                favorites_only,
                cursor,
                limit,
            )
            .await?,
        )
    })
    .await
}

pub async fn db_history_favorite(history_id: String, favorite: bool) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_history_favorite(history_id, favorite).await?) }).await
}

pub async fn db_delete_history(connection_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_delete_history(connection_id).await?) }).await
}

pub async fn db_privileges(session_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_privileges(session_id).await?) }).await
}

/// Stages a connection string; the review never contains the password.
pub async fn db_import_uri(uri: Vec<u8>) -> Result<String, BridgeError> {
    let uri = SecretString::from(secret_string("uri", uri)?);
    with_core(move |c| async move { to_json(&c.db_import_uri(uri).await?) }).await
}

pub async fn db_import_pgpass(contents: Vec<u8>) -> Result<String, BridgeError> {
    let contents = SecretString::from(secret_string("contents", contents)?);
    with_core(move |c| async move { to_json(&c.db_import_pgpass(contents).await?) }).await
}

/// `$PGPASSFILE` or `~/.pgpass`; "null" when there is none.
pub async fn db_import_default_pgpass() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_import_default_pgpass().await?) }).await
}

pub async fn db_confirm_import(ticket: String, choices_json: String) -> Result<u32, BridgeError> {
    let choices: Vec<cc_app_core::DbImportChoice> = from_json("choices", &choices_json)?;
    with_core(move |c| async move { Ok(c.db_confirm_import(ticket, choices).await? as u32) }).await
}

pub async fn db_discard_import(ticket: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_discard_import(ticket).await?) }).await
}

pub async fn db_history_policy(connection_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_history_policy(connection_id).await?) }).await
}

pub async fn db_set_history_policy(
    connection_id: String,
    policy: String,
) -> Result<(), BridgeError> {
    let policy = from_json("policy", &format!("\"{policy}\""))?;
    with_core(move |c| async move { Ok(c.db_set_history_policy(connection_id, policy).await?) })
        .await
}

pub async fn db_save_history_snippet(
    history_id: String,
    name: String,
) -> Result<String, BridgeError> {
    with_core(move |c| async move { Ok(c.db_save_history_snippet(history_id, name).await?) }).await
}

#[derive(serde::Deserialize)]
struct Selection {
    rows: Option<Vec<u32>>,
    columns: Option<Vec<u32>>,
}

/// Loaded rows of one result formatted for the clipboard.
pub async fn db_copy_result(
    query_id: String,
    result_index: u32,
    selection_json: String,
    options_json: String,
) -> Result<String, BridgeError> {
    let selection: Selection = from_json("selection", &selection_json)?;
    let options = from_json("options", &options_json)?;
    with_core(move |c| async move {
        Ok(c.db_copy_result(
            query_id,
            result_index,
            selection.rows,
            selection.columns,
            options,
        )
        .await?)
    })
    .await
}

pub async fn db_export_start(
    query_id: String,
    result_index: u32,
    options_json: String,
    path: String,
) -> Result<String, BridgeError> {
    let options = from_json("options", &options_json)?;
    with_core(move |c| async move {
        Ok(c.db_export_start(query_id, result_index, options, path)
            .await?)
    })
    .await
}

pub async fn db_export_progress(export_id: String) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_export_progress(export_id).await?) }).await
}

pub async fn db_export_cancel(export_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_export_cancel(export_id).await?) }).await
}

pub async fn db_review_cell_edits(
    session_id: String,
    table_json: String,
    edits_json: String,
) -> Result<String, BridgeError> {
    let table: DbObjectRef = from_json("table", &table_json)?;
    let edits: Vec<cc_db_core::edit::DbCellEdit> = from_json("edits", &edits_json)?;
    with_core(
        move |c| async move { to_json(&c.db_review_cell_edits(session_id, table, edits).await?) },
    )
    .await
}

pub async fn db_commit_cell_edits(session_id: String, ticket: String) -> Result<u64, BridgeError> {
    with_core(move |c| async move { Ok(c.db_commit_cell_edits(session_id, ticket).await?) }).await
}

pub async fn db_discard_cell_edits(ticket: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_discard_cell_edits(ticket).await?) }).await
}

/// MongoDB collection description, or sampled schema with `sample` documents.
pub async fn db_inspect(
    session_id: String,
    object_json: String,
    sample: Option<u32>,
) -> Result<String, BridgeError> {
    let object: DbObjectRef = from_json("object", &object_json)?;
    with_core(move |c| async move { to_json(&c.db_inspect(session_id, object, sample).await?) })
        .await
}

pub async fn db_ai_providers() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_ai_providers().await?) }).await
}

/// Stages an assistant request; the review lists what consent would send.
pub async fn db_ai_prepare(
    session_id: String,
    provider_id: String,
    request_json: String,
) -> Result<String, BridgeError> {
    let request: cc_app_core::DbAiRequest = from_json("request", &request_json)?;
    with_core(
        move |c| async move { to_json(&c.db_ai_prepare(session_id, provider_id, request).await?) },
    )
    .await
}

pub async fn db_ai_ask(
    session_id: String,
    ticket: String,
    consent_json: String,
) -> Result<(), BridgeError> {
    let consent: cc_app_core::DbAiConsent = from_json("consent", &consent_json)?;
    with_core(move |c| async move { Ok(c.db_ai_ask(session_id, ticket, consent).await?) }).await
}

pub async fn db_ai_poll(ticket: String, wait_ms: u32) -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_ai_poll(ticket, u64::from(wait_ms)).await?) })
        .await
}

pub async fn db_ai_cancel(ticket: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_ai_cancel(ticket).await?) }).await
}

pub async fn db_ai_revoke(session_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_ai_revoke(session_id).await?) }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exactly what the connection form sends for a new connection.
    const NEW_FROM_FORM: &str = r#"{"id":"","name":"forever/forever-postgres-0","engine":"postgresql","host":"127.0.0.1","port":15432,"database":"forever","username":"postgres","auth":{"password_secret_id":null,"mechanism":"default","client_certificate":null},"tls":{"mode":"prefer","certificate_authority_pem":null,"server_name":null},"route":{"kind":"direct"},"production":false,"limits":{"max_rows":1000,"statement_timeout_seconds":60,"connect_timeout_seconds":10,"result_memory_bytes":67108864},"options":{"kind":"postgresql","application_name":"ConsoleCrypt","search_path":[]},"group_id":null,"tags":["kubernetes"],"color":null}"#;

    #[test]
    fn new_connections_from_the_form_parse() {
        let connection = connection_from_json(NEW_FROM_FORM).unwrap();
        assert_eq!(connection.id, cc_models::ObjectId::NIL);
        assert_eq!(connection.port, 15432);
        assert_eq!(connection.tls.mode, cc_models::database::DbTlsMode::Prefer);
    }
}
