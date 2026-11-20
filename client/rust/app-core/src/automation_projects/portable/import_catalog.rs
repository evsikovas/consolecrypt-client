//! SAME catalog row's private three-step transaction model. Pure metadata, no
//! capability, SQL connection, filesystem, imported IDs or automatic recovery.
use super::*;
use cc_automation_core::portable_package::PortablePackageSummary;

const FIELD: &str = "portableImport";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum ImportState {
    Preparing,
    Prepared,
    Ready,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ImportMarker {
    version: u8,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    state: ImportState,
    encrypted_sha256: String,
    portable_digest: String,
    source_digest: String,
    files: usize,
    bytes: u64,
    reservation_row_sha256: Option<String>,
    publication_fingerprint: Option<String>,
}
fn digest(value: &str) -> bool {
    cc_models::automation::is_digest(value)
}
fn marker(row: &Value) -> AppResult<ImportMarker> {
    if row.get("publication").is_some() {
        return Err(invalid());
    }
    let value = row.get(FIELD).ok_or_else(invalid)?;
    if serde_json::to_vec(value).map_err(|_| invalid())?.len() > 4096 {
        return Err(invalid());
    }
    let marker: ImportMarker = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if marker.version != 1
        || marker.project.is_nil()
        || marker.operation.is_nil()
        || marker.transaction.is_nil()
        || !digest(&marker.encrypted_sha256)
        || !digest(&marker.portable_digest)
        || !digest(&marker.source_digest)
        || marker.files > 10_000
        || marker.bytes > cc_crypto_core::MAX_PORTABLE_PROJECT_PLAIN as u64
    {
        return Err(invalid());
    }
    let expected = match marker.state {
        ImportState::Preparing => 1,
        ImportState::Prepared => 2,
        ImportState::Ready => 3,
    };
    if !(row.get("version").and_then(Value::as_u64) == Some(1)
        || marker.state == ImportState::Ready && super::super::execution_credentials::supported_row_version(row))
        || row.pointer("/source/kind").and_then(Value::as_str) != Some("managed")
        || row.get("revision").and_then(Value::as_u64).is_none_or(|v| {
            if expected == 3 {
                v < 3
            } else {
                v != expected
            }
        })
    {
        return Err(invalid());
    }
    let status = match marker.state {
        ImportState::Preparing => "importPreparing",
        ImportState::Prepared => "importPrepared",
        ImportState::Ready => "ready",
    };
    if row.pointer("/source/status").and_then(Value::as_str) != Some(status) {
        return Err(invalid());
    }
    match marker.state {
        ImportState::Preparing
            if marker.reservation_row_sha256.is_none()
                && marker.publication_fingerprint.is_none() => {}
        ImportState::Prepared | ImportState::Ready
            if marker.reservation_row_sha256.as_deref().is_some_and(digest)
                && marker
                    .publication_fingerprint
                    .as_deref()
                    .is_some_and(digest) => {}
        _ => return Err(invalid()),
    }
    Ok(marker)
}
fn pending(
    row: &Value,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    expected: ImportState,
) -> AppResult<ImportMarker> {
    let marker = marker(row)?;
    if marker.project != project
        || marker.operation != operation
        || marker.transaction != transaction
        || marker.state != expected
        || row.as_object().is_none_or(|m| {
            m.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "version"
                        | "name"
                        | "source"
                        | "revision"
                        | "created_at"
                        | "updated_at"
                        | FIELD
                )
            })
        })
        || row
            .get("source")
            .and_then(Value::as_object)
            .is_none_or(|m| {
                m.keys()
                    .any(|k| !matches!(k.as_str(), "kind" | "path" | "status"))
            })
    {
        return Err(invalid());
    }
    Ok(marker)
}
pub(super) fn reservation(
    profile: &Path,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    name: &str,
    encrypted_sha256: &str,
    summary: &PortablePackageSummary,
    now: i64,
) -> AppResult<Value> {
    if !profile.is_absolute()
        || project.is_nil()
        || operation.is_nil()
        || transaction.is_nil()
        || now <= 0
    {
        return Err(invalid());
    }
    let name = valid_name(name)?;
    let marker = ImportMarker {
        version: 1,
        project,
        operation,
        transaction,
        state: ImportState::Preparing,
        encrypted_sha256: encrypted_sha256.into(),
        portable_digest: summary.portable_digest.clone(),
        source_digest: summary.source_digest.clone(),
        files: summary.files,
        bytes: summary.bytes,
        reservation_row_sha256: None,
        publication_fingerprint: None,
    };
    let path = profile
        .join("automation")
        .join("project-revisions")
        .join(project.to_string())
        .join(format!("generation-{transaction}"));
    let row = json!({"version":1,"name":name,"source":{"kind":"managed","path":path,"status":"importPreparing"},
        "revision":1,"created_at":now,"updated_at":now,FIELD:marker});
    pending(
        &row,
        project,
        operation,
        transaction,
        ImportState::Preparing,
    )?;
    Ok(row)
}
pub(super) fn prepared(
    row: &Value,
    expected_row_sha: &str,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    fingerprint: &str,
    now: i64,
) -> AppResult<Value> {
    if now <= 0 || !digest(fingerprint) || managed::row_stamp(row)? != expected_row_sha {
        return Err(invalid());
    }
    let mut marker = pending(row, project, operation, transaction, ImportState::Preparing)?;
    marker.state = ImportState::Prepared;
    marker.reservation_row_sha256 = Some(expected_row_sha.into());
    marker.publication_fingerprint = Some(fingerprint.into());
    let mut next = row.clone();
    next[FIELD] = serde_json::to_value(marker).map_err(|_| invalid())?;
    next["revision"] = Value::from(2);
    next["updated_at"] = Value::from(now);
    next["source"]["status"] = Value::from("importPrepared");
    pending(
        &next,
        project,
        operation,
        transaction,
        ImportState::Prepared,
    )?;
    Ok(next)
}
pub(super) fn ready(
    row: &Value,
    expected_row_sha: &str,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    fingerprint: &str,
    now: i64,
) -> AppResult<Value> {
    if now <= 0 || managed::row_stamp(row)? != expected_row_sha {
        return Err(invalid());
    }
    let mut marker = pending(row, project, operation, transaction, ImportState::Prepared)?;
    if marker.publication_fingerprint.as_deref() != Some(fingerprint) {
        return Err(invalid());
    }
    marker.state = ImportState::Ready;
    let mut next = row.clone();
    next[FIELD] = serde_json::to_value(marker).map_err(|_| invalid())?;
    next["revision"] = Value::from(3);
    next["updated_at"] = Value::from(now);
    next["source"]["status"] = Value::from("ready");
    pending(&next, project, operation, transaction, ImportState::Ready)?;
    Ok(next)
}
pub(crate) fn ready_provenance(id: &str, row: &Value) -> AppResult<Option<(Uuid, String)>> {
    if row.get(FIELD).is_none() {
        return Ok(None);
    }
    let marker = marker(row)?;
    if marker.state != ImportState::Ready || marker.project.to_string() != id {
        return Err(invalid());
    }
    Ok(Some((
        marker.transaction,
        marker.publication_fingerprint.ok_or_else(invalid)?,
    )))
}
/// No marker means the legacy project behavior is unchanged. Future/pending
/// markers deny ordinary writes; nothing removes or normalizes their raw data.
pub(crate) fn ordinary_supported(row: &Value) -> bool {
    !row.as_object().is_some_and(|m| m.contains_key(FIELD))
        || marker(row).is_ok_and(|m| m.state == ImportState::Ready)
}

