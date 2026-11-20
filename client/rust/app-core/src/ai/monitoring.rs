//! Selected monitoring observations -> exact sanitized preview -> one-use AI call.
//! No metrics query, host resolver, history, writer or execution capability.
use crate::session::Unlocked;
use crate::{AppCore, AppError, AppResult};
use cc_ai_core::monitoring::PreparedMetricsRequest;
use cc_ai_core::provider::{LlmProvider, Locality};
use cc_ai_core::{AiError, CancellationToken};
use cc_models::{ai::AiProviderConfig, ObjectId};
use cc_monitoring_core::SourceConfig;
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use uuid::Uuid;

const TTL: Duration = Duration::from_secs(120);
const MAX_REQUEST: usize = 48 * 1024;
fn invalid(rule: &'static str) -> AppError {
    AppError::invalid("monitoring_ai", rule)
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
fn id(raw: &str) -> AppResult<Uuid> {
    let id = Uuid::parse_str(raw).map_err(|_| invalid("request_invalid"))?;
    if id.is_nil() || id.to_string() != raw {
        return Err(invalid("request_invalid"));
    }
    Ok(id)
}
fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> AppResult<T> {
    if raw.len() > MAX_REQUEST {
        return Err(invalid("request_budget"));
    }
    serde_json::from_str(raw).map_err(|_| invalid("request_invalid"))
}

// Preserve nested duplicate rejection BEFORE Value's map representation can
// collapse fields. Typed selected-data validation happens in ai-core as well.
struct Selected(Value);
impl<'de> Deserialize<'de> for Selected {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Selected;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded JSON data without duplicate fields")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Selected, E> {
                Ok(Selected(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Selected, E> {
                Ok(Selected(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Selected, E> {
                Ok(Selected(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Selected, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Selected(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite JSON number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Selected, E> {
                Ok(Selected(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Selected, E> {
                Ok(Selected(Value::String(v)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Selected, E> {
                Ok(Selected(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Selected, E> {
                Ok(Selected(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Selected, A::Error> {
                let mut values = Vec::new();
                while let Some(Selected(v)) = a.next_element()? {
                    values.push(v);
                }
                Ok(Selected(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Selected, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if values.contains_key(&k) {
                        return Err(de::Error::custom("duplicate selected field"));
                    }
                    let Selected(v) = a.next_value()?;
                    values.insert(k, v);
                }
                Ok(Selected(Value::Object(values)))
            }
        }
        d.deserialize_any(V)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepare {
    version: u8,
    profile_id: String,
    source_id: String,
    source_revision: u64,
    provider_id: String,
    action: String,
    question: String,
    selected_data: Selected,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Send {
    version: u8,
    profile_id: String,
    ticket: String,
    approve_external: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Discard {
    version: u8,
    profile_id: String,
    ticket: String,
}

struct Context {
    profile: String,
    scope: String,
    source: SourceConfig,
    source_revision: u64,
    cfg: AiProviderConfig,
    fences: BTreeMap<ObjectId, i64>,
    unlock: Weak<Unlocked>,
}
impl Context {
    fn check(
        &self,
        u: &Arc<Unlocked>,
        provider: &Arc<dyn LlmProvider>,
        cancel: &CancellationToken,
    ) -> AppResult<()> {
        let working = u.working();
        let before = working.version();
        if cancel.is_cancelled()
            || !self
                .unlock
                .upgrade()
                .is_some_and(|old| Arc::ptr_eq(&old, u))
            || u.notices.scope_id().as_deref() != Some(&self.scope)
            || !u.ai.metrics_provider_current(&self.cfg, provider)
        {
            return Err(invalid("context_changed"));
        }
        if self
            .fences
            .iter()
            .any(|(id, rev)| working.revision_of(*id) != Some(*rev))
            || crate::monitoring_sources::source_for_request(
                u,
                &self.source.id,
                self.source_revision,
            )? != self.source
            || before != working.version()
        {
            return Err(invalid("context_changed"));
        }
        Ok(())
    }
}
struct Review {
    context: Context,
    request: PreparedMetricsRequest,
    provider: Arc<dyn LlmProvider>,
    external: bool,
    deadline: Instant,
    cancellation: crate::automation_prepare::CancelOnDrop,
}
#[derive(Default)]
struct Registry {
    closed: bool,
    pending: HashMap<Uuid, Review>,
    active: HashMap<Uuid, ActiveRequest>,
}
struct ActiveRequest {
    cancel: CancellationToken,
    provider: ObjectId,
}
pub(super) struct MetricsRuntime {
    state: Mutex<Registry>,
    preparing: Arc<tokio::sync::Semaphore>,
}
impl Default for MetricsRuntime {
    fn default() -> Self {
        Self {
            state: Mutex::new(Registry::default()),
            preparing: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
}
impl MetricsRuntime {
    pub(super) fn close(&self) {
        let mut state = lock(&self.state);
        state.closed = true;
        state.pending.clear();
        for active in state.active.values() {
            active.cancel.cancel();
        }
    }
    pub(super) fn invalidate_device(&self) {
        let mut state = lock(&self.state);
        state.pending.retain(|_, review| {
            review.context.cfg.id.to_string() != super::DEVICE_LOCAL_PROVIDER_ID
        });
        for active in state.active.values() {
            if active.provider.to_string() == super::DEVICE_LOCAL_PROVIDER_ID {
                active.cancel.cancel();
            }
        }
    }
}
struct Active {
    state: Arc<MetricsRuntime>,
    id: Uuid,
    cancel: CancellationToken,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.cancel.cancel();
        lock(&self.state.state).active.remove(&self.id);
    }
}

impl AppCore {
    pub async fn monitoring_ai_prepare_json(&self, raw: &str) -> AppResult<String> {
        let input: Prepare = parse(raw)?;
        if input.version != 1
            || input.source_revision == 0
            || input.source_revision > 9_007_199_254_740_991
            || input.question.len() > 4096
        {
            return Err(invalid("request_invalid"));
        }
        id(&input.profile_id)?;
        id(&input.source_id)?;
        id(&input.provider_id)?;
        let selected = serde_json::to_string(&input.selected_data.0)
            .map_err(|_| invalid("request_invalid"))?;
        // Validate the selected-data schema BEFORE any API-key resolution.
        // This temporary strict projection is not displayed or sent; the
        // reviewed request below uses the actual selected provider's policy.
        drop(
            PreparedMetricsRequest::new(
                cc_ai_core::PrivacyProfile::Strict,
                &input.action,
                &input.question,
                &selected,
            )
            .map_err(|_| invalid("selected_data_invalid"))?,
        );
        let (owner, u) = self.unlocked().await?;
        let _prepare =
            u.ai.metrics
                .preparing
                .clone()
                .try_acquire_owned()
                .map_err(|_| invalid("preview_limit"))?;
        let cancellation = crate::automation_prepare::CancelOnDrop(CancellationToken::new());
        let context = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            if owner.profile_id.to_string() != input.profile_id {
                return Err(invalid("context_changed"));
            }
            let working = u.working();
            let before = working.version();
            let source = crate::monitoring_sources::source_for_request(
                &u,
                &input.source_id,
                input.source_revision,
            )?;
            let cfg = u.ai.resolve_config(Some(&input.provider_id))?;
            let mut watched = BTreeMap::new();
            let settings = working.all_vault_settings();
            if settings.len() > 128 {
                return Err(invalid("context_budget"));
            }
            for setting in settings {
                watched.insert(
                    setting.id,
                    working
                        .revision_of(setting.id)
                        .ok_or_else(|| invalid("context_changed"))?,
                );
            }
            if cfg.id.to_string() != super::DEVICE_LOCAL_PROVIDER_ID {
                for id in std::iter::once(cfg.id).chain(cfg.api_key_secret_id) {
                    watched.insert(
                        id,
                        working
                            .revision_of(id)
                            .ok_or_else(|| invalid("context_changed"))?,
                    );
                }
            }
            let scope = u
                .notices
                .scope_id()
                .ok_or_else(|| invalid("context_changed"))?;
            if before != working.version()
                || !working.watch_revisions(watched.clone(), cancellation.0.clone())
            {
                return Err(invalid("context_changed"));
            }
            Context {
                profile: input.profile_id,
                scope,
                source,
                source_revision: input.source_revision,
                cfg,
                fences: watched,
                unlock: Arc::downgrade(&u),
            }
        };
        let mut shutdown = u.sharing_shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let provider = tokio::select! {biased;
            _=cancellation.0.cancelled()=>return Err(invalid("context_changed")),
            _=shutdown.changed()=>return Err(AppError::VaultLocked),
            r=tokio::time::timeout(Duration::from_secs(15),u.ai.provider_for(&context.cfg))=>
                r.map_err(|_|invalid("provider_timeout"))??,
        };
        let mut request = PreparedMetricsRequest::new(
            provider.privacy_profile(),
            &input.action,
            &input.question,
            &selected,
        )
        .map_err(|_| invalid("selected_data_invalid"))?;
        if context.cfg.id.to_string() == super::DEVICE_LOCAL_PROVIDER_ID {
            if request.preview().len() > 8192 {
                return Err(invalid("local_context_budget"));
            }
            request = request.output_token_limit(1024);
        }
        let ticket = Uuid::new_v4();
        let preview = request.preview();
        let external = provider.locality() != Locality::Loopback;
        let response = json!({"version":1,"ticket":ticket,"previewText":preview,"external":external,
            "expiresUtc":(chrono::Utc::now()+chrono::Duration::seconds(120)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)});
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        context.check(&u, &provider, &cancellation.0)?;
        let runtime = u.ai.metrics.clone();
        {
            let mut state = lock(&runtime.state);
            state
                .pending
                .retain(|_, r| r.deadline > Instant::now() && !r.cancellation.0.is_cancelled());
            if state.closed || state.pending.len() >= 4 {
                return Err(invalid("preview_limit"));
            }
            state.pending.insert(
                ticket,
                Review {
                    context,
                    request,
                    provider,
                    external,
                    deadline: Instant::now() + TTL,
                    cancellation,
                },
            );
        }
        let weak = Arc::downgrade(&runtime);
        tokio::spawn(async move {
            tokio::time::sleep(TTL).await;
            if let Some(runtime) = weak.upgrade() {
                lock(&runtime.state).pending.remove(&ticket);
            }
        });
        serde_json::to_string(&response).map_err(|_| invalid("response_invalid"))
    }

    pub async fn monitoring_ai_discard_json(&self, raw: &str) -> AppResult<()> {
        let input: Discard = parse(raw)?;
        if input.version != 1 {
            return Err(invalid("request_invalid"));
        }
        id(&input.profile_id)?;
        let ticket = id(&input.ticket)?;
        let (owner, u) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&u).await?;
        if owner.profile_id.to_string() != input.profile_id {
            return Err(invalid("context_changed"));
        }
        let mut state = lock(&u.ai.metrics.state);
        state.pending.remove(&ticket);
        if let Some(active) = state.active.get(&ticket) {
            active.cancel.cancel();
        }
        Ok(())
    }

    pub async fn monitoring_ai_send_json(&self, raw: &str) -> AppResult<String> {
        let input: Send = parse(raw)?;
        if input.version != 1 {
            return Err(invalid("request_invalid"));
        }
        id(&input.profile_id)?;
        let ticket = id(&input.ticket)?;
        let (owner, u) = self.unlocked().await?;
        let runtime = u.ai.metrics.clone();
        let review = {
            let _profile = self.hold_session(&owner).await?;
            let _unlock = owner.hold_unlocked(&u).await?;
            if owner.profile_id.to_string() != input.profile_id {
                return Err(invalid("context_changed"));
            }
            let mut state = lock(&runtime.state);
            if state.closed || state.active.len() >= 2 {
                return Err(invalid("generation_limit"));
            }
            let review = state
                .pending
                .get(&ticket)
                .ok_or_else(|| invalid("preview_expired"))?;
            if review.external && !input.approve_external {
                return Err(invalid("external_consent_required"));
            }
            if review.deadline <= Instant::now() || review.context.profile != input.profile_id {
                return Err(invalid("preview_expired"));
            }
            review
                .context
                .check(&u, &review.provider, &review.cancellation.0)?;
            let review = state
                .pending
                .remove(&ticket)
                .expect("review checked under same mutex");
            state.active.insert(
                ticket,
                ActiveRequest {
                    cancel: review.cancellation.0.clone(),
                    provider: review.context.cfg.id,
                },
            );
            review
        };
        let cancel = review.cancellation.0.clone();
        let _active = Active {
            state: runtime,
            id: ticket,
            cancel: cancel.clone(),
        };
        let mut shutdown = u.sharing_shutdown.subscribe();
        if *shutdown.borrow() {
            return Err(AppError::VaultLocked);
        }
        let Review {
            context,
            request,
            provider,
            deadline,
            cancellation: _cancellation,
            ..
        } = review;
        drop(u);
        let remaining = deadline.saturating_duration_since(Instant::now());
        let reply = tokio::select! {biased;
            _=cancel.cancelled()=>return Err(invalid("context_changed")),
            _=shutdown.changed()=>return Err(AppError::VaultLocked),
            result=tokio::time::timeout(remaining,request.generate(provider.as_ref()))=>
                result.map_err(|_|AppError::from(AiError::Timeout))?.map_err(|_|invalid("response_invalid"))?,
        };
        let original = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = self.hold_session(&owner).await?;
        let _unlock = owner.hold_unlocked(&original).await?;
        context.check(&original, &provider, &cancel)?;
        serde_json::to_string(&reply).map_err(|_| invalid("response_invalid"))
    }
}

#[cfg(test)]
mod tests;
