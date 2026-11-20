//! The only new unsafe boundary: synchronous bounded metadata queries and one
//! create-only handle-relative directory syscall. No ambient paths/ACL repair.
#![deny(unsafe_op_in_unsafe_fn)]
use super::*;
use cap_std::fs::Dir;
use std::{
    fs::File,
    mem::size_of,
    os::windows::{
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT,
            FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{
            HANDLE, INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
            RtlNtStatusToDosError, UNICODE_STRING,
        },
        Security::{
            GetKernelObjectSecurity, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::{
            FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_TRAVERSE, READ_CONTROL, SYNCHRONIZE,
        },
        System::{
            IO::IO_STATUS_BLOCK,
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    },
};

struct Aligned {
    words: Vec<usize>,
    len: usize,
}
impl Aligned {
    fn new(len: usize) -> io::Result<Self> {
        need((1..=MAX_DESCRIPTOR).contains(&len))?;
        Ok(Self {
            words: vec![0; len.div_ceil(size_of::<usize>())],
            len,
        })
    }
    fn pointer(&mut self) -> *mut core::ffi::c_void {
        self.words.as_mut_ptr().cast()
    }
    fn bytes(&self) -> &[u8] {
        // SAFETY: fully initialized usize storage; byte length never exceeds it.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast(), self.len) }
    }
    fn copy(raw: &[u8]) -> io::Result<Self> {
        let mut out = Self::new(raw.len())?;
        // SAFETY: disjoint initialized source and correctly sized writable output.
        unsafe {
            ptr::copy_nonoverlapping(raw.as_ptr(), out.pointer().cast(), raw.len());
        }
        Ok(out)
    }
}
fn user() -> io::Result<Vec<u8>> {
    let mut handle = ptr::null_mut();
    // SAFETY: pseudo process handle is borrowed, output is one initialized HANDLE.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    need(!handle.is_null() && handle != INVALID_HANDLE_VALUE)?;
    // SAFETY: successful OpenProcessToken transferred a unique owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut data = Aligned::new(4096)?;
    let mut actual = 0;
    // SAFETY: correctly aligned initialized bounded output and live token handle.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            data.pointer(),
            data.len as u32,
            &mut actual,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    need(actual as usize >= size_of::<TOKEN_USER>() && actual as usize <= data.len)?;
    // SAFETY: successful API returned at least one aligned TOKEN_USER. Its SID
    // POINTER is only converted to an offset, never dereferenced before bounds.
    let info = unsafe { &*data.words.as_ptr().cast::<TOKEN_USER>() };
    let base = data.words.as_ptr() as usize;
    let offset = (info.User.Sid as usize)
        .checked_sub(base)
        .ok_or_else(denied)?;
    need(offset >= size_of::<TOKEN_USER>())?;
    Ok(sid_at(&data.bytes()[..actual as usize], offset)?.to_vec())
}
fn security(file: &File) -> io::Result<Aligned> {
    let mut length = 0;
    // SAFETY: size query with no output buffer and one writable DWORD.
    let result = unsafe {
        GetKernelObjectSecurity(file.as_raw_handle(), 5, ptr::null_mut(), 0, &mut length)
    };
    need(result == 0 && io::Error::last_os_error().raw_os_error() == Some(122))?;
    need((20..=MAX_DESCRIPTOR).contains(&(length as usize)))?;
    let mut data = Aligned::new(length as usize)?;
    let mut actual = 0;
    // SAFETY: exactly bounded caller-owned aligned output; original file stays live.
    if unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle(),
            5,
            data.pointer(),
            data.len as u32,
            &mut actual,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    need(actual as usize >= 20 && actual as usize <= data.len)?;
    data.len = actual as usize;
    Ok(data)
}
fn file_identity(file: &File) -> io::Result<(u64, [u8; 16])> {
    let meta = file.metadata()?;
    need(meta.is_dir() && meta.file_attributes() & 0x400 == 0)?;
    crate::project_identity_windows::stable(file)
}
fn verify_file(file: &File, owner: &[u8], protected: bool) -> io::Result<()> {
    let before = file_identity(file)?;
    let sd = security(file)?;
    descriptor(sd.bytes(), owner, protected)?;
    need(file_identity(file)? == before)
}
pub(in crate::project::publication) fn verify(dir: &Dir, protected: bool) -> io::Result<()> {
    super::super::windows::acl_io(|| {
        let file = dir.try_clone()?.into_std_file();
        verify_file(&file, &user()?, protected)
    })
}
struct CreateRequest {
    _parent: File,
    _descriptor: Aligned,
    _wide: Vec<u16>,
    _name: Box<UNICODE_STRING>,
    attributes: Box<OBJECT_ATTRIBUTES>,
    iosb: Box<IO_STATUS_BLOCK>,
    output: Box<HANDLE>,
}
/// Returned Dir owns the exact created handle; no drop/reopen adoption gap.
pub(in crate::project::publication) fn create(parent: &Dir, name: &str) -> io::Result<Dir> {
    super::super::windows::acl_io(|| {
        super::super::windows::valid_name(name)?;
        let owner = user()?;
        let parent = parent.try_clone()?.into_std_file();
        verify_file(&parent, &owner, false)?;
        let parent_id = file_identity(&parent)?;
        let mut descriptor = Aligned::copy(&new_descriptor(&owner)?)?;
        let mut wide = name.encode_utf16().collect::<Vec<_>>();
        let mut unicode = Box::new(UNICODE_STRING {
            Length: (wide.len() * 2) as u16,
            MaximumLength: (wide.len() * 2) as u16,
            Buffer: wide.as_mut_ptr(),
        });
        let attributes = Box::new(OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &mut *unicode,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: descriptor.pointer().cast(),
            SecurityQualityOfService: ptr::null_mut(),
        });
        let mut request = CreateRequest {
            _parent: parent,
            _descriptor: descriptor,
            _wide: wide,
            _name: unicode,
            attributes,
            iosb: Box::default(),
            output: Box::new(ptr::null_mut()),
        };
        // SAFETY: every kernel-visible input/output is initialized and heap-held
        // through return. Single-component relative name, FILE_CREATE only;
        // protected descriptor is applied AS the new directory is created.
        let status = unsafe {
            NtCreateFile(
                &mut *request.output,
                READ_CONTROL
                    | SYNCHRONIZE
                    | FILE_LIST_DIRECTORY
                    | FILE_TRAVERSE
                    | FILE_READ_ATTRIBUTES,
                &*request.attributes,
                &mut *request.iosb,
                ptr::null(),
                0x10,
                3,
                FILE_CREATE,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                ptr::null(),
                0,
            )
        };
        if status == 0x103 {
            // Synchronous create unexpectedly unconfirmed: do NOT read a possibly
            // still-kernel-written handle/output or free ANY referenced input.
            // Shared existing publication health is poisoned BEFORE retention.
            super::super::windows::acl_indeterminate();
            std::mem::forget(request);
            return Err(denied());
        }
        if status < 0 {
            // SAFETY: NTSTATUS conversion is synchronous and touches no pointers.
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(status) } as i32,
            ));
        }
        need(!(*request.output).is_null() && *request.output != INVALID_HANDLE_VALUE)?;
        // SAFETY: completed successful FILE_CREATE transferred exactly one handle.
        let created = unsafe { File::from_raw_handle(*request.output) };
        need(status == 0 && request.iosb.Information == 2)?;
        verify_file(&created, &owner, true)?;
        need(file_identity(&created)?.0 == parent_id.0)?;
        verify_file(&request._parent, &owner, false)?;
        Ok(Dir::from_std_file(created))
    })
}

