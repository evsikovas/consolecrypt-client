//! Opt-in event recording stays in the profile's encrypted local settings.
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::{ListQuery, ResourceApi, ResourceChange, ResourceRow, WatchOptions};
use cc_storage_core::Storage;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

const KEY: &str = "kubernetes.events.v1";
const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// A bounded snapshot for one approved AI question. The tool layer applies its
/// own privacy profile, projection and result budget before any provider I/O.
pub(crate) async fn ai_snapshot(
    storage: &Storage,
    context_id: &str,
) -> AppResult<Vec<serde_json::Value>> {
    let old: Vec<KubeHistoricalEvent> = storage.setting_get(KEY).await?.unwrap_or_default();
    let now = chrono::Utc::now().timestamp_millis();
    let events = merge(old, Vec::new(), now);
    let mut result = Vec::new();
    let mut bytes = 0;
    for event in events
        .into_iter()
        .filter(|event| event.context_id == context_id)
        .take(200)
    {
        let date = |ms| chrono::DateTime::from_timestamp_millis(ms).map(|time| time.to_rfc3339());
        let item = serde_json::json!({
            "apiVersion":"v1", "kind":"Event",
            "metadata":{"uid":event.uid,"name":event.uid,"namespace":event.namespace},
            "involvedObject":{"uid":event.object_uid,"name":event.object_name,"kind":event.object_kind,"namespace":event.namespace},
            "firstTimestamp":date(event.first_seen_ms),"lastTimestamp":date(event.last_seen_ms),
            "type":event.event_type,"reason":event.reason,"message":event.message,"count":event.count,
        });
        bytes += serde_json::to_vec(&item)
            .map_err(|_| AppError::invalid("kubernetes", "invalid_history"))?
            .len();
        if bytes > 240 * 1024 {
            break;
        }
        result.push(item);
    }
    Ok(result)
}

/// Runs once per unlock, then at the next expiry (at most an hour apart).
/// Offline/disabled contexts are pruned too; merely hiding old rows is not
/// sufficient to enforce the retention policy in encrypted storage.
pub(crate) async fn retention_task(storage: Storage) {
    loop {
        let now = chrono::Utc::now().timestamp_millis();
        let next = storage
            .write(move |tx| {
                let Some(old): Option<Vec<KubeHistoricalEvent>> = tx.setting_get(KEY)? else {
                    return Ok::<_, cc_storage_core::StorageError>(3_600_000_i64);
                };
                let records = merge(old, Vec::new(), now);
                let next = records
                    .iter()
                    .map(|event| event.last_seen_ms + RETENTION_MS - now + 1)
                    .min()
                    .unwrap_or(3_600_000)
                    .clamp(1_000, 3_600_000);
                tx.setting_set(KEY, &records)?;
                Ok(next)
            })
            .await;
        let Ok(next) = next else {
            return;
        };
        tokio::time::sleep(std::time::Duration::from_millis(next as u64)).await;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubeHistoricalEvent {
    pub context_id: String,
    pub uid: String,
    pub namespace: String,
    pub object_uid: String,
    pub object_kind: String,
    pub object_name: String,
    pub event_type: String,
    pub reason: String,
    pub message: String,
    pub count: u64,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
}
#[derive(Debug, Clone, Serialize)]
pub struct KubeHistoryStatus {
    pub enabled: bool,
    pub state: String,
}
pub(crate) struct EventRecorder {
    task: tokio::task::AbortHandle,
    cancel: CancellationToken,
    state: Arc<Mutex<String>>,
}
impl Drop for EventRecorder {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}
impl EventRecorder {
    pub(crate) fn start(
        storage: Storage,
        context_id: String,
        api: ResourceApi,
        parent: CancellationToken,
    ) -> Self {
        let cancel = parent.child_token();
        let task_cancel = cancel.clone();
        let state = Arc::new(Mutex::new("connecting".into()));
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = task_cancel.cancelled() => return,
                result = record(storage, context_id, api, task_cancel.clone(), task_state.clone()) => result,
            };
            *task_state.lock().unwrap_or_else(|e| e.into_inner()) = if result.is_ok() {
                "closed"
            } else {
                "unavailable"
            }
            .into();
        });
        Self {
            task: task.abort_handle(),
            cancel,
            state,
        }
    }
    pub(crate) fn status(&self) -> String {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

async fn record(
    storage: Storage,
    context_id: String,
    api: ResourceApi,
    cancel: CancellationToken,
    state: Arc<Mutex<String>>,
) -> Result<(), ()> {
    let discovery = api.discover().await.map_err(|_| ())?;
    let kind = discovery
        .resources
        .into_iter()
        .find(|k| k.group.is_empty() && k.resource == "events")
        .ok_or(())?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    let watch_cancel = cancel.child_token();
    let watch = api.watch(
        &kind,
        WatchOptions {
            query: ListQuery {
                limit: Some(200),
                ..ListQuery::default()
            },
            ..WatchOptions::default()
        },
        sender,
        watch_cancel.clone(),
    );
    tokio::pin!(watch);
    let _cancel_watch = watch_cancel.drop_guard();
    loop {
        let change = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(()),
            result = &mut watch => return result.map_err(|_| ()),
            item = receiver.recv() => item.ok_or(())?,
        };
        *state.lock().unwrap_or_else(|e| e.into_inner()) = "recording".into();
        let rows = match change {
            ResourceChange::SnapshotPage { page } => page.items,
            ResourceChange::Added { resource } | ResourceChange::Modified { resource } => {
                vec![resource]
            }
            ResourceChange::Reconnecting => {
                *state.lock().unwrap_or_else(|e| e.into_inner()) = "reconnecting".into();
                continue;
            }
            _ => continue,
        };
        let now = chrono::Utc::now().timestamp_millis();
        let records: Vec<_> = rows
            .iter()
            .filter_map(|row| project(&context_id, row, now))
            .collect();
        if records.is_empty() {
            continue;
        }
        storage
            .write(move |tx| {
                let old: Vec<KubeHistoricalEvent> = tx.setting_get(KEY)?.unwrap_or_default();
                let merged = merge(old, records, now);
                tx.setting_set(KEY, &merged)
            })
            .await
            .map_err(|_: cc_storage_core::StorageError| ())?;
    }
}

