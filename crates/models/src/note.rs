//! Notes, tasks and reminders (synced E2EE, indexed locally). A note may be
//! attached to one inventory object or to a Kubernetes/Docker resource that
//! lives outside the vault; a missing target leaves the note in place.

use crate::{ObjectId, Timestamp, ValidationError};
use serde::{Deserialize, Serialize};

pub const MAX_TITLE_CHARS: usize = 256;
pub const MAX_BODY_BYTES: usize = 256 * 1024;
pub const MAX_TAGS: usize = 32;
pub const MAX_TAG_CHARS: usize = 64;

/// New fields default so notes written by older clients still decode, and are
/// omitted when empty so an unattached plain note serializes as before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: ObjectId,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<NoteTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<NoteTarget>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteTask {
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub due_at: Option<Timestamp>,
    #[serde(default)]
    pub done_at: Option<Timestamp>,
}

/// Inventory objects a note can be attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteObjectKind {
    Host,
    RdpHost,
    DbConnection,
    KubeContext,
}

/// What a note is about. Kubernetes resources omit the API version so a note
/// survives a version change; Docker containers are named so a note survives
/// re-creation (like the Docker database route).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoteTarget {
    Object {
        object_kind: NoteObjectKind,
        id: ObjectId,
    },
    KubeResource {
        context_id: ObjectId,
        #[serde(default)]
        group: String,
        resource: String,
        object_kind: String,
        #[serde(default)]
        namespace: Option<String>,
        name: String,
    },
    DockerContainer {
        /// The SSH host of the Docker engine; None is this computer.
        #[serde(default)]
        host_id: Option<ObjectId>,
        name: String,
    },
}

impl Note {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let title = self.title.trim();
        if title.is_empty()
            || title.chars().count() > MAX_TITLE_CHARS
            || title.chars().any(char::is_control)
        {
            return Err(invalid("title", "invalid title"));
        }
        if self.body.len() > MAX_BODY_BYTES {
            return Err(invalid("body", "note is too long"));
        }
        if self.tags.len() > MAX_TAGS
            || self.tags.iter().any(|tag| {
                tag.trim().is_empty()
                    || tag.chars().count() > MAX_TAG_CHARS
                    || tag.chars().any(char::is_control)
            })
        {
            return Err(invalid("tags", "invalid tags"));
        }
        if let Some(target) = &self.target {
            target.validate()?;
        }
        Ok(())
    }
}

impl NoteTarget {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Object { id, .. } => {
                if *id == ObjectId::NIL {
                    return Err(invalid("target", "invalid object reference"));
                }
            }
            Self::KubeResource {
                context_id,
                group,
                resource,
                object_kind,
                namespace,
                name,
            } => {
                let token = |value: &str, maximum: usize| {
                    !value.is_empty()
                        && value.len() <= maximum
                        && value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
                };
                if *context_id == ObjectId::NIL
                    || !(group.is_empty() || token(group, 253))
                    || !token(resource, 63)
                    || !token(object_kind, 63)
                    || namespace
                        .as_deref()
                        .is_some_and(|ns| crate::kubernetes::validate_namespace(ns).is_err())
                    // Object names allow `:` (RBAC) but never separators or spaces.
                    || name.is_empty()
                    || name.len() > 253
                    || name
                        .chars()
                        .any(|c| c.is_whitespace() || c.is_control() || c == '/')
                {
                    return Err(invalid("target", "invalid Kubernetes resource"));
                }
            }
            Self::DockerContainer { host_id, name } => {
                // Docker container names: [a-zA-Z0-9][a-zA-Z0-9_.-]*
                if *host_id == Some(ObjectId::NIL)
                    || name.is_empty()
                    || name.len() > 255
                    || !name.starts_with(|c: char| c.is_ascii_alphanumeric())
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
                {
                    return Err(invalid("target", "invalid Docker container"));
                }
            }
        }
        Ok(())
    }
}

