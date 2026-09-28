//! Indexable documents and conversions from the plaintext domain model.

use cc_models::history::HistoryEntry;
use cc_models::host::Host;
use cc_models::note::Note;
use cc_models::snippet::{Snippet, SnippetType};
use cc_models::ObjectId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// What kind of object a [`Document`] was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    Snippet,
    Note,
    History,
    Host,
}

impl DocKind {
    /// All kinds, in a stable order.
    pub const ALL: [DocKind; 4] = [
        DocKind::Snippet,
        DocKind::Note,
        DocKind::History,
        DocKind::Host,
    ];

    /// Stable storage label.
    pub const fn as_str(&self) -> &'static str {
        match self {
            DocKind::Snippet => "snippet",
            DocKind::Note => "note",
            DocKind::History => "history",
            DocKind::Host => "host",
        }
    }

    /// Inverse of [`DocKind::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

impl fmt::Display for DocKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A unit of the local knowledge base: `{id, kind, title, body, tags}`.
///
/// Documents hold plaintext user content (commands, notes, history). They are
/// stored only in the local, rebuildable index and must never be logged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub id: ObjectId,
    pub kind: DocKind,
    pub title: String,
    pub body: String,
    /// Normalized on insert (trimmed, lowercased, whitespace → `-`, deduped).
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Document {
    pub fn new(
        id: ObjectId,
        kind: DocKind,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id,
            kind,
            title: title.into(),
            body: body.into(),
            tags: Vec::new(),
        }
    }

    /// Builder-style tag setter; tags are normalized.
    pub fn with_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.tags = normalize_tags(tags);
        self
    }

    /// Snippet → document: title = name, body = description + template,
    /// tags = snippet tags + snippet type label.
    pub fn from_snippet(s: &Snippet) -> Self {
        let mut body = String::new();
        if !s.description.trim().is_empty() {
            body.push_str(s.description.trim());
            body.push('\n');
        }
        body.push_str(&s.template);
        let mut tags: Vec<String> = s.tags.clone();
        if let Some(package) = &s.package_name {
            tags.push(package.clone());
        }
        tags.push(snippet_type_tag(s.snippet_type).to_owned());
        Self::new(s.id, DocKind::Snippet, s.name.clone(), body).with_tags(tags)
    }

    /// Note → document.
    pub fn from_note(n: &Note) -> Self {
        Self::new(n.id, DocKind::Note, n.title.clone(), n.body.clone()).with_tags(&n.tags)
    }

    /// History entry → document (title = first line of the command).
    pub fn from_history(h: &HistoryEntry) -> Self {
        let title: String = h.command.lines().next().unwrap_or_default().to_owned();
        let mut tags = Vec::new();
        if h.exit_code.is_some_and(|c| c != 0) {
            tags.push("failed");
        }
        Self::new(h.id, DocKind::History, title, h.command.clone()).with_tags(tags)
    }

    /// Host → document: title = name, body = address, username and notes.
    /// No credential data exists on [`Host`] (secrets are referenced by id).
    pub fn from_host(h: &Host) -> Self {
        let mut body = h.address.clone();
        if let Some(u) = &h.username {
            body.push('\n');
            body.push_str(u);
        }
        if !h.notes.trim().is_empty() {
            body.push('\n');
            body.push_str(h.notes.trim());
        }
        Self::new(h.id, DocKind::Host, h.name.clone(), body).with_tags(&h.tags)
    }

    /// Text used to compute an embedding for this document.
    pub fn embedding_text(&self) -> String {
        let mut s = String::with_capacity(self.title.len() + self.body.len() + 32);
        s.push_str(&self.title);
        s.push('\n');
        s.push_str(&self.body);
        if !self.tags.is_empty() {
            s.push('\n');
            s.push_str(&self.tags.join(" "));
        }
        s
    }

    /// Stable content fingerprint (FNV-1a 64, hex). Used to detect stale
    /// embeddings; not a security primitive.
    pub fn content_hash(&self) -> String {
        let mut h = Fnv64::new();
        h.write(self.kind.as_str().as_bytes());
        h.write(&[0]);
        h.write(self.title.as_bytes());
        h.write(&[0]);
        h.write(self.body.as_bytes());
        h.write(&[0]);
        for t in &self.tags {
            h.write(t.as_bytes());
            h.write(&[0]);
        }
        format!("{:016x}", h.finish())
    }

    pub(crate) fn normalized(&self) -> Self {
        let mut d = self.clone();
        d.tags = normalize_tags(&self.tags);
        d
    }
}

/// Normalize tags: trim, lowercase, inner whitespace → `-`, drop empty, dedupe
/// (first occurrence order preserved).
pub fn normalize_tags<I, S>(tags: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = normalize_tag(t.as_ref());
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

pub(crate) fn normalize_tag(t: &str) -> String {
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

/// Label used as an implicit tag for snippets of the given type.
pub const fn snippet_type_tag(t: SnippetType) -> &'static str {
    match t {
        SnippetType::Shell => "shell",
        SnippetType::Bash => "bash",
        SnippetType::Zsh => "zsh",
        SnippetType::Powershell => "powershell",
        SnippetType::Cmd => "cmd",
        SnippetType::Sql => "sql",
        SnippetType::Postgresql => "postgresql",
        SnippetType::Kubectl => "kubectl",
        SnippetType::Helm => "helm",
        SnippetType::Docker => "docker",
        SnippetType::Terraform => "terraform",
        SnippetType::Ansible => "ansible",
        SnippetType::RedisCli => "redis-cli",
        SnippetType::Cql => "cql",
        SnippetType::OpensearchDsl => "opensearch-dsl",
        SnippetType::HttpCurl => "curl",
    }
}

/// Minimal FNV-1a 64-bit hasher (stable across Rust versions, unlike
/// `DefaultHasher`).
struct Fnv64(u64);

impl Fnv64 {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    const fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_normalized() {
        assert_eq!(
            normalize_tags(["  K8s ", "k8s", "Prod  Cluster", ""]),
            vec!["k8s", "prod-cluster"]
        );
    }

    #[test]
    fn content_hash_is_stable_and_sensitive() {
        let id = ObjectId::new();
        let a = Document::new(id, DocKind::Note, "t", "b").with_tags(["x"]);
        let b = Document::new(id, DocKind::Note, "t", "b").with_tags(["X"]);
        assert_eq!(a.content_hash(), b.content_hash());
        let c = Document::new(id, DocKind::Note, "t", "b2").with_tags(["x"]);
        assert_ne!(a.content_hash(), c.content_hash());
        // Known FNV-1a value for a fixed input keeps the format stable.
        let mut h = Fnv64::new();
        h.write(b"a");
        assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn kind_roundtrip() {
        for k in DocKind::ALL {
            assert_eq!(DocKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(DocKind::parse("secret"), None);
    }
}
