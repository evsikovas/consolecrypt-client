//! Client-side conflict policy (ADR-0003 table). The server never merges.
//!
//! | Object class | Default |
//! |---|---|
//! | Secrets | never auto-merge: remote wins in place, local kept as a conflict copy, user notified |
//! | Hosts, groups, snippets, notes, tunnels, … | remote wins in place, local saved as a conflict copy |
//! | Settings, AI providers | last-writer-wins by `updated_at` |
//! | Deleted remotely, edited locally | local edit resurrects the object as a new revision |
//!
//! Additional rules (not in the ADR table): local delete vs remote edit →
//! remote wins (no data loss); identical payloads → remote kept, no copy.

use crate::codec::{CodecError, ObjectCodec};
use crate::error::SyncError;
use crate::events::ConflictResolution;
use crate::store::check_size;
use cc_models::{KekClass, ObjectKind, ObjectPayload, VaultObject};
use cc_protocol::{ObjectId, Timestamp};
use cc_storage_core::{ConflictContext, ConflictCopy, Resolution};

/// Automatic resolution strategy for an object kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictStrategy {
    /// Remote wins in place; the local version becomes a new object.
    ConflictCopy,
    /// Newer `updated_at` wins.
    LastWriterWins,
}

/// Default strategy per ADR-0003.
pub fn strategy_for(kind: ObjectKind) -> ConflictStrategy {
    match kind {
        ObjectKind::VaultSettings | ObjectKind::AiProvider => ConflictStrategy::LastWriterWins,
        _ => ConflictStrategy::ConflictCopy,
    }
}

/// Suffix appended to the name/title of conflict copies.
pub const CONFLICT_COPY_SUFFIX: &str = " (conflict copy)";

/// The local version of an object re-homed under `new_id` (name/title
/// suffixed where the kind has one). References to the original id held by
/// other objects are left untouched on purpose.
pub fn make_conflict_copy(payload: &ObjectPayload, new_id: ObjectId) -> ObjectPayload {
    let mut p = payload.clone();
    fn suffix(s: &mut String) {
        if !s.ends_with(CONFLICT_COPY_SUFFIX) {
            s.push_str(CONFLICT_COPY_SUFFIX);
        }
    }
    match &mut p.object {
        VaultObject::Host(o) => {
            o.id = new_id;
            suffix(&mut o.name);
        }
        VaultObject::Group(o) => {
            o.id = new_id;
            suffix(&mut o.name);
        }
        VaultObject::JumpProfile(o) => {
            o.id = new_id;
            suffix(&mut o.name);
        }
        VaultObject::Proxy(o) => {
            o.id = new_id;
            suffix(&mut o.name);
        }
        VaultObject::Credential(o) => {
            o.id = new_id;
            suffix(&mut o.name);
        }
        VaultObject::Tunnel(o) => {
            o.id = new_id;
            suffix(&mut o.name);
        }
        VaultObject::KnownHost(o) => o.id = new_id,
        VaultObject::Secret(o) => o.id = new_id,
        VaultObject::Snippet(o) => {
            o.id = new_id;
            suffix(&mut o.name);
        }
        VaultObject::Note(o) => {
            o.id = new_id;
            suffix(&mut o.title);
        }
        VaultObject::HistoryEntry(o) => o.id = new_id,
        VaultObject::AiConversation(o) => {
            o.id = new_id;
            suffix(&mut o.title);
        }
        VaultObject::VaultSettings(o) => o.id = new_id,
        VaultObject::AiProvider(o) => {
            o.id = new_id;
            suffix(&mut o.name);
        }
    }
    p
}

/// Last-modified time used for LWW.
pub fn payload_updated_at(payload: &ObjectPayload) -> Timestamp {
    match &payload.object {
        VaultObject::Host(o) => o.updated_at,
        VaultObject::Group(o) => o.updated_at,
        VaultObject::JumpProfile(o) => o.updated_at,
        VaultObject::Proxy(o) => o.updated_at,
        VaultObject::Credential(o) => o.updated_at,
        VaultObject::Tunnel(o) => o.updated_at,
        VaultObject::KnownHost(o) => o.updated_at,
        VaultObject::Secret(o) => o.updated_at,
        VaultObject::Snippet(o) => o.updated_at,
        VaultObject::Note(o) => o.updated_at,
        VaultObject::HistoryEntry(o) => o.executed_at,
        VaultObject::AiConversation(o) => o.updated_at,
        VaultObject::VaultSettings(o) => o.updated_at,
        VaultObject::AiProvider(o) => o.updated_at,
    }
}

/// A decided resolution plus what to report.
pub(crate) struct Decision {
    pub(crate) resolution: Resolution,
    pub(crate) label: ConflictResolution,
    pub(crate) kek_class: Option<KekClass>,
}

