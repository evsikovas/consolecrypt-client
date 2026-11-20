use crate::{AutomationError, Result};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

const MAX_FILES: usize = 10_000;
const MAX_ENTRIES: usize = 20_000;
const MAX_BYTES: u64 = 100 * 1024 * 1024;

fn is_vault_password_path(path: &str) -> bool {
    path.split_once('/').is_some_and(|(folder, name)| {
        folder.trim_end_matches([' ', '.']).eq_ignore_ascii_case(".vault")
            && name.trim_end_matches([' ', '.']).eq_ignore_ascii_case("password")
    })
}

pub fn validate_project_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 1024
        || value
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':')
    {
        return Err(AutomationError::Invalid("invalid project path"));
    }
    let parts: Vec<_> = value.split('/').collect();
    if parts.len() > 32
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == ".." || part.len() > 255)
        || parts[0] == ".git"
    {
        return Err(AutomationError::Invalid("project path escapes its root"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectEntry {
    File {
        path: String,
        mode: u32,
        bytes: u64,
        sha256: String,
    },
    Directory {
        path: String,
    },
}

impl ProjectEntry {
    pub fn path(&self) -> &str {
        match self {
            Self::File { path, .. } | Self::Directory { path } => path,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectRevision {
    pub version: u32,
    pub digest: String,
    pub entries: Vec<ProjectEntry>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    pub path: String,
    pub text: String,
    pub sha256: String,
    pub line_endings: String,
}

/// Explicit capture policy. Ordinary snapshots retain historical exclusions;
/// publication refuses omitted data rather than silently selecting a lossy copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRevisionPolicy {
    Snapshot,
    RejectExcludedEntries,
}

#[derive(Debug)]
pub struct ProjectRoot {
    root: Dir,
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    publication_boundary: bool,
}

impl ProjectRoot {
    /// Publication/selected-generation I/O policy only. This clones the
    /// SAME capability; it is not a new path selection or approval authority.
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    pub(crate) fn publication_root(&self) -> Result<Self> {
        publication::check_local_directory(&self.root)?;
        Ok(Self {
            root: self.root.try_clone()?,
            publication_boundary: true,
        })
    }
    fn child_directory(&self, parent: &Dir, name: &str) -> Result<Dir> {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        if self.publication_boundary {
            return publication::open_boundary_directory(parent, name);
        }
        Ok(parent.open_dir_nofollow(name)?)
    }
    fn child_metadata(&self, parent: &Dir, name: &str) -> Result<cap_std::fs::Metadata> {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        if self.publication_boundary {
            return publication::boundary_metadata(parent, Path::new(name));
        }
        Ok(parent.symlink_metadata(name)?)
    }
    fn create_new_file(
        &self,
        parent: &Dir,
        name: &str,
        options: &OpenOptions,
        mode: u32,
    ) -> Result<cap_std::fs::File> {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        if self.publication_boundary {
            return publication::create_boundary_file(parent, Path::new(name), mode);
        }
        let _ = mode;
        Ok(parent.open_with(name, options)?)
    }
    /// Directory identity only, not a content/permission assertion. The
    /// validation caller must ALSO use the existing complete revision walker.
    pub fn same_directory(&self, other: &Self) -> Result<bool> {
        #[cfg(unix)]
        {
            let a = self.root.dir_metadata()?;
            let b = other.root.dir_metadata()?;
            if a.ino() == 0 || b.ino() == 0 {
                return Err(AutomationError::Invalid(
                    "unsupported project directory identity",
                ));
            }
            Ok(a.dev() == b.dev() && a.ino() == b.ino())
        }
        #[cfg(windows)]
        {
            let a = self.root.try_clone()?.into_std_file();
            let b = other.root.try_clone()?.into_std_file();
            Ok(crate::project_identity_windows::same(&a, &b)?)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = other;
            Err(AutomationError::Invalid(
                "unsupported project directory identity",
            ))
        }
    }
    /// Opaque freshness metadata from the SAME full-width identity as
    /// same_directory. This digest grants nothing and does not attest content.
    pub fn directory_identity_digest(&self) -> Result<String> {
        publication::directory_identity_digest(&self.root)
    }
    /// Fresh current name must still identify the captured directory. This
    /// check has a defined timepoint; it does not lock names against subsequent
    /// same-UID renames. Signed immutable job snapshots retain separate policy.
    pub fn reopen_same_directory(&self, path: &Path) -> Result<Self> {
        let current = Self::open(path)?;
        if !self.same_directory(&current)? {
            return Err(AutomationError::FileChanged);
        }
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        let current = if self.publication_boundary {
            current.publication_root()?
        } else {
            current
        };
        Ok(current)
    }

    pub(crate) fn import_directory(&self, relative: &str) -> Result<()> {
        let (parent, name) = self.parent(relative, false)?;
        #[cfg(target_os = "macos")]
        if self.publication_boundary {
            publication::create_boundary_directory(&parent, &name)?;
            return Ok(());
        }
        let builder = cap_std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use cap_std::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        parent.create_dir_with(name, &builder)?;
        Ok(())
    }
    pub(crate) fn sync_import_file(&self, file: &cap_std::fs::File) -> Result<()> {
        #[cfg(target_os = "macos")]
        if self.publication_boundary {
            return publication::full_sync_file(file);
        }
        file.sync_all()?;
        Ok(())
    }
    pub(crate) fn import_file(&self, relative: &str, mode: u32) -> Result<cap_std::fs::File> {
        if ![0o600, 0o700].contains(&mode) {
            return Err(AutomationError::Protocol("invalid imported mode"));
        }
        let (parent, name) = self.parent(relative, false)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(mode);
        }
        let file = self.create_new_file(&parent, &name, &options, mode)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(AutomationError::Invalid(
                "imported file is not a new regular file",
            ));
        }
        Ok(file)
    }
    fn parent(&self, relative: &str, create: bool) -> Result<(Dir, String)> {
        validate_project_path(relative)?;
        let mut parts: Vec<_> = relative.split('/').collect();
        let name = parts
            .pop()
            .ok_or(AutomationError::Invalid("empty file path"))?;
        let mut directory = self.root.try_clone()?;
        for component in parts {
            directory = match self.child_directory(&directory, component) {
                Ok(child) => child,
                Err(AutomationError::Io(error))
                    if create && error.kind() == std::io::ErrorKind::NotFound =>
                {
                    let builder = cap_std::fs::DirBuilder::new();
                    #[cfg(unix)]
                    let builder = {
                        use cap_std::fs::DirBuilderExt;
                        let mut builder = builder;
                        builder.mode(0o700);
                        builder
                    };
                    match directory.create_dir_with(component, &builder) {
                        Ok(()) => (),
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                        Err(error) => return Err(error.into()),
                    }
                    self.child_directory(&directory, component)?
                }
                Err(error) => return Err(error),
            };
        }
        Ok((directory, name.into()))
    }

    /// Establish authority over a folder explicitly selected by the user. All
    /// subsequent reads stay capability-relative, including renamed directories.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() || std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(AutomationError::Invalid(
                "select an absolute project folder, not a link",
            ));
        }
        Ok(Self {
            root: Dir::open_ambient_dir(path, cap_std::ambient_authority())?,
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
            publication_boundary: false,
        })
    }

    pub(crate) fn open_file(&self, relative: &str) -> Result<cap_std::fs::File> {
        validate_project_path(relative)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let (parent, name) = self.parent(relative, false)?;
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        let file = if self.publication_boundary {
            publication::read_boundary_file(&self.root, Path::new(relative))?
        } else {
            parent.open_with(name, &options)?
        };
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
        let file = parent.open_with(name, &options)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(AutomationError::Invalid(
                "project contains a special file or hardlink",
            ));
        }
        Ok(file)
    }

    pub fn read_text(&self, relative: &str) -> Result<String> {
        let file = self.open_file(relative)?;
        if file.metadata()?.len() > 256 * 1024 {
            return Err(AutomationError::Invalid("file exceeds text preview limit"));
        }
        let mut bytes = Vec::new();
        file.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 256 * 1024 {
            return Err(AutomationError::Invalid(
                "file grew beyond text preview limit",
            ));
        }
        String::from_utf8(bytes).map_err(|_| AutomationError::Invalid("file is not UTF-8 text"))
    }

    pub fn read_file(&self, relative: &str) -> Result<ProjectFile> {
        if is_vault_password_path(relative) {
            return Err(AutomationError::Invalid("project password is not an editor file"));
        }
        let text = self.read_text(relative)?;
        let line_endings = if text.contains("\r\n") {
            if text.replace("\r\n", "").contains('\n') {
                "mixed"
            } else {
                "crlf"
            }
        } else if text.contains('\n') {
            "lf"
        } else {
            "none"
        };
        Ok(ProjectFile {
            path: relative.into(),
            sha256: hex::encode(Sha256::digest(text.as_bytes())),
            text,
            line_endings: line_endings.into(),
        })
    }

    /// Move a reviewed file without replacing an existing destination.
    pub fn move_file(&self, source: &str, directory: &str, expected_sha256: &str) -> Result<()> {
        let name = source.rsplit('/').next().unwrap_or_default();
        let target = if directory.is_empty() {
            name.to_string()
        } else {
            format!("{directory}/{name}")
        };
        self.change_file(source, Some(&target), expected_sha256)
    }

    /// Rename in the original directory, or delete one reviewed regular file.
    pub fn edit_file_entry(&self, source: &str, new_name: Option<&str>, expected_sha256: &str) -> Result<()> {
        let target = match new_name {
            Some(name) => {
                if name.contains('/') {
                    return Err(AutomationError::Invalid("enter a file name, not a path"));
                }
                validate_project_path(name)?;
                Some(match source.rsplit_once('/') {
                    Some((directory, _)) => format!("{directory}/{name}"),
                    None => name.to_string(),
                })
            }
            None => None,
        };
        self.change_file(source, target.as_deref(), expected_sha256)
    }

    /// Rename a reviewed directory or remove it once empty.
    pub fn edit_directory_entry(&self, source: &str, new_name: Option<&str>, expected_digest: &str) -> Result<()> {
        fn protected(path: &str) -> bool {
            path.split('/').any(|part| {
                let part = part.trim_end_matches([' ', '.']);
                part.eq_ignore_ascii_case(".git") || part.eq_ignore_ascii_case(".vault")
            })
        }
        validate_project_path(source)?;
        if protected(source) {
            return Err(AutomationError::Invalid("cannot change protected project directories"));
        }
        if let Some(name) = new_name {
            validate_project_path(name)?;
            if name.contains('/') || protected(name) {
                return Err(AutomationError::Invalid("enter a directory name, not a path"));
            }
        }
        let (parent, name) = self.parent(source, false)?;
        let directory = self.child_directory(&parent, &name)?;
        let current = self.revision()?;
        if current.digest != expected_digest || !current.entries.iter().any(|e| {
            matches!(e, ProjectEntry::Directory { path } if path == source)
        }) {
            return Err(AutomationError::FileChanged);
        }
        if let Some(target) = new_name {
            if target == name { return Err(AutomationError::FileChanged); }
            publication::rename_project_directory(&parent, &name, target, directory)?;
        } else {
            if directory.entries()?.next().is_some() {
                return Err(AutomationError::Invalid("project directory is not empty"));
            }
            directory.remove_open_dir()?;
        }
        Ok(())
    }

    fn change_file(&self, source: &str, target: Option<&str>, expected_sha256: &str) -> Result<()> {
        if target == Some(source) {
            return Err(AutomationError::FileChanged);
        }
        for path in std::iter::once(source).chain(target) {
            validate_project_path(path)?;
            if path.eq_ignore_ascii_case(crate::GIT_SOURCE_FILE)
                || is_vault_password_path(path)
                || path.split('/').any(|part| part.eq_ignore_ascii_case(".git"))
            {
                return Err(AutomationError::Invalid("cannot change protected project files"));
            }
        }
        let (source_parent, name) = self.parent(source, false)?;
        let destination = target.map(|path| self.parent(path, false)).transpose()?;
        #[cfg(unix)]
        let mut file = self.open_file(source)?;
        #[cfg(windows)]
        let mut file = publication::open_project_move_file(&source_parent, &name)?;
        let before = file.metadata()?;
        if before.len() > MAX_BYTES || expected_sha256.len() != 64 {
            return Err(AutomationError::FileChanged);
        }
        let (bytes, digest) = hash_project_file(&mut file, MAX_BYTES)?;
        let current = file.metadata()?;
        if bytes != before.len()
            || bytes != current.len()
            || before.modified()? != current.modified()?
            || current.nlink() != 1
            || digest != expected_sha256
        {
            return Err(AutomationError::FileChanged);
        }
        #[cfg(unix)]
        {
            let named = self.child_metadata(&source_parent, &name)?;
            if named.ino() != current.ino()
                || named.dev() != current.dev()
                || !named.is_file()
                || named.nlink() != 1
            {
                return Err(AutomationError::FileChanged);
            }
            if let Some((target_parent, target_name)) = destination {
                rustix::fs::renameat_with(
                    &source_parent,
                    &name,
                    &target_parent,
                    &target_name,
                    rustix::fs::RenameFlags::NOREPLACE,
                )
                .map_err(|error| AutomationError::Io(error.into()))?;
            } else {
                source_parent.remove_file(&name)?;
            }
        }
        #[cfg(windows)]
        if let Some((target_parent, target_name)) = destination {
            publication::rename_project_move_file(&file, &target_parent, &target_name)?;
        } else {
            publication::delete_project_file(&file)?;
        }
        Ok(())
    }

    /// Save one reviewed file with optimistic conflict detection. Write a new
    /// inode and replace the directory entry; never write through a hardlink or
    /// symlink. External editors are not a filesystem transaction coordinator.
    pub fn write_file(
        &self,
        relative: &str,
        expected_sha256: Option<&str>,
        text: &str,
    ) -> Result<ProjectFile> {
        if is_vault_password_path(relative) {
            return Err(AutomationError::Invalid("Vault password files use the dedicated secret source"));
        }
        if relative == crate::GIT_SOURCE_FILE {
            return Err(AutomationError::Invalid(
                "Git provenance is maintained by source import",
            ));
        }
        validate_project_path(relative)?;
        if text.len() > 256 * 1024 {
            return Err(AutomationError::Invalid("file exceeds editor limit"));
        }
        let (parent, name) = self.parent(relative, expected_sha256.is_none())?;
        let original = match self.child_metadata(&parent, &name) {
            Ok(_) => Some(self.read_file(relative)?),
            Err(AutomationError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if original.as_ref().map(|file| file.sha256.as_str()) != expected_sha256 {
            return Err(AutomationError::FileChanged);
        }
        let text = if original
            .as_ref()
            .is_some_and(|file| file.line_endings == "crlf")
        {
            text.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            text.to_string()
        };
        if text.len() > 256 * 1024 {
            return Err(AutomationError::Invalid("saved file exceeds editor limit"));
        }
        let temporary = format!(".cc-edit-{}.tmp", uuid::Uuid::new_v4());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        let saved_mode = {
            use cap_std::fs::{MetadataExt, OpenOptionsExt};
            let mode = if original.is_some() {
                self.open_file(relative)?.metadata()?.mode() & 0o777
            } else {
                0o600
            };
            options.mode(mode);
            mode
        };
        #[cfg(not(unix))]
        let saved_mode = 0o600;
        let result = (|| -> Result<()> {
            let mut file = self.create_new_file(&parent, &temporary, &options, saved_mode)?;
            file.write_all(text.as_bytes())?;
            self.sync_import_file(&file)?;
            drop(file);
            if expected_sha256.is_some() {
                if self.read_file(relative)?.sha256.as_str() != expected_sha256.unwrap_or_default()
                {
                    return Err(AutomationError::FileChanged);
                }
                parent.rename(&temporary, &parent, &name)?;
            } else {
                // No-clobber creation, even if another editor creates the name
                // between our initial read and this commit.
                parent.hard_link(&temporary, &parent, &name)?;
                parent.remove_file(&temporary)?;
            }
            #[cfg(unix)]
            {
                // cap-std may hold an O_PATH capability: fsync on that handle
                // returns EBADF. Reopen the SAME directory read-only relative
                // to its capability, without following a link or ambient path.
                use cap_std::fs::OpenOptionsExt;
                let mut options = OpenOptions::new();
                options
                    .read(true)
                    .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW);
                parent.open_with(".", &options)?.sync_all()?;
            }
            Ok(())
        })();
        let _ = parent.remove_file(&temporary);
        result?;
        self.read_file(relative)
    }

    /// Build a complete built-in role in a private sibling and publish it with
    /// a no-replace directory rename. Existing files/directories are preserved.
    pub fn create_role(&self, name: &str) -> Result<ProjectFile> {
        if name.is_empty()
            || name.len() > 64
            || !name.as_bytes()[0].is_ascii_lowercase()
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(AutomationError::Invalid("use a lowercase role identifier"));
        }
        let revision = self.revision()?;
        // Include a possibly new `roles` parent as well as the role root,
        // eight subdirectories and seven files. Reject before creating it.
        if revision.entries.len() + 17 > MAX_ENTRIES
            || revision
                .entries
                .iter()
                .filter(|entry| matches!(entry, ProjectEntry::File { .. }))
                .count()
                + 7
                > MAX_FILES
            || revision.bytes + 4096 > MAX_BYTES
        {
            return Err(AutomationError::Invalid("role exceeds project budget"));
        }
        let relative = format!("roles/{name}");
        let (parent, target) = self.parent(&relative, true)?;
        match self.child_metadata(&parent, &target) {
            Ok(_) => return Err(AutomationError::FileChanged),
            Err(AutomationError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
        let temporary = format!(".cc-role-{}", uuid::Uuid::new_v4());
        let builder = cap_std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use cap_std::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        parent.create_dir_with(&temporary, &builder)?;
        let result = (|| -> Result<()> {
            let stage = ProjectRoot {
                root: self.child_directory(&parent, &temporary)?,
                #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
                publication_boundary: self.publication_boundary,
            };
            for directory in [
                "defaults",
                "files",
                "handlers",
                "meta",
                "tasks",
                "templates",
                "tests",
                "vars",
            ] {
                stage.import_directory(directory)?;
            }
            let files = [
                ("tasks/main.yml", "---\n- name: Describe this role\n  ansible.builtin.debug:\n    msg: ConsoleCrypt role scaffold\n".to_owned()),
                ("defaults/main.yml", "---\n{}\n".to_owned()),
                ("vars/main.yml", "---\n{}\n".to_owned()),
                ("handlers/main.yml", "---\n[]\n".to_owned()),
                ("meta/main.yml", "---\ndependencies: []\n".to_owned()),
                ("README.md",format!("# {name}\n\nDescribe this role, its variables and prerequisites before use.\n\nCall it from a project playbook using `roles: [{{role: {name}}}]`.\nChoose and document your own license before publishing the role.\n")),
                ("tests/README.md", "# Role checks\n\nReference this role from a playbook at the project root.\nIn ConsoleCrypt, select the inventory and run validation before execution.\nNo localhost target or deployment is created automatically.\n".to_owned()),
            ];
            for (path, text) in files {
                stage.write_file(path, None, &text)?;
            }
            drop(stage);
            #[cfg(unix)]
            rustix::fs::renameat_with(
                &parent,
                &temporary,
                &parent,
                &target,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(|error| AutomationError::Io(error.into()))?;
            // Windows directory rename fails when ANY destination already
            // exists. Unlike a file rename it cannot replace an empty folder.
            #[cfg(windows)]
            parent.rename(&temporary, &parent, &target)?;
            #[cfg(not(any(unix, windows)))]
            return Err(AutomationError::Invalid("role creation unavailable"));
            Ok(())
        })();
        // Only this newly created, capability-relative staging entry is ever
        // removed. Never roll back or delete the destination after publishing.
        if result.is_err() {
            let _ = parent.remove_dir_all(&temporary);
        }
        result?;
        self.read_file(&format!("{relative}/tasks/main.yml"))
    }

    pub fn revision(&self) -> Result<ProjectRevision> {
        self.revision_with_policy(ProjectRevisionPolicy::Snapshot)
    }
    pub fn revision_for_publication(&self) -> Result<ProjectRevision> {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        let strict = self.publication_root()?;
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        let root = &strict;
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
        let root = self;
        root.revision_with_policy(ProjectRevisionPolicy::RejectExcludedEntries)
    }
    fn revision_with_policy(&self, policy: ProjectRevisionPolicy) -> Result<ProjectRevision> {
        let mut entries = Vec::new();
        let mut bytes = 0;
        let mut files = 0;
        let source = self.git_source()?;
        #[cfg(windows)]
        let executable_paths = source
            .map(|(source, _)| source.executable_paths.into_iter().collect())
            .unwrap_or_default();
        #[cfg(not(windows))]
        let executable_paths = {
            let _ = source;
            std::collections::BTreeSet::new()
        };
        self.walk(
            &self.root,
            "",
            &mut entries,
            &mut bytes,
            &mut files,
            &executable_paths,
            policy,
        )?;
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        // serde_json maps may use either BTreeMap or preserve_order in a host
        // workspace. Insert keys recursively in sorted order for Python parity.
        let canonical = canonical(serde_json::json!({"version": 1, "entries": entries}));
        let encoded = serde_json::to_vec(&canonical)
            .map_err(|_| AutomationError::Invalid("revision serialization"))?;
        Ok(ProjectRevision {
            version: 1,
            digest: hex::encode(Sha256::digest(encoded)),
            entries,
            bytes,
        })
    }

    fn walk(
        &self,
        directory: &Dir,
        prefix: &str,
        records: &mut Vec<ProjectEntry>,
        bytes: &mut u64,
        files: &mut usize,
        executable_paths: &std::collections::BTreeSet<String>,
        policy: ProjectRevisionPolicy,
    ) -> Result<()> {
        let before = directory.dir_metadata()?;
        let mut names = Vec::new();
        for entry in directory.entries()? {
            if names.len() + records.len() >= MAX_ENTRIES {
                return Err(AutomationError::Invalid(
                    "project entry count exceeds limit",
                ));
            }
            names.push(
                entry?
                    .file_name()
                    .into_string()
                    .map_err(|_| AutomationError::Invalid("project filename is not Unicode"))?,
            );
        }
        names.sort();
        for name in names {
            if name == ".git" {
                if policy == ProjectRevisionPolicy::RejectExcludedEntries {
                    // Before symlink_metadata/open/traversal, regardless of type.
                    return Err(AutomationError::PublicationUnsupported(
                        "excluded .git entry",
                    ));
                }
                continue;
            }
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            // This fixed local secret is consumed only by Native's explicit
            // Vault action. Never hash/upload it, show it in the tree or export
            // it to AI/public project archives, even when it is a symlink.
            if is_vault_password_path(&path) {
                if policy == ProjectRevisionPolicy::RejectExcludedEntries {
                    return Err(AutomationError::PublicationUnsupported("excluded local Vault password"));
                }
                continue;
            }
            validate_project_path(&path)?;
            let metadata = self.child_metadata(directory, &name)?;
            if metadata.file_type().is_symlink() {
                return Err(AutomationError::Invalid("project contains a symlink"));
            }
            if metadata.is_dir() {
                records.push(ProjectEntry::Directory { path: path.clone() });
                self.walk(
                    &self.child_directory(directory, &name)?,
                    &path,
                    records,
                    bytes,
                    files,
                    executable_paths,
                    policy,
                )?;
            } else {
                if !metadata.is_file()
                    || *files >= MAX_FILES
                    || metadata.len() > MAX_BYTES.saturating_sub(*bytes)
                {
                    return Err(AutomationError::Invalid(
                        "project file type, count or byte limit",
                    ));
                }
                *files += 1;
                let mut file = self.open_file(&path)?;
                let opened = file.metadata()?;
                let (size, sha256) = hash_project_file(&mut file, MAX_BYTES.saturating_sub(*bytes))?;
                *bytes += size;
                let after = file.metadata()?;
                if opened.len() != size
                    || opened.len() != after.len()
                    || opened.modified()? != after.modified()?
                    || after.nlink() != 1
                {
                    return Err(AutomationError::Invalid("project changed while capturing"));
                }
                #[cfg(unix)]
                let mode = {
                    use cap_std::fs::MetadataExt;
                    if opened.mode() & 0o111 != 0 {
                        0o700
                    } else {
                        0o600
                    }
                };
                #[cfg(not(unix))]
                let mode = if executable_paths.contains(&path) {
                    0o700
                } else {
                    0o600
                };
                records.push(ProjectEntry::File {
                    path,
                    mode,
                    bytes: size,
                    sha256,
                });
            }
        }
        if before.modified()? != directory.dir_metadata()?.modified()? {
            return Err(AutomationError::Invalid("project changed while capturing"));
        }
        Ok(())
    }
}

fn hash_project_file(file: &mut impl Read, limit: u64) -> Result<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut size = 0;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            return Ok((size, hex::encode(hasher.finalize())));
        }
        size += count as u64;
        if size > limit {
            return Err(AutomationError::Invalid("project exceeds byte limit"));
        }
        hasher.update(&buffer[..count]);
    }
}

