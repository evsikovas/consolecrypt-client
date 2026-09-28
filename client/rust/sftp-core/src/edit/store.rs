//! Local storage of edit sessions: private directories and files, the
//! per-session manifest, leftovers after a crash and secure removal.
//!
//! Layout: `<root>/<session-uuid>/<file name>` plus the manifest
//! `<root>/<session-uuid>/.cc-edit-session.json` (host id, remote path, base
//! version — no file contents). Directories are 0700, files 0600 (Unix);
//! macOS: `<root>/.metadata_never_index` keeps Spotlight out.

use super::types::EditError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Manifest file name inside a session directory.
pub(crate) const MANIFEST: &str = ".cc-edit-session.json";
const MANIFEST_FORMAT: u32 = 1;
const MAX_NAME_BYTES: usize = 200;

/// The remote version a working copy is based on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BaseVersion {
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    #[serde(with = "hex32")]
    pub sha256: [u8; 32],
    pub permissions: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

impl BaseVersion {
    pub(crate) fn new(e: &crate::RemoteEntry, sha256: [u8; 32]) -> Self {
        Self {
            size: e.size,
            modified: e.modified,
            sha256,
            permissions: e.permissions,
            uid: e.uid,
            gid: e.gid,
        }
    }

    /// Does the remote entry still describe this version (size + mtime)?
    pub(crate) fn matches(&self, e: &crate::RemoteEntry) -> bool {
        self.size == e.size && self.modified == e.modified
    }
}

mod hex32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let mut out = [0u8; 32];
        hex::decode_to_slice(s, &mut out).map_err(serde::de::Error::custom)?;
        Ok(out)
    }
}

/// Per-session manifest (enables "Recover unsaved edits?").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub format: u32,
    pub session_id: Uuid,
    pub host_id: String,
    pub remote_path: String,
    pub target_path: String,
    pub file_name: String,
    pub created_at: DateTime<Utc>,
    pub base: BaseVersion,
}

impl Manifest {
    pub(crate) fn new(
        session_id: Uuid,
        host_id: &str,
        remote_path: &str,
        target_path: &str,
        file_name: &str,
        base: BaseVersion,
    ) -> Self {
        Self {
            format: MANIFEST_FORMAT,
            session_id,
            host_id: host_id.to_string(),
            remote_path: remote_path.to_string(),
            target_path: target_path.to_string(),
            file_name: file_name.to_string(),
            created_at: Utc::now(),
            base,
        }
    }

    pub(crate) fn save(&self, dir: &Path) -> Result<(), EditError> {
        let path = dir.join(MANIFEST);
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| EditError::local(&path, io::Error::new(io::ErrorKind::InvalidData, e)))?;
        // Write-then-rename so a crash never leaves a truncated manifest.
        let tmp = dir.join(format!("{MANIFEST}.tmp"));
        write_private_file(&tmp, &json, false).map_err(|e| EditError::local(&tmp, e))?;
        fs::rename(&tmp, &path).map_err(|e| EditError::local(&path, e))
    }

    pub(crate) fn load(dir: &Path) -> Option<Self> {
        let data = fs::read(dir.join(MANIFEST)).ok()?;
        let m: Self = serde_json::from_slice(&data).ok()?;
        // The file name becomes a path component: never trust it blindly.
        (m.format == MANIFEST_FORMAT && sanitize_file_name(&m.file_name) == m.file_name)
            .then_some(m)
    }
}

/// An edit directory left behind (crash, disconnect, `StopMode::KeepFiles`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Leftover {
    pub session_id: Uuid,
    pub dir: PathBuf,
    /// From the manifest (absent if it is missing / unreadable).
    pub host_id: Option<String>,
    pub remote_path: Option<String>,
    pub target_path: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    /// The working copy, if present.
    pub working_file: Option<PathBuf>,
    /// `Some(true)`: the working copy differs from the version last
    /// downloaded / uploaded (unsaved edits on the server).
    pub locally_modified: Option<bool>,
}

/// List leftover session directories under `root` (non-UUID entries are
/// ignored). Nothing is modified.
pub fn list_leftovers(root: &Path) -> Result<Vec<Leftover>, EditError> {
    let rd = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(EditError::local(root, e)),
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|n| Uuid::parse_str(n).ok())
        else {
            continue;
        };
        let dir = entry.path();
        let manifest = Manifest::load(&dir);
        let working_file = manifest
            .as_ref()
            .map(|m| dir.join(&m.file_name))
            .filter(|p| p.is_file());
        let locally_modified = match (&manifest, &working_file) {
            (Some(m), Some(p)) => fs::read(p).ok().map(|d| sha256(&d) != m.base.sha256),
            _ => None,
        };
        out.push(Leftover {
            session_id: id,
            dir,
            host_id: manifest.as_ref().map(|m| m.host_id.clone()),
            remote_path: manifest.as_ref().map(|m| m.remote_path.clone()),
            target_path: manifest.as_ref().map(|m| m.target_path.clone()),
            created_at: manifest.as_ref().map(|m| m.created_at),
            working_file,
            locally_modified,
        });
    }
    out.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then(a.session_id.cmp(&b.session_id))
    });
    Ok(out)
}

/// Securely remove the leftover `session_id` under `root`.
pub fn remove_leftover(
    root: &Path,
    session_id: Uuid,
    overwrite_limit: u64,
) -> Result<(), EditError> {
    let dir = root.join(session_id.to_string());
    if !dir.is_dir() {
        return Err(EditError::UnknownSession);
    }
    secure_remove_dir(&dir, overwrite_limit).map_err(|e| EditError::local(&dir, e))
}

pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(data).into()
}

/// Create `path` (and parents) with owner-only permissions.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        // Windows: per-user app-data directories are private by default ACLs.
        fs::create_dir_all(path)
    }
}

/// Create a new (must not exist) owner-only directory.
pub(crate) fn create_session_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new().mode(0o700).create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

/// Write an owner-only file (0600). With `create_new` it must not exist;
/// otherwise it is truncated in place (same inode, so editors see a change).
pub(crate) fn write_private_file(path: &Path, data: &[u8], create_new: bool) -> io::Result<()> {
    let mut o = fs::OpenOptions::new();
    o.write(true);
    if create_new {
        o.create_new(true);
    } else {
        o.create(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path)?;
    #[cfg(unix)]
    if !create_new {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(data)?;
    f.sync_all()
}

/// Prepare the edit root: 0700, Spotlight marker and (optionally) a Time
/// Machine exclusion on macOS.
pub(crate) fn prepare_root(root: &Path, exclude_from_backup: bool) -> io::Result<()> {
    create_private_dir(root)?;
    #[cfg(target_os = "macos")]
    {
        let marker = root.join(".metadata_never_index");
        if !marker.exists() {
            match write_private_file(&marker, b"", true) {
                Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
                _ => {}
            }
        }
        let flag = root.join(".cc-backup-excluded");
        if exclude_from_backup && !flag.exists() {
            let ok = std::process::Command::new("/usr/bin/tmutil")
                .arg("addexclusion")
                .arg(root)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                tracing::debug!(root = %root.display(), "tmutil addexclusion failed (ignored)");
            }
            // Attempted once per edit root (best effort, no process per open).
            let _ = write_private_file(&flag, b"", true);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // TODO(client): Windows/Linux backup exclusion — no portable mechanism;
        // next: evaluate per-platform markers (e.g. CACHEDIR.TAG) with the backup tools we support.
        let _ = exclude_from_backup;
    }
    Ok(())
}

/// Best-effort secure removal: overwrite regular files up to `limit` bytes
/// with zeros, then delete the tree (symlinks are never followed).
pub(crate) fn secure_remove_dir(dir: &Path, limit: u64) -> io::Result<()> {
    wipe_tree(dir, limit, 0);
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

fn wipe_tree(dir: &Path, limit: u64, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() {
            wipe_tree(&p, limit, depth + 1);
        } else if ft.is_file() {
            let _ = overwrite_with_zeros(&p, limit);
            let _ = fs::remove_file(&p);
        }
    }
}

fn overwrite_with_zeros(p: &Path, limit: u64) -> io::Result<()> {
    let meta = fs::symlink_metadata(p)?;
    if !meta.is_file() || meta.len() > limit || meta.len() == 0 {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
    }
    let mut f = fs::OpenOptions::new().write(true).open(p)?;
    let zeros = [0u8; 64 * 1024];
    let mut left = meta.len();
    while left > 0 {
        let n = left.min(zeros.len() as u64) as usize;
        f.write_all(&zeros[..n])?;
        left -= n as u64;
    }
    f.sync_data()
}

/// Local name for a remote file name: characters that are invalid on any
/// supported OS become `_`, Windows device names are prefixed, long names are
/// shortened keeping the extension (so the OS still picks the right app).
pub(crate) fn sanitize_file_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    while s.ends_with([' ', '.']) {
        s.pop();
    }
    if s.is_empty() {
        return "file".into();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved || s == MANIFEST || s.starts_with(&format!("{MANIFEST}.")) {
        s.insert(0, '_');
    }
    if s.len() > MAX_NAME_BYTES {
        let (stem, ext) = split_ext(&s);
        let ext = ext.filter(|e| e.len() <= 20).map(|e| format!(".{e}"));
        let keep = MAX_NAME_BYTES - ext.as_ref().map_or(0, String::len);
        let mut cut = keep.min(stem.len());
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        s = format!("{}{}", &stem[..cut], ext.unwrap_or_default());
    }
    s
}

/// `name.ext` → (`name`, `Some("ext")`); dot-files have no extension.
pub(crate) fn split_ext(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], Some(&name[i + 1..])),
        _ => (name, None),
    }
}

/// `<stem>.remote-<UTC timestamp>[.<ext>]` (extension kept so the copy opens
/// in the same editor).
pub(crate) fn remote_copy_name(file_name: &str, ts: DateTime<Utc>, n: u32) -> String {
    let stamp = ts.format("%Y%m%dT%H%M%SZ");
    let suffix = if n > 0 {
        format!("-{n}")
    } else {
        String::new()
    };
    match split_ext(file_name) {
        (stem, Some(ext)) => format!("{stem}.remote-{stamp}{suffix}.{ext}"),
        (stem, None) => format!("{stem}.remote-{stamp}{suffix}"),
    }
}

/// Last component of a remote path.
pub(crate) fn remote_base_name(path: &str) -> &str {
    let t = path.trim_end_matches('/');
    t.rsplit('/').next().unwrap_or(t)
}

/// Directory part of a remote path (`""` for a bare name, `/` for the root).
pub(crate) fn remote_parent(path: &str) -> &str {
    let t = path.trim_end_matches('/');
    match t.rfind('/') {
        Some(0) => "/",
        Some(i) => &t[..i],
        None => "",
    }
}
