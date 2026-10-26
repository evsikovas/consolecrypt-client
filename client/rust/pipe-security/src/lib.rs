//! # cc-pipe-security — owner-only Windows named pipes
//!
//! [`OwnerOnly`] builds `SECURITY_ATTRIBUTES` whose protected DACL grants
//! access to the current user's SID and to SYSTEM only (no inherited ACEs,
//! no Administrators, no other users), for
//! `tokio::net::windows::named_pipe::ServerOptions::create_with_security_attributes_raw`.
//! Security review SEC-13: the default named-pipe descriptor grants full
//! control to Administrators and the creator owner and read access to
//! Everyone and the anonymous account; the agent pipe serves vault keys.
//!
//! This crate is the one place in the workspace allowed to use `unsafe`
//! (the workspace lint denies it elsewhere): four Win32 calls, each with its
//! safety argument. On other platforms it is empty.
//!
//! Author: Alexander Evsikov <i@evsikov.net>

#![cfg_attr(windows, allow(unsafe_code))]

#[cfg(windows)]
pub use imp::{create_owner_only_pipe, OwnerOnly};

/// The SDDL of the descriptor for a user SID string: a protected DACL with
/// generic-all for that user and for LocalSystem only.
pub fn owner_only_sddl(user_sid: &str) -> String {
    format!("D:P(A;;GA;;;{user_sid})(A;;GA;;;SY)")
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::io;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// Create a pipe instance with `options`, readable and writable only by
    /// the user `attrs` was built for (and SYSTEM).
    pub fn create_owner_only_pipe(
        options: &ServerOptions,
        name: &str,
        attrs: &mut OwnerOnly,
    ) -> io::Result<NamedPipeServer> {
        // SAFETY: `attrs.as_ptr()` points to a SECURITY_ATTRIBUTES whose
        // descriptor `attrs` owns and keeps alive across the call.
        unsafe { options.create_with_security_attributes_raw(name, attrs.as_ptr()) }
    }

    /// `SECURITY_ATTRIBUTES` for an owner-only pipe. Keep it alive while
    /// pipe instances are created with [`OwnerOnly::as_ptr`].
    pub struct OwnerOnly {
        descriptor: PSECURITY_DESCRIPTOR,
        attributes: SECURITY_ATTRIBUTES,
    }

    impl std::fmt::Debug for OwnerOnly {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("OwnerOnly").finish_non_exhaustive()
        }
    }

    // SAFETY: the descriptor is heap memory from LocalAlloc owned by this
    // value and only read by CreateNamedPipeW; nothing ties it to a thread.
    unsafe impl Send for OwnerOnly {}
    // SAFETY: shared references never touch the descriptor.
    unsafe impl Sync for OwnerOnly {}

    impl OwnerOnly {
        /// Descriptor for the user this process runs as.
        pub fn current_user() -> io::Result<Self> {
            let sddl = super::owner_only_sddl(&current_user_sid()?);
            let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
            let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            // SAFETY: `wide` is NUL-terminated and outlives the call; on
            // success `descriptor` receives a LocalAlloc'd self-relative
            // descriptor, freed in Drop.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                descriptor,
                attributes: SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: descriptor,
                    bInheritHandle: 0,
                },
            })
        }

        /// The `SECURITY_ATTRIBUTES` pointer for one `CreateNamedPipeW`
        /// call; valid while `self` is alive and not moved.
        pub fn as_ptr(&mut self) -> *mut c_void {
            (&mut self.attributes as *mut SECURITY_ATTRIBUTES).cast()
        }
    }

    impl Drop for OwnerOnly {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptor… with
            // LocalAlloc and freed exactly once here.
            unsafe { LocalFree(self.descriptor) };
        }
    }

    /// The string SID (`S-1-5-21-…`) of the process token's user.
    fn current_user_sid() -> io::Result<String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the pseudo handle of the current process needs no close;
        // `token` receives a real handle that is closed below.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            // TOKEN_USER plus the variable-length SID that follows it.
            let mut buffer = vec![0u64; 64];
            let mut needed = 0u32;
            let size = (buffer.len() * 8) as u32;
            // SAFETY: `buffer` is 8-byte aligned (TOKEN_USER holds pointers)
            // and `size` bytes long; Windows writes at most that much.
            let ok = unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    size,
                    &mut needed,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: on success the buffer starts with a TOKEN_USER whose
            // SID pointer points into the same buffer.
            let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
            let mut text: *mut u16 = std::ptr::null_mut();
            // SAFETY: the SID is valid for the buffer's lifetime; `text`
            // receives a LocalAlloc'd NUL-terminated string freed below.
            if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `text` is NUL-terminated (see above).
            let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
            // SAFETY: `len` u16 values were just read from `text`.
            let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
            // SAFETY: freed exactly once.
            unsafe { LocalFree(text.cast()) };
            Ok(sid)
        })();
        // SAFETY: `token` is the handle opened above.
        unsafe { CloseHandle(token) };
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn descriptor_for_the_current_user_is_built() {
            let sid = current_user_sid().unwrap();
            assert!(sid.starts_with("S-1-"), "{sid}");
            let mut attrs = OwnerOnly::current_user().unwrap();
            assert!(!attrs.as_ptr().is_null());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sddl_grants_only_the_user_and_system() {
        assert_eq!(
            super::owner_only_sddl("S-1-5-21-1-2-3-1001"),
            "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)(A;;GA;;;SY)"
        );
    }
}
