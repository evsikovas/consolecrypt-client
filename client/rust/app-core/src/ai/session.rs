//! [`AiSession`]: AI state of one unlocked vault (lives in
//! [`crate::session::Unlocked`], so lock / close drops all of it).
//!
//! * the per-profile search index, rebuilt from the working set when opened
//!   and kept current from [`AppEvent::ObjectsChanged`];
//! * the provider cache (one `LlmProvider` per config revision; the API key
//!   is read from its Secret object here, at construction, and nowhere else);
//! * the background [`EmbeddingSync`] for the default provider's embedding
//!   model (cancellable, restarted when that config changes);
//! * in-memory conversations, AI proposals, approval tokens and cancellable
//!   requests.

use super::dto::{AiIndexStateDto, AiIndexStatusDto};
use super::index::{self, doc_kind, with_index};
use crate::dto::{parse_id, AppEvent, ExecResultDto};
use crate::error::{AppError, AppResult};
use crate::session::AppCtx;
use crate::ssh::SshRuntime;
use crate::writer::VaultWriter;
use async_trait::async_trait;
use cc_ai_core::policy::{ApprovedCommand, RunProposal};
use cc_ai_core::provider::LlmProvider;
use cc_ai_core::{AiError, CancellationToken, Conversation, EmbeddingSync, PrivacyProfile};
use cc_models::ai::{AiConversation, AiProviderConfig};
use cc_models::secret::SecretKind;
use cc_models::{ObjectId, ObjectKind, VaultObject};
use cc_search_core::SearchIndex;
use cc_storage_core::ProfileId;
use cc_terminal_core::TerminalId;
use secrecy::SecretString;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Notify};
use tokio::task::JoinHandle;

/// How long an approval token stays valid.
pub(crate) const APPROVAL_TTL: Duration = Duration::from_secs(300);
/// How long an AI proposal is remembered for `approve_run`.
const PROPOSAL_TTL: Duration = Duration::from_secs(3600);
const MAX_PROPOSALS: usize = 256;
/// Retry delay of the embedding sync after a failure.
const EMBED_RETRY: Duration = Duration::from_secs(60);
/// How long `shutdown` waits for background work / in-flight users.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Cancel a background task and wait for it to finish its in-flight work
/// (aborting only if it does not stop in time).
async fn stop_task(cancel: CancellationToken, mut handle: JoinHandle<()>) {
    cancel.cancel();
    if tokio::time::timeout(SHUTDOWN_WAIT, &mut handle)
        .await
        .is_err()
    {
        tracing::warn!("AI background task did not stop in time; aborting");
        handle.abort();
    }
}

// ---- execution backend ---------------------------------------------------------------

/// Where approved commands run and where terminal hooks come from (the SSH
/// runtime; a fake in unit tests).
#[async_trait]
pub(crate) trait ExecBackend: Send + Sync {
    async fn exec(&self, host_id: ObjectId, command: &str) -> AppResult<ExecResultDto>;
    fn terminal_host(&self, id: TerminalId) -> AppResult<ObjectId>;
    async fn terminal_write(&self, id: TerminalId, data: &[u8]) -> AppResult<()>;
    fn last_command(&self, id: TerminalId) -> Option<String>;
    fn last_error(&self, id: TerminalId) -> Option<String>;
}

/// [`ExecBackend`] over the session's SSH runtime.
struct SshExecBackend {
    ssh: Arc<SshRuntime>,
}

#[async_trait]
impl ExecBackend for SshExecBackend {
    async fn exec(&self, host_id: ObjectId, command: &str) -> AppResult<ExecResultDto> {
        self.ssh.exec(host_id, command).await
    }
    fn terminal_host(&self, id: TerminalId) -> AppResult<ObjectId> {
        Ok(self.ssh.terminals.info(id)?.host_id)
    }
    async fn terminal_write(&self, id: TerminalId, data: &[u8]) -> AppResult<()> {
        Ok(self.ssh.terminals.write(id, data).await?)
    }
    fn last_command(&self, id: TerminalId) -> Option<String> {
        self.ssh.terminals.last_command(id)
    }
    fn last_error(&self, id: TerminalId) -> Option<String> {
        self.ssh.terminals.last_error(id)
    }
}

