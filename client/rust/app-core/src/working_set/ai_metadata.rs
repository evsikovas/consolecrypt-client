//! Bounded metadata guard for explicitly selected AI facts. Never secret values.
use super::*;
use crate::{AppError, AppResult};
use sha2::{Digest, Sha256};
use std::io::{self, Write};

#[derive(Clone)]
pub(crate) struct AiContextProjection {
    pub revisions: BTreeMap<ObjectId, i64>,
    pub digest: String,
    pub tags: BTreeMap<ObjectId, Vec<String>>,
}
struct BoundedHash {
    hash: Sha256,
    remaining: usize,
}
impl Write for BoundedHash {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.remaining -= bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl WorkingSet {
    pub(crate) fn ai_context_projection(
        &self,
        roots: &[ObjectId],
    ) -> AppResult<AiContextProjection> {
        let invalid = || AppError::invalid("automation_ai_context", "context_metadata_unavailable");
        if roots.len() > 2 {
            return Err(invalid());
        }
        let inner = self.read();
        if inner.entries.len() > 16_384 {
            return Err(invalid());
        }
        // References only; the existing graph traversal is reused verbatim.
        let borrowed = inner
            .entries
            .iter()
            .map(|(id, entry)| (*id, entry))
            .collect();
        let mut revisions = BTreeMap::new();
        let mut tags = BTreeMap::new();
        for root in roots {
            if inner
                .origin
                .get(root)
                .is_some_and(|vault| inner.ai_blocked.contains(vault))
            {
                return Err(invalid());
            }
            revisions.extend(crate::kubernetes_runtime::dependencies(&borrowed, *root)?);
            if let Some(Entry {
                item: Item::Object(object),
                ..
            }) = inner.entries.get(root)
            {
                if let VaultObject::Host(host) = object.as_ref() {
                    if host.tags.len() > 64
                        || host
                            .tags
                            .iter()
                            .any(|v| v.len() > 256 || v.chars().any(char::is_control))
                    {
                        return Err(invalid());
                    }
                    tags.insert(*root, host.tags.clone());
                }
            }
        }
        if !roots.is_empty() {
            for (id, entry) in &inner.entries {
                if entry.kind() == ObjectKind::KnownHost {
                    revisions.insert(*id, entry.revision);
                }
                if revisions.len() > 512 {
                    return Err(invalid());
                }
            }
        }
        let mut hash = BoundedHash {
            hash: Sha256::new(),
            remaining: 256 * 1024,
        };
        for (id, revision) in &revisions {
            let entry = inner.entries.get(id).ok_or_else(invalid)?;
            serde_json::to_writer(&mut hash, &(id, revision, inner.origin.get(id)))
                .map_err(|_| invalid())?;
            match &entry.item {
                Item::Object(value) => {
                    serde_json::to_writer(&mut hash, value.as_ref()).map_err(|_| invalid())?
                }
                Item::Secret { kind } => {
                    serde_json::to_writer(&mut hash, kind).map_err(|_| invalid())?
                }
            }
        }
        Ok(AiContextProjection {
            revisions,
            digest: hex::encode(hash.hash.finalize()),
            tags,
        })
    }
}