/// Decide how to resolve one conflicted object. `Ok(None)` = the server
/// state is not known yet (pull first). Codec errors (e.g. vault locked)
/// leave the conflict for a later attempt.
pub(crate) fn decide(
    ctx: &ConflictContext,
    codec: &dyn ObjectCodec,
) -> Result<Option<Decision>, SyncError> {
    let object_id = ctx.local.object_id;
    let local = if ctx.local.deleted {
        None
    } else {
        let body = ctx
            .local
            .body
            .as_ref()
            .ok_or_else(|| SyncError::Invalid("live local object without body".into()))?;
        Some(codec.decrypt_hinted(
            object_id,
            ctx.local.revision,
            body,
            ctx.local.kek_class_hint,
        )?)
    };
    let kek_class = local
        .as_ref()
        .map(|p| p.object.kind().kek_class())
        .or(ctx.local.kek_class_hint);

    let rebase = |p: &ObjectPayload, base: i64| -> Result<Resolution, SyncError> {
        let body = codec.encrypt(object_id, base + 1, p)?;
        check_size(&body)?;
        Ok(Resolution::Rebase {
            base_revision: base,
            body,
            kek_class_hint: kek_class,
        })
    };
    let copy = |p: &ObjectPayload| -> Result<Resolution, SyncError> {
        let copy_id = ObjectId::new();
        let cp = make_conflict_copy(p, copy_id);
        let body = codec.encrypt(copy_id, 1, &cp)?;
        check_size(&body)?;
        Ok(Resolution::AcceptRemote {
            copy: Some(ConflictCopy {
                object_id: copy_id,
                body,
                kek_class_hint: kek_class,
            }),
        })
    };
    let decision = |resolution, label| {
        Ok(Some(Decision {
            resolution,
            label,
            kek_class,
        }))
    };

    let Some(remote) = &ctx.remote else {
        if ctx.conflict_revision == 0 {
            // The server has no such object (e.g. vault re-created).
            return match &local {
                Some(p) => decision(rebase(p, 0)?, ConflictResolution::LocalKept),
                None => decision(Resolution::DropLocal, ConflictResolution::LocalDropped),
            };
        }
        return Ok(None);
    };

    match (&local, remote.deleted) {
        (None, true) => decision(
            Resolution::AcceptRemote { copy: None },
            ConflictResolution::BothDeleted,
        ),
        (None, false) => decision(
            Resolution::AcceptRemote { copy: None },
            ConflictResolution::RemoteKept,
        ),
        (Some(p), true) => decision(
            rebase(p, remote.revision)?,
            ConflictResolution::LocalEditResurrected,
        ),
        (Some(p), false) => {
            let remote_payload = match &remote.body {
                Some(b) => {
                    match codec.decrypt_hinted(object_id, remote.revision, b, remote.kek_class_hint)
                    {
                        Ok(rp) => Some(rp),
                        Err(CodecError::Locked) => return Err(CodecError::Locked.into()),
                        Err(e) => {
                            tracing::warn!(%object_id, error = %e, "remote version of conflicted object is undecryptable");
                            None
                        }
                    }
                }
                None => None,
            };
            if remote_payload.as_ref() == Some(p) {
                return decision(
                    Resolution::AcceptRemote { copy: None },
                    ConflictResolution::Identical,
                );
            }
            match (strategy_for(p.object.kind()), &remote_payload) {
                (ConflictStrategy::LastWriterWins, Some(rp)) => {
                    if payload_updated_at(p) > payload_updated_at(rp) {
                        decision(rebase(p, remote.revision)?, ConflictResolution::LocalKept)
                    } else {
                        decision(
                            Resolution::AcceptRemote { copy: None },
                            ConflictResolution::RemoteKept,
                        )
                    }
                }
                // Conflict copy — also the safe fallback when the remote
                // version cannot be read.
                _ => decision(copy(p)?, ConflictResolution::RemoteKeptLocalCopied),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_models::host::Host;
    use cc_models::secret::{Secret, SecretKind, SecretValue};

    #[test]
    fn conflict_copy_changes_id_and_name() {
        let h = Host::new("db", "10.0.0.1");
        let p = ObjectPayload::new(VaultObject::Host(h.clone()));
        let id = ObjectId::new();
        let c = make_conflict_copy(&p, id);
        assert_eq!(c.object.id(), id);
        let VaultObject::Host(ch) = &c.object else {
            panic!()
        };
        assert_eq!(ch.name, "db (conflict copy)");
        assert_eq!(ch.address, h.address);
        // Idempotent suffix.
        let c2 = make_conflict_copy(&c, ObjectId::new());
        let VaultObject::Host(ch2) = &c2.object else {
            panic!()
        };
        assert_eq!(ch2.name, "db (conflict copy)");

        let s = ObjectPayload::new(VaultObject::Secret(Secret::new(
            SecretKind::Password,
            SecretValue::new("pw"),
        )));
        let sc = make_conflict_copy(&s, id);
        assert_eq!(sc.object.id(), id);
        assert_eq!(
            strategy_for(ObjectKind::Secret),
            ConflictStrategy::ConflictCopy
        );
        assert_eq!(
            strategy_for(ObjectKind::VaultSettings),
            ConflictStrategy::LastWriterWins
        );
    }
}
