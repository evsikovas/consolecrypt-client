//! Reverse snapshot transfer: only new files in an empty, caller-owned root.
//! The helper's checksum is evidence to verify, never a substitute for hashing.
use crate::{
    validate_project_path, AutomationError, HelperPeer, ProjectEntry, ProjectRevision, ProjectRoot,
    Result, UploadedRevision,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    entries: Vec<ProjectEntry>,
    total: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    revision_digest: String,
    path: String,
    offset: u64,
    data: String,
    eof: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyChunk {
    review_digest: String,
    path: String,
    offset: u64,
    data: String,
    eof: bool,
}
#[derive(Clone, Copy)]
enum DownloadSource {
    Revision,
    DependencyCandidate,
}
impl DownloadSource {
    fn page(self, capture: &UploadedRevision, offset: usize) -> (&'static str, serde_json::Value) {
        match self {
            Self::Revision => (
                "files",
                json!({"revision_id":capture.revision_id,"offset":offset,"limit":100}),
            ),
            Self::DependencyCandidate => (
                "dependency_files",
                json!({"preparation_id":capture.revision_id,"offset":offset,"limit":100}),
            ),
        }
    }
    fn chunk(
        self,
        capture: &UploadedRevision,
        path: &str,
        offset: u64,
        limit: u64,
    ) -> (&'static str, serde_json::Value) {
        match self {
            Self::Revision => (
                "read_chunk",
                json!({"revision_id":capture.revision_id,"path":path,"offset":offset,"limit":limit}),
            ),
            Self::DependencyCandidate => (
                "dependency_chunk",
                json!({"preparation_id":capture.revision_id,"path":path,"offset":offset,"limit":limit}),
            ),
        }
    }
    async fn request(
        self,
        peer: &Arc<HelperPeer>,
        operation: &str,
        body: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        match self {
            Self::Revision => peer.request(operation, body, timeout).await,
            Self::DependencyCandidate => {
                peer.request_dependency_bounded(operation, body, timeout, 176 * 1024)
                    .await
            }
        }
    }
    fn decode_chunk(self, value: serde_json::Value) -> Result<Chunk> {
        match self {
            Self::Revision => serde_json::from_value(value).map_err(|_| invalid()),
            Self::DependencyCandidate => {
                let value: DependencyChunk =
                    serde_json::from_value(value).map_err(|_| invalid())?;
                Ok(Chunk {
                    revision_digest: value.review_digest,
                    path: value.path,
                    offset: value.offset,
                    data: value.data,
                    eof: value.eof,
                })
            }
        }
    }
}
fn invalid() -> AutomationError {
    AutomationError::Protocol("invalid or changed downloaded snapshot")
}
impl ProjectRoot {
    /// Call from a bounded filesystem worker. Cancellation leaves a partial
    /// owned staging directory; the caller must never publish it as ready.
    pub async fn download(
        &self,
        capture: &UploadedRevision,
        peer: &Arc<HelperPeer>,
        cancel: &CancellationToken,
    ) -> Result<ProjectRevision> {
        self.download_from(capture, peer, cancel, DownloadSource::Revision)
            .await
    }

    /// Uncommitted candidate only; fixed private RPC, never helper commit.
    pub(crate) async fn download_dependency_candidate(
        &self,
        capture: &UploadedRevision,
        peer: &Arc<HelperPeer>,
        cancel: &CancellationToken,
    ) -> Result<ProjectRevision> {
        self.download_from(capture, peer, cancel, DownloadSource::DependencyCandidate)
            .await
    }

    async fn download_from(
        &self,
        capture: &UploadedRevision,
        peer: &Arc<HelperPeer>,
        cancel: &CancellationToken,
        source: DownloadSource,
    ) -> Result<ProjectRevision> {
        if capture.revision_id.is_nil()
            || !cc_models::automation::is_digest(&capture.digest)
            || capture.entry_count > 20_000
            || capture.file_count > 10_000
            || capture.bytes > 100 * 1024 * 1024
            || !self.revision()?.entries.is_empty()
        {
            return Err(invalid());
        }
        let mut entries = Vec::new();
        let mut files = 0usize;
        let mut total_bytes = 0u64;
        let deadline = Instant::now() + Duration::from_secs(300);
        while entries.len() < capture.entry_count {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AutomationError::Timeout);
            }
            let (operation, body) = source.page(capture, entries.len());
            let raw = tokio::select! {biased;_=cancel.cancelled()=>return Err(AutomationError::Closed),
            result=source.request(peer,operation,body,remaining.min(Duration::from_secs(30)))=>result?,};
            let page: Page = serde_json::from_value(raw).map_err(|_| invalid())?;
            if page.total != capture.entry_count
                || page.entries.is_empty()
                || page.entries.len() > 100
                || entries.len() + page.entries.len() > capture.entry_count
            {
                return Err(invalid());
            }
            for entry in page.entries {
                validate_project_path(entry.path())?;
                if entries
                    .last()
                    .is_some_and(|previous: &ProjectEntry| previous.path() >= entry.path())
                {
                    return Err(invalid());
                }
                if let ProjectEntry::File {
                    bytes,
                    mode,
                    sha256,
                    ..
                } = &entry
                {
                    files += 1;
                    total_bytes = total_bytes.checked_add(*bytes).ok_or_else(invalid)?;
                    if files > capture.file_count
                        || total_bytes > capture.bytes
                        || ![0o600, 0o700].contains(mode)
                        || !cc_models::automation::is_digest(sha256)
                    {
                        return Err(invalid());
                    }
                }
                entries.push(entry);
            }
        }
        if files != capture.file_count || total_bytes != capture.bytes {
            return Err(invalid());
        }
        for entry in &entries {
            if cancel.is_cancelled() {
                return Err(AutomationError::Closed);
            }
            match entry {
                ProjectEntry::Directory { path } => self.import_directory(path)?,
                ProjectEntry::File {
                    path,
                    mode,
                    bytes,
                    sha256,
                } => {
                    let mut output = self.import_file(path, *mode)?;
                    let mut offset = 0u64;
                    let mut hash = Sha256::new();
                    while offset < *bytes {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return Err(AutomationError::Timeout);
                        }
                        let limit = (*bytes - offset).min(128 * 1024);
                        let (operation, body) = source.chunk(capture, path, offset, limit);
                        let raw = tokio::select! {biased;_=cancel.cancelled()=>return Err(AutomationError::Closed),
                        result=source.request(peer,operation,body,remaining.min(Duration::from_secs(30)))=>result?,};
                        let chunk = source.decode_chunk(raw)?;
                        if chunk.revision_digest != capture.digest
                            || chunk.path != *path
                            || chunk.offset != offset
                            || chunk.data.len() > 175000
                        {
                            return Err(invalid());
                        }
                        let content = STANDARD.decode(chunk.data).map_err(|_| invalid())?;
                        if content.len() as u64 != limit || chunk.eof != (offset + limit == *bytes)
                        {
                            return Err(invalid());
                        }
                        hash.update(&content);
                        output.write_all(&content)?;
                        offset += limit;
                    }
                    if hex::encode(hash.finalize()) != *sha256 {
                        return Err(invalid());
                    }
                    output.sync_all()?;
                }
            }
        }
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        let actual = self.revision()?;
        if actual.digest != capture.digest || actual.bytes != capture.bytes {
            return Err(invalid());
        }
        Ok(actual)
    }
}
