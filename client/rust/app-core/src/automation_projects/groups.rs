//! Ansible groups live in profile/project metadata, separate from host folders.
use super::*;
use cc_automation_core::ProjectInventory;
use std::collections::BTreeSet;

pub(super) fn from_row(row: &Value) -> AppResult<ProjectInventory> {
    let inventory = match row.get("inventory") {
        None => Ok(ProjectInventory::default()),
        Some(value) => {
            ProjectInventory::parse(&serde_json::to_vec(value).map_err(AppError::internal)?)
                .map_err(automation_error)
        }
    }?;
    super::folder_bindings::validate_row(row, &inventory)?;
    Ok(inventory)
}

impl AppCore {
    pub async fn automation_project_inventory(&self, id: String) -> AppResult<Value> {
        let context = self.automation_project_context(&id).await?;
        let folders = super::folder_bindings::describe(self, &context).await?;
        let original = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&context.owner).await?;
        let _epoch = context.owner.hold_unlocked(&original).await?;
        // Resolve through the original scope before taking final guards below.
        Ok(
            json!({"version":1,"project_id":id,"project_revision":context.project.revision,"profile_id":context.owner.profile_id.to_string(),"notification_scope_id":original.notices.scope_id().ok_or(AppError::VaultLocked)?,"inventory":context.inventory,"folder_mapping":folders}),
        )
    }
    pub async fn automation_project_save_inventory(
        &self,
        id: String,
        expected_revision: u64,
        inventory_json: String,
    ) -> AppResult<AutomationProjectSummary> {
        project_id(&id)?;
        let mut inventory =
            ProjectInventory::parse(inventory_json.as_bytes()).map_err(automation_error)?;
        inventory.normalize().map_err(automation_error)?;
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _epoch = owner.hold_unlocked(&unlocked).await?;
        let profile_path = owner.dir.clone();
        let members = inventory
            .groups
            .values()
            .flatten()
            .chain(inventory.selected_hosts.iter().flatten())
            .copied()
            .collect::<BTreeSet<_>>();
        for member in members {
            crate::inventory::host_of(
                &unlocked,
                crate::dto::parse_id("host_id", &member.to_string())?,
            )?;
        }
        owner
            .storage()?
            .write(move |tx| {
                let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap_or_default();
                if catalog.version != 1 {
                    return Err(AppError::invalid(
                        "automation_inventory",
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
                        "automation_inventory",
                        "project changed; reload its groups",
                    ));
                }
                super::folder_bindings::validate_row(row, &inventory)?;
                row["inventory"] = serde_json::to_value(inventory).map_err(AppError::internal)?;
                row["revision"] = json!(expected_revision + 1);
                row["updated_at"] = json!(chrono::Utc::now().timestamp());
                let result = summary(&id, row, false);
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
    async fn groups_are_profile_local_revision_checked_and_never_move_host_folders() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        let password = "correct horse project groups fixture 42";
        app.create_local_profile("Groups fixture".into(), password.into())
            .await
            .unwrap();
        let (owner, unlocked) = app.unlocked().await.unwrap();
        let host = cc_models::host::Host::new("Group member", "192.0.2.1");
        let host_id = host.id;
        unlocked
            .writer
            .put(cc_models::VaultObject::Host(host.clone()))
            .await
            .unwrap();
        let project = app
            .automation_project_create("Group fixture".into(), None)
            .await
            .unwrap();
        let document =
            json!({"version":1,"groups":{"web":[host_id],"readers":[host_id]}}).to_string();
        let changed = app
            .automation_project_save_inventory(project.id.clone(), 1, document.clone())
            .await
            .unwrap();
        assert_eq!(changed.revision, Some(2));
        assert!(app
            .automation_project_save_inventory(project.id.clone(), 1, document.clone())
            .await
            .is_err());
        assert_eq!(
            app.automation_project_inventory(project.id.clone())
                .await
                .unwrap()["inventory"]["groups"]["web"],
            json!([host_id])
        );
        assert_eq!(unlocked.working().host(host_id).unwrap(), host);
        let missing = json!({"version":1,"groups":{"web":[Uuid::new_v4()]}}).to_string();
        assert!(app
            .automation_project_save_inventory(project.id.clone(), 2, missing)
            .await
            .is_err());
        drop(unlocked);
        drop(owner);
        app.lock().await.unwrap();
        assert!(app
            .automation_project_inventory(project.id.clone())
            .await
            .is_err());
        app.unlock_with_passphrase(password.into()).await.unwrap();
        assert_eq!(
            app.automation_project_inventory(project.id.clone())
                .await
                .unwrap()["project_revision"],
            2
        );
        app.create_local_profile("Other profile".into(), password.into())
            .await
            .unwrap();
        assert!(app.automation_project_inventory(project.id).await.is_err());
        app.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn future_inventory_is_preserved_and_cannot_be_overwritten() {
        let folder = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            folder.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Future groups".into(),
            "correct horse future groups fixture 42".into(),
        )
        .await
        .unwrap();
        let project = app
            .automation_project_create("Future groups".into(), None)
            .await
            .unwrap();
        let (owner, _) = app.unlocked().await.unwrap();
        let id = project.id.clone();
        let future = json!({"version":2,"opaque":{"keep":true}});
        let saved = future.clone();
        owner
            .storage()
            .unwrap()
            .write(move |tx| {
                let mut catalog: Catalog = tx.setting_get(PROJECTS)?.unwrap();
                catalog.records.get_mut(&id).unwrap()["inventory"] = saved;
                tx.setting_set(PROJECTS, &catalog)
            })
            .await
            .unwrap();
        assert!(app.automation_projects().await.unwrap().projects[0].read_only);
        assert!(app
            .automation_project_save_inventory(
                project.id.clone(),
                1,
                json!({"version":1,"groups":{}}).to_string()
            )
            .await
            .is_err());
        let retained: Catalog = owner
            .storage()
            .unwrap()
            .setting_get(PROJECTS)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retained.records[&project.id]["inventory"], future);
        app.shutdown().await.unwrap();
    }
}