fn text(value: &serde_json::Value, max: usize) -> String {
    let value = value.as_str().unwrap_or_default();
    let mut boundary = value.len().min(max);
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].into()
}
fn timestamp(value: &serde_json::Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|date| date.timestamp_millis())
}
fn project(context: &str, row: &ResourceRow, now: i64) -> Option<KubeHistoricalEvent> {
    let object = &row.object;
    let uid = text(&object["metadata"]["uid"], 256);
    if uid.is_empty() {
        return None;
    }
    let regarding = if object["regarding"].is_object() {
        &object["regarding"]
    } else {
        &object["involvedObject"]
    };
    let raw = if object["note"].is_string() {
        &object["note"]
    } else {
        &object["message"]
    };
    let message =
        cc_ai_core::sanitizer::sanitize(cc_models::ai::PrivacyProfile::Standard, &text(raw, 8192))
            .into_string();
    let first = timestamp(&object["firstTimestamp"])
        .or_else(|| timestamp(&object["eventTime"]))
        .or_else(|| timestamp(&object["metadata"]["creationTimestamp"]))
        .unwrap_or(now)
        .min(now);
    let last = timestamp(&object["series"]["lastObservedTime"])
        .or_else(|| timestamp(&object["lastTimestamp"]))
        .or_else(|| timestamp(&object["eventTime"]))
        .unwrap_or(first)
        .min(now)
        .max(first);
    if last < now - RETENTION_MS {
        return None;
    }
    Some(KubeHistoricalEvent {
        context_id: context.into(),
        uid,
        namespace: row.namespace.clone().unwrap_or_default(),
        object_uid: text(&regarding["uid"], 256),
        object_kind: text(&regarding["kind"], 256),
        object_name: text(&regarding["name"], 253),
        event_type: text(&object["type"], 64),
        reason: text(&object["reason"], 256),
        message,
        count: object["series"]["count"]
            .as_u64()
            .or_else(|| object["count"].as_u64())
            .unwrap_or(1)
            .max(1),
        first_seen_ms: first,
        last_seen_ms: last,
    })
}
fn merge(
    old: Vec<KubeHistoricalEvent>,
    new: Vec<KubeHistoricalEvent>,
    now: i64,
) -> Vec<KubeHistoricalEvent> {
    let mut entries = BTreeMap::<(String, String), KubeHistoricalEvent>::new();
    for item in old.into_iter().chain(new) {
        if item.last_seen_ms < now - RETENTION_MS {
            continue;
        }
        let key = (item.context_id.clone(), item.uid.clone());
        if let Some(previous) = entries.get_mut(&key) {
            previous.count = previous.count.max(item.count);
            previous.first_seen_ms = previous.first_seen_ms.min(item.first_seen_ms);
            previous.last_seen_ms = previous.last_seen_ms.max(item.last_seen_ms);
            if item.last_seen_ms >= previous.last_seen_ms {
                previous.message = item.message;
            }
        } else {
            entries.insert(key, item);
        }
    }
    let mut result: Vec<_> = entries.into_values().collect();
    result.sort_by(|a, b| {
        b.last_seen_ms
            .cmp(&a.last_seen_ms)
            .then_with(|| b.uid.cmp(&a.uid))
    });
    let mut bytes = 0;
    result
        .into_iter()
        .take(4096)
        .take_while(|event| {
            bytes += event.message.len() + event.reason.len() + event.object_name.len() + 2048;
            bytes <= 8 * 1024 * 1024
        })
        .collect()
}

