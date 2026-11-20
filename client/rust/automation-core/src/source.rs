//! Provenance is ordinary bounded snapshot data, therefore already bound by
//! the existing signed revision digest. No manifest/schema migration is needed.
use crate::{validate_project_path, AutomationError, ProjectRoot, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read};

pub const GIT_SOURCE_FILE: &str = ".consolecrypt-git-source.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitProjectSource {
    pub version: u8,
    pub kind: String,
    pub url: String,
    pub reference: String,
    pub commit: String,
    pub ca_sha256: Option<String>,
    pub executable_paths: Vec<String>,
}
impl std::fmt::Debug for GitProjectSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitProjectSource")
            .field("commit", &self.commit)
            .finish_non_exhaustive()
    }
}
impl GitProjectSource {
    pub fn validate(&self) -> Result<()> {
        let url = url::Url::parse(&self.url).map_err(|_| invalid())?;
        if self.version != 1
            || self.kind != "git"
            || self.url.len() > 2048
            || self.url.contains('\\')
            || self
                .url
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
            || !matches!(url.scheme(), "http" | "https")
            || (url.scheme() == "http" && self.ca_sha256.is_some())
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.port() == Some(0)
            || ![40, 64].contains(&self.commit.len())
            || !self
                .commit
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || self
                .ca_sha256
                .as_ref()
                .is_some_and(|value| !cc_models::automation::is_digest(value))
            || self.reference.is_empty()
            || self.reference.len() > 240
            || self.reference.starts_with(['-', '/'])
            || self.reference.ends_with(['/', '.'])
            || self.reference.ends_with(".lock")
            || self.reference.contains("..")
            || self.reference.contains("//")
            || !self
                .reference
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_./-".contains(&c))
            || self.executable_paths.len() > 10_000
        {
            return Err(invalid());
        }
        let mut paths = BTreeSet::new();
        for path in &self.executable_paths {
            validate_project_path(path)?;
            if path == GIT_SOURCE_FILE || !paths.insert(path) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
fn invalid() -> AutomationError {
    AutomationError::Invalid("invalid or unsupported Git source metadata")
}

impl ProjectRoot {
    pub fn git_source(&self) -> Result<Option<(GitProjectSource, String)>> {
        let file = match self.open_file(GIT_SOURCE_FILE) {
            Ok(file) => file,
            Err(AutomationError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None)
            }
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err(invalid());
        }
        let value = crate::json::parse(&bytes)?;
        let source: GitProjectSource = serde_json::from_value(value).map_err(|_| invalid())?;
        source.validate()?;
        Ok(Some((source, hex::encode(Sha256::digest(bytes)))))
    }
}
