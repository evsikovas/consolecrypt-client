//! Directory capability derivation only; not a second project walker/SafeRoot.
//! All mutations remain relative to pinned directories, never ambient paths.
use super::*;
use std::{ffi::OsString, path::{Component, PathBuf}};
#[cfg(target_os = "linux")]
#[path = "linux_locality.rs"]
mod linux_locality;
#[cfg(target_os = "linux")]
use linux_locality as platform;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Isolated public Darwin ACL ABI; no production setter.
#[path = "macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn check_local_directory(dir: &Dir) -> Result<()> {
    supported(dir)?;
    #[cfg(target_os = "macos")]
    macos::verify_empty_acl(dir)?;
    Ok(())
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn boundary_error(error: std::io::Error) -> AutomationError {
    if error.kind() == std::io::ErrorKind::Unsupported {
        AutomationError::PublicationUnsupported(
            if cfg!(target_os = "linux") { "strong Linux publication path resolution unavailable" }
            else { "strong Darwin publication path resolution unavailable" },
        )
    } else {
        AutomationError::Io(error)
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn read_boundary_file(dir: &Dir, path: &Path) -> Result<cap_std::fs::File> {
    platform::read_file(dir, path).map_err(boundary_error)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn create_boundary_file(dir: &Dir, path: &Path, mode: u32) -> Result<cap_std::fs::File> {
    platform::create_file(dir, path, mode).map_err(boundary_error)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn boundary_metadata(dir: &Dir, path: &Path) -> Result<cap_std::fs::Metadata> {
    platform::metadata(dir, path).map_err(boundary_error)
}

struct Pinned {
    name: OsString,
    dir: Dir,
    identity: DirectoryIdentity,
}
pub(super) struct ManagedNamespace {
    anchor: Dir,
    ancestors: Vec<Pinned>,
    profile_index: usize,
    project: Uuid,
    pub(super) root: Dir,
}
fn directory(metadata: &cap_std::fs::Metadata) -> Result<()> {
    require(metadata.is_dir() && !metadata.file_type().is_symlink())?;
    #[cfg(windows)]
    require(cap_std::fs::MetadataExt::file_attributes(metadata) & 0x400 == 0)?;
    Ok(())
}
pub(super) fn open_child(parent: &Dir, name: impl AsRef<Path>) -> Result<Dir> {
    let name = name.as_ref();
    let parts = name.components().collect::<Vec<_>>();
    require(parts.len() == 1 && matches!(parts[0], Component::Normal(_)))?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // Observe metadata only after the platform adapter validates its held
        // descriptor (Linux NO_XDEV; Darwin APFS/current mount comparison).
        let child = platform::open_child(parent, name).map_err(boundary_error)?;
        directory(&child.dir_metadata()?)?;
        Ok(child)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let before = parent.symlink_metadata(name)?;
        directory(&before)?;
        let child = parent.open_dir_nofollow(name)?;
        let opened = child.dir_metadata()?;
        directory(&opened)?;
        // Supplemental race check; persistent Windows identity remains128bit.
        require(before.dev() == opened.dev() && before.ino() == opened.ino())?;
        Ok(child)
    }
}
fn open_ancestor(parent: &Dir, name: impl AsRef<Path>) -> Result<Dir> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let child = platform::open_ancestor(parent, name.as_ref()).map_err(boundary_error)?;
        directory(&child.dir_metadata()?)?;
        Ok(child)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        open_child(parent, name)
    }
}
fn private(dir: &Dir) -> Result<()> {
    owned_permissions(dir, true)
}
fn owned_permissions(dir: &Dir, private: bool) -> Result<()> {
    directory(&dir.dir_metadata()?)?;
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        let metadata = dir.dir_metadata()?;
        let forbidden = if private { 0o077 } else { 0o022 };
        require(
            cap_std::fs::MetadataExt::uid(&metadata) == rustix::process::geteuid().as_raw()
                && metadata.permissions().mode() & forbidden == 0,
        )?;
    }
    #[cfg(target_os = "macos")]
    macos::verify_empty_acl(dir)?;
    #[cfg(windows)]
    private_acl::verify(dir, false)?;
    // Existing profile/automation ACLs are observed, never silently repaired.
    let _ = private;
    Ok(())
}
/// Reuse the existing role policy for the copied stage/selected child too.
pub(super) fn verify_private_child(dir: &Dir) -> Result<()> {
    owned_permissions(dir, true)?;
    #[cfg(windows)]
    private_acl::verify(dir, true)?;
    Ok(())
}
pub(super) fn mkdir(parent: &Dir, name: &str) -> Result<Dir> {
    #[cfg(target_os = "macos")]
    macos::verify_empty_acl(parent)?;
    #[cfg(windows)]
    let created = private_acl::create(parent, name)?;
    #[cfg(not(windows))]
    let created = {
        let mut builder = cap_std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use cap_std::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        parent.create_dir_with(name, &builder)?;
        open_child(parent, name)?
    };
    #[cfg(target_os = "macos")]
    verify_private_child(&created)?;
    sync_directory(parent)?;
    Ok(created)
}
fn outside_all(dir:&Dir,excluded:&[DirectoryIdentity])->Result<()> {
    let id=identity(dir)?;
    require(excluded.iter().all(|other|*other!=id))
}
fn supported(dir: &Dir) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    platform::supported(dir).map_err(|_| {
        AutomationError::PublicationUnsupported(
            if cfg!(target_os = "linux") { "publication requires supported local Linux filesystem" }
            else { "publication requires supported writable local APFS" },
        )
    })?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        if !windows::is_local_ntfs(dir.as_handle())? {
            return Err(AutomationError::PublicationUnsupported(
                "publication requires local NTFS",
            ));
        }
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = dir;
        return Err(AutomationError::PublicationUnsupported(
            "unsupported publication platform",
        ));
    }
    let _ = dir;
    Ok(())
}
/// Fixed new-only seed capability. Existing legacy projects parent may be0755;
/// its NEW UUID child is private. No existing project or generation is adopted.
pub(super) struct ManagedSeedNamespace(ManagedNamespace);
impl ManagedSeedNamespace {
    pub(super) fn create(profile:&Path, project:Uuid)->Result<Self> {
        let mut n=ManagedNamespace::profile_pins(profile,project,&[])?;
        let mut current=n.root.try_clone()?;
        for (index,name) in ["automation".to_owned(),"projects".to_owned(),project.to_string()].into_iter().enumerate() {
            let next=if index==2 {
                // Bounded family capacity includes unpublished partials. Never
                // scan unbounded junk or adopt a collided previous UUID.
                let mut count=0usize;
                for entry in current.entries()? { let _=entry?; count+=1; require(count<=256)?; }
                if count>=256 {return Err(AutomationError::QueueFull);}
                // Exclusive create is the absence CAS; a collision is never opened.
                mkdir(&current,&name)?
            } else {match open_child(&current,&name) {
                Ok(dir)=>dir,
                Err(AutomationError::Io(e)) if e.kind()==std::io::ErrorKind::NotFound=>mkdir(&current,&name)?,
                Err(e)=>return Err(e),
            }};
            owned_permissions(&next,index==2)?;
            if index==2 {verify_private_child(&next)?;}
            require(same_volume(&identity(&current)?,&identity(&next)?))?;
            n.ancestors.push(Pinned{name:name.into(),identity:identity(&next)?,dir:next.try_clone()?});
            current=next;
        }
        n.root=current;
        let out=Self(n);out.check()?;Ok(out)
    }
    pub(super) fn root(&self)->Result<Dir>{Ok(self.0.root.try_clone()?)}
    pub(super) fn check(&self)->Result<()> {
        self.0.check_retained_policy(true)?;
        verify_private_child(&self.0.root)
    }
}
impl ManagedNamespace {
    pub(super) fn open(
        profile: &Path,
        project: Uuid,
        original: &ProjectRoot,
        candidate: &ProjectRoot,
    ) -> Result<Self> {
        Self::derive(profile, project, original, candidate, true)
    }
    pub(super) fn existing(
        profile: &Path,
        project: Uuid,
        original: &ProjectRoot,
        candidate: &ProjectRoot,
    ) -> Result<Self> {
        Self::derive(profile, project, original, candidate, false)
    }
    fn derive(
        profile: &Path,
        project: Uuid,
        original: &ProjectRoot,
        candidate: &ProjectRoot,
        create: bool,
    ) -> Result<Self> {
        let base = identity(&original.root)?;
        let candidate = identity(&candidate.root)?;
        require(base != candidate)?;
        let mut namespace = Self::profile(profile, project, original, &candidate)?;
        namespace.extend(original, &candidate, create, false)?;
        namespace.check_named_source(original)?;
        Ok(namespace)
    }
    fn profile(
        profile: &Path,
        project: Uuid,
        original: &ProjectRoot,
        candidate: &DirectoryIdentity,
    ) -> Result<Self> {
        let base=identity(&original.root)?;
        let namespace=Self::profile_pins(profile,project,&[base,candidate.clone()])?;
        check_source(&namespace.ancestors[namespace.profile_index].dir,project,original)?;
        Ok(namespace)
    }
    // SAME retained profile-ancestor capability derivation. New portable
    // projects have no fictitious original root or old SQL row.
    fn profile_pins(profile:&Path,project:Uuid,excluded:&[DirectoryIdentity])->Result<Self> {
        require(!project.is_nil() && profile.is_absolute())?;
        let mut anchor_path = PathBuf::new();
        let mut names = Vec::<OsString>::new();
        for component in profile.components() {
            match component {
                Component::Prefix(prefix) => {
                    #[cfg(windows)]
                    {
                        // No UNC/redirector/NT device namespace fallback.
                        require(matches!(
                            prefix.kind(),
                            std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                        ))?;
                    }
                    anchor_path.push(prefix.as_os_str());
                }
                Component::RootDir => anchor_path.push(component.as_os_str()),
                Component::Normal(name) => {
                    require(names.len() < 64)?;
                    names.push(name.into());
                }
                _ => return Err(recovery()),
            }
        }
        require(!names.is_empty() && anchor_path.is_absolute())?;
        let anchor = Dir::open_ambient_dir(&anchor_path, cap_std::ambient_authority())?;
        directory(&anchor.dir_metadata()?)?;
        outside_all(&anchor,excluded)?;
        let mut current = anchor.try_clone()?;
        let mut ancestors = Vec::new();
        // Read-only nofollow traversal of every profile ancestor. Retaining all
        // handles preserves actual Windows directory no-DELETE-share behavior.
        for name in names {
            current = open_ancestor(&current, &name)?;
            outside_all(&current,excluded)?;
            ancestors.push(Pinned {
                name,
                identity: identity(&current)?,
                dir: current.try_clone()?,
            });
        }
        private(&current)?;
        supported(&current)?; // locality/subset BEFORE ANY namespace writes
        let profile_index = ancestors.len() - 1;
        Ok(Self {
            anchor,
            ancestors,
            profile_index,
            project,
            root: current,
        })
    }
    fn extend(
        &mut self,
        original: &ProjectRoot,
        candidate: &DirectoryIdentity,
        create: bool,
        allow_missing: bool,
    ) -> Result<()> {
        let base = identity(&original.root)?;
        self.extend_caps(&[base,candidate.clone()],create,allow_missing,false)?;
        self.recheck_base(original)
    }
    fn extend_caps(&mut self,excluded:&[DirectoryIdentity],create:bool,allow_missing:bool,fresh:bool)->Result<()> {
        let mut current = self.root.try_clone()?;
        for (index, name) in [
            OsString::from("automation"),
            OsString::from("project-revisions"),
            OsString::from(self.project.to_string()),
        ]
        .into_iter()
        .enumerate()
        {
            let next = if fresh&&index==2 {
                // Never adopt an existing family for a newly issued project ID.
                // ALL entries count; stop at257, never scan unbounded junk.
                let mut count=0usize;
                for entry in current.entries()? {
                    let entry=entry?;count+=1;if count>256 {return Err(AutomationError::QueueFull);}
                    let name=entry.file_name();let text=name.to_str().ok_or_else(recovery)?;
                    let id=Uuid::parse_str(text).map_err(|_|recovery())?;
                    require(!id.is_nil()&&id.to_string()==text)?;
                }
                require(!exists(&current,name.to_str().ok_or_else(recovery)?)?)?;
                if count>=256 {return Err(AutomationError::QueueFull);}
                if !create {break;}
                mkdir(&current,name.to_str().ok_or_else(recovery)?)?
            } else {match open_child(&current, &name) {
                Ok(dir) => dir,
                Err(AutomationError::Io(error))
                    if create && error.kind() == std::io::ErrorKind::NotFound =>
                {
                    // A competing creator is not silently adopted: explicit
                    // retry must reopen/revalidate its private identity.
                    let name = name.to_str().ok_or_else(recovery)?;
                    mkdir(&current, name)?
                }
                Err(AutomationError::Io(error))
                    if allow_missing && error.kind() == std::io::ErrorKind::NotFound =>
                {
                    break;
                }
                Err(error) => return Err(error),
            }};
            outside_all(&next,excluded)?;
            // Existing automation/ is made by the current managed-project
            // creator and may be0755 under the private profile. Require owned
            // and not writable by others; NEW revision family stays0700.
            owned_permissions(&next, index != 0)?;
            #[cfg(windows)]
            if index != 0 {
                private_acl::verify(&next, true)?;
            }
            require(same_volume(&identity(&current)?, &identity(&next)?))?;
            self.ancestors.push(Pinned {
                name,
                identity: identity(&next)?,
                dir: next.try_clone()?,
            });
            current = next;
        }
        self.root = current;
        self.check_retained()
    }
    pub(super) fn existing_fresh_project(profile:&Path,project:Uuid)->Result<Self> {
        let mut namespace=Self::profile_pins(profile,project,&[])?;
        namespace.extend_caps(&[],false,false,false)?;
        require(namespace.ancestors.len()==namespace.profile_index+4)?;
        namespace.check()?;
        Ok(namespace)
    }
    pub(super) fn fresh_project(profile:&Path,project:Uuid,create:bool)->Result<Self> {
        let mut namespace=Self::profile_pins(profile,project,&[])?;
        namespace.extend_caps(&[],create,true,true)?;
        if create {require(namespace.ancestors.len()==namespace.profile_index+4)?;}
        namespace.check_retained()?;
        Ok(namespace)
    }
    pub(super) fn admit_base(
        profile: &Path,
        project: Uuid,
        original: &ProjectRoot,
    ) -> Result<Self> {
        let base = identity(&original.root)?;
        let mut namespace = Self::profile(profile, project, original, &base)?;
        namespace.extend(original, &base, false, true)?;
        Ok(namespace)
    }
    pub(super) fn recheck_base(&self, original: &ProjectRoot) -> Result<()> {
        self.check_retained()?;
        check_source(
            &self.ancestors[self.profile_index].dir,
            self.project,
            original,
        )
    }
    pub(super) fn check(&self) -> Result<()> {
        self.check_retained()?;
        private(&self.root)
    }
    fn check_retained(&self) -> Result<()> {
        self.check_retained_policy(false)
    }
    fn check_retained_policy(&self, seed: bool) -> Result<()> {
        let mut current = self.anchor.try_clone()?;
        for (index, pinned) in self.ancestors.iter().enumerate() {
            // Both current NAME and held handle must match. No resolve()/path
            // fallback after a rename/symlink/ancestor replacement.
            require(identity(&pinned.dir)? == pinned.identity)?;
            current = if index <= self.profile_index {
                open_ancestor(&current, &pinned.name)?
            } else {
                open_child(&current, &pinned.name)?
            };
            require(identity(&current)? == pinned.identity)?;
            if index >= self.profile_index {
                supported(&pinned.dir)?;
                supported(&current)?;
                let strict = index != self.profile_index + 1 && !(seed && index == self.profile_index + 2);
                owned_permissions(&pinned.dir, strict)?;
                owned_permissions(&current, strict)?;
                #[cfg(windows)]
                if index > self.profile_index + 1 && !(seed && index == self.profile_index + 2) {
                    private_acl::verify(&pinned.dir, true)?;
                    private_acl::verify(&current, true)?;
                }
            }
        }
        require(identity(&current)? == identity(&self.root)?)?;
        supported(&self.root)
    }
    pub(super) fn check_named_source(&self, original: &ProjectRoot) -> Result<()> {
        self.check()?;
        check_source(
            &self.ancestors[self.profile_index].dir,
            self.project,
            original,
        )
    }
}
fn same_volume(a: &DirectoryIdentity, b: &DirectoryIdentity) -> bool {
    match (a, b) {
        (DirectoryIdentity::Unix { device: a, .. }, DirectoryIdentity::Unix { device: b, .. }) => {
            a == b
        }
        (
            DirectoryIdentity::Windows { volume: a, .. },
            DirectoryIdentity::Windows { volume: b, .. },
        ) => a == b,
        _ => false,
    }
}
fn check_source(profile: &Dir, project: Uuid, original: &ProjectRoot) -> Result<()> {
    let automation = open_child(profile, "automation")?;
    let projects = open_child(&automation, "projects")?;
    let named = open_child(&projects, project.to_string())?;
    if identity(&named)? != identity(&original.root)? {
        return Err(AutomationError::FileChanged);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn full_sync_file(file: &cap_std::fs::File) -> Result<()> {
    macos::full_sync_file(file).map_err(boundary_error)
}
