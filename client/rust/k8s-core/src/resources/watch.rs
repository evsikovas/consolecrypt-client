use super::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const MAX_WATCH_LINE: usize = 2 * 1024 * 1024;
const MAX_CACHE_ITEMS: usize = 20_000;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceChange {
    SnapshotBegin,
    SnapshotPage { page: ResourcePage },
    Added { resource: ResourceRow },
    Modified { resource: ResourceRow },
    Deleted { resource: ResourceRow },
    Bookmark { resource_version: String },
    Reconnecting,
}

#[derive(Debug, Default)]
pub struct WatchOptions {
    pub query: ListQuery,
    pub resource_version: Option<String>,
}

impl ResourceApi {
    /// The receiver is bounded by its owner. Awaiting send provides
    /// backpressure; dropping the receiver or cancelling stops network reads.
    /// A stale watch (410) creates a fresh paginated snapshot before resuming.
    pub async fn watch(
        &self,
        kind: &ResourceKind,
        options: WatchOptions,
        sender: mpsc::Sender<ResourceChange>,
        stop: CancellationToken,
    ) -> Result<(), ResourceError> {
        self.checked_kind(kind, "watch")?;
        let closed = self.guard.cancellation();
        tokio::select! {
            biased;
            _ = closed.cancelled() => Err(ResourceError::Closed),
            _ = stop.cancelled() => Ok(()),
            _ = sender.closed() => Ok(()),
            result = self.watch_loop(kind, options, sender.clone()) => result,
        }
    }

