use super::*;
use crate::project::service_identity_boundary as boundary;
use cap_std::fs::{Dir, Metadata, MetadataExt, PermissionsExt};
use std::{io::Write, path::Path};

const KEY: &str = "identity.v1";
const ENROLLMENT: &str = "enrollment.v1";
const RECORD_MAGIC:&[u8;8]=b"CCSE\0\0\0\x01";
const RECORD_OVERHEAD:usize=8+4+32;
fn completion(raw:&[u8])->[u8;32] {
    let mut hash=Sha256::new();hash.update(b"ConsoleCrypt Original Service Enrollment Commit v1\0");
    hash.update(RECORD_MAGIC);hash.update((raw.len() as u32).to_le_bytes());hash.update(raw);hash.finalize().into()
}
fn write_record(file:&mut cap_std::fs::File,raw:&[u8],root:&Dir,sync_dir:&mut impl FnMut(&Dir)->Result<()>)->Result<()> {
    file.write_all(RECORD_MAGIC)?;file.write_all(&(raw.len() as u32).to_le_bytes())?;file.write_all(raw)?;
    file.sync_all()?;sync_dir(root)?; // durable body alone is INCOMPLETE.
    file.write_all(&completion(raw))?;file.sync_all()?;sync_dir(root)?;
    Ok(()) // Only this path may acknowledge local file+directory sync success.
}
struct PinnedEnrollment {
    file: cap_std::fs::File,
    digest: [u8; 32],
}

pub(super) struct Guard {
    parent: Dir,
    name: String,
    root: Dir,
    file: cap_std::fs::File,
    digest: [u8; 32],
    enrollment: Option<PinnedEnrollment>,
}
fn private_dir(dir: &Dir) -> Result<()> {
    let m = dir.dir_metadata()?;
    if !m.is_dir()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.permissions().mode() & 0o7777 != 0o700
        || rustix::process::geteuid().is_root()
    {
        return Err(denied());
    }
    boundary::check(dir)?;
    // Existing publisher boundary also permits TMPFS for ephemeral work. An
    // identity reported persisted must specifically refuse that volatile case.
    if rustix::fs::fstatfs(dir)
        .map_err(std::io::Error::from)?
        .f_type as i64
        != libc::EXT4_SUPER_MAGIC as i64
    {
        return Err(denied());
    }
    Ok(())
}
fn private_file(m: &Metadata) -> Result<()> {
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.nlink() != 1
        || m.permissions().mode() & 0o7777 != 0o600
        || m.len() != BYTES as u64
    {
        return Err(denied());
    }
    Ok(())
}
fn same(a: &Metadata, b: &Metadata) -> Result<()> {
    if a.ino() == 0 || a.ino() != b.ino() || a.dev() != b.dev() {
        return Err(denied());
    }
    Ok(())
}
fn inventory(root: &Dir) -> Result<bool> {
    let mut key = false;
    let mut enrollment = false;
    let mut count = 0;
    for entry in root.read_dir(".")? {
        count += 1;
        if count > 2 {
            return Err(denied());
        }
        let name = entry?.file_name();
        if name == KEY && !key {
            key = true;
        } else if name == ENROLLMENT && !enrollment {
            enrollment = true;
        } else {
            return Err(denied());
        }
    }
    if !key {
        return Err(denied());
    }
    Ok(enrollment)
}
fn enrollment_bytes(file: &cap_std::fs::File) -> Result<Zeroizing<Vec<u8>>> {
    let before = file.metadata()?;
    if !before.is_file()
        || before.uid() != rustix::process::geteuid().as_raw()
        || before.nlink() != 1
        || before.permissions().mode() & 0o7777 != 0o600
        || before.len() == 0
        || before.len() > (crate::service_restart::MAX_RECORD+RECORD_OVERHEAD) as u64
    {
        return Err(denied());
    }
    let mut raw = Zeroizing::new(vec![0; before.len() as usize]);
    let mut used = 0;
    while used < raw.len() {
        let n =
            rustix::io::pread(file, &mut raw[used..], used as u64).map_err(std::io::Error::from)?;
        if n == 0 {
            return Err(denied());
        }
        used += n;
    }
    let mut extra = [0];
    if rustix::io::pread(file, &mut extra, used as u64).map_err(std::io::Error::from)? != 0 {
        return Err(denied());
    }
    let after = file.metadata()?;
    same(&before, &after)?;
    if !after.is_file() || after.uid()!=rustix::process::geteuid().as_raw() || after.nlink()!=1
        || after.permissions().mode()&0o7777!=0o600
        || before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(denied());
    }
    if raw.len()<RECORD_OVERHEAD || &raw[..8]!=RECORD_MAGIC {return Err(denied());}
    let count=u32::from_le_bytes(raw[8..12].try_into().map_err(|_|denied())?) as usize;
    if count>crate::service_restart::MAX_RECORD || raw.len()!=count+RECORD_OVERHEAD
        || completion(&raw[12..12+count]).as_slice()!=&raw[12+count..] {return Err(denied());}
    Ok(Zeroizing::new(raw[12..12+count].to_vec()))
}
fn read(file: &cap_std::fs::File) -> Result<Zeroizing<Vec<u8>>> {
    private_file(&file.metadata()?)?;
    let mut bytes = Zeroizing::new(vec![0; BYTES]);
    let mut used = 0;
    while used < BYTES {
        let count = rustix::io::pread(file, &mut bytes[used..], used as u64)
            .map_err(std::io::Error::from)?;
        if count == 0 {
            return Err(denied());
        }
        used += count;
    }
    let mut extra = [0u8; 1];
    if rustix::io::pread(file, &mut extra, BYTES as u64).map_err(std::io::Error::from)? != 0 {
        return Err(denied());
    }
    private_file(&file.metadata()?)?;
    Ok(bytes)
}
fn sync(dir: &Dir) -> Result<()> {
    // cap_std's originally supplied directory can legitimately be O_PATH.
    // Reopen ONLY the held descriptor's fixed dot entry, never an ambient name
    // or weaker fallback; prove the readable descriptor is the same directory.
    let fd = rustix::fs::openat2(
        dir,
        ".",
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::BENEATH
            | rustix::fs::ResolveFlags::NO_SYMLINKS
            | rustix::fs::ResolveFlags::NO_MAGICLINKS
            | rustix::fs::ResolveFlags::NO_XDEV,
    )
    .map_err(std::io::Error::from)?;
    let readable = Dir::from_std_file(std::fs::File::from(fd));
    same(&dir.dir_metadata()?, &readable.dir_metadata()?)?;
    rustix::fs::fsync(&readable).map_err(std::io::Error::from)?;
    same(&dir.dir_metadata()?, &readable.dir_metadata()?)?;
    Ok(())
}

