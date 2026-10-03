//! Application data directories and per-profile paths
//! (`<app data>/profiles/<id>/vault.db`, CLIENT_ARCHITECTURE §4, ADR-0106).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the data root (portable installs,
/// `cc` CLI end-to-end tests).
pub const DATA_DIR_ENV: &str = "CONSOLECRYPT_DATA_DIR";

/// `directories` project identifiers (matches the app bundle id
/// `io.consolecrypt.*`).
const QUALIFIER: &str = "io";
const ORGANIZATION: &str = "consolecrypt";
const APPLICATION: &str = "ConsoleCrypt";

/// Maximum profile id length.
pub const MAX_PROFILE_ID_LEN: usize = 64;

/// Errors resolving or creating directories.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DirsError {
    /// No home / app-data directory could be determined.
    #[error("could not determine the application data directory")]
    NoDataDir,
    /// Profile ids are 1..=64 chars of `[A-Za-z0-9_-]` (they become path
    /// components; this rules out traversal like `..` or separators).
    #[error("invalid profile id")]
    InvalidProfileId,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// Root of all ConsoleCrypt data on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    root: PathBuf,
}

impl AppDirs {
    /// Platform default (local, non-roaming — device keys and databases are
    /// per installation):
    /// * macOS: `~/Library/Application Support/io.consolecrypt.ConsoleCrypt`
    /// * Windows: `%LOCALAPPDATA%\consolecrypt\ConsoleCrypt\data`
    /// * Linux: `$XDG_DATA_HOME/consolecrypt` (`~/.local/share/consolecrypt`)
    pub fn system() -> Result<Self, DirsError> {
        directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .map(|p| Self {
                root: p.data_local_dir().to_path_buf(),
            })
            .ok_or(DirsError::NoDataDir)
    }

    /// [`DATA_DIR_ENV`] if set and non-empty, else [`AppDirs::system`].
    pub fn from_env_or_system() -> Result<Self, DirsError> {
        match std::env::var_os(DATA_DIR_ENV) {
            Some(v) if !v.is_empty() => Ok(Self::with_root(PathBuf::from(v))),
            _ => Self::system(),
        }
    }

    /// Explicit root (tests, portable mode).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The data root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/profiles`.
    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    /// `<root>/logs` (redacted application logs; never secrets).
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Paths of profile `profile_id` (not created).
    pub fn profile(&self, profile_id: &str) -> Result<ProfilePaths, DirsError> {
        validate_profile_id(profile_id)?;
        Ok(ProfilePaths {
            dir: self.profiles_dir().join(profile_id),
        })
    }

    /// Paths of profile `profile_id`, creating its directories (owner-only
    /// permissions on Unix).
    pub fn ensure_profile(&self, profile_id: &str) -> Result<ProfilePaths, DirsError> {
        let p = self.profile(profile_id)?;
        create_private_dir(&self.root)?;
        create_private_dir(&self.profiles_dir())?;
        create_private_dir(&p.dir)?;
        create_private_dir(&p.cache_dir())?;
        Ok(p)
    }

    /// Ids of existing profile directories (sorted; invalid names skipped).
    pub fn list_profiles(&self) -> Result<Vec<String>, DirsError> {
        let dir = self.profiles_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut ids: Vec<String> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| validate_profile_id(n).is_ok())
            .collect();
        ids.sort();
        Ok(ids)
    }
}

/// Files of one profile (one local database + one vault, ADR-0106).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePaths {
    dir: PathBuf,
}

impl ProfilePaths {
    /// `<root>/profiles/<id>`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// SQLCipher database with objects, envelopes, outbox (storage-core).
    pub fn vault_db(&self) -> PathBuf {
        self.dir.join("vault.db")
    }

    /// Rebuildable local search index (search-core).
    pub fn search_db(&self) -> PathBuf {
        self.dir.join("search.db")
    }

    /// Disposable cache.
    pub fn cache_dir(&self) -> PathBuf {
        self.dir.join("cache")
    }
}

/// Check a profile id: 1..=64 chars of `[A-Za-z0-9_-]`.
pub fn validate_profile_id(id: &str) -> Result<(), DirsError> {
    let ok = !id.is_empty()
        && id.len() <= MAX_PROFILE_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(DirsError::InvalidProfileId)
    }
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_default_preserves_pre_rdp_directory_identity() {
        let prior = directories::ProjectDirs::from("io", "consolecrypt", "ConsoleCrypt").unwrap();
        let current = AppDirs::system().unwrap();
        assert_eq!(current.root(), prior.data_local_dir());
        assert_eq!(DATA_DIR_ENV, "CONSOLECRYPT_DATA_DIR");
        let dev = directories::ProjectDirs::from("io", "consolecrypt", "ConsoleCryptDev").unwrap();
        assert_ne!(current.root(), dev.data_local_dir());
        // Pure path resolution: no user directories are opened or created.
    }

    #[test]
    fn profile_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = AppDirs::with_root(tmp.path());
        let p = dirs.ensure_profile("default").unwrap();
        assert!(p.dir().is_dir());
        assert!(p.cache_dir().is_dir());
        assert_eq!(p.vault_db(), tmp.path().join("profiles/default/vault.db"));
        assert_eq!(p.search_db(), tmp.path().join("profiles/default/search.db"));
        dirs.ensure_profile("work-2").unwrap();
        assert_eq!(dirs.list_profiles().unwrap(), vec!["default", "work-2"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(p.dir()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }

    #[test]
    fn profile_ids_cannot_escape() {
        let dirs = AppDirs::with_root("/tmp/x");
        for bad in [
            "",
            "..",
            "a/b",
            "a\\b",
            ".hidden",
            "sp ace",
            &"x".repeat(65),
        ] {
            assert!(dirs.profile(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn system_dir_resolves() {
        // CI machines always have a home directory.
        let d = AppDirs::system().unwrap();
        assert!(d.root().is_absolute());
        assert!(d.profiles_dir().ends_with("profiles"));
    }
}
