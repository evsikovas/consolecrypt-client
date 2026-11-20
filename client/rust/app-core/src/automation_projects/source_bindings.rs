//! Device-local encrypted binding documents; large maps do not inflate Catalog.
use super::*;
use cc_automation_core::ExternalInventoryConfig;
use sha2::{Digest, Sha256};

pub(super) type SourceSelection = cc_automation_core::InventorySelection;
pub(super) fn selected_inventory(
    mut inventory: cc_automation_core::ProjectInventory,
    hosts: Vec<Uuid>,
) -> AppResult<cc_automation_core::ProjectInventory> {
    inventory.version = 2;
    inventory.selected_hosts = Some(hosts);
    inventory.normalize().map_err(automation_error)?;
    cc_automation_core::ProjectInventory::parse(
        &serde_json::to_vec(&inventory).map_err(AppError::internal)?,
    )
    .map_err(automation_error)?;
    Ok(inventory)
}

fn key(id: &str) -> String {
    format!("automation.inventory.sources.v1.{id}")
}
pub(super) fn supported_marker(row: &Value) -> bool {
    match row.get("external_inventory") {
        None => true,
        Some(value) => {
            value.as_object().is_some_and(|v| v.len() == 2)
                && value["version"] == json!(1)
                && value["sha256"]
                    .as_str()
                    .is_some_and(cc_models::automation::is_digest)
        }
    }
}
fn decode(row: &Value, payload: Option<Value>) -> AppResult<Option<ExternalInventoryConfig>> {
    if row.get("external_inventory").is_none() {
        return Ok(None);
    }
    if !supported_marker(row) {
        return Err(AppError::invalid(
            "inventory_bindings",
            "newer inventory configuration is read-only",
        ));
    }
    let payload = payload.ok_or_else(|| {
        AppError::invalid("inventory_bindings", "inventory configuration is missing")
    })?;
    let bytes = serde_json::to_vec(&payload).map_err(AppError::internal)?;
    if row["external_inventory"]["sha256"] != json!(hex::encode(Sha256::digest(&bytes))) {
        return Err(AppError::invalid(
            "inventory_bindings",
            "inventory configuration changed",
        ));
    }
    Ok(Some(
        ExternalInventoryConfig::parse(&bytes).map_err(automation_error)?,
    ))
}
pub(super) async fn load(
    owner: &crate::session::Session,
    id: &str,
    row: &Value,
) -> AppResult<Option<ExternalInventoryConfig>> {
    if row.get("external_inventory").is_none() {
        return Ok(None);
    }
    let payload: Option<Value> = owner.storage()?.setting_get(&key(id)).await?;
    decode(row, payload)
}
pub(super) async fn save(
    owner: &crate::session::Session,
    id: String,
    expected_revision: u64,
    config: ExternalInventoryConfig,
) -> AppResult<AutomationProjectSummary> {
    save_documents(owner, id, expected_revision, Some(config), None).await
}
pub(super) async fn save_documents(
    owner: &crate::session::Session,
    id: String,
    expected_revision: u64,
    config: Option<ExternalInventoryConfig>,
    inventory: Option<cc_automation_core::ProjectInventory>,
) -> AppResult<AutomationProjectSummary> {
    let payload = config
        .map(serde_json::to_value)
        .transpose()
        .map_err(AppError::internal)?;
    let hash = payload
        .as_ref()
        .map(|value| serde_json::to_vec(value).map(|bytes| hex::encode(Sha256::digest(bytes))))
        .transpose()
        .map_err(AppError::internal)?;
    let profile_path = owner.dir.clone();
    owner
        .storage()?
        .write(move |tx| {
            let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
            if catalog.version != 1 {
                return Err(AppError::invalid(
                    "inventory_bindings",
                    "newer catalog is read-only",
                ));
            }
            let row = catalog
                .records
                .get_mut(&id)
                .ok_or_else(|| AppError::not_found("automation project", &id))?;
            if !managed::supported_at(profile_path.as_deref(), &id, row)
                || row["revision"] != json!(expected_revision)
                || expected_revision == u64::MAX
            {
                return Err(AppError::invalid(
                    "inventory_bindings",
                    "project changed; preview its inventory again",
                ));
            }
            // A newer/missing body is not silently overwritten even if its Catalog
            // marker is readable. Keep both documents unchanged on every failure.
            decode(row, tx.setting_get(&key(&id))?)?;
            if let Some(inventory) = inventory {
                row["inventory"] = serde_json::to_value(inventory).map_err(AppError::internal)?;
            }
            if let Some(hash) = hash {
                row["external_inventory"] = json!({"version":1,"sha256":hash});
            } else {
                row.as_object_mut().unwrap().remove("external_inventory");
            }
            row["revision"] = json!(expected_revision + 1);
            row["updated_at"] = json!(chrono::Utc::now().timestamp());
            let result = summary(&id, row, false);
            if let Some(payload) = payload {
                tx.setting_set(&key(&id), &payload)?;
            } else {
                tx.setting_delete(&key(&id))?;
            }
            tx.setting_set(PROJECTS, &catalog)?;
            Ok::<_, AppError>(result)
        })
        .await
}
impl AppCore {
    pub async fn automation_inventory_select(
        &self,
        selection_json: String,
    ) -> AppResult<AutomationProjectSummary> {
        if selection_json.len() > 3 * 1024 * 1024 {
            return Err(AppError::invalid(
                "inventory_selection",
                "selection too large",
            ));
        }
        let selection =
            SourceSelection::parse(selection_json.as_bytes()).map_err(automation_error)?;
        if selection.version != 1 || selection.saved_hosts.len() > 10000 {
            return Err(AppError::invalid(
                "inventory_selection",
                "unsupported selection",
            ));
        }
        project_id(&selection.project_id)?;
        if let Some(config) = &selection.configuration {
            let id = selection.preview_id.clone().ok_or_else(|| {
                AppError::invalid("inventory_selection", "preview external sources first")
            })?;
            let config_json = serde_json::to_string(config).map_err(AppError::internal)?;
            return self
                .automation_inventory_commit_inner(id, config_json, Some(selection))
                .await;
        }
        if selection.preview_id.is_some() {
            return Err(AppError::invalid(
                "inventory_selection",
                "unexpected external preview",
            ));
        }
        let context = self
            .automation_project_context(&selection.project_id)
            .await?;
        if context.project.revision != Some(selection.expected_revision) {
            return Err(AppError::invalid(
                "inventory_selection",
                "project changed; reload inventory",
            ));
        }
        let inventory = selected_inventory(context.inventory, selection.saved_hosts)?;
        let unlock = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _epoch = context.owner.hold_unlocked(&unlock).await?;
        for id in inventory.selected_hosts.as_ref().unwrap() {
            crate::inventory::host_of(&unlock, crate::dto::parse_id("host_id", &id.to_string())?)?;
        }
        save_documents(
            &context.owner,
            selection.project_id,
            selection.expected_revision,
            None,
            Some(inventory),
        )
        .await
    }
    pub async fn automation_inventory_configuration(&self, id: String) -> AppResult<Value> {
        let context = self.automation_project_context(&id).await?;
        let unlock = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _epoch = context.owner.hold_unlocked(&unlock).await?;
        Ok(
            json!({"version":1,"project_id":id,"project_revision":context.project.revision,"configuration":context.external_inventory,"saved_inventory":context.inventory}),
        )
    }
    pub async fn automation_inventory_clear(
        &self,
        id: String,
        expected_revision: u64,
    ) -> AppResult<AutomationProjectSummary> {
        project_id(&id)?;
        let (owner, unlock) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&unlock).await?;
        let profile_path = owner.dir.clone();
        owner
            .storage()?
            .write(move |tx| {
                let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
                if catalog.version != 1 {
                    return Err(AppError::invalid(
                        "inventory_bindings",
                        "newer catalog is read-only",
                    ));
                }
                let row = catalog
                    .records
                    .get_mut(&id)
                    .ok_or_else(|| AppError::not_found("automation project", &id))?;
                if !managed::supported_at(profile_path.as_deref(), &id, row)
                    || row["revision"] != json!(expected_revision)
                    || expected_revision == u64::MAX
                {
                    return Err(AppError::invalid(
                        "inventory_bindings",
                        "project changed; reload its inventory",
                    ));
                }
                decode(row, tx.setting_get(&key(&id))?)?;
                if row.get("external_inventory").is_none() {
                    return Ok(summary(&id, row, false));
                }
                row.as_object_mut().unwrap().remove("external_inventory");
                row["revision"] = json!(expected_revision + 1);
                row["updated_at"] = json!(chrono::Utc::now().timestamp());
                let result = summary(&id, row, false);
                tx.setting_delete(&key(&id))?;
                tx.setting_set(PROJECTS, &catalog)?;
                Ok::<_, AppError>(result)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn saved_host_selection_is_atomic_revision_checked_and_preserves_ansible_groups() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Saved selection".into(),
            "correct horse selection fixture 42".into(),
        )
        .await
        .unwrap();
        let (owner, unlock) = app.unlocked().await.unwrap();
        let host = cc_models::host::Host::new("Allowed", "192.0.2.1");
        let host_id = host.id;
        unlock
            .writer
            .put(cc_models::VaultObject::Host(host))
            .await
            .unwrap();
        let project = app
            .automation_project_create("Select targets".into(), None)
            .await
            .unwrap();
        app.automation_project_save_inventory(
            project.id.clone(),
            1,
            json!({"version":1,"groups":{"web":[host_id]}}).to_string(),
        )
        .await
        .unwrap();
        save(&owner, project.id.clone(), 2, configuration())
            .await
            .unwrap();
        let mut request = json!({"version":1,"project_id":project.id,"expected_revision":3,"saved_hosts":[host_id],"configuration":null,"preview_id":null});
        let result = app
            .automation_inventory_select(request.to_string())
            .await
            .unwrap();
        assert_eq!(result.revision, Some(4));
        let document = app
            .automation_inventory_configuration(project.id.clone())
            .await
            .unwrap();
        assert!(document["configuration"].is_null());
        assert_eq!(document["saved_inventory"]["version"], 2);
        assert_eq!(
            document["saved_inventory"]["selected_hosts"],
            json!([host_id])
        );
        assert_eq!(
            document["saved_inventory"]["groups"]["web"],
            json!([host_id])
        );
        assert!(app
            .automation_inventory_select(request.to_string())
            .await
            .is_err());
        request["expected_revision"] = json!(4);
        request["saved_hosts"] = json!([Uuid::new_v4()]);
        assert!(app
            .automation_inventory_select(request.to_string())
            .await
            .is_err());
        assert_eq!(
            app.automation_inventory_configuration(project.id.clone())
                .await
                .unwrap(),
            document
        );
        drop(unlock);
        drop(owner);
        app.shutdown().await.unwrap();
    }
    fn configuration() -> ExternalInventoryConfig {
        ExternalInventoryConfig::parse(br#"{"version":1,"sources":[{"path":"hosts.ini","format":"ini"}],"mode":"external_only","bindings":{}}"#).unwrap()
    }
    #[tokio::test]
    async fn bindings_are_atomic_revision_checked_profile_local_and_preserve_groups() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        let password = "correct horse source bindings fixture 42";
        app.create_local_profile("Bindings".into(), password.into())
            .await
            .unwrap();
        let project = app
            .automation_project_create("Bindings project".into(), None)
            .await
            .unwrap();
        let (owner, unlock) = app.unlocked().await.unwrap();
        let result = save(&owner, project.id.clone(), 1, configuration())
            .await
            .unwrap();
        assert_eq!(result.revision, Some(2));
        assert!(save(&owner, project.id.clone(), 1, configuration())
            .await
            .is_err());
        let groups = app
            .automation_project_save_inventory(
                project.id.clone(),
                2,
                json!({"version":1,"groups":{"web":[]}}).to_string(),
            )
            .await
            .unwrap();
        assert_eq!(groups.revision, Some(3));
        assert!(app
            .automation_inventory_configuration(project.id.clone())
            .await
            .unwrap()["configuration"]
            .is_object());
        assert!(app
            .automation_inventory_clear(project.id.clone(), 2)
            .await
            .is_err());
        drop(unlock);
        drop(owner);
        app.lock().await.unwrap();
        assert!(app
            .automation_inventory_configuration(project.id.clone())
            .await
            .is_err());
        app.unlock_with_passphrase(password.into()).await.unwrap();
        assert_eq!(
            app.automation_inventory_configuration(project.id.clone())
                .await
                .unwrap()["project_revision"],
            3
        );
        app.automation_inventory_clear(project.id.clone(), 3)
            .await
            .unwrap();
        assert!(app
            .automation_inventory_configuration(project.id.clone())
            .await
            .unwrap()["configuration"]
            .is_null());
        assert_eq!(
            app.automation_project_inventory(project.id.clone())
                .await
                .unwrap()["inventory"]["groups"]["web"],
            json!([])
        );
        app.create_local_profile("Other bindings profile".into(), password.into())
            .await
            .unwrap();
        assert!(app
            .automation_inventory_configuration(project.id)
            .await
            .is_err());
        app.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn future_or_changed_binding_body_and_marker_cannot_be_overwritten() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Future bindings".into(),
            "correct horse future source bindings 42".into(),
        )
        .await
        .unwrap();
        let project = app
            .automation_project_create("Future".into(), None)
            .await
            .unwrap();
        let (owner, _) = app.unlocked().await.unwrap();
        save(&owner, project.id.clone(), 1, configuration())
            .await
            .unwrap();
        let id = project.id.clone();
        let future = json!({"version":2,"private_future_data":"preserve"});
        let retained = future.clone();
        owner
            .storage()
            .unwrap()
            .write(move |tx| tx.setting_set(&key(&id), &future))
            .await
            .unwrap();
        assert!(save(&owner, project.id.clone(), 2, configuration())
            .await
            .is_err());
        assert!(app
            .automation_inventory_clear(project.id.clone(), 2)
            .await
            .is_err());
        assert_eq!(
            owner
                .storage()
                .unwrap()
                .setting_get::<Value>(&key(&project.id))
                .await
                .unwrap()
                .unwrap(),
            retained
        );
        let id = project.id.clone();
        owner
            .storage()
            .unwrap()
            .write(move |tx| {
                let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
                catalog.records.get_mut(&id).unwrap()["external_inventory"] =
                    json!({"version":2,"sha256":"a".repeat(64)});
                tx.setting_set(PROJECTS, &catalog)
            })
            .await
            .unwrap();
        assert!(app.automation_projects().await.unwrap().projects[0].read_only);
        assert!(app.automation_inventory_clear(project.id, 2).await.is_err());
        app.shutdown().await.unwrap();
    }
}