impl AppCore {
    pub async fn kube_history_status(&self, session_id: String) -> AppResult<KubeHistoryStatus> {
        let live = self.kube_live(&session_id).await?;
        Ok(KubeHistoryStatus {
            enabled: live.history.is_some(),
            state: live
                .history
                .as_ref()
                .map(|r| r.status())
                .unwrap_or_else(|| "disabled".into()),
        })
    }
    pub async fn kube_event_history(
        &self,
        context_id: String,
        namespace: Option<String>,
        warning_only: bool,
        before_ms: Option<i64>,
        before_uid: Option<String>,
        limit: usize,
    ) -> AppResult<Vec<KubeHistoricalEvent>> {
        if limit == 0 || limit > 500 {
            return Err(AppError::invalid("kubernetes", "invalid_limit"));
        }
        let (owner, unlocked) = self.unlocked().await?;
        let id = crate::dto::parse_id("context_id", &context_id)?;
        if unlocked.working().kube_context(id).is_none() {
            return Err(AppError::not_found("kube_context", id));
        }
        let old: Vec<KubeHistoricalEvent> =
            owner.storage()?.setting_get(KEY).await?.unwrap_or_default();
        let now = chrono::Utc::now().timestamp_millis();
        let mut events = merge(old, Vec::new(), now);
        events.retain(|event| {
            event.context_id == context_id
                && namespace.as_ref().is_none_or(|ns| ns == &event.namespace)
                && (!warning_only || event.event_type == "Warning")
                && before_ms.is_none_or(|before| {
                    event.last_seen_ms < before
                        || (event.last_seen_ms == before
                            && before_uid.as_ref().is_some_and(|uid| event.uid < *uid))
                })
        });
        events.truncate(limit);
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(events)
    }
    pub async fn kube_event_history_clear(&self, context_id: String) -> AppResult<()> {
        let (owner, _) = self.unlocked().await?;
        crate::dto::parse_id("context_id", &context_id)?;
        owner
            .storage()?
            .write(move |tx| {
                let mut events: Vec<KubeHistoricalEvent> = tx.setting_get(KEY)?.unwrap_or_default();
                events.retain(|event| event.context_id != context_id);
                tx.setting_set(KEY, &events)
            })
            .await
            .map_err(AppError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_events_merge_counts_and_expire_after_seven_days() {
        let now = RETENTION_MS * 3;
        let event = KubeHistoricalEvent {
            context_id: "ctx".into(),
            uid: "event".into(),
            namespace: "default".into(),
            object_uid: "pod".into(),
            object_kind: "Pod".into(),
            object_name: "web".into(),
            event_type: "Warning".into(),
            reason: "BackOff".into(),
            message: "restart".into(),
            count: 2,
            first_seen_ms: now - 1000,
            last_seen_ms: now - 1000,
        };
        let mut changed = event.clone();
        changed.count = 5;
        changed.last_seen_ms = now;
        let result = merge(vec![event], vec![changed], now);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].count, 5);
        assert_eq!(result[0].first_seen_ms, now - 1000);
        assert!(merge(result, Vec::new(), now + RETENTION_MS + 1).is_empty());
    }
}