#[cfg(test)]
pub(super) fn fixture_user() -> Vec<u8> {
    user().unwrap()
}
#[cfg(test)]
pub(super) fn fixture_security(file: &File) -> Vec<u8> {
    security(file).unwrap().bytes().to_vec()
}
#[cfg(test)]
pub(super) fn fixture_set(path: &std::path::Path, raw: &[u8]) {
    use std::os::windows::ffi::OsStrExt;
    let temp = std::env::temp_dir().canonicalize().unwrap();
    let actual = path.canonicalize().unwrap();
    let relative = actual
        .strip_prefix(&temp)
        .expect("only owned temporary fixture paths");
    let top = relative
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_str()
        .unwrap();
    assert!(top.starts_with("owned-private-acl-") || top.starts_with("owned-publication-"));
    let metadata = std::fs::symlink_metadata(path).unwrap();
    assert!(metadata.is_dir() && metadata.file_attributes() & 0x400 == 0);
    assert!(!actual.as_os_str().encode_wide().any(|c| c == 0));
    let wide = actual
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut sd = Aligned::copy(raw).unwrap();
    // SAFETY: TEST-ONLY exact newly owned temp path, bounded owned descriptor,
    // synchronous call. Sets current-user owner +protected DACL; no privileges,
    // real profile, production writer or global folder permissions are changed.
    let result = unsafe {
        windows_sys::Win32::Security::SetFileSecurityW(wide.as_ptr(), 0x8000_0005, sd.pointer())
    };
    assert_ne!(
        result,
        0,
        "owned fixture descriptor setup failed: {}",
        io::Error::last_os_error()
    );
}