    async fn watch_loop(
        &self,
        kind: &ResourceKind,
        options: WatchOptions,
        sender: mpsc::Sender<ResourceChange>,
    ) -> Result<(), ResourceError> {
        let mut version = options.resource_version;
        if version.as_ref().is_some_and(|v| !safe_query_value(v, 128)) {
            return Err(ResourceError::InvalidRequest);
        }
        let mut failures = 0_u32;
        loop {
            if version.is_none() {
                sender
                    .send(ResourceChange::SnapshotBegin)
                    .await
                    .map_err(|_| ResourceError::Closed)?;
                let mut query = options.query.clone();
                query.continue_token = None;
                let mut items = 0;
                let mut bytes = 0;
                let mut seen_tokens = BTreeSet::new();
                loop {
                    let page = self.list(kind, &query).await?;
                    items += page.items.len();
                    bytes += page
                        .items
                        .iter()
                        .map(|item| item.object.to_string().len())
                        .sum::<usize>();
                    if items > MAX_CACHE_ITEMS || bytes > MAX_CACHE_BYTES {
                        return Err(ResourceError::TooLarge);
                    }
                    // Every page in a paginated Kubernetes list belongs to
                    // one snapshot and must carry the same resourceVersion.
                    if let Some(previous) = &version {
                        if previous != &page.resource_version {
                            return Err(ResourceError::InvalidResponse);
                        }
                    } else {
                        if page.resource_version.is_empty() {
                            return Err(ResourceError::InvalidResponse);
                        }
                        version = Some(page.resource_version.clone());
                    }
                    query.continue_token = page.continue_token.clone();
                    sender
                        .send(ResourceChange::SnapshotPage { page })
                        .await
                        .map_err(|_| ResourceError::Closed)?;
                    if let Some(token) = &query.continue_token {
                        if !seen_tokens.insert(token.clone()) {
                            return Err(ResourceError::InvalidResponse);
                        }
                    } else {
                        break;
                    }
                }
            }
            let mut query = options.query.clone();
            query.continue_token = None;
            let mut path = self.list_path(kind, &query, "watch")?;
            let parameters = {
                let mut pairs = url::form_urlencoded::Serializer::new(String::new());
                pairs
                    .append_pair("watch", "true")
                    .append_pair("allowWatchBookmarks", "true")
                    .append_pair("timeoutSeconds", "240")
                    .append_pair("resourceVersion", version.as_deref().unwrap_or_default());
                pairs.finish()
            };
            path.push('&');
            path.push_str(&parameters);
            let result = self.watch_once(kind, &path, &sender, &mut version).await;
            match result {
                Ok(()) => {
                    failures = 0;
                    // Empty responses must not cause a busy retry loop.
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(ResourceError::Gone) => {
                    version = None;
                    failures = 0;
                }
                Err(
                    ResourceError::Transport | ResourceError::Timeout | ResourceError::Unavailable,
                ) => {
                    sender
                        .send(ResourceChange::Reconnecting)
                        .await
                        .map_err(|_| ResourceError::Closed)?;
                    failures = (failures + 1).min(5);
                    tokio::time::sleep(Duration::from_secs(1_u64 << failures)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn watch_once(
        &self,
        kind: &ResourceKind,
        path: &str,
        sender: &mpsc::Sender<ResourceChange>,
        version: &mut Option<String>,
    ) -> Result<(), ResourceError> {
        let mut body = tokio::time::timeout(
            Duration::from_secs(30),
            self.open_body(Method::GET, path, Vec::new(), None),
        )
        .await
        .map_err(|_| ResourceError::Timeout)??;
        let mut pending = Zeroizing::new(Vec::new());
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(300), body.frame())
                .await
                .map_err(|_| ResourceError::Timeout)?;
            let Some(frame) = frame else {
                break;
            };
            if let Ok(data) = frame.map_err(|_| ResourceError::Transport)?.into_data() {
                for chunk in data.split_inclusive(|byte| *byte == b'\n') {
                    if pending.len().saturating_add(chunk.len()) > MAX_WATCH_LINE {
                        return Err(ResourceError::TooLarge);
                    }
                    pending.extend_from_slice(chunk);
                    if chunk.last() == Some(&b'\n') {
                        self.consume_watch_line(kind, &pending, sender, version)
                            .await?;
                        pending.clear();
                    }
                }
            }
        }
        if !pending.is_empty() {
            self.consume_watch_line(kind, &pending, sender, version)
                .await?;
        }
        Ok(())
    }

    async fn consume_watch_line(
        &self,
        kind: &ResourceKind,
        bytes: &[u8],
        sender: &mpsc::Sender<ResourceChange>,
        version: &mut Option<String>,
    ) -> Result<(), ResourceError> {
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        let mut event = ProtectedJson(
            serde_json::from_slice(bytes).map_err(|_| ResourceError::InvalidResponse)?,
        );
        let event_type = event["type"]
            .as_str()
            .ok_or(ResourceError::InvalidResponse)?
            .to_owned();
        let mut object = ProtectedJson(event["object"].take());
        if event_type == "ERROR" {
            return Err(match object["code"].as_u64() {
                Some(410) => ResourceError::Gone,
                Some(401) => ResourceError::Unauthorized,
                Some(403) => ResourceError::Forbidden,
                _ => ResourceError::Unavailable,
            });
        }
        let rv = response_string(&object["metadata"]["resourceVersion"], 128)?
            .filter(|v| !v.is_empty())
            .ok_or(ResourceError::InvalidResponse)?;
        redact_object(&mut object, kind, ReadOptions::default());
        let change = match event_type.as_str() {
            "ADDED" => ResourceChange::Added {
                resource: row_from_value(object.into_inner(), kind)?,
            },
            "MODIFIED" => ResourceChange::Modified {
                resource: row_from_value(object.into_inner(), kind)?,
            },
            "DELETED" => ResourceChange::Deleted {
                resource: row_from_value(object.into_inner(), kind)?,
            },
            "BOOKMARK" => ResourceChange::Bookmark {
                resource_version: rv.clone(),
            },
            _ => return Err(ResourceError::InvalidResponse),
        };
        sender
            .send(change)
            .await
            .map_err(|_| ResourceError::Closed)?;
        *version = Some(rv);
        Ok(())
    }

    /// Emits complete lines, with a hard line-size cap. Consumers retain at
    /// most MAX_LOG_LINES/MAX_LOG_BYTES (LogBuffer provides that ring).
    pub async fn follow_logs(
        &self,
        namespace: &str,
        pod: &str,
        query: &LogQuery,
        sender: mpsc::Sender<String>,
        stop: CancellationToken,
    ) -> Result<(), ResourceError> {
        let path = self.log_path(namespace, pod, query, true)?;
        let closed = self.guard.cancellation();
        tokio::select! {
            biased;
            _ = closed.cancelled() => Err(ResourceError::Closed),
            _ = stop.cancelled() => Ok(()),
            _ = sender.closed() => Ok(()),
            result = async {
                let mut body = self.open_body(Method::GET, &path, Vec::new(), None).await?;
                let mut pending = Zeroizing::new(Vec::new());
                while let Some(frame) = body.frame().await {
                    if let Ok(data) = frame.map_err(|_| ResourceError::Transport)?.into_data() {
                        for chunk in data.split_inclusive(|byte| *byte == b'\n') {
                            if pending.len().saturating_add(chunk.len()) > 256 * 1024 { return Err(ResourceError::TooLarge); }
                            pending.extend_from_slice(chunk);
                            if chunk.last() == Some(&b'\n') {
                                let line = String::from_utf8(pending.to_vec()).map_err(|_| ResourceError::InvalidResponse)?;
                                sender.send(line).await.map_err(|_| ResourceError::Closed)?;
                                pending.clear();
                            }
                        }
                    }
                }
                if !pending.is_empty() {
                    sender.send(String::from_utf8(pending.to_vec()).map_err(|_| ResourceError::InvalidResponse)?)
                        .await.map_err(|_| ResourceError::Closed)?;
                }
                Ok(())
            } => result,
        }
    }
}

#[derive(Default)]
pub struct LogBuffer {
    lines: std::collections::VecDeque<Zeroizing<String>>,
    bytes: usize,
}
impl std::fmt::Debug for LogBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogBuffer")
            .field("lines", &self.lines.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl LogBuffer {
    pub fn push(&mut self, text: String) {
        for line in text.split_inclusive('\n') {
            let line = if line.len() > MAX_LOG_BYTES {
                let start = (line.len() - MAX_LOG_BYTES..line.len())
                    .find(|i| line.is_char_boundary(*i))
                    .unwrap_or(line.len());
                &line[start..]
            } else {
                line
            };
            self.bytes += line.len();
            self.lines.push_back(Zeroizing::new(line.to_owned()));
            while self.lines.len() > MAX_LOG_LINES || self.bytes > MAX_LOG_BYTES {
                if let Some(line) = self.lines.pop_front() {
                    self.bytes -= line.len();
                }
            }
        }
    }

    pub fn text(&self, filter: Option<&str>) -> String {
        self.lines
            .iter()
            .filter(|line| filter.is_none_or(|filter| line.contains(filter)))
            .map(|line| line.as_str())
            .collect()
    }
    pub fn len(&self) -> usize {
        self.lines.len()
    }
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_ring_retains_only_latest_lines_and_respects_byte_budget() {
        let mut ring = LogBuffer::default();
        for line in 0..MAX_LOG_LINES + 50 {
            ring.push(format!("line-{line}\n"));
        }
        assert_eq!(ring.len(), MAX_LOG_LINES);
        assert!(!ring.text(None).contains("line-0\n"));
        assert!(ring.text(Some("line-20049")).contains("line-20049"));
        ring.push("x".repeat(MAX_LOG_BYTES + 50));
        assert!(ring.text(None).len() <= MAX_LOG_BYTES);
        assert_eq!(ring.len(), 1);
    }
}
