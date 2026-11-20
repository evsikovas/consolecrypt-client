//! Read-only comparison against a captured Git tree; never invokes Git/hooks.
use crate::{AutomationError, ProjectEntry, ProjectRevision, ProjectRoot, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectChange {
    pub path: String,
    pub kind: String,
    pub before: Option<ProjectEntry>,
    pub after: Option<ProjectEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDiff {
    pub baseline_digest: String,
    pub current_digest: String,
    pub changes: Vec<ProjectChange>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFileDiff {
    pub path: String,
    pub baseline_digest: String,
    pub current_digest: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub preview: String,
}

pub fn compare_revisions(before: &ProjectRevision, after: &ProjectRevision) -> ProjectDiff {
    let old: BTreeMap<_, _> = before.entries.iter().map(|e| (e.path(), e)).collect();
    let new: BTreeMap<_, _> = after.entries.iter().map(|e| (e.path(), e)).collect();
    let paths: std::collections::BTreeSet<_> = old.keys().chain(new.keys()).copied().collect();
    let changes = paths
        .into_iter()
        .filter_map(|path| {
            let before = old.get(path).copied();
            let after = new.get(path).copied();
            if before == after {
                return None;
            }
            Some(ProjectChange {
                path: path.into(),
                kind: match (before, after) {
                    (None, _) => "added",
                    (_, None) => "deleted",
                    _ => "modified",
                }
                .into(),
                before: before.cloned(),
                after: after.cloned(),
            })
        })
        .collect();
    ProjectDiff {
        baseline_digest: before.digest.clone(),
        current_digest: after.digest.clone(),
        changes,
    }
}

fn preview_text(
    root: &ProjectRoot,
    revision: &ProjectRevision,
    path: &str,
) -> Result<(Option<String>, &'static str)> {
    match revision.entries.iter().find(|e| e.path() == path) {
        None => Ok((None, "missing")),
        Some(ProjectEntry::Directory { .. }) => Ok((None, "directory")),
        Some(ProjectEntry::File { bytes, sha256, .. }) => {
            if *bytes > 256 * 1024 {
                return Ok((None, "too_large"));
            }
            match root.read_file(path) {
                Ok(file) if file.sha256 == *sha256 => {
                    if file.text.contains('\0') {
                        Ok((None, "binary"))
                    } else {
                        Ok((Some(file.text), "text"))
                    }
                }
                Ok(_) => Err(AutomationError::FileChanged),
                Err(AutomationError::Invalid("file is not UTF-8 text")) => Ok((None, "binary")),
                Err(error) => Err(error),
            }
        }
    }
}

/// A bounded per-file text preview. The full trees are verified both before and
/// after reading; stale digest requests must be reviewed again, never guessed.
pub fn compare_file(
    old: &ProjectRoot,
    old_revision: &ProjectRevision,
    new: &ProjectRoot,
    new_revision: &ProjectRevision,
    path: &str,
) -> Result<ProjectFileDiff> {
    crate::validate_project_path(path)?;
    if old.revision()? != *old_revision || new.revision()? != *new_revision {
        return Err(AutomationError::FileChanged);
    }
    let (before, old_kind) = preview_text(old, old_revision, path)?;
    let (after, new_kind) = preview_text(new, new_revision, path)?;
    if old.revision()? != *old_revision || new.revision()? != *new_revision {
        return Err(AutomationError::FileChanged);
    }
    let preview = if [old_kind, new_kind].contains(&"too_large") {
        "too_large"
    } else if [old_kind, new_kind].contains(&"binary") {
        "binary"
    } else if [old_kind, new_kind].contains(&"directory") {
        "directory"
    } else if old_kind == "missing" && new_kind == "missing" {
        return Err(AutomationError::Invalid("diff path is absent"));
    } else {
        "text"
    };
    // The caller renders side-by-side text. This avoids quadratic edit-distance
    // work on adversarial files and caps each side at 256 KiB.
    Ok(ProjectFileDiff {
        path: path.into(),
        baseline_digest: old_revision.digest.clone(),
        current_digest: new_revision.digest.clone(),
        before,
        after,
        preview: preview.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_changes_preserve_text_and_binary_summary_without_touching_files() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("edited.yml"), "old\r\n").unwrap();
        std::fs::write(b.path().join("edited.yml"), "new\n").unwrap();
        std::fs::write(a.path().join("deleted"), "gone").unwrap();
        std::fs::create_dir(b.path().join("empty")).unwrap();
        std::fs::write(b.path().join("binary"), [0xff, 0, 1]).unwrap();
        std::fs::write(b.path().join("large"), vec![b'x'; 256 * 1024 + 1]).unwrap();
        let old = ProjectRoot::open(a.path()).unwrap();
        let new = ProjectRoot::open(b.path()).unwrap();
        let before = old.revision().unwrap();
        let after = new.revision().unwrap();
        let diff = compare_revisions(&before, &after);
        assert_eq!(diff.changes.len(), 5);
        assert!(
            diff.changes
                .iter()
                .any(|c| c.path == "deleted" && c.kind == "deleted")
        );
        let text = compare_file(&old, &before, &new, &after, "edited.yml").unwrap();
        assert_eq!(text.before.as_deref(), Some("old\r\n"));
        assert_eq!(text.after.as_deref(), Some("new\n"));
        assert_eq!(
            compare_file(&old, &before, &new, &after, "binary")
                .unwrap()
                .preview,
            "binary"
        );
        assert_eq!(
            compare_file(&old, &before, &new, &after, "large")
                .unwrap()
                .preview,
            "too_large"
        );
        assert_eq!(
            compare_file(&old, &before, &new, &after, "empty")
                .unwrap()
                .preview,
            "directory"
        );
        assert!(compare_file(&old, &before, &new, &after, "../outside").is_err());
        assert!(compare_file(&old, &before, &new, &after, "absent").is_err());
        std::fs::write(b.path().join("edited.yml"), "changed after review").unwrap();
        assert!(matches!(
            compare_file(&old, &before, &new, &after, "edited.yml"),
            Err(AutomationError::FileChanged)
        ));
    }
    #[test]
    fn executable_mode_and_type_changes_are_visible() {
        let before = ProjectRevision {
            version: 1,
            digest: "a".repeat(64),
            bytes: 1,
            entries: vec![
                ProjectEntry::File {
                    path: "run".into(),
                    mode: 0o600,
                    bytes: 1,
                    sha256: "c".repeat(64),
                },
                ProjectEntry::Directory { path: "old".into() },
            ],
        };
        let mut after = before.clone();
        after.digest = "b".repeat(64);
        if let ProjectEntry::File { mode, .. } = &mut after.entries[0] {
            *mode = 0o700;
        }
        after.entries[1] = ProjectEntry::File {
            path: "old".into(),
            mode: 0o600,
            bytes: 0,
            sha256: "d".repeat(64),
        };
        assert_eq!(compare_revisions(&before, &after).changes.len(), 2);
        assert!(compare_revisions(&before, &before).changes.is_empty());
    }
}