fn invalid(field: &'static str, reason: &'static str) -> ValidationError {
    ValidationError::new(field, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note() -> Note {
        let now = chrono::Utc::now();
        Note {
            id: ObjectId::new(),
            title: "Rotate certificates".into(),
            body: "before **Friday**".into(),
            tags: vec!["ops".into()],
            pinned: false,
            task: None,
            remind_at: None,
            target: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn notes_from_older_clients_decode_with_defaults() {
        let now = chrono::Utc::now();
        let old = serde_json::json!({
            "id": ObjectId::new(), "title": "t", "body": "b",
            "created_at": now, "updated_at": now,
        });
        let decoded: Note = serde_json::from_value(old).unwrap();
        assert!(decoded.tags.is_empty() && !decoded.pinned);
        assert!(decoded.task.is_none() && decoded.remind_at.is_none() && decoded.target.is_none());
    }

    #[test]
    fn targets_roundtrip_with_stable_wire_names() {
        let mut value = note();
        for target in [
            NoteTarget::Object {
                object_kind: NoteObjectKind::DbConnection,
                id: ObjectId::new(),
            },
            NoteTarget::KubeResource {
                context_id: ObjectId::new(),
                group: "apps".into(),
                resource: "deployments".into(),
                object_kind: "Deployment".into(),
                namespace: Some("prod".into()),
                name: "api".into(),
            },
            NoteTarget::DockerContainer {
                host_id: None,
                name: "postgres".into(),
            },
        ] {
            value.target = Some(target);
            value.task = Some(NoteTask {
                done: false,
                due_at: Some(chrono::Utc::now()),
                done_at: None,
            });
            value.validate().unwrap();
            let json = serde_json::to_value(&value).unwrap();
            assert!(["object", "kube_resource", "docker_container"]
                .contains(&json["target"]["kind"].as_str().unwrap()));
            assert_eq!(serde_json::from_value::<Note>(json).unwrap(), value);
        }
    }

    #[test]
    fn validation_rejects_bad_text_and_targets() {
        let mut bad = note();
        bad.title = "  ".into();
        assert!(bad.validate().is_err());
        bad.title = "a".repeat(MAX_TITLE_CHARS + 1);
        assert!(bad.validate().is_err());
        let mut bad = note();
        bad.body = "x".repeat(MAX_BODY_BYTES + 1);
        assert!(bad.validate().is_err());
        let mut bad = note();
        bad.tags = vec!["".into()];
        assert!(bad.validate().is_err());
        for target in [
            NoteTarget::DockerContainer {
                host_id: None,
                name: "-bad".into(),
            },
            NoteTarget::DockerContainer {
                host_id: Some(ObjectId::NIL),
                name: "ok".into(),
            },
            NoteTarget::KubeResource {
                context_id: ObjectId::new(),
                group: String::new(),
                resource: "pods".into(),
                object_kind: "Pod".into(),
                namespace: Some("Bad_NS".into()),
                name: "web".into(),
            },
            NoteTarget::KubeResource {
                context_id: ObjectId::new(),
                group: String::new(),
                resource: "pods".into(),
                object_kind: "Pod".into(),
                namespace: None,
                name: "a/b".into(),
            },
            NoteTarget::Object {
                object_kind: NoteObjectKind::Host,
                id: ObjectId::NIL,
            },
        ] {
            let mut bad = note();
            bad.target = Some(target);
            assert!(bad.validate().is_err(), "{:?}", bad.target);
        }
        // RBAC names contain colons.
        let mut ok = note();
        ok.target = Some(NoteTarget::KubeResource {
            context_id: ObjectId::new(),
            group: "rbac.authorization.k8s.io".into(),
            resource: "clusterroles".into(),
            object_kind: "ClusterRole".into(),
            namespace: None,
            name: "system:aggregate-to-view".into(),
        });
        ok.validate().unwrap();
    }
}
