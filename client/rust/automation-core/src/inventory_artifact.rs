//! Private chunked inventory input. A reference never substitutes for verifying
//! the helper's actual canonical bytes against native-owned expected data.
use crate::{AutomationError, HelperPeer, InputDigestWitness, Result, UploadedRevision};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_BYTES: usize = 5 * 1024 * 1024;
fn invalid() -> AutomationError {
    AutomationError::Protocol("invalid or changed bound inventory artifact")
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InventoryReference {
    pub key: String,
    pub digest: String,
}
pub struct InventoryArtifact {
    pub reference: InventoryReference,
    pub witness: InputDigestWitness,
}
impl std::fmt::Debug for InventoryArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InventoryArtifact")
            .field("reference", &self.reference)
            .finish_non_exhaustive()
    }
}
struct UploadGuard {
    id: Uuid,
    peer: Arc<HelperPeer>,
}
impl Drop for UploadGuard {
    fn drop(&mut self) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let peer = self.peer.clone();
            let id = self.id;
            runtime.spawn(async move {
                let _ = peer
                    .request(
                        "inventory_upload_abort",
                        json!({"transfer_id":id}),
                        Duration::from_secs(5),
                    )
                    .await;
            });
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    revision_digest: String,
    key: String,
    digest: String,
    bytes: usize,
}
impl InventoryArtifact {
    pub async fn store(
        peer: &Arc<HelperPeer>,
        capture: &UploadedRevision,
        expected: &Value,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        let bytes = serde_json::to_vec(expected).map_err(|_| invalid())?;
        if bytes.is_empty()
            || bytes.len() > MAX_BYTES
            || capture.revision_id.is_nil()
            || !cc_models::automation::is_digest(&capture.digest)
        {
            return Err(invalid());
        }
        let response = tokio::select! {biased;_=cancel.cancelled()=>return Err(AutomationError::Closed),
        result=peer.request("inventory_upload_begin",json!({"revision_id":capture.revision_id,"digest":hex::encode(Sha256::digest(&bytes)),"bytes":bytes.len()}),Duration::from_secs(15))=>result?,};
        let id = response
            .get("transfer_id")
            .and_then(Value::as_str)
            .and_then(|v| Uuid::parse_str(v).ok())
            .filter(|v| !v.is_nil())
            .ok_or_else(invalid)?;
        if response.as_object().is_none_or(|v| v.len() != 1) {
            return Err(invalid());
        }
        let _guard = UploadGuard {
            id,
            peer: peer.clone(),
        };
        let deadline = Instant::now() + Duration::from_secs(100);
        for (index, chunk) in bytes.chunks(128 * 1024).enumerate() {
            if Instant::now() >= deadline {
                return Err(AutomationError::Timeout);
            }
            let response = tokio::select! {biased;_=cancel.cancelled()=>return Err(AutomationError::Closed),
            result=peer.request("inventory_upload_chunk",json!({"transfer_id":id,"offset":index*128*1024,"data":STANDARD.encode(chunk)}),Duration::from_secs(15))=>result?,};
            if response != json!({"offset":index*128*1024+chunk.len()}) {
                return Err(invalid());
            }
        }
        let response = tokio::select! {biased;_=cancel.cancelled()=>return Err(AutomationError::Closed),
        result=peer.request("inventory_upload_commit",json!({"transfer_id":id}),Duration::from_secs(15))=>result?,};
        let receipt: Receipt = serde_json::from_value(response).map_err(|_| invalid())?;
        if receipt.version != 1
            || receipt.revision_digest != capture.digest
            || receipt.key != receipt.digest
            || !cc_models::automation::is_digest(&receipt.digest)
            || receipt.bytes == 0
            || receipt.bytes > MAX_BYTES
        {
            return Err(invalid());
        }
        let reference = InventoryReference {
            key: receipt.key,
            digest: receipt.digest,
        };
        let witness =
            Self::read(peer, capture, &reference, receipt.bytes, expected, cancel).await?;
        Ok(Self { reference, witness })
    }
    pub async fn read(
        peer: &Arc<HelperPeer>,
        capture: &UploadedRevision,
        reference: &InventoryReference,
        bytes: usize,
        expected: &Value,
        cancel: &CancellationToken,
    ) -> Result<InputDigestWitness> {
        if bytes == 0
            || bytes > MAX_BYTES
            || !cc_models::automation::is_digest(&reference.key)
            || !cc_models::automation::is_digest(&reference.digest)
        {
            return Err(invalid());
        }
        let deadline = Instant::now() + Duration::from_secs(100);
        let mut body = Vec::with_capacity(bytes);
        while body.len() < bytes {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AutomationError::Timeout);
            }
            let raw = tokio::select! {biased;_=cancel.cancelled()=>return Err(AutomationError::Closed),
            result=peer.request("inventory_chunk",json!({"revision_id":capture.revision_id,"key":reference.key,"offset":body.len(),"limit":128*1024}),remaining.min(Duration::from_secs(15)))=>result?,};
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Chunk {
                revision_digest: String,
                key: String,
                offset: usize,
                data: String,
                eof: bool,
            }
            let chunk: Chunk = serde_json::from_value(raw).map_err(|_| invalid())?;
            if chunk.revision_digest != capture.digest
                || chunk.key != reference.key
                || chunk.offset != body.len()
                || chunk.data.len() > 175000
            {
                return Err(invalid());
            }
            let part = STANDARD.decode(chunk.data).map_err(|_| invalid())?;
            if part.is_empty()
                || part.len() > 128 * 1024
                || part.len() > bytes - body.len()
                || chunk.eof != (body.len() + part.len() == bytes)
            {
                return Err(invalid());
            }
            body.extend_from_slice(&part);
        }
        if cancel.is_cancelled() {
            return Err(AutomationError::Closed);
        }
        let witness = InputDigestWitness {
            digest: reference.digest.clone(),
            canonical_json: String::from_utf8(body).map_err(|_| invalid())?,
        };
        witness.verify_inventory(expected)?;
        Ok(witness)
    }
}
