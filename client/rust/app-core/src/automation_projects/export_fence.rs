//! Export-specific recheck reuses project readers; no second project resolver.
use super::*;

impl AutomationProjectContext {
    /// Caller holds the current profile mutex and this context's unlock guard.
    /// Re-read SQL metadata inside those guards: project/group/source saves do
    /// not bump the vault WorkingSet version and cannot be fenced by it alone.
    pub(crate) async fn matches_inventory_export(&self, revision: u64) -> AppResult<bool> {
        let document: Catalog = self
            .owner
            .storage()?
            .setting_get(PROJECTS)
            .await?
            .unwrap_or_default();
        let Some(row) = document.records.get(&self.project.id) else {
            return Ok(false);
        };
        if document.version != 1
            || !document.extra.is_empty()
            || !supported(row)
            || row.get("revision").and_then(Value::as_u64) != Some(revision)
        {
            return Ok(false);
        }
        Ok(groups::from_row(row)? == self.inventory
            && source_bindings::load(&self.owner, &self.project.id, row).await?
                == self.external_inventory)
    }
}