// ---- state types -----------------------------------------------------------------------

struct CachedProvider {
    config: AiProviderConfig,
    provider: Arc<dyn LlmProvider>,
}

/// An Ask AI conversation: ai-core's sanitizer session + history, and the
/// plaintext record (persisted when the vault settings allow it).
/// A conversation of this unlock (the provider id is immutable, readable
/// without waiting for a running answer).
#[derive(Clone)]
pub(crate) struct ConvHandle {
    pub provider_id: ObjectId,
    pub state: Arc<tokio::sync::Mutex<ConversationState>>,
}

pub(crate) struct ConversationState {
    pub conv: Conversation,
    pub record: AiConversation,
    pub persisted: bool,
}

struct StoredProposal {
    run: RunProposal,
    at: Instant,
}

/// An approved command waiting to be run (single use).
pub(crate) struct StoredApproval {
    pub approved: ApprovedCommand,
    pub host_id: ObjectId,
    expires: Instant,
}

/// What the index currently reflects: the working-set version it was
/// synced at and the content hash of every indexed document.
#[derive(Default)]
struct Indexed {
    version: u64,
    hashes: HashMap<ObjectId, String>,
}

struct EmbedTask {
    fingerprint: String,
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

/// AI state of an unlocked vault.
pub(crate) struct AiSession {
    ctx: Arc<AppCtx>,
    profile_id: ProfileId,
    search_path: Option<PathBuf>,
    pub(crate) writer: VaultWriter,
    backend: RwLock<Arc<dyn ExecBackend>>,
    index: Mutex<Option<Arc<SearchIndex>>>,
    init: tokio::sync::Mutex<()>,
    writes: tokio::sync::Mutex<()>,
    indexed: Mutex<Indexed>,
    providers: Mutex<HashMap<ObjectId, CachedProvider>>,
    conversations: Mutex<HashMap<String, ConvHandle>>,
    proposals: Mutex<HashMap<String, StoredProposal>>,
    approvals: Mutex<HashMap<String, StoredApproval>>,
    requests: Mutex<HashMap<String, CancellationToken>>,
    embed: tokio::sync::Mutex<Option<EmbedTask>>,
    indexer: Mutex<Option<JoinHandle<()>>>,
    dirty: Notify,
    closed: CancellationToken,
    events: Mutex<Option<broadcast::Receiver<AppEvent>>>,
    status: Mutex<AiIndexStatusDto>,
}

impl std::fmt::Debug for AiSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiSession")
            .field("profile_id", &self.profile_id)
            .field("closed", &self.closed.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl AiSession {
    /// Create the state and subscribe to app events **before** the first
    /// index build reads the working set, so no change is missed.
    pub(crate) fn new(
        ctx: Arc<AppCtx>,
        profile_id: ProfileId,
        search_path: Option<PathBuf>,
        writer: VaultWriter,
        ssh: Arc<SshRuntime>,
    ) -> Arc<Self> {
        let events = ctx.events.subscribe();
        Arc::new(Self {
            ctx,
            profile_id,
            search_path,
            writer,
            backend: RwLock::new(Arc::new(SshExecBackend { ssh })),
            index: Mutex::new(None),
            init: tokio::sync::Mutex::new(()),
            writes: tokio::sync::Mutex::new(()),
            indexed: Mutex::new(Indexed::default()),
            providers: Mutex::new(HashMap::new()),
            conversations: Mutex::new(HashMap::new()),
            proposals: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            requests: Mutex::new(HashMap::new()),
            embed: tokio::sync::Mutex::new(None),
            indexer: Mutex::new(None),
            dirty: Notify::new(),
            closed: CancellationToken::new(),
            events: Mutex::new(Some(events)),
            status: Mutex::new(AiIndexStatusDto::building()),
        })
    }

    /// Start the indexer (open + rebuild, then follow object events). The
    /// handle is aborted on lock.
    pub(crate) fn start(self: &Arc<Self>) {
        let handle = tokio::spawn(Arc::clone(self).run_indexer());
        *lock(&self.indexer) = Some(handle);
    }

    /// Lock / close: cancel everything and drop the index, providers,
    /// conversations, proposals and approvals.
    ///
    /// Background work is cancelled and **awaited**, not aborted: an
    /// aborted task would leave its blocking index operation running with a
    /// reference to the connection, and the SQLCipher connection must be
    /// closed before `shutdown` returns (closing it during process exit can
    /// crash; see `AppCore::shutdown`).
    pub(crate) async fn shutdown(&self) {
        self.closed.cancel();
        for (_, t) in lock(&self.requests).drain() {
            t.cancel();
        }
        let embed = self.embed.lock().await.take();
        if let Some(t) = embed {
            stop_task(t.cancel, t.handle).await;
        }
        let indexer = lock(&self.indexer).take();
        if let Some(h) = indexer {
            stop_task(self.closed.clone(), h).await;
        }
        // Wait for an open / rebuild started by a facade call.
        let _init = self.init.lock().await;
        let index = lock(&self.index).take();
        lock(&self.providers).clear();
        lock(&self.conversations).clear();
        lock(&self.proposals).clear();
        lock(&self.approvals).clear();
        if let Some(index) = index {
            // In-flight searches / answers hold clones; let them finish.
            let deadline = Instant::now() + SHUTDOWN_WAIT;
            while Arc::strong_count(&index) > 1 && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let _ = tokio::task::spawn_blocking(move || drop(index)).await;
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.is_cancelled()
    }

    fn check_open(&self) -> AppResult<()> {
        if self.is_closed() {
            Err(AppError::VaultLocked)
        } else {
            Ok(())
        }
    }

    pub(crate) fn backend(&self) -> Arc<dyn ExecBackend> {
        self.backend
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn set_backend(&self, backend: Arc<dyn ExecBackend>) {
        *self.backend.write().unwrap_or_else(|p| p.into_inner()) = backend;
    }

    pub(crate) fn working(&self) -> &Arc<crate::working_set::WorkingSet> {
        &self.writer.working
    }

    // ---- index -------------------------------------------------------------------------

    /// The open index, if any (never blocks).
    pub(crate) fn index_if_open(&self) -> Option<Arc<SearchIndex>> {
        lock(&self.index).clone()
    }

    /// The index, opening and rebuilding it on first use.
    pub(crate) async fn index(&self) -> AppResult<Arc<SearchIndex>> {
        self.check_open()?;
        if let Some(i) = self.index_if_open() {
            return Ok(i);
        }
        let _g = self.init.lock().await;
        self.check_open()?;
        if let Some(i) = self.index_if_open() {
            return Ok(i);
        }
        let opened = index::open(
            self.ctx.secure.clone(),
            self.profile_id,
            self.search_path.clone(),
        )
        .await;
        let idx = match opened {
            Ok(i) => Arc::new(i),
            Err(e) => {
                self.set_state(AiIndexStateDto::Failed, Some(e.to_string()));
                self.emit_status().await;
                return Err(e);
            }
        };
        let was_empty = with_index(&idx, |i| i.needs_rebuild()).await?;
        // Always reconcile with the working set on open (cheap; unchanged
        // documents keep their embeddings). `needs_rebuild` only tells
        // whether the file was fresh/recreated.
        self.rebuild_into(&idx).await?;
        tracing::debug!(fresh = was_empty, "search index opened");
        if self.is_closed() {
            return Err(AppError::VaultLocked);
        }
        *lock(&self.index) = Some(idx.clone());
        self.set_state(AiIndexStateDto::Ready, None);
        self.emit_status().await;
        self.dirty.notify_one();
        Ok(idx)
    }

    async fn rebuild_into(&self, idx: &Arc<SearchIndex>) -> AppResult<()> {
        let _w = self.writes.lock().await;
        let version = self.working().version();
        let docs = index::all_documents(self.working());
        let hashes: HashMap<ObjectId, String> =
            docs.iter().map(|d| (d.id, d.content_hash())).collect();
        let stats = with_index(idx, move |i| i.rebuild(docs)).await?;
        *lock(&self.indexed) = Indexed { version, hashes };
        tracing::debug!(
            documents = stats.documents,
            embeddings_kept = stats.embeddings_kept,
            "search index rebuilt from the working set"
        );
        Ok(())
    }

    /// Full rebuild (explicit re-index).
    pub(crate) async fn rebuild(&self) -> AppResult<()> {
        let fresh = self.index_if_open().is_none();
        let idx = self.index().await?;
        if !fresh {
            self.rebuild_into(&idx).await?;
            self.emit_status().await;
            self.dirty.notify_one();
        }
        Ok(())
    }

    /// The index, brought up to date with the working set first. Local
    /// writes bump the working-set version before the facade returns, so a
    /// search right after a save sees it (read-your-writes) without waiting
    /// for the object event.
    pub(crate) async fn fresh_index(&self) -> AppResult<Arc<SearchIndex>> {
        let idx = self.index().await?;
        if lock(&self.indexed).version != self.working().version() {
            self.reconcile(&idx).await?;
        }
        Ok(idx)
    }

    /// Diff the working set against what is indexed (content hashes) and
    /// apply upserts / deletions. Cheap when nothing changed.
    async fn reconcile(&self, idx: &Arc<SearchIndex>) -> AppResult<()> {
        let _w = self.writes.lock().await;
        let version = self.working().version();
        if lock(&self.indexed).version == version {
            return Ok(());
        }
        let docs = index::all_documents(self.working());
        let mut hashes: HashMap<ObjectId, String> = HashMap::with_capacity(docs.len());
        let (upserts, deletes) = {
            let known = lock(&self.indexed);
            let mut upserts = Vec::new();
            for d in docs {
                let h = d.content_hash();
                if known.hashes.get(&d.id) != Some(&h) {
                    upserts.push(d.clone());
                }
                hashes.insert(d.id, h);
            }
            let deletes: Vec<ObjectId> = known
                .hashes
                .keys()
                .filter(|id| !hashes.contains_key(id))
                .copied()
                .collect();
            (upserts, deletes)
        };
        let changed = !upserts.is_empty() || !deletes.is_empty();
        if changed {
            with_index(idx, move |i| {
                i.upsert_many(upserts.iter())?;
                for id in deletes {
                    i.delete(id)?;
                }
                Ok(())
            })
            .await?;
        }
        *lock(&self.indexed) = Indexed { version, hashes };
        if changed {
            self.dirty.notify_one();
        }
        Ok(())
    }

    async fn run_indexer(self: Arc<Self>) {
        let Some(mut rx) = lock(&self.events).take() else {
            return;
        };
        if let Err(e) = self.index().await {
            tracing::warn!(error = %e, "search index not available");
        }
        self.ensure_embedding().await;
        loop {
            let ev = tokio::select! {
                () = self.closed.cancelled() => break,
                ev = rx.recv() => ev,
            };
            let reindex = match ev {
                Ok(AppEvent::ObjectsChanged { kind, .. }) => match kind {
                    ObjectKind::AiProvider => {
                        self.prune_providers();
                        self.ensure_embedding().await;
                        false
                    }
                    // History indexing follows the vault settings.
                    ObjectKind::VaultSettings => true,
                    k => doc_kind(k).is_some(),
                },
                Ok(_) => false,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    self.prune_providers();
                    self.ensure_embedding().await;
                    true
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
            if reindex {
                if let Err(e) = self.fresh_index().await {
                    tracing::warn!(error = %e, "search index update failed");
                }
            }
        }
    }

    // ---- status ------------------------------------------------------------------------

    fn set_state(&self, state: AiIndexStateDto, error: Option<String>) {
        let mut s = lock(&self.status);
        s.state = state;
        if error.is_some() || state == AiIndexStateDto::Ready {
            s.last_error = error;
        }
    }

    fn set_last_error(&self, error: Option<String>) {
        lock(&self.status).last_error = error;
    }

    /// Current status (counts read from the index).
    pub(crate) async fn status(&self) -> AiIndexStatusDto {
        if let Some(idx) = self.index_if_open() {
            let r = with_index(&idx, |i| Ok((i.embedding_stats()?, i.embedding_model()?))).await;
            if let Ok((stats, model)) = r {
                let mut s = lock(&self.status);
                s.documents = u32::try_from(stats.documents).unwrap_or(u32::MAX);
                s.embedded = u32::try_from(stats.embedded).unwrap_or(u32::MAX);
                s.embedding_model = model.map(|m| m.id);
            }
        }
        lock(&self.status).clone()
    }

    async fn emit_status(&self) {
        let s = self.status().await;
        if !self.is_closed() {
            self.ctx.emit(AppEvent::AiIndexStatus(s));
        }
    }

    // ---- providers -----------------------------------------------------------------------

    /// The default provider: the one flagged `is_default`, else the only one.
    pub(crate) fn default_config(&self) -> Option<AiProviderConfig> {
        let all = self.working().ai_providers();
        if let Some(d) = all.iter().find(|p| p.is_default) {
            return Some(d.clone());
        }
        match all.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        }
    }

    /// Provider config by id, or the default one.
    pub(crate) fn resolve_config(&self, id: Option<&str>) -> AppResult<AiProviderConfig> {
        match id.map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => {
                let oid = parse_id("provider_id", id)?;
                self.working()
                    .ai_provider(oid)
                    .ok_or_else(|| AppError::not_found("ai provider", oid))
            }
            None => self.default_config().ok_or(AppError::AiNotConfigured),
        }
    }

    /// The provider for `cfg` (cached per config revision). The API key is
    /// decrypted from its Secret object only here and handed to
    /// `build_provider`, which keeps it solely in the HTTP client's
    /// sensitive `Authorization` header.
    pub(crate) async fn provider_for(
        &self,
        cfg: &AiProviderConfig,
    ) -> AppResult<Arc<dyn LlmProvider>> {
        self.check_open()?;
        if let Some(c) = lock(&self.providers).get(&cfg.id) {
            if c.config == *cfg {
                return Ok(c.provider.clone());
            }
        }
        let api_key = match cfg.api_key_secret_id {
            Some(id) => Some(self.api_key(id).await?),
            None => None,
        };
        let provider = cc_ai_core::build_provider(cfg, api_key)?;
        lock(&self.providers).insert(
            cfg.id,
            CachedProvider {
                config: cfg.clone(),
                provider: provider.clone(),
            },
        );
        Ok(provider)
    }

    async fn api_key(&self, secret_id: ObjectId) -> AppResult<SecretString> {
        let secret = self.writer.read_secret(secret_id).await?;
        if secret.kind != SecretKind::ApiKey {
            return Err(AppError::invalid(
                "api_key_secret_id",
                "does not reference an API key",
            ));
        }
        Ok(SecretString::from(secret.value.expose_secret()))
    }

    /// Drop cached providers whose config changed or was deleted.
    fn prune_providers(&self) {
        let current: HashMap<ObjectId, AiProviderConfig> = self
            .working()
            .ai_providers()
            .into_iter()
            .map(|p| (p.id, p))
            .collect();
        lock(&self.providers).retain(|id, c| current.get(id) == Some(&c.config));
    }

    // ---- embeddings ----------------------------------------------------------------------

    fn embed_fingerprint(cfg: &AiProviderConfig) -> String {
        format!(
            "{}|{:?}|{}|{:?}|{:?}|{}",
            cfg.id,
            cfg.provider,
            cfg.base_url,
            cfg.embedding_model,
            cfg.api_key_secret_id,
            cfg.timeout_secs
        )
    }

    /// (Re)start the background embedding sync for the default provider's
    /// embedding model; stop it when there is none.
    pub(crate) async fn ensure_embedding(self: &Arc<Self>) {
        if self.is_closed() {
            return;
        }
        let mut slot = self.embed.lock().await;
        if self.is_closed() {
            return;
        }
        let cfg = self
            .default_config()
            .filter(|c| c.embedding_model.is_some());
        let fp = cfg.as_ref().map(Self::embed_fingerprint);
        if let (Some(t), Some(fp)) = (slot.as_ref(), fp.as_ref()) {
            if &t.fingerprint == fp && !t.handle.is_finished() {
                return;
            }
        }
        if let Some(old) = slot.take() {
            stop_task(old.cancel, old.handle).await;
        }
        let (Some(cfg), Some(fingerprint)) = (cfg, fp) else {
            return;
        };
        let provider = match self.provider_for(&cfg).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "embedding provider unavailable");
                self.set_last_error(Some(e.to_string()));
                return;
            }
        };
        if provider.embedding_model_id().is_none() || self.is_closed() {
            return;
        }
        let cancel = self.closed.child_token();
        let handle = tokio::spawn(Arc::clone(self).embed_loop(provider, cancel.clone()));
        *slot = Some(EmbedTask {
            fingerprint,
            cancel,
            handle,
        });
    }