pub(crate) fn canonical(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let sorted: std::collections::BTreeMap<_, _> = object.into_iter().collect();
            serde_json::Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key, canonical(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(array) => {
            serde_json::Value::Array(array.into_iter().map(canonical).collect())
        }
        other => other,
    }
}

impl ProjectRoot {
    /// A new import must be ACTUALLY empty, including snapshot-excluded names.
    pub(crate) fn require_empty_import(&self) -> Result<()> {
        if self.root.entries()?.next().transpose()?.is_some() {
            return Err(AutomationError::FileChanged);
        }
        Ok(())
    }
}

// Private until the filesystem and AppCore commit gates have passed.
pub(crate) mod publication;

// Reuse the existing descriptor-relative Linux publication kernel for the
// single fixed service key record, not a second path walker or ACL checker.
#[cfg(target_os = "linux")]
pub(crate) mod service_identity_boundary {
    use super::*;
    pub(crate) fn check(dir: &Dir) -> Result<()> {
        publication::check_local_directory(dir)
    }
    pub(crate) fn child(dir: &Dir, name: &str) -> Result<Dir> {
        publication::open_boundary_directory(dir, name)
    }
    pub(crate) fn read(dir: &Dir, name: &Path) -> Result<cap_std::fs::File> {
        publication::read_boundary_file(dir, name)
    }
    pub(crate) fn create(dir: &Dir, name: &Path, mode: u32) -> Result<cap_std::fs::File> {
        publication::create_boundary_file(dir, name, mode)
    }
}
