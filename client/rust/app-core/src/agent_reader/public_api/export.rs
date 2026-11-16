//! ONLY this original handle's generated Node exports. No arbitrary source API.
//! Published destination paths are NEVER deleted: identity-check/unlink is racy.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    io::{Seek, SeekFrom},
    path::PathBuf,
};

// Never block on a swapped Unix FIFO or follow a Windows reparse leaf.
pub(super) fn open_regular(path: &Path) -> AppResult<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(path).map_err(|_| denied())?;
    let m = file.metadata().map_err(|_| denied())?;
    if !m.is_file() || m.len() > 16384 {
        return Err(denied());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() != 1 {
            return Err(denied());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(denied());
        }
    }
    Ok(file)
}
fn live_export(original: &NativeExports, ctx: &ReaderOriginalContext) -> AppResult<()> {
    original.slot.check().map_err(failure)?;
    let now = u64::try_from(ctx.now()?.timestamp()).map_err(|_| failure(Failure::Expired))?;
    if now >= original.expires_at {
        return Err(failure(Failure::Expired));
    }
    Ok(())
}

pub(super) struct NativeExportFile {
    path: PathBuf,
    file: fs::File,
    hash: [u8; 32],
    bytes: usize,
}
impl NativeExportFile {
    pub fn created(path: PathBuf, file: fs::File, bytes: &[u8]) -> Self {
        Self {
            path,
            file,
            hash: Sha256::digest(bytes).into(),
            bytes: bytes.len(),
        }
    }
    fn read_native(&self) -> AppResult<Zeroizing<Vec<u8>>> {
        local_path(&self.path, false)?;
        let visible = open_regular(&self.path)?;
        if !same(&self.file, &visible).map_err(|_| denied())? {
            return Err(denied());
        }
        let mut f = self.file.try_clone().map_err(|_| denied())?;
        f.seek(SeekFrom::Start(0)).map_err(|_| denied())?;
        let mut data = Zeroizing::new(Vec::new());
        f.take(16385).read_to_end(&mut data).map_err(|_| denied())?;
        if self.bytes > 16384
            || data.len() != self.bytes
            || <[u8; 32]>::from(Sha256::digest(&*data)) != self.hash
        {
            return Err(denied());
        }
        let current = open_regular(&self.path)?;
        if !same(&self.file, &current).map_err(|_| denied())? {
            return Err(denied());
        }
        Ok(data)
    }
}
// Guard is acquired on a Native-owned directory, never an arbitrary picker
// parent. On Windows withholding SHARE_DELETE binds its path through ACL work.
struct DirectoryGuard {
    file: fs::File,
    path: PathBuf,
}
impl DirectoryGuard {
    fn open(path: &Path) -> AppResult<Self> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options
                .custom_flags(0x02000000 | 0x00200000)
                .share_mode(1 | 2);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path).map_err(|_| denied())?;
        let value = Self {
            file,
            path: path.to_path_buf(),
        };
        value.check()?;
        Ok(value)
    }
    fn check(&self) -> AppResult<()> {
        let metadata = self.file.metadata().map_err(|_| denied())?;
        let path_metadata = fs::symlink_metadata(&self.path).map_err(|_| denied())?;
        if !metadata.is_dir() || !path_metadata.is_dir() || path_metadata.file_type().is_symlink() {
            return Err(denied());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0
                || path_metadata.file_attributes() & 0x400 != 0
            {
                return Err(denied());
            }
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options
                .custom_flags(0x02000000 | 0x00200000)
                .share_mode(1 | 2);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let visible = options.open(&self.path).map_err(|_| denied())?;
        if !same(&self.file, &visible).map_err(|_| denied())? {
            return Err(denied());
        }
        Ok(())
    }
}
pub(super) struct NativeExports {
    public: NativeExportFile,
    invitation: Option<NativeExportFile>,
    // File guards drop before the directory, allowing owned cleanup on Windows.
    directory_guard: DirectoryGuard,
    _directory: tempfile::TempDir,
    slot: ReaderOwnerSlot,
    expires_at: u64,
}
impl NativeExports {
    pub fn new(
        directory: tempfile::TempDir,
        slot: ReaderOwnerSlot,
        public: NativeExportFile,
        invitation: Option<NativeExportFile>,
        expires_at: u64,
    ) -> AppResult<Self> {
        let directory_guard = DirectoryGuard::open(directory.path())?;
        Ok(Self {
            public,
            invitation,
            directory_guard,
            _directory: directory,
            slot,
            expires_at,
        })
    }
}
#[cfg(windows)]
fn same(a: &fs::File, b: &fs::File) -> std::io::Result<bool> {
    cc_automation_core::same_existing_file_identity(a, b)
}
#[cfg(unix)]
fn same(a: &fs::File, b: &fs::File) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let a = a.metadata()?;
    let b = b.metadata()?;
    Ok(a.dev() == b.dev() && a.ino() != 0 && a.ino() == b.ino())
}
#[cfg(not(any(unix, windows)))]
fn same(_a: &fs::File, _b: &fs::File) -> std::io::Result<bool> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "file identity",
    ))
}
fn local_path(path: &Path, missing_leaf: bool) -> AppResult<()> {
    if !path.is_absolute()
        || path.as_os_str().len() > 1024
        || path.file_name().is_none()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(denied());
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if !matches!(path.components().next(),Some(Component::Prefix(v)) if matches!(v.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
        {
            return Err(denied());
        }
    }
    for (index, p) in path.ancestors().enumerate() {
        let m = match fs::symlink_metadata(p) {
            Ok(m) => m,
            Err(e) if index == 0 && missing_leaf && e.kind() == std::io::ErrorKind::NotFound => {
                continue
            }
            Err(_) => return Err(denied()),
        };
        if m.file_type().is_symlink() {
            return Err(denied());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(denied());
            }
        }
        if index == 0 {
            if missing_leaf || !m.is_file() {
                return Err(denied());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if m.nlink() != 1 {
                    return Err(denied());
                }
            }
        } else if !m.is_dir() {
            return Err(denied());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let p = fs::metadata(path.parent().ok_or_else(denied)?).map_err(|_| denied())?;
        if p.mode() & 0o022 != 0 && p.mode() & 0o1000 == 0 {
            return Err(denied());
        }
    }
    Ok(())
}
struct Copy {
    file: tempfile::NamedTempFile,
    directory_guard: DirectoryGuard,
    _origin_guard: DirectoryGuard,
    directory: tempfile::TempDir,
    path: PathBuf,
    hash: [u8; 32],
    bytes: usize,
}
impl Copy {
    async fn prepare(source: &NativeExportFile, destination: PathBuf) -> AppResult<Self> {
        local_path(&destination, true)?;
        // Only the ORIGINAL Native-owned protected parent, never the user-selected
        // destination parent, may hold bytes before atomic publication.
        let origin_guard = DirectoryGuard::open(source.path.parent().ok_or_else(denied)?)?;
        let data = source.read_native()?;
        let directory = tempfile::Builder::new()
            .prefix(".consolecrypt-reader-")
            .tempdir_in(&origin_guard.path)
            .map_err(|_| denied())?;
        let directory_guard = DirectoryGuard::open(directory.path())?;
        origin_guard.check()?;
        directory_guard.check()?;
        crate::kubernetes_proxy::restrict_directory(directory.path())
            .await
            .map_err(|_| denied())?;
        origin_guard.check()?;
        directory_guard.check()?;
        let mut file = tempfile::NamedTempFile::new_in(directory.path()).map_err(|_| denied())?;
        origin_guard.check()?;
        directory_guard.check()?;
        file.write_all(&data)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| denied())?;
        directory_guard.check()?;
        Ok(Self {
            file,
            directory_guard,
            _origin_guard: origin_guard,
            directory,
            path: destination,
            hash: source.hash,
            bytes: data.len(),
        })
    }
    fn publish(self) -> AppResult<Published> {
        self.directory_guard.check()?;
        self._origin_guard.check()?;
        let file = self.file.persist_noclobber(&self.path).map_err(|error| {
            #[cfg(windows)]
            let cross_volume = error.error.raw_os_error() == Some(17);
            #[cfg(unix)]
            let cross_volume = error.error.raw_os_error() == Some(libc::EXDEV);
            #[cfg(not(any(windows, unix)))]
            let cross_volume = false;
            failure(if cross_volume {
                Failure::Unavailable
            } else {
                Failure::OutcomeUnknown
            })
        })?;
        Ok(Published {
            file,
            _directory_guard: self.directory_guard,
            _origin_guard: self._origin_guard,
            _directory: self.directory,
            path: self.path,
            hash: self.hash,
            bytes: self.bytes,
        })
    }
}
struct Published {
    file: fs::File,
    _directory_guard: DirectoryGuard,
    _origin_guard: DirectoryGuard,
    _directory: tempfile::TempDir,
    path: PathBuf,
    hash: [u8; 32],
    bytes: usize,
}
impl Published {
    fn verify(&self) -> AppResult<()> {
        local_path(&self.path, false)?;
        let visible = open_regular(&self.path)?;
        if !same(&self.file, &visible).map_err(|_| denied())? {
            return Err(denied());
        }
        let mut f = self.file.try_clone().map_err(|_| denied())?;
        f.seek(SeekFrom::Start(0)).map_err(|_| denied())?;
        let mut data = Zeroizing::new(Vec::new());
        f.take(16385).read_to_end(&mut data).map_err(|_| denied())?;
        if data.len() != self.bytes || <[u8; 32]>::from(Sha256::digest(&*data)) != self.hash {
            return Err(denied());
        }
        Ok(())
    }
}
struct OriginalExport {
    shared: Arc<Shared>,
    original: Option<NativeExports>,
}
impl Drop for OriginalExport {
    fn drop(&mut self) {
        if let Some(value) = self.original.take() {
            let _ = self.shared.publish(Held::Export(value));
        }
        self.shared.transferring.store(false, Ordering::SeqCst);
    }
}
impl ReaderActionHandle {
    pub async fn transfer_export_json(
        &self,
        public_destination: String,
        invitation_destination: Option<String>,
    ) -> AppResult<String> {
        let core = self.original_core()?;
        self.shared.check()?;
        if self.shared.transferring.swap(true, Ordering::SeqCst) {
            return Err(denied());
        }
        let value = { self.shared.held.lock().map_err(|_| denied())?.take() };
        let exports = match value {
            Some(Held::Export(v)) => v,
            other => {
                if let Some(v) = other {
                    let _ = self.shared.publish(v);
                }
                self.shared.transferring.store(false, Ordering::SeqCst);
                return Err(denied());
            }
        };
        let guard = OriginalExport {
            shared: self.shared.clone(),
            original: Some(exports),
        };
        let original = guard.original.as_ref().ok_or_else(denied)?;
        original.slot.check().map_err(failure)?;
        self.shared.original.check(&core).await?;
        original.directory_guard.check()?;
        if self.shared.original.now()?.timestamp() < 0
            || self.shared.original.now()?.timestamp() as u64 >= original.expires_at
        {
            return Err(failure(Failure::Expired));
        }
        let public = PathBuf::from(public_destination);
        local_path(&public, true)?;
        let invitation = invitation_destination.map(PathBuf::from);
        if let Some(path) = &invitation {
            if original.invitation.is_none() || path == &public {
                return Err(denied());
            }
            local_path(path, true)?;
        }
        let mut copies = Vec::new();
        copies.push(Copy::prepare(&original.public, public.clone()).await?);
        if let Some(path) = &invitation {
            copies.push(
                Copy::prepare(
                    original.invitation.as_ref().ok_or_else(denied)?,
                    path.clone(),
                )
                .await?,
            );
        }
        self.shared.original.check(&core).await?;
        self.shared.check()?;
        original.slot.check().map_err(failure)?;
        let mut published = Vec::new();
        for copy in copies {
            // All ACL/preparation awaits have finished. Recheck the ORIGINAL
            // signed export expiry immediately before EACH irreversible publish.
            let fence = async {
                self.shared.original.check(&core).await?;
                self.shared.check()?;
                live_export(original, &self.shared.original)
            }
            .await;
            if let Err(error) = fence {
                return Err(if published.is_empty() {
                    error
                } else {
                    failure(Failure::OutcomeUnknown)
                });
            }
            // Once publication was invoked, ANY later error remains unknown.
            let value = copy.publish().map_err(|error| {
                if published.is_empty() {
                    error
                } else {
                    failure(Failure::OutcomeUnknown)
                }
            })?;
            published.push(value);
            if self.shared.original.check(&core).await.is_err()
                || self.shared.check().is_err()
                || original.slot.check().is_err()
                || self
                    .shared
                    .original
                    .now()
                    .map(|t| t.timestamp() < 0 || t.timestamp() as u64 >= original.expires_at)
                    .unwrap_or(true)
                || published.last().ok_or_else(denied)?.verify().is_err()
            {
                return Err(failure(Failure::OutcomeUnknown));
            }
        }
        for value in &published {
            value
                .verify()
                .map_err(|_| failure(Failure::OutcomeUnknown))?;
        }
        let result = json!({"kind":"nodeExportTransferred","publicDestination":public.to_string_lossy(),
            "invitationDestination":invitation.as_ref().map(|p|p.to_string_lossy().into_owned()),
            "publicSha256":hex::encode(original.public.hash),"publicBytes":original.public.bytes,
            "invitationBytes":invitation.as_ref().and(original.invitation.as_ref()).map(|v|v.bytes),
            "actorApply":"notObserved","copiesIrreversible":true});
        let encoded =
            serde_json::to_string(&result).map_err(|_| failure(Failure::OutcomeUnknown))?;
        self.shared
            .original
            .check(&core)
            .await
            .map_err(|_| failure(Failure::OutcomeUnknown))?;
        self.shared
            .check()
            .map_err(|_| failure(Failure::OutcomeUnknown))?;
        if self
            .shared
            .original
            .now()
            .map(|t| t.timestamp() < 0 || t.timestamp() as u64 >= original.expires_at)
            .unwrap_or(true)
        {
            return Err(failure(Failure::OutcomeUnknown));
        }
        // No remove_file(destination), even when identity mismatches. Only owned
        // private staging roots drop; user-selected published copies remain.
        Ok(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn generated(root: &Path) -> NativeExportFile {
        let path = root.join("native-created.json");
        let bytes = b"actual owned export bytes";
        let mut f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        f.write_all(bytes).unwrap();
        f.sync_all().unwrap();
        NativeExportFile::created(path, f, bytes)
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn transfer_after_actual_preparation_signed_expiry_fence_denies_publish() {
        use super::super::tests::{files, fixture, live_nonce, request};
        let (root, app, selection) = fixture().await;
        let (a, b) = files(&app, root.path()).await;
        let handle = app
            .reader_prepare_action_json(&request(&selection, &a, &b, &live_nonce(&app).await))
            .await
            .unwrap();
        handle.confirm_json().await.unwrap();
        let Some(Held::Export(mut exports)) = handle.shared.held.lock().unwrap().take() else {
            panic!("original generated export missing")
        };
        let destination = root.path().join("expired-never-published.json");
        let copy = Copy::prepare(&exports.public, destination.clone())
            .await
            .unwrap();
        // Private unit boundary injection changes ONLY this held export deadline;
        // source Offer bytes, OS clock, ACL work and filesystem stay actual.
        // It models expiry during the completed preparation await, not a runtime
        // elapsed-time or signed/Actor acceptance claim.
        exports.expires_at = stamp().unwrap().seconds();
        assert!(live_export(&exports, &handle.shared.original).is_err());
        drop(copy);
        assert!(!destination.exists());
        handle.shared.publish(Held::Export(exports)).unwrap();
        handle.cancel();
        app.shutdown().await.unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn transfer_actual_windows_directory_guard_denies_rename_and_path_replacement() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("private-staging");
        fs::create_dir(&path).unwrap();
        let guard = DirectoryGuard::open(&path).unwrap();
        let renamed = root.path().join("renamed");
        assert!(fs::rename(&path, &renamed).is_err());
        assert!(fs::remove_dir(&path).is_err());
        guard.check().unwrap();
        drop(guard);
        fs::rename(&path, &renamed).unwrap();
        fs::create_dir(&path).unwrap();
        let replacement = DirectoryGuard::open(&path).unwrap();
        let original = DirectoryGuard::open(&renamed).unwrap();
        assert!(!same(&replacement.file, &original.file).unwrap());
    }
    #[cfg(unix)]
    #[test]
    fn transfer_actual_directory_rebinding_denies_before_payload_creation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("private-staging");
        fs::create_dir(&path).unwrap();
        let guard = DirectoryGuard::open(&path).unwrap();
        fs::rename(&path, root.path().join("original")).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(guard.check().is_err());
        assert!(!path.join("invitation").exists());
    }
    #[tokio::test]
    async fn transfer_actual_files_identity_noclobber_and_partial_published_outputs_are_never_deleted(
    ) {
        let root = tempfile::tempdir().unwrap();
        let source = generated(root.path());
        let first = root.path().join("first.json");
        let second = root.path().join("second.json");
        let a = Copy::prepare(&source, first.clone()).await.unwrap();
        let b = Copy::prepare(&source, second.clone()).await.unwrap();
        // Real race at second noclobber publication: no mocked error or altered syscall.
        fs::write(&second, b"foreign sentinel").unwrap();
        let published = a.publish().unwrap();
        published.verify().unwrap();
        assert!(b.publish().is_err());
        drop(published);
        assert_eq!(
            fs::read(&first).unwrap(),
            source.read_native().unwrap().as_slice()
        );
        assert_eq!(fs::read(&second).unwrap(), b"foreign sentinel");
        assert!(Copy::prepare(&source, second.clone()).await.is_err());
        let other_path = root.path().join("other.json");
        fs::write(&other_path, b"other").unwrap();
        let other = fs::File::open(other_path).unwrap();
        assert!(!same(&source.file, &other).unwrap());
        let same_file = fs::File::open(&source.path).unwrap();
        assert!(same(&source.file, &same_file).unwrap());
    }
}
