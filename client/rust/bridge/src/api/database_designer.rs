//! Table designer: dialect catalogue, DDL planning, the current structure of
//! a table and applying a reviewed designer script. JSON in and out.
use crate::api::error::BridgeError;
use crate::state::{from_json, to_json, with_core};
use cc_db_core::{designer::TableDesign, DbObjectRef, DbQueryRequest};
use cc_models::database::DbEngine;

fn engine(value: &str) -> Result<DbEngine, BridgeError> {
    from_json(
        "engine",
        &serde_json::Value::String(value.to_owned()).to_string(),
    )
}

/// JSON of the designer's `DialectInfo` for an engine.
pub async fn db_design_dialect(engine_name: String) -> Result<String, BridgeError> {
    let engine = engine(&engine_name)?;
    with_core(move |c| async move { to_json(&c.db_design_dialect(engine)?) }).await
}

/// JSON `DesignPlan` for a design (`original_json`: the loaded structure in
/// alter mode, or None to create).
pub async fn db_design_plan(
    engine_name: String,
    original_json: Option<String>,
    design_json: String,
) -> Result<String, BridgeError> {
    let engine = engine(&engine_name)?;
    let original: Option<TableDesign> = original_json
        .map(|json| from_json("original", &json))
        .transpose()?;
    let design: TableDesign = from_json("design", &design_json)?;
    with_core(move |c| async move { to_json(&c.db_design_plan(engine, original, design)) }).await
}

/// JSON `TableDesign` of an existing table.
pub async fn db_table_design(
    session_id: String,
    table_json: String,
) -> Result<String, BridgeError> {
    let table: DbObjectRef = from_json("table", &table_json)?;
    with_core(move |c| async move { to_json(&c.db_table_design(session_id, table).await?) }).await
}

/// Runs a reviewed designer script (the request carries its review ticket);
/// JSON `DesignApplyResult`.
pub async fn db_apply_design(
    session_id: String,
    request_json: String,
) -> Result<String, BridgeError> {
    let request: DbQueryRequest = from_json("request", &request_json)?;
    with_core(move |c| async move { to_json(&c.db_apply_design(session_id, request).await?) }).await
}
