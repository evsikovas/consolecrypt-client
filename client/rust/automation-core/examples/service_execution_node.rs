//! Private process entry. Persistent identity is explicit and never enrollment.
use cc_automation_core::{
    HelperLaunch, ServiceIdentity, service_node_stdio, service_node_stdio_with_identity,
    service_node_unix_with_identity,
};
use std::ffi::OsString;

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 && args.len() != 6 && args.len() != 7 {
        std::process::exit(64);
    }
    let launch = HelperLaunch::Native {
        python: args[0].clone().into(),
        module_dir: args[1].clone().into(),
        root: args[2].clone().into(),
    };
    let result = if args.len() == 3 {
        service_node_stdio(launch)
    } else if args.len() == 7 {
        if args[3] != "--identity-existing" || args[6] != "--private-ipc" {
            std::process::exit(64);
        }
        identity(&args[3..6]).and_then(|identity| service_node_unix_with_identity(launch, identity))
    } else {
        identity(&args[3..]).and_then(|identity| service_node_stdio_with_identity(launch, identity))
    };
    if result.is_err() {
        // Fixed exit only: never emit key material, paths, or low-level errors.
        std::process::exit(65);
    }
}

fn unavailable() -> cc_automation_core::AutomationError {
    cc_automation_core::AutomationError::Invalid("service identity unavailable")
}

#[cfg(target_os = "linux")]
fn identity(args: &[OsString]) -> cc_automation_core::Result<ServiceIdentity> {
    use std::os::unix::ffi::OsStrExt;
    let mode = args[0].to_str().ok_or_else(unavailable)?;
    if mode != "--identity-create" && mode != "--identity-existing" {
        return Err(unavailable());
    }
    let raw = args[1].as_os_str().as_bytes();
    // No symlink/lexical alias or ambient fallback, including intermediate names.
    if raw.len() < 2
        || raw.len() > 4096
        || raw[0] != b'/'
        || raw[1..]
            .split(|b| *b == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b"..")
    {
        return Err(unavailable());
    }
    let text = args[2].to_str().ok_or_else(unavailable)?;
    let id = uuid::Uuid::parse_str(text).map_err(|_| unavailable())?;
    if id.is_nil() || id.to_string() != text {
        return Err(unavailable());
    }
    let fd = rustix::fs::openat2(
        rustix::fs::CWD,
        std::path::Path::new(&args[1]),
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::NO_SYMLINKS | rustix::fs::ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|_| unavailable())?;
    let parent = cap_std::fs::Dir::from_std_file(std::fs::File::from(fd));
    // Existing holder enforces actual EXT, UID, private mode, named inode and
    // exclusive lock. Loading never regenerates/adopts corrupt or missing state.
    if mode == "--identity-create" {
        ServiceIdentity::create(parent, id)
    } else {
        ServiceIdentity::open_existing(parent, id)
    }
}

#[cfg(not(target_os = "linux"))]
fn identity(_: &[OsString]) -> cc_automation_core::Result<ServiceIdentity> {
    Err(unavailable())
}
