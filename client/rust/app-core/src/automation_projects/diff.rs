//! Digest-bound, read-only Git review. No reset, fetch, push or local Git process.
use super::*;

impl AppCore {
    pub async fn automation_project_diff(
        &self,
        id: String,
        current_digest: Option<String>,
        offset: u32,
        limit: u32,
    ) -> AppResult<Value> {
        if limit == 0
            || limit > 100
            || offset > 20_000
            || current_digest
                .as_ref()
                .is_some_and(|v| !cc_models::automation::is_digest(v))
        {
            return Err(AppError::invalid("automation_diff", "invalid diff page"));
        }
        let context = self.automation_project_context(&id).await?;
        let baseline = context.baseline.clone().ok_or_else(|| {
            AppError::invalid(
                "automation_diff",
                "original Git tree unavailable; import the repository again",
            )
        })?;
        let profile =
            context.owner.dir.clone().ok_or_else(|| {
                AppError::invalid("automation_diff", "persistent profile required")
            })?;
        let source = context.root_selection.clone();
        let retained_io=managed::admit_io(self,&context).await?;
        let slot=retained_io.clone();
        let result=tokio::task::spawn_blocking(move||->AppResult<Value>{
            let _slot=slot;
            let (_,original)=baseline::open_verified(&profile,&baseline)?;
            let root=source.open()?;
            let current=root.revision().map_err(automation_error)?;
            root.recheck()?;
            if current_digest.is_some_and(|v|v!=current.digest) {return Err(automation_error(cc_automation_core::AutomationError::FileChanged));}
            let diff=cc_automation_core::compare_revisions(&original,&current);
            let total=diff.changes.len();
            Ok(json!({"baseline_digest":diff.baseline_digest,"current_digest":diff.current_digest,
                "total":total,"offset":offset,"changes":diff.changes.into_iter().skip(offset as usize).take(limit as usize).collect::<Vec<_>>()}))
        }).await.map_err(|_|AppError::internal("diff worker stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        Ok(result)
    }

    pub async fn automation_project_file_diff(
        &self,
        id: String,
        path: String,
        current_digest: String,
    ) -> AppResult<Value> {
        cc_automation_core::validate_project_path(&path).map_err(automation_error)?;
        if !cc_models::automation::is_digest(&current_digest) {
            return Err(AppError::invalid(
                "automation_diff",
                "invalid review digest",
            ));
        }
        let context = self.automation_project_context(&id).await?;
        let baseline = context.baseline.clone().ok_or_else(|| {
            AppError::invalid(
                "automation_diff",
                "original Git tree unavailable; import the repository again",
            )
        })?;
        let profile =
            context.owner.dir.clone().ok_or_else(|| {
                AppError::invalid("automation_diff", "persistent profile required")
            })?;
        let source = context.root_selection.clone();
        let retained_io=managed::admit_io(self,&context).await?;
        let slot=retained_io.clone();
        let result = tokio::task::spawn_blocking(move || -> AppResult<Value> {
            let _slot = slot;
            let (old, original) = baseline::open_verified(&profile, &baseline)?;
            let new = source.open()?;
            let current = new.revision().map_err(automation_error)?;
            if current.digest != current_digest {
                return Err(automation_error(
                    cc_automation_core::AutomationError::FileChanged,
                ));
            }
            new.recheck()?;
            let value=serde_json::to_value(
                cc_automation_core::compare_file(&old, &original, &new, &current, &path)
                    .map_err(automation_error)?,
            )
            .map_err(AppError::internal)?;
            new.recheck()?;
            Ok(value)
        })
        .await
        .map_err(|_| AppError::internal("diff worker stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        Ok(result)
    }
}
