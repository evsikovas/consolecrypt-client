//! Strict dependency archive evidence. No resolver, execution or publication.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

pub const MAX_ARCHIVE: usize = 32 * 1024 * 1024;
pub const MAX_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_FILES: usize = 10_000;
pub const MAX_ENTRIES: usize = 20_000;
pub const MAX_TAR: usize = MAX_BYTES as usize + MAX_FILES * 2 * 1024 + 1024 * 1024;
pub type Result<T> = std::result::Result<T, AdmissionError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionError {
    Source,
    Archive,
    ArchiveHash,
    Budget,
    Path,
    Tree,
    Conflict,
    Review,
    Io,
    Recovery,
    Unsupported,
    Transfer,
    Cancelled,
}
impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Dependency admission failed: {:?}", self)
    }
}
impl std::error::Error for AdmissionError {}
impl From<std::io::Error> for AdmissionError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}
impl From<crate::AutomationError> for AdmissionError {
    fn from(_: crate::AutomationError) -> Self {
        Self::Tree
    }
}
fn require(value: bool, error: AdmissionError) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(error)
    }
}
fn sha(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_lowercase() || (i > 0 && (b.is_ascii_digit() || b == b'_')))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Role,
    Collection,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSource {
    pub url: String,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceSelection {
    kind: DependencyKind,
    name: String,
    version: String,
    source: ArtifactSource,
    strip_components: u8,
}
impl SourceSelection {
    /// Call only with an exact source supplied/reviewed by the Native user.
    /// Constructing a pin is not evidence of publisher identity or code safety.
    pub fn new(
        kind: DependencyKind,
        name: String,
        version: String,
        source: ArtifactSource,
        strip_components: u8,
    ) -> Result<Self> {
        let result = Self {
            kind,
            name,
            version,
            source,
            strip_components,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        let valid_name = match self.kind {
            DependencyKind::Role => identifier(&self.name),
            DependencyKind::Collection => {
                let parts: Vec<_> = self.name.split('.').collect();
                parts.len() == 2
                    && parts.iter().all(|s| identifier(s))
                    && self.name != "ansible.builtin"
            }
        };
        let valid_version = !self.version.is_empty()
            && self.version.len() <= 80
            && self
                .version
                .bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_alphanumeric() || (i > 0 && b"._+-".contains(&b)))
            && !self.version.eq_ignore_ascii_case("latest");
        let raw = &self.source.url;
        let url = url::Url::parse(raw).map_err(|_| AdmissionError::Source)?;
        require(
            valid_name
                && valid_version
                && self.strip_components <= 1
                && digest(&self.source.sha256)
                && raw.len() <= 2048
                && raw.is_ascii()
                && raw.starts_with("https://")
                && !raw
                    .bytes()
                    .any(|b| b.is_ascii_control() || b.is_ascii_whitespace() || b == b'\\')
                && url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.port() != Some(0)
                && !raw[8..].split('/').next().unwrap_or("").contains('@'),
            AdmissionError::Source,
        )
    }
    pub fn target(&self) -> String {
        match self.kind {
            DependencyKind::Role => format!("roles/{}", self.name),
            DependencyKind::Collection => format!(
                "collections/ansible_collections/{}",
                self.name.replace('.', "/")
            ),
        }
    }
    pub fn origin(&self) -> String {
        format!(
            "https://{}",
            self.source.url[8..].split('/').next().unwrap_or("")
        )
    }
    pub fn source(&self) -> &ArtifactSource {
        &self.source
    }
}

fn encoded(value: &impl Serialize) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value).map_err(|_| AdmissionError::Tree)?;
    serde_json::to_vec(&crate::project::canonical(value)).map_err(|_| AdmissionError::Tree)
}

use crate::{ProjectEntry, ProjectRevision};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};
use unicode_normalization::UnicodeNormalization;

/// Archive-derived evidence. Private fields prevent a helper from fabricating
/// this witness by deserializing a claimed receipt or snapshot.
#[derive(Clone, Debug)]
pub struct VerifiedArchive {
    pub(crate) selection: SourceSelection,
    pub(crate) tree: ProjectRevision,
}
impl VerifiedArchive {
    pub fn tree(&self) -> &ProjectRevision {
        &self.tree
    }
    pub fn selection(&self) -> &SourceSelection {
        &self.selection
    }
}