    async fn embed_loop(
        self: Arc<Self>,
        provider: Arc<dyn LlmProvider>,
        cancel: CancellationToken,
    ) {
        loop {
            let wait_for_change = match self.index().await {
                Ok(idx) => {
                    let sync = EmbeddingSync::new(provider.clone(), idx)
                        .with_batch_size(32)
                        .with_max_batches(8);
                    match sync.run(Some(&cancel)).await {
                        Ok(stats) => {
                            self.set_last_error(None);
                            self.emit_status().await;
                            if stats.remaining > 0 && stats.embedded > 0 {
                                continue;
                            }
                            None
                        }
                        Err(AiError::Cancelled) => return,
                        Err(e) => {
                            tracing::warn!(error = %e, "embedding sync failed");
                            self.set_last_error(Some(e.to_string()));
                            self.emit_status().await;
                            Some(EMBED_RETRY)
                        }
                    }
                }
                Err(_) => Some(EMBED_RETRY),
            };
            match wait_for_change {
                None => tokio::select! {
                    () = cancel.cancelled() => return,
                    () = self.dirty.notified() => {}
                },
                Some(retry) => tokio::select! {
                    () = cancel.cancelled() => return,
                    () = self.dirty.notified() => {}
                    () = tokio::time::sleep(retry) => {}
                },
            }
        }
    }

