//! Per-profile local search index (CLIENT_SPEC §12.3, ADR-0105 §9): the
//! SQLCipher key in the SecureStore, opening/recreating the file, and the
//! documents fed from the decrypted working set.
//!
//! The index is derived data: an unreadable file (lost key, corruption,
//! schema change) is recreated and rebuilt. It is never synced.

use crate::error::{AppError, AppResult};
use crate::secrets::blocking;
use crate::working_set::WorkingSet;
use cc_models::settings::TerminalHistoryMode;
use cc_models::{ObjectKind, VaultObject};
use cc_platform_core::{ExposeSecret, SecureStore};
use cc_search_core::{DocKind, Document, IndexKey, SearchIndex};
use cc_storage_core::ProfileId;
use std::path::PathBuf;
use std::sync::Arc;
use zeroize::Zeroizing;

/// Secure-store item holding a profile's random 32-byte index key.
pub(crate) fn index_key_name(profile: ProfileId) -> String {
    format!("profiles/{profile}/search-index-key")
}

/// Load the profile's index key, creating (or replacing a malformed) one.
async fn load_or_create_key(
    store: Arc<dyn SecureStore>,
    profile: ProfileId,
) -> AppResult<IndexKey> {
    blocking(move || {
        let name = index_key_name(profile);
        if let Some(v) = store.get(&name)? {
            if let Ok(bytes) = <[u8; 32]>::try_from(v.expose_secret()) {
                let bytes = Zeroizing::new(bytes);
                return Ok(IndexKey::from_bytes(&bytes));
            }
            tracing::warn!("stored search index key is malformed; generating a new one");
        }
        let mut bytes = Zeroizing::new([0u8; 32]);
        cc_crypto_core::fill_random(bytes.as_mut())?;
        store.set(&name, bytes.as_ref())?;
        Ok(IndexKey::from_bytes(&bytes))
    })
    .await
}

/// Open the profile's index (`ProfilePaths::search_db`, SQLCipher with the
/// SecureStore key). Falls back to an in-memory index when the key or the
/// file is unavailable, so search keeps working for this unlock without
/// writing plaintext to disk.
pub(crate) async fn open(
    store: Arc<dyn SecureStore>,
    profile: ProfileId,
    path: Option<PathBuf>,
) -> AppResult<SearchIndex> {
    let on_disk = async {
        let path = path.ok_or_else(|| AppError::internal("no profile directory"))?;
        let key = load_or_create_key(store, profile).await?;
        blocking(move || {
            SearchIndex::open_or_recreate(&path, Some(&key))
                .map_err(|e| AppError::Storage(format!("search index: {e}")))
        })
        .await
    }
    .await;
    match on_disk {
        Ok(i) => Ok(i),
        Err(e) => {
            tracing::warn!(error = %e, "search index file unavailable; using an in-memory index");
            blocking(|| {
                SearchIndex::open_in_memory()
                    .map_err(|e| AppError::Storage(format!("search index: {e}")))
            })
            .await
        }
    }
}

/// Run a blocking index operation off the async runtime.
pub(crate) async fn with_index<T, F>(index: &Arc<SearchIndex>, f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(&SearchIndex) -> cc_search_core::Result<T> + Send + 'static,
{
    let idx = Arc::clone(index);
    blocking(move || f(&idx).map_err(|e| AppError::Storage(format!("search index: {e}")))).await
}

/// Terminal history is indexed unless the vault disables it.
pub(crate) fn history_enabled(ws: &WorkingSet) -> bool {
    ws.all_vault_settings()
        .into_iter()
        .max_by_key(|s| s.updated_at)
        .is_none_or(|s| s.terminal_history_mode != TerminalHistoryMode::Disabled)
}

/// Index kind of an object kind (indexed kinds only).
pub(crate) fn doc_kind(kind: ObjectKind) -> Option<DocKind> {
    match kind {
        ObjectKind::Snippet => Some(DocKind::Snippet),
        ObjectKind::Note => Some(DocKind::Note),
        ObjectKind::Host => Some(DocKind::Host),
        ObjectKind::HistoryEntry => Some(DocKind::History),
        _ => None,
    }
}

/// Document of an indexable object (hosts carry no credential data).
fn document(obj: &VaultObject, history: bool) -> Option<Document> {
    match obj {
        VaultObject::Snippet(s) => Some(Document::from_snippet(s)),
        VaultObject::Note(n) => Some(Document::from_note(n)),
        VaultObject::Host(h) => Some(Document::from_host(h)),
        VaultObject::HistoryEntry(h) if history => Some(Document::from_history(h)),
        _ => None,
    }
}

/// Every indexable document of the working set.
pub(crate) fn all_documents(ws: &WorkingSet) -> Vec<Document> {
    let history = history_enabled(ws);
    ws.objects()
        .iter()
        .filter_map(|o| document(o, history))
        .collect()
}
