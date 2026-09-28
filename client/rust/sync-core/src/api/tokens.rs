//! Token persistence boundary.

use async_trait::async_trait;
use cc_protocol::auth::TokenPair;

/// Error from a [`TokenStore`] implementation (never contains token values).
#[derive(Debug, Clone, thiserror::Error)]
#[error("token store: {0}")]
pub struct TokenStoreError(pub String);

/// Persists the current [`TokenPair`] (e.g. in the OS keychain). The
/// [`crate::ApiClient`] calls `save` after every login/refresh (refresh
/// tokens are single-use and rotate) and `clear` when the session ends.
#[async_trait]
pub trait TokenStore: Send + Sync {
    /// Current tokens, if signed in.
    async fn load(&self) -> Result<Option<TokenPair>, TokenStoreError>;
    /// Replace the stored tokens.
    async fn save(&self, tokens: &TokenPair) -> Result<(), TokenStoreError>;
    /// Forget the tokens (logout, revoked session, reuse detected).
    async fn clear(&self) -> Result<(), TokenStoreError>;
}

/// In-memory [`TokenStore`] (tests, CLI sessions that don't persist).
#[derive(Default)]
pub struct MemoryTokenStore {
    inner: std::sync::Mutex<Option<TokenPair>>,
}

impl MemoryTokenStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for MemoryTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let has = self.inner.lock().map(|g| g.is_some()).unwrap_or(false);
        f.debug_struct("MemoryTokenStore")
            .field("has_tokens", &has)
            .finish()
    }
}

#[async_trait]
impl TokenStore for MemoryTokenStore {
    async fn load(&self) -> Result<Option<TokenPair>, TokenStoreError> {
        self.inner
            .lock()
            .map(|g| g.clone())
            .map_err(|_| TokenStoreError("poisoned".into()))
    }

    async fn save(&self, tokens: &TokenPair) -> Result<(), TokenStoreError> {
        *self
            .inner
            .lock()
            .map_err(|_| TokenStoreError("poisoned".into()))? = Some(tokens.clone());
        Ok(())
    }

    async fn clear(&self) -> Result<(), TokenStoreError> {
        *self
            .inner
            .lock()
            .map_err(|_| TokenStoreError("poisoned".into()))? = None;
        Ok(())
    }
}