    // ---- proposals & approvals ---------------------------------------------------------

    /// Remember an AI proposal; returns its id.
    pub(crate) fn store_proposal(&self, run: &RunProposal) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let mut g = lock(&self.proposals);
        g.retain(|_, p| p.at.elapsed() < PROPOSAL_TTL);
        if g.len() >= MAX_PROPOSALS {
            if let Some(oldest) = g.iter().min_by_key(|(_, p)| p.at).map(|(k, _)| k.clone()) {
                g.remove(&oldest);
            }
        }
        g.insert(
            id.clone(),
            StoredProposal {
                run: run.clone(),
                at: Instant::now(),
            },
        );
        id
    }

    pub(crate) fn proposal(&self, id: &str) -> Option<RunProposal> {
        lock(&self.proposals)
            .get(id.trim())
            .filter(|p| p.at.elapsed() < PROPOSAL_TTL)
            .map(|p| p.run.clone())
    }

    /// Store an approval; returns its token.
    pub(crate) fn store_approval(&self, approved: ApprovedCommand, host_id: ObjectId) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        let mut g = lock(&self.approvals);
        g.retain(|_, a| a.expires > Instant::now());
        g.insert(
            token.clone(),
            StoredApproval {
                approved,
                host_id,
                expires: Instant::now() + APPROVAL_TTL,
            },
        );
        token
    }

    /// Consume an approval token (single use).
    pub(crate) fn take_approval(&self, token: &str) -> AppResult<StoredApproval> {
        let a = lock(&self.approvals)
            .remove(token.trim())
            .ok_or_else(|| AppError::ApprovalRequired("unknown or already used approval".into()))?;
        if a.expires <= Instant::now() {
            return Err(AppError::ApprovalRequired("the approval expired".into()));
        }
        Ok(a)
    }

    // ---- cancellable requests ----------------------------------------------------------

    pub(crate) fn register_request(&self) -> (String, CancellationToken) {
        let id = uuid::Uuid::new_v4().to_string();
        let token = self.closed.child_token();
        lock(&self.requests).insert(id.clone(), token.clone());
        (id, token)
    }

    pub(crate) fn finish_request(&self, id: &str) {
        lock(&self.requests).remove(id);
    }

    /// Cancel a running request; `false` if unknown / finished.
    pub(crate) fn cancel_request(&self, id: &str) -> bool {
        match lock(&self.requests).remove(id.trim()) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    // ---- conversations -----------------------------------------------------------------

    /// Persisted conversations of the vault.
    pub(crate) fn stored_conversations(&self) -> Vec<AiConversation> {
        self.working()
            .objects()
            .into_iter()
            .filter_map(|o| match o {
                VaultObject::AiConversation(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    /// A conversation of this unlock.
    pub(crate) fn memory_conversation(&self, id: &str) -> Option<ConvHandle> {
        lock(&self.conversations).get(id.trim()).cloned()
    }

    /// A persisted conversation record.
    pub(crate) fn stored_conversation(&self, id: &str) -> AppResult<AiConversation> {
        let oid = parse_id("conversation_id", id)?;
        self.stored_conversations()
            .into_iter()
            .find(|c| c.id == oid)
            .ok_or_else(|| AppError::not_found("conversation", oid))
    }

    /// Start a conversation with `provider_id`, sanitized under `profile`
    /// (the provider's effective profile). With `record`, a persisted
    /// conversation is resumed.
    pub(crate) fn open_conversation(
        &self,
        record: Option<AiConversation>,
        provider_id: ObjectId,
        profile: PrivacyProfile,
    ) -> (String, ConvHandle) {
        let now = chrono::Utc::now();
        let persisted = record.is_some();
        // TODO(ai-core): a resumed conversation starts with an empty model
        // context — `Conversation` cannot be rebuilt from stored messages;
        // next: `Conversation::from_messages(profile, …)` in ai-core.
        let record = record.unwrap_or_else(|| AiConversation {
            id: ObjectId::new(),
            title: String::new(),
            provider_id: Some(provider_id),
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
        });
        let id = record.id.to_string();
        let handle = ConvHandle {
            provider_id,
            state: Arc::new(tokio::sync::Mutex::new(ConversationState {
                conv: Conversation::new(profile),
                record,
                persisted,
            })),
        };
        lock(&self.conversations).insert(id.clone(), handle.clone());
        (id, handle)
    }

    pub(crate) fn memory_conversations(&self) -> Vec<ConvHandle> {
        lock(&self.conversations).values().cloned().collect()
    }

    pub(crate) fn forget_conversation(&self, id: &str) -> bool {
        lock(&self.conversations).remove(id.trim()).is_some()
    }
}