pub(super) fn create(parent: Dir, id: Uuid) -> Result<ServiceIdentity> {
    if id.is_nil() {
        return Err(denied());
    }
    private_dir(&parent)?;
    let name = format!("service-identity-{id}");
    // Explicit create only. Existing empty/partial/foreign state is never
    // adopted, replaced, automatically deleted or used to regenerate keys.
    rustix::fs::mkdirat(&parent, name.as_str(), rustix::fs::Mode::RWXU)
        .map_err(std::io::Error::from)?;
    let finish = || -> Result<ServiceIdentity> {
        let root = boundary::child(&parent, &name)?;
        private_dir(&root)?;
        let keys = DeviceSecretKeys::generate().map_err(|_| denied())?;
        let bytes = encode(id, &keys)?;
        let mut file = boundary::create(&root, Path::new(KEY), 0o600)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        private_file(&file.metadata()?)?;
        let created = file.metadata()?;
        sync(&root)?;
        sync(&parent)?;
        drop(file);
        drop(keys);
        drop(bytes);
        let result = open(parent, id)?;
        same(&created, &result.guard.file.metadata()?)?;
        Ok(result)
    };
    finish().map_err(|_| {
        AutomationError::Invalid("service identity creation requires explicit recovery")
    })
}
pub(super) fn open(parent: Dir, id: Uuid) -> Result<ServiceIdentity> {
    if id.is_nil() {
        return Err(denied());
    }
    private_dir(&parent)?;
    let name = format!("service-identity-{id}");
    let root = boundary::child(&parent, &name)?;
    private_dir(&root)?;
    let has_enrollment = inventory(&root)?;
    let file = boundary::read(&root, Path::new(KEY))?;
    private_file(&file.metadata()?)?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(std::io::Error::from)?;
    let bytes = read(&file)?;
    let keys = decode(&bytes, id)?;
    let facts = ServiceIdentityFacts {
        identity_id: id,
        key_epoch: 1,
        public_keys: keys.public_keys(),
    };
    let enrollment = if has_enrollment {
        let file = boundary::read(&root, Path::new(ENROLLMENT))?;
        let raw = enrollment_bytes(&file)?;
        crate::service_restart::EnrollmentRecord::parse(&raw, &facts)?;
        Some(PinnedEnrollment {
            file,
            digest: checksum(&raw),
        })
    } else {
        None
    };
    let guard = Guard {
        parent,
        name,
        root,
        file,
        digest: checksum(&bytes),
        enrollment,
    };
    guard.check(&facts)?;
    Ok(ServiceIdentity {
        keys: Some(keys),
        facts,
        guard,
    })
}
impl Guard {
    pub(super) fn check(&self, facts: &ServiceIdentityFacts) -> Result<()> {
        private_dir(&self.parent)?;
        private_dir(&self.root)?;
        if inventory(&self.root)? != self.enrollment.is_some() {
            return Err(denied());
        }
        let named = boundary::child(&self.parent, &self.name)?;
        private_dir(&named)?;
        same(&named.dir_metadata()?, &self.root.dir_metadata()?)?;
        let named_file = boundary::read(&self.root, Path::new(KEY))?;
        private_file(&named_file.metadata()?)?;
        same(&named_file.metadata()?, &self.file.metadata()?)?;
        let bytes = read(&self.file)?;
        if checksum(&bytes) != self.digest
            || decode(&bytes, facts.identity_id)?.public_keys() != facts.public_keys
            || facts.key_epoch != 1
        {
            return Err(denied());
        }
        if let Some(record) = &self.enrollment {
            let current = boundary::read(&self.root, Path::new(ENROLLMENT))?;
            same(&current.metadata()?, &record.file.metadata()?)?;
            let raw = enrollment_bytes(&record.file)?;
            if checksum(&raw) != record.digest {
                return Err(denied());
            }
            crate::service_restart::EnrollmentRecord::parse(&raw, facts)?;
            let after = boundary::read(&self.root, Path::new(ENROLLMENT))?;
            same(&after.metadata()?, &record.file.metadata()?)?;
        }
        // Repeat the held-vs-current directory/file boundary after the read.
        let named = boundary::child(&self.parent, &self.name)?;
        same(&named.dir_metadata()?, &self.root.dir_metadata()?)?;
        let named_file = boundary::read(&self.root, Path::new(KEY))?;
        private_file(&named_file.metadata()?)?;
        same(&named_file.metadata()?, &self.file.metadata()?)?;
        Ok(())
    }
}

