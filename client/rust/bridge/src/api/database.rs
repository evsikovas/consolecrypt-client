//! Databases: saved connections in the vault, live sessions, guarded queries
//! (polled event batches), structure browsing and query history. JSON in and
//! out; passwords travel as bytes and never return.
use crate::api::error::BridgeError;
use crate::state::{from_json, opt_secret_string, to_json, with_core};
use cc_db_core::{DbObjectRef, DbQueryRequest};
use cc_models::{
    database::{DbConnection, DbEngine},
    secret::SecretValue,
};
use cc_ssh_core::secrecy::SecretString;

pub async fn db_connections() -> Result<String, BridgeError> {
    with_core(move |c| async move { to_json(&c.db_connections().await?) }).await
}

pub async fn db_save_connection(
    connection_json: String,
    password: Option<Vec<u8>>,
    private_key: Option<Vec<u8>>,
) -> Result<String, BridgeError> {
    let connection: DbConnection = from_json("connection", &connection_json)?;
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

pub async fn db_delete_connection(id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { Ok(c.db_delete_connection(id).await?) }).await
}

pub async fn db_test_connection(
    connection_json: String,
    password: Option<Vec<u8>>,
) -> Result<String, BridgeError> {
    let connection: DbConnection = from_json("connection", &connection_json)?;
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
