//! Android app storage adapter. The app syscall filter permits openat and
//! statx, so check the mount of each retained descriptor during traversal.
use cap_std::fs::Dir;
use rustix::fd::AsFd;
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, StatxFlags};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read},
    path::{Component, Path},
};

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "Android local mount identity unavailable",
    )
}

fn mount_id(file: impl AsFd) -> io::Result<u64> {
    let file = file.as_fd();
    let stat = fs::statx(file, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID).ok();
    if let Some(info) =
        stat.filter(|info| StatxFlags::from_bits_retain(info.stx_mask).contains(StatxFlags::MNT_ID))
    {
        return Ok(info.stx_mnt_id);
    }
    // Older Android kernels publish the same mount ID in verified procfs.
    let fdinfo = rustix_linux_procfs::proc_self_fdinfo_fd(file).map_err(|_| unsupported())?;
    let mut details = String::new();
    File::from(fdinfo).take(4096).read_to_string(&mut details)?;
    details
        .lines()
        .find_map(|line| line.strip_prefix("mnt_id:")?.trim().parse().ok())
        .ok_or_else(unsupported)
}

fn same_mount(parent: &Dir, file: impl AsFd) -> io::Result<()> {
    if mount_id(parent)? != mount_id(file)? {
        return Err(rustix::io::Errno::XDEV.into());
    }
    Ok(())
}

fn single_name(path: &Path) -> io::Result<&OsStr> {
    let mut parts = path.components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(name)), None) => Ok(name),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "one relative name required",
        )),
    }
}

pub(super) fn open_ancestor(parent: &Dir, name: &Path) -> io::Result<Dir> {
    let name = single_name(name)?;
    let flags = OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let fd = match fs::openat(parent, name, flags | OFlags::RDONLY, Mode::empty()) {
        // A system ancestor may grant search without directory listing.
        Err(rustix::io::Errno::ACCESS) => {
            fs::openat(parent, name, flags | OFlags::PATH, Mode::empty())?
        }
        result => result?,
    };
    Ok(Dir::from_std_file(File::from(fd)))
}

pub(super) fn open_child(parent: &Dir, name: &Path) -> io::Result<Dir> {
    let fd = fs::openat(
        parent,
        single_name(name)?,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    same_mount(parent, &fd)?;
    Ok(Dir::from_std_file(File::from(fd)))
}

pub(super) fn supported(dir: &Dir) -> io::Result<()> {
    let kind = fs::fstatfs(dir)?.f_type as i64;
    if kind != libc::EXT4_SUPER_MAGIC as i64 && kind != libc::F2FS_SUPER_MAGIC as i64 {
        return Err(unsupported());
    }
    mount_id(dir).map(|_| ())
}

fn open_file(root: &Dir, path: &Path, flags: OFlags, mode: Mode) -> io::Result<cap_std::fs::File> {
    let mut parts = path.components();
    let Some(Component::Normal(leaf)) = parts.next_back() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative file path required",
        ));
    };
    let mut parent = root.try_clone()?;
    for part in parts {
        parent = open_child(&parent, Path::new(part.as_os_str()))?;
    }
    let fd = fs::openat(
        &parent,
        leaf,
        flags | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        mode,
    )?;
    same_mount(&parent, &fd)?;
    if FileType::from_raw_mode(fs::fstat(&fd)?.st_mode) == FileType::Symlink {
        return Err(rustix::io::Errno::LOOP.into());
    }
    Ok(cap_std::fs::File::from_std(File::from(fd)))
}

pub(super) fn read_file(root: &Dir, path: &Path) -> io::Result<cap_std::fs::File> {
    open_file(root, path, OFlags::RDONLY | OFlags::NONBLOCK, Mode::empty())
}

pub(super) fn create_file(root: &Dir, path: &Path, mode: u32) -> io::Result<cap_std::fs::File> {
    if mode & !0o777 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid file mode",
        ));
    }
    open_file(
        root,
        path,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
        Mode::from_bits_truncate(mode),
    )
}

pub(super) fn metadata(root: &Dir, path: &Path) -> io::Result<cap_std::fs::Metadata> {
    open_file(root, path, OFlags::PATH, Mode::empty())?.metadata()
}