impl Guard {
    pub(super) fn enrollment(
        &self,
        facts: &ServiceIdentityFacts,
    ) -> Result<Option<crate::service_restart::EnrollmentRecord>> {
        self.check(facts)?;
        match &self.enrollment {
            None => Ok(None),
            Some(held) => {
                let raw = enrollment_bytes(&held.file)?;
                let record = crate::service_restart::EnrollmentRecord::parse(&raw, facts)?;
                self.check(facts)?;
                Ok(Some(record))
            }
        }
    }
    pub(super) fn store_enrollment(
        &mut self,
        raw: &[u8],
        facts: &ServiceIdentityFacts,
    ) -> Result<()> {
        self.store_with_sync(raw,facts,&mut sync)
    }
    fn store_with_sync(&mut self,raw:&[u8],facts:&ServiceIdentityFacts,sync_dir:&mut impl FnMut(&Dir)->Result<()>)->Result<()> {
        self.check(facts)?;
        if self.enrollment.is_some() {
            return Err(denied());
        }
        crate::service_restart::EnrollmentRecord::parse(raw, facts)?;
        // The name is fixed, original held parent/identity is retained, and a
        // failed/partial create is preserved; it makes later checks fail closed.
        let mut file = boundary::create(&self.root, Path::new(ENROLLMENT), 0o600)?;
        let finish = || -> Result<PinnedEnrollment> {
            write_record(&mut file,raw,&self.root,sync_dir)?;
            let created=file.metadata()?;
            drop(file); // Existing create kernel intentionally returns O_WRONLY.
            let file=boundary::read(&self.root,Path::new(ENROLLMENT))?;
            same(&created,&file.metadata()?)?;
            let readback=enrollment_bytes(&file)?;
            if readback.as_slice()!=raw {return Err(denied());}
            Ok(PinnedEnrollment {file,digest:checksum(raw)})
        };
        self.enrollment = Some(finish().map_err(|_| {
            AutomationError::Invalid("service enrollment requires explicit recovery")
        })?);
        self.check(facts)
    }
}
#[cfg(test)]
pub(super) fn faulted_store(identity:&mut ServiceIdentity,raw:&[u8],failed_sync:usize)->Result<()> {
    let mut phase=0;
    identity.guard.store_with_sync(raw,&identity.facts,&mut |dir| {
        phase+=1;
        if phase==failed_sync {return Err(AutomationError::Io(std::io::Error::other("owned injected directory sync failure")));}
        sync(dir)
    })
}
pub(super) fn runtime_dir(path: &Path) -> Result<Dir> {
    use std::os::unix::ffi::OsStrExt;
    let raw = path.as_os_str().as_bytes();
    if raw.len() < 2
        || raw.len() > 4096
        || raw[0] != b'/'
        || raw[1..]
            .split(|b| *b == b'/')
            .any(|v| v.is_empty() || v == b"." || v == b"..")
    {
        return Err(denied());
    }
    let fd = rustix::fs::openat2(
        rustix::fs::CWD,
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::NO_SYMLINKS | rustix::fs::ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(std::io::Error::from)?;
    let root = Dir::from_std_file(std::fs::File::from(fd));
    private_dir(&root)?;
    Ok(root)
}
