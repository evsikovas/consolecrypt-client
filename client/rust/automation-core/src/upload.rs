use crate::{AutomationError, HelperPeer, ProjectEntry, ProjectRevision, ProjectRoot, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{io::Read, sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone, Copy)]
enum UploadMode {
    Revision,
    Dependency,
}
impl UploadMode {
    async fn request(
        self,
        peer: &Arc<HelperPeer>,
        operation: &str,
        body: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        match self {
            Self::Revision => peer.request(operation, body, timeout).await,
            Self::Dependency => {
                peer.request_dependency_bounded(operation, body, timeout, 4096)
                    .await
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadedRevision {
    pub revision_id: Uuid,
    pub digest: String,
    pub file_count: usize,
    pub entry_count: usize,
    pub bytes: u64,
}

struct IncompleteUpload {
    peer: std::sync::Weak<HelperPeer>,
    id: Uuid,
    committed: bool,
    mode: UploadMode,
}

impl Drop for IncompleteUpload {
    fn drop(&mut self) {
        if !self.committed {
            if matches!(self.mode, UploadMode::Dependency) {
                if let Some(peer) = self.peer.upgrade() {
                    peer.abort_dependency_exchange();
                }
                return;
            }
            if let (Some(peer), Ok(runtime)) =
                (self.peer.upgrade(), tokio::runtime::Handle::try_current())
            {
                let id = self.id;
                runtime.spawn(async move {
                    let _ = peer
                        .request(
                            "upload_abort",
                            json!({"upload_id": id}),
                            Duration::from_secs(5),
                        )
                        .await;
                });
            }
        }
    }
}

impl ProjectRoot {
    /// Stream bounded chunks over the private helper channel. A changed project
    /// is rejected before commit; the receiver independently rehashes every file
    /// and the complete snapshot. No archive, shell or Windows path mapping.
    pub async fn upload(
        &self,
        revision: &ProjectRevision,
        peer: &Arc<HelperPeer>,
    ) -> Result<UploadedRevision> {
        self.upload_from(revision, peer, UploadMode::Revision).await
    }

    pub(crate) async fn upload_dependency(
        &self,
        revision: &ProjectRevision,
        peer: &Arc<HelperPeer>,
    ) -> Result<UploadedRevision> {
        self.upload_from(revision, peer, UploadMode::Dependency)
            .await
    }

    async fn upload_from(
        &self,
        revision: &ProjectRevision,
        peer: &Arc<HelperPeer>,
        mode: UploadMode,
    ) -> Result<UploadedRevision> {
        if self.revision()?.digest != revision.digest {
            return Err(AutomationError::Invalid("project changed after review"));
        }
        let result = mode
            .request(
                peer,
                "upload_begin",
                json!({"digest": revision.digest}),
                Duration::from_secs(30),
            )
            .await?;
        let id: Uuid = serde_json::from_value(result.get("upload_id").cloned().unwrap_or_default())
            .map_err(|_| AutomationError::Protocol("invalid transfer identifier"))?;
        let mut transfer = IncompleteUpload {
            peer: Arc::downgrade(peer),
            id,
            committed: false,
            mode,
        };
        let mut buffer = vec![0u8; 128 * 1024];
        for entry in &revision.entries {
            mode.request(
                peer,
                "upload_entry",
                json!({"upload_id": id, "entry": entry}),
                Duration::from_secs(30),
            )
            .await?;
            if let ProjectEntry::File { path, bytes, .. } = entry {
                let mut file = self.open_file(path)?;
                let mut offset = 0u64;
                loop {
                    let count = file.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    if offset + count as u64 > *bytes {
                        return Err(AutomationError::Invalid(
                            "project file grew during transfer",
                        ));
                    }
                    let reply = mode
                        .request(
                            peer,
                            "upload_chunk",
                            json!({"upload_id": id, "offset": offset,
                        "data": STANDARD.encode(&buffer[..count])}),
                            Duration::from_secs(30),
                        )
                        .await?;
                    offset += count as u64;
                    if reply.get("offset").and_then(|v| v.as_u64()) != Some(offset) {
                        return Err(AutomationError::Protocol("transfer cursor mismatch"));
                    }
                }
                if offset != *bytes {
                    return Err(AutomationError::Invalid(
                        "project file changed during transfer",
                    ));
                }
            }
        }
        if self.revision()?.digest != revision.digest {
            return Err(AutomationError::Invalid("project changed during transfer"));
        }
        let reply = mode
            .request(
                peer,
                "upload_commit",
                json!({"upload_id": id}),
                Duration::from_secs(60),
            )
            .await?;
        let committed: UploadedRevision = serde_json::from_value(reply)
            .map_err(|_| AutomationError::Protocol("invalid imported revision"))?;
        if committed.digest != revision.digest
            || committed.bytes != revision.bytes
            || committed.entry_count != revision.entries.len()
        {
            return Err(AutomationError::Protocol(
                "imported revision differs from native revision",
            ));
        }
        transfer.committed = true;
        Ok(committed)
    }
}