pub(crate) fn portable_path(name: &str) -> Result<()> {
    crate::validate_project_path(name).map_err(|_| AdmissionError::Path)?;
    // Conservative v1 subset: reject all non-ASCII names rather than pretend
    // Rust lowercase implements full Unicode casefold. NFC is checked too.
    require(
        name.is_ascii() && name.nfc().eq(name.chars()),
        AdmissionError::Unsupported,
    )?;
    for part in name.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        let device = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
            || (stem.len() == 4
                && (stem.starts_with("com") || stem.starts_with("lpt"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'));
        require(
            !device
                && !part.ends_with([' ', '.'])
                && !part
                    .bytes()
                    .any(|b| b < 32 || b == 127 || b"<>\"|?*".contains(&b)),
            AdmissionError::Path,
        )?;
    }
    Ok(())
}
pub(crate) fn revision(entries: Vec<ProjectEntry>) -> Result<ProjectRevision> {
    let bytes = entries
        .iter()
        .map(|entry| match entry {
            ProjectEntry::File { bytes, .. } => *bytes,
            _ => 0,
        })
        .sum();
    let digest = sha(&encoded(
        &serde_json::json!({"version":1,"entries":entries}),
    )?);
    Ok(ProjectRevision {
        version: 1,
        digest,
        entries,
        bytes,
    })
}
fn octal(raw: &[u8]) -> Result<u64> {
    require(
        raw.iter().all(|b| matches!(b, b'0'..=b'7' | b' ' | 0)),
        AdmissionError::Archive,
    )?;
    let text = std::str::from_utf8(raw)
        .map_err(|_| AdmissionError::Archive)?
        .trim_matches([' ', '\0']);
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8).map_err(|_| AdmissionError::Archive)
}
fn field(raw: &[u8]) -> Result<&str> {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    require(raw[end..].iter().all(|b| *b == 0), AdmissionError::Archive)?;
    std::str::from_utf8(&raw[..end]).map_err(|_| AdmissionError::Path)
}

/// Hash the exact compressed artifact before any parser work. Derive a bounded
/// virtual USTAR tree from raw headers; never extract or run downloaded files.
/// PAX/GNU/sparse metadata is unsupported, not automatically expanded by a tar
/// library. Single gzip member only; trailing/second archives are rejected.
pub fn verify_archive(selection: SourceSelection, raw: &[u8]) -> Result<VerifiedArchive> {
    selection.validate()?;
    require(
        !raw.is_empty() && raw.len() <= MAX_ARCHIVE,
        AdmissionError::Budget,
    )?;
    require(
        sha(raw) == selection.source.sha256,
        AdmissionError::ArchiveHash,
    )?;
    let mut expanded = Vec::new();
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = flate2::bufread::GzDecoder::new(raw);
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = decoder
                .read(&mut buffer)
                .map_err(|_| AdmissionError::Archive)?;
            if count == 0 {
                break;
            }
            require(expanded.len() + count <= MAX_TAR, AdmissionError::Budget)?;
            expanded.extend_from_slice(&buffer[..count]);
        }
        require(decoder.into_inner().is_empty(), AdmissionError::Archive)?;
    } else {
        require(raw.len() <= MAX_TAR, AdmissionError::Budget)?;
        expanded.extend_from_slice(raw);
    }
    require(expanded.len() % 512 == 0, AdmissionError::Archive)?;
    let mut entries: BTreeMap<String, ProjectEntry> = BTreeMap::new();
    let mut folded: BTreeMap<String, String> = BTreeMap::new();
    let mut explicit = BTreeSet::new();
    let mut wrapper = None;
    let mut offset = 0usize;
    let mut members = 0usize;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut ended = false;
    fn claim(
        entries: &mut BTreeMap<String, ProjectEntry>,
        folded: &mut BTreeMap<String, String>,
        name: String,
        entry: ProjectEntry,
    ) -> Result<()> {
        portable_path(&name)?;
        require(entries.len() < MAX_ENTRIES, AdmissionError::Budget)?;
        let lower = name.to_ascii_lowercase();
        if let Some(previous) = folded.get(&lower) {
            require(previous == &name, AdmissionError::Path)?;
        }
        if let Some(previous) = entries.get(&name) {
            require(
                matches!(
                    (previous, &entry),
                    (
                        ProjectEntry::Directory { .. },
                        ProjectEntry::Directory { .. }
                    )
                ),
                AdmissionError::Path,
            )?;
            return Ok(());
        }
        folded.insert(lower, name.clone());
        entries.insert(name, entry);
        Ok(())
    }
    while offset + 512 <= expanded.len() {
        let header = &expanded[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            require(
                offset + 1024 <= expanded.len() && expanded[offset..].iter().all(|b| *b == 0),
                AdmissionError::Archive,
            )?;
            ended = true;
            break;
        }
        members += 1;
        require(members <= MAX_ENTRIES, AdmissionError::Budget)?;
        require(
            &header[257..263] == b"ustar\0" && &header[263..265] == b"00",
            AdmissionError::Unsupported,
        )?;
        let checksum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    *b as u64
                }
            })
            .sum();
        require(
            checksum == octal(&header[148..156])?,
            AdmissionError::Archive,
        )?;
        let kind = header[156];
        require(matches!(kind, 0 | b'0' | b'5'), AdmissionError::Unsupported)?;
        require(
            header[157..257].iter().all(|b| *b == 0),
            AdmissionError::Unsupported,
        )?;
        let size = octal(&header[124..136])?;
        let mode = octal(&header[100..108])?;
        require(size <= MAX_BYTES, AdmissionError::Budget)?;
        let name = field(&header[..100])?;
        let prefix = field(&header[345..500])?;
        let mut name = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if kind == b'5' && name.ends_with('/') {
            name.pop();
        }
        portable_path(&name)?;
        require(explicit.insert(name.clone()), AdmissionError::Path)?;
        let padding = (size as usize)
            .checked_add(511)
            .ok_or(AdmissionError::Budget)?
            / 512
            * 512;
        let next = offset
            .checked_add(512)
            .and_then(|n| n.checked_add(padding))
            .ok_or(AdmissionError::Budget)?;
        require(next <= expanded.len(), AdmissionError::Archive)?;
        if selection.strip_components == 1 {
            let (first, rest) = name.split_once('/').unwrap_or((&name, ""));
            if let Some(expected) = &wrapper {
                require(expected == first, AdmissionError::Path)?
            } else {
                wrapper = Some(first.to_owned());
            }
            if rest.is_empty() {
                require(kind == b'5' && size == 0, AdmissionError::Path)?;
                offset = next;
                continue;
            }
            name = rest.into();
        }
        let parts: Vec<_> = name.split('/').collect();
        for i in 1..parts.len() {
            let parent = parts[..i].join("/");
            claim(
                &mut entries,
                &mut folded,
                parent.clone(),
                ProjectEntry::Directory { path: parent },
            )?;
        }
        if kind == b'5' {
            require(size == 0, AdmissionError::Archive)?;
            claim(
                &mut entries,
                &mut folded,
                name.clone(),
                ProjectEntry::Directory { path: name },
            )?;
        } else {
            files += 1;
            bytes = bytes.checked_add(size).ok_or(AdmissionError::Budget)?;
            require(
                files <= MAX_FILES && bytes <= MAX_BYTES,
                AdmissionError::Budget,
            )?;
            let portable_mode = if mode & 0o111 != 0 { 0o700 } else { 0o600 };
            #[cfg(windows)]
            require(portable_mode == 0o600, AdmissionError::Unsupported)?;
            let body = &expanded[offset + 512..offset + 512 + size as usize];
            claim(
                &mut entries,
                &mut folded,
                name.clone(),
                ProjectEntry::File {
                    path: name,
                    mode: portable_mode,
                    bytes: size,
                    sha256: sha(body),
                },
            )?;
        }
        offset = next;
    }
    require(ended, AdmissionError::Archive)?;
    let required = match selection.kind {
        DependencyKind::Role => ["tasks/main.yml", "tasks/main.yaml"],
        DependencyKind::Collection => ["galaxy.yml", "MANIFEST.json"],
    };
    require(
        required
            .iter()
            .any(|name| matches!(entries.get(*name), Some(ProjectEntry::File { .. }))),
        AdmissionError::Tree,
    )?;
    Ok(VerifiedArchive {
        selection,
        tree: revision(entries.into_values().collect())?,
    })
}
