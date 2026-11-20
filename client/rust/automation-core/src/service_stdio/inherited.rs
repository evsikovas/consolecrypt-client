//! Sole fixed inherited FD boundary. No caller-selected descriptor or listener.
#![allow(unsafe_code)]
use super::denied;
use crate::Result;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, Ordering};
static TAKEN: AtomicBool = AtomicBool::new(false);
pub(super) fn take() -> Result<UnixListener> {
    if std::env::var("LISTEN_PID").ok().as_deref() != Some(std::process::id().to_string().as_str())
        || std::env::var("LISTEN_FDS").ok().as_deref() != Some("1")
        || std::env::var("LISTEN_FDNAMES").ok().as_deref() != Some("control")
    {
        return Err(denied());
    }
    if TAKEN
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(denied());
    }
    // SAFETY: integer-only validity query, before ownership or typed use. The
    // once-only standalone entry executes before any helper/thread can close FD3.
    if unsafe { libc::fcntl(3, libc::F_GETFD) } < 0 {
        return Err(denied());
    }
    // SAFETY: CLI enters this exactly once, before any helper/thread is started.
    // The systemd contract transfers FD3 ownership. FromRawFd performs no deref;
    // all following syscalls validate the owned handle before typed conversion.
    let fd = unsafe { OwnedFd::from_raw_fd(3) };
    for (option, expected) in [(libc::SO_TYPE, libc::SOCK_STREAM), (libc::SO_ACCEPTCONN, 1)] {
        let mut value: libc::c_int = 0;
        let mut length = std::mem::size_of_val(&value) as libc::socklen_t;
        // SAFETY: valid owned FD, initialized aligned int and exact writable size.
        let rc = unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                (&mut value as *mut libc::c_int).cast(),
                &mut length,
            )
        };
        if rc != 0 || length as usize != std::mem::size_of_val(&value) || value != expected {
            return Err(denied());
        }
    }
    // SAFETY: fcntl uses the owned descriptor and integer flags only. The original
    // inherited FD itself becomes CLOEXEC; no duplicate leaks into the helper.
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(denied());
    }
    let listener = UnixListener::from(fd);
    listener.local_addr()?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}