/// Current Native SQL metadata, not a package witness or permission. Only the
/// exact unedited prepared2/ready3 states can enter explicit recovery review.
#[derive(Clone, Serialize)]
pub(super) struct Recovery {
    pub project: Uuid,
    pub operation: Uuid,
    pub transaction: Uuid,
    pub revision: u64,
    pub row_sha: String,
    pub reservation_sha: String,
    pub fingerprint: String,
    pub name: String,
    pub encrypted_sha256: String,
    pub portable_digest: String,
}
pub(super) fn recoverable(id: &str, row: &Value) -> AppResult<Recovery> {
    let marker = marker(row)?;
    let revision = row
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(invalid)?;
    if marker.project.to_string() != id
        || !matches!(
            (&marker.state, revision),
            (ImportState::Prepared, 2) | (ImportState::Ready, 3)
        )
    {
        return Err(invalid());
    }
    let _strict = pending(
        row,
        marker.project,
        marker.operation,
        marker.transaction,
        marker.state.clone(),
    )?;
    let name = row
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if valid_name(name)? != name {
        return Err(invalid());
    }
    Ok(Recovery {
        project: marker.project,
        operation: marker.operation,
        transaction: marker.transaction,
        revision,
        row_sha: managed::row_stamp(row)?,
        reservation_sha: marker.reservation_row_sha256.ok_or_else(invalid)?,
        fingerprint: marker.publication_fingerprint.ok_or_else(invalid)?,
        name: name.into(),
        encrypted_sha256: marker.encrypted_sha256,
        portable_digest: marker.portable_digest,
    })
}

#[cfg(test)]
mod tests;
