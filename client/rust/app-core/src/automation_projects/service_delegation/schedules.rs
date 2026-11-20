//! Local schedule display metadata. It cannot reconstruct a delegation/grant.
//! Written in the SAME durable transaction as original owner delivery metadata.
use super::*;
use cc_storage_core::{ServiceOwnerDelivery, ServiceOwnerIdentity, ServiceOwnerRecord, Tx};
use std::collections::BTreeMap;
const KEY: &str = "automation.service-schedule-metadata.v1";
const LIMIT: usize = 256 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Metadata {
    service_id: String,
    runner_id: String,
    schedule_id: String,
    project_id: String,
    playbook: String,
    project_revision: u64,
    schedule_revision: u64,
    owner_epoch: u64,
    policy_revision: u64,
    policy_digest: String,
    definition_digest: String,
    expires_at: u64,
    schedule: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u8,
    owner: ServiceOwnerIdentity,
    records: BTreeMap<String, Metadata>,
}
fn definition_ok(value: &Value) -> bool {
    let Some(row) = value.as_object() else {
        return false;
    };
    let Some(zone) = row.get("zone").and_then(Value::as_str) else {
        return false;
    };
    let Some(days) = row.get("weekdays").and_then(Value::as_array) else {
        return false;
    };
    row.len() == 9
        && row.get("version") == Some(&json!(1))
        && !zone.is_empty()
        && zone.len() <= 128
        && zone.is_ascii()
        && !zone.chars().any(char::is_control)
        && !zone.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_+-".contains(&b))
        })
        && row
            .get("hour")
            .and_then(Value::as_u64)
            .is_some_and(|v| v <= 23)
        && row
            .get("minute")
            .and_then(Value::as_u64)
            .is_some_and(|v| v <= 59)
        && !days.is_empty()
        && days.len() <= 7
        && days.iter().all(|v| v.as_u64().is_some_and(|v| v <= 6))
        && days
            .iter()
            .filter_map(Value::as_u64)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == days.len()
        && row.get("graceSeconds") == Some(&json!(60))
        && row.get("gapPolicy") == Some(&json!("skip"))
        && row.get("foldPolicy") == Some(&json!("first"))
        && row.get("missedPolicy") == Some(&json!("skip"))
}
impl Metadata {
    pub(super) fn from_confirmed(value: &ConfirmedServiceDelegation) -> AppResult<Option<Self>> {
        let Some(setup) = &value.template.public_setup else {
            return Ok(None);
        };
        let body = value.delegation.body();
        let result = Self {
            service_id: body.service_id.clone(),
            runner_id: value.template.runner.id.clone(),
            schedule_id: body.schedule_id.clone(),
            project_id: body.project_id.clone(),
            playbook: body.playbook.clone(),
            project_revision: body.project_revision,
            schedule_revision: body.schedule_revision,
            owner_epoch: body.owner_epoch,
            policy_revision: body.policy_revision,
            policy_digest: value.delegation.digest().map_err(|_| invalid())?,
            definition_digest: body.definition_digest.clone(),
            expires_at: body.expires_at,
            schedule: setup.definition.clone(),
        };
        result.validate()?;
        Ok(Some(result))
    }
    fn validate(&self) -> AppResult<()> {
        for raw in [
            &self.service_id,
            &self.runner_id,
            &self.schedule_id,
            &self.project_id,
        ] {
            let id = Uuid::parse_str(raw).map_err(|_| invalid())?;
            if id.is_nil() || id.to_string() != *raw {
                return Err(invalid());
            }
        }
        if self.service_id != self.runner_id
            || self.project_revision == 0
            || self.schedule_revision == 0
            || self.owner_epoch == 0
            || self.policy_revision == 0
            || self.expires_at == 0
            || self.expires_at > 253402300799
            || !cc_models::automation::is_digest(&self.policy_digest)
            || !definition_ok(&self.schedule)
            || hex::encode(sha2::Sha256::digest(
                serde_json::to_vec(&self.schedule).map_err(AppError::internal)?,
            )) != self.definition_digest
        {
            return Err(invalid());
        }
        cc_automation_core::validate_project_path(&self.playbook).map_err(|_| invalid())?;
        Ok(())
    }
    fn matches(&self, row: &ServiceOwnerRecord) -> bool {
        self.service_id == row.service_id.to_string()
            && self.policy_revision == row.policy_revision
            && self.owner_epoch == row.owner_epoch
            && self.policy_digest == row.policy_digest
    }
}
pub(super) fn put(tx: &Tx<'_>, owner: &ServiceOwnerIdentity, metadata: Metadata) -> AppResult<()> {
    metadata.validate()?;
    let row = tx
        .service_owner_snapshot(owner)?
        .records()
        .iter()
        .find(|row| metadata.matches(row))
        .cloned()
        .ok_or_else(invalid)?;
    if row.delivery != ServiceOwnerDelivery::DeliveryUnknown {
        return Err(invalid());
    }
    let mut catalog: Catalog = tx.setting_get(KEY)?.unwrap_or_else(|| Catalog {
        version: 1,
        owner: owner.clone(),
        records: BTreeMap::new(),
    });
    if catalog.version != 1 || catalog.owner != *owner || catalog.records.len() > 32 {
        return Err(invalid());
    }
    catalog
        .records
        .insert(metadata.service_id.clone(), metadata);
    if catalog.records.len() > 32
        || serde_json::to_vec(&catalog)
            .map_err(AppError::internal)?
            .len()
            > LIMIT
    {
        return Err(invalid());
    }
    tx.setting_set(KEY, &catalog)?;
    Ok(())
}
pub(super) fn list(tx: &Tx<'_>, owner: &ServiceOwnerIdentity) -> AppResult<Vec<Value>> {
    let snapshot = tx.service_owner_snapshot(owner)?;
    let catalog: Option<Catalog> = tx.setting_get(KEY)?;
    if let Some(catalog) = &catalog {
        if catalog.version != 1
            || catalog.owner != *owner
            || catalog.records.len() > 32
            || serde_json::to_vec(catalog)
                .map_err(AppError::internal)?
                .len()
                > LIMIT
        {
            return Err(invalid());
        }
        for (key, metadata) in &catalog.records {
            metadata.validate()?;
            if key != &metadata.service_id {
                return Err(invalid());
            }
        }
    }
    snapshot.records().iter().map(|row|{
        let metadata=catalog.as_ref().and_then(|c|c.records.get(&row.service_id.to_string())).filter(|m|m.matches(row));
        let delivery=match row.delivery{ServiceOwnerDelivery::PendingEnrollment=>"pendingEnrollment",
            ServiceOwnerDelivery::Acknowledged=>"acknowledged",ServiceOwnerDelivery::DeliveryUnknown=>"deliveryUnknown",
            ServiceOwnerDelivery::RevokedLocally=>"localBlocked"};
        let expiry=metadata.map(|m|chrono::DateTime::from_timestamp(m.expires_at as i64,0).ok_or_else(invalid)
            .map(|v|v.to_rfc3339_opts(chrono::SecondsFormat::Secs,true))).transpose()?;
        Ok(json!({"runnerId":row.service_id,"serviceId":row.service_id,"scheduleId":metadata.map(|m|&m.schedule_id),
            "projectId":metadata.map(|m|&m.project_id),"playbook":metadata.map(|m|&m.playbook),
            "projectRevision":metadata.map(|m|m.project_revision),"schedule":metadata.map(|m|&m.schedule),
            "expiresUtc":expiry,"policyRevision":row.policy_revision,"delivery":delivery,
            "metadataAvailable":metadata.is_some(),"remoteState":"notObserved"}))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_definition_is_closed_and_has_no_unbounded_cron_or_secret_fields() {
        let valid = json!({"version":1,"zone":"UTC","hour":12,"minute":30,"weekdays":[0,2,4],
            "graceSeconds":60,"gapPolicy":"skip","foldPolicy":"first","missedPolicy":"skip"});
        assert!(definition_ok(&valid));
        let mut extra = valid.clone();
        extra["password"] = json!("notallowed");
        assert!(!definition_ok(&extra));
        let mut duplicate = valid;
        duplicate["weekdays"] = json!([1, 1]);
        assert!(!definition_ok(&duplicate));
    }
    #[tokio::test]
    async fn actual_profile_catalog_is_atomic_read_only_and_joins_exact_delivery_without_network() {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Schedule metadata fixture".into(),
            "synthetic schedule fixture passphrase42".into(),
        )
        .await
        .unwrap();
        let (owner, u) = app.unlocked().await.unwrap();
        let identity = ServiceOwnerIdentity {
            profile_id: owner.profile_id,
            vault_id: u.vault_id,
            device_id: owner.device_id(),
            signing_fingerprint: hex::encode(sha2::Sha256::digest(
                owner.identity().public_keys().signing,
            )),
            personal: None,
        };
        let service = Uuid::new_v4();
        let schedule = serde_json::json!({"version":1,"zone":"UTC","hour":12,"minute":30,"weekdays":[0,2,4],
            "graceSeconds":60,"gapPolicy":"skip","foldPolicy":"first","missedPolicy":"skip"});
        let metadata = Metadata {
            service_id: service.to_string(),
            runner_id: service.to_string(),
            schedule_id: Uuid::new_v4().to_string(),
            project_id: Uuid::new_v4().to_string(),
            playbook: "playbook.yml".into(),
            project_revision: 1,
            schedule_revision: 1,
            owner_epoch: 1,
            policy_revision: 1,
            policy_digest: "e".repeat(64),
            definition_digest: hex::encode(sha2::Sha256::digest(
                serde_json::to_vec(&schedule).unwrap(),
            )),
            expires_at: 2000000000,
            schedule,
        };
        let row = ServiceOwnerRecord {
            service_id: service,
            node_local_profile: "cc-service-local-v1-test-only".into(),
            owner_epoch: 1,
            policy_revision: 1,
            revocation_epoch: 1,
            clock_floor: 100,
            signer_fingerprint: "b".repeat(64),
            recipient_fingerprint: "c".repeat(64),
            route_trust_digest: "d".repeat(64),
            policy_digest: "e".repeat(64),
            status_digest: "f".repeat(64),
            delivery: ServiceOwnerDelivery::DeliveryUnknown,
        };
        let frame = json!({"version":1,"profileId":owner.profile_id,"notificationScopeId":u.notices.scope_id().unwrap()});
        let storage = owner.storage().unwrap();
        let (saved_identity, saved_row, saved_metadata) =
            (identity.clone(), row.clone(), metadata.clone());
        storage
            .write_service_owner(move |tx| -> AppResult<()> {
                let previous = tx.service_owner_snapshot(&saved_identity)?;
                tx.service_owner_compare_exchange(&previous, vec![saved_row])?;
                put(tx, &saved_identity, saved_metadata)?;
                Ok(())
            })
            .await
            .unwrap();
        let result: Value = serde_json::from_str(
            &app.automation_service_schedules_json(frame.to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["records"][0]["scheduleId"], metadata.schedule_id);
        assert_eq!(result["records"][0]["projectId"], metadata.project_id);
        assert_eq!(result["records"][0]["delivery"], "deliveryUnknown");
        assert_eq!(result["records"][0]["remoteState"], "notObserved");
        assert!(!result.to_string().contains("policyDigest"));
        assert!(owner.automation.connections.lock().unwrap().is_empty());
        let acknowledge_owner = identity.clone();
        storage
            .write_service_owner(move |tx| -> AppResult<()> {
                let previous = tx.service_owner_snapshot(&acknowledge_owner)?;
                let mut rows = previous.records().to_vec();
                rows[0].delivery = ServiceOwnerDelivery::Acknowledged;
                tx.service_owner_compare_exchange(&previous, rows)?;
                Ok(())
            })
            .await
            .unwrap();
        let result: Value = serde_json::from_str(
            &app.automation_service_schedules_json(frame.to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["records"][0]["delivery"], "acknowledged");
        assert_eq!(result["records"][0]["metadataAvailable"], true);
        // A failed metadata admission rolls back the owner-row insertion too.
        let mut bad = metadata;
        let mut second = row;
        second.service_id = Uuid::new_v4();
        second.node_local_profile.push_str("-second");
        bad.service_id = second.service_id.to_string();
        bad.runner_id = bad.service_id.clone();
        bad.policy_digest = "a".repeat(64);
        let rejected_owner = identity.clone();
        assert!(
            storage
                .write_service_owner(move |tx| -> AppResult<()> {
                    let previous = tx.service_owner_snapshot(&rejected_owner)?;
                    let mut rows = previous.records().to_vec();
                    rows.push(second);
                    tx.service_owner_compare_exchange(&previous, rows)?;
                    put(tx, &rejected_owner, bad)?;
                    Ok(())
                })
                .await
                .is_err()
        );
        let result: Value = serde_json::from_str(
            &app.automation_service_schedules_json(frame.to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["records"].as_array().unwrap().len(), 1);
        let mut wrong = frame.clone();
        wrong["notificationScopeId"] = json!("retired");
        assert!(
            app.automation_service_schedules_json(wrong.to_string())
                .await
                .is_err()
        );
        drop(u);
        drop(owner);
        app.shutdown().await.unwrap();
    }
}
