//! One coherent metadata-only password credential projection. No plaintext,
//! resolver, general snapshot clone, mutation hook or additional registry.
use super::*;
use cc_models::credential::CredentialKind;
const MAX_ENTRIES: usize = 16_384;
const MAX_METADATA_BYTES: usize = 64 * 1024;
#[derive(Clone, PartialEq)]
pub(crate) struct VaultCredentialMetadata {
    credential: Credential,
    credential_revision: i64,
    secret: ObjectId,
    secret_revision: i64,
}
impl VaultCredentialMetadata {
    pub(crate) fn secret(&self) -> ObjectId {
        self.secret
    }
    pub(crate) fn credential(&self) -> ObjectId {
        self.credential.id
    }
    pub(crate) fn username(&self) -> Option<&str> {
        self.credential.username.as_deref()
    }
    pub(crate) fn fences(&self) -> BTreeMap<ObjectId, i64> {
        BTreeMap::from([
            (self.credential.id, self.credential_revision),
            (self.secret, self.secret_revision),
        ])
    }
}
struct MetadataBudget(usize);
impl std::io::Write for MetadataBudget {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(raw.len())
            .filter(|n| *n <= MAX_METADATA_BYTES)
            .ok_or_else(|| std::io::Error::other("credential metadata budget"))?;
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl WorkingSet {
    pub(crate) fn collector_service_credential_choices(
        &self,
    ) -> Option<Vec<(ObjectId, i64, String)>> {
        let inner = self.read();
        if inner.entries.len() > MAX_ENTRIES {
            return None;
        }
        let mut rows = Vec::new();
        for (id, entry) in &inner.entries {
            if entry.kind() != ObjectKind::Credential {
                continue;
            }
            let Some(value) = metadata(&inner, *id) else {
                continue;
            };
            let Some(username) = value.username() else {
                continue;
            };
            if username.is_empty() || username.len() > 256 || username.chars().any(char::is_control)
            {
                continue;
            }
            if rows.len() >= 64 {
                return None;
            }
            let name = &value.credential.name;
            if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
                return None;
            }
            rows.push((*id, value.credential_revision, name.clone()));
        }
        rows.sort_by_key(|row| row.0);
        Some(rows)
    }
    /// A personal Password credential, its exact referenced personal Password
    /// secret metadata, and absence of an inline-host owner under ONE read lock.
    /// Borrowed traversal is capped16,384 entries; only this <=64KiB credential
    /// is cloned. Oversized/unavailable/foreign/inline-owned data fails closed.
    /// Secret values never enter WorkingSet or this non-Serde projection.
    pub(crate) fn vault_credential_metadata(
        &self,
        id: ObjectId,
    ) -> Option<VaultCredentialMetadata> {
        let inner = self.read();
        metadata(&inner, id)
    }
}

fn metadata(inner: &Inner, id: ObjectId) -> Option<VaultCredentialMetadata> {
    if inner.entries.len() > MAX_ENTRIES || inner.origin.contains_key(&id) {
        return None;
    }
    let entry = inner.entries.get(&id)?;
    let Item::Object(object) = &entry.item else {
        return None;
    };
    let VaultObject::Credential(credential) = object.as_ref() else {
        return None;
    };
    if credential.id != id || credential.kind != CredentialKind::Password {
        return None;
    }
    let secret = credential.secret_id?;
    let material = inner.entries.get(&secret)?;
    if inner.origin.contains_key(&secret)
        || !matches!(
            material.item,
            Item::Secret {
                kind: SecretKind::Password
            }
        )
    {
        return None;
    }
    if inner.entries.values().any(|entry| {
        let Item::Object(object) = &entry.item else {
            return false;
        };
        let host = match object.as_ref() {
            VaultObject::Host(host) => host,
            VaultObject::RdpHost(rdp) => &rdp.host,
            _ => return false,
        };
        crate::host_auth::inline_credential(host) == Some(id)
    }) {
        return None;
    }
    // Counting serializer handles ALL current metadata fields without caching
    // an encoded buffer or maintaining a second Credential field schema.
    serde_json::to_writer(MetadataBudget(0), credential).ok()?;
    Some(VaultCredentialMetadata {
        credential: credential.clone(),
        credential_revision: entry.revision,
        secret,
        secret_revision: material.revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::{
        secret::{Secret, SecretValue},
        ObjectPayload,
    };
    use cc_storage_core::LocalState;
    fn put(ws: &WorkingSet, object: VaultObject) {
        let id = object.id();
        ws.apply(
            id,
            Readback::Live(Box::new(DecryptedObject {
                object_id: id,
                revision: 1,
                local_state: LocalState::LocalOnly,
                conflict_origin: None,
                payload: ObjectPayload::new(object),
            })),
        );
    }
    fn fixture() -> (WorkingSet, ObjectId) {
        let ws = WorkingSet::new();
        let secret = Secret::new(
            SecretKind::Password,
            SecretValue::new("synthetic never-cached password"),
        );
        let mut credential = Credential::new("Selected personal", CredentialKind::Password);
        credential.secret_id = Some(secret.id);
        let id = credential.id;
        put(&ws, VaultObject::Secret(secret));
        put(&ws, VaultObject::Credential(credential));
        (ws, id)
    }
    #[test]
    fn exact_selected_metadata_ignores_counter_noise_but_not_equal_revision_payload() {
        let (ws, id) = fixture();
        let first = ws.vault_credential_metadata(id).unwrap();
        let version = ws.version();
        put(&ws, VaultObject::Credential(ws.credential(id).unwrap()));
        ws.apply(ObjectId::new(), Readback::Missing);
        assert_ne!(version, ws.version());
        assert!(ws.vault_credential_metadata(id).as_ref() == Some(&first));
        let mut changed = ws.credential(id).unwrap();
        changed.username = Some("same revision changed metadata".into());
        put(&ws, VaultObject::Credential(changed));
        assert!(ws.vault_credential_metadata(id).as_ref() != Some(&first));
    }
    #[test]
    fn metadata_counter_budget_rejects_oversized_selected_credential_without_buffer_clone() {
        let (ws, id) = fixture();
        let mut changed = ws.credential(id).unwrap();
        changed.name = "x".repeat(MAX_METADATA_BYTES);
        put(&ws, VaultObject::Credential(changed));
        assert!(ws.vault_credential_metadata(id).is_none());
    }
    #[test]
    fn borrowed_inline_owner_scan_has_exact_entry_quota() {
        let (ws, id) = fixture();
        {
            let mut inner = ws.write();
            while inner.entries.len() < MAX_ENTRIES {
                inner.entries.insert(
                    ObjectId::new(),
                    Entry {
                        revision: 1,
                        item: Item::Secret {
                            kind: SecretKind::ApiKey,
                        },
                    },
                );
            }
        }
        assert!(ws.vault_credential_metadata(id).is_some());
        {
            let mut inner = ws.write();
            inner.entries.insert(
                ObjectId::new(),
                Entry {
                    revision: 1,
                    item: Item::Secret {
                        kind: SecretKind::ApiKey,
                    },
                },
            );
        }
        assert!(ws.vault_credential_metadata(id).is_none());
    }
}
