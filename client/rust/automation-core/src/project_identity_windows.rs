//! Read-only identity of existing owned directory handles. No path fallback,
//! file opening, async I/O, privileges or mutable filesystem operations.
//! GetFileInformationByHandleEx(FileIdInfo) provides128-bit IDs; older64-bit
//! metadata IDs are deliberately unsupported (not unique on ReFS).
use std::{fs::File,io,mem::size_of,os::windows::io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandleEx,FileIdInfo,FILE_ID_INFO};
#[derive(Debug,PartialEq,Eq)]
struct Identity { volume:u64, file:[u8;16] }
fn checked(info:FILE_ID_INFO)->io::Result<Identity> {
    let id=info.FileId.Identifier;
    if info.VolumeSerialNumber==0 || id==[0;16] || id==[0xff;16] {
        return Err(io::Error::new(io::ErrorKind::Unsupported,"unsupported project directory identity"));
    }
    Ok(Identity{volume:info.VolumeSerialNumber,file:id})
}
fn identity(file:&File)->io::Result<Identity> {
    // SAFETY: the SDK default initializes all integer/byte fields. Its exact
    // SDK size bounds one synchronous API output.
    // `file` borrows an owning File for the ENTIRE call; its handle cannot close
    // here. No OVERLAPPED/pending buffer lifetime, pointers or paths escape.
    let mut info=FILE_ID_INFO::default();
    let result=unsafe{GetFileInformationByHandleEx(file.as_raw_handle(),FileIdInfo,(&mut info as *mut FILE_ID_INFO).cast(),size_of::<FILE_ID_INFO>() as u32)};
    if result==0 {return Err(io::Error::last_os_error());}
    checked(info)
}
pub fn same(a:&File,b:&File)->io::Result<bool> {Ok(identity(a)?==identity(b)?)}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_and_sentinel_identifiers_are_not_identity_or_fallback() {
        assert_eq!(size_of::<FILE_ID_INFO>(),24);
        // SDK default, no uninitialized buffer or mock OS call.
        let mut info=FILE_ID_INFO::default();
        assert!(checked(info).is_err());
        info.VolumeSerialNumber=1;assert!(checked(info).is_err());
        info.FileId.Identifier=[0xff;16];assert!(checked(info).is_err());
        info.FileId.Identifier=[1;16];assert!(checked(info).is_ok());
    }
    #[test]
    fn actual_non_filesystem_handle_error_has_no_metadata_or_path_fallback() {
        // Read-only system null device, no user data/admin/config mutation.
        let file=File::open("NUL").unwrap();
        let error=identity(&file).unwrap_err();
        assert!(error.raw_os_error().is_some(),"real API error required; no synthetic identity fallback");
    }
}

/// Persist the SAME full-width identity used by same(); never a metadata fallback.
pub(crate) fn stable(file: &File) -> io::Result<(u64, [u8; 16])> {
    let id = identity(file)?;
    Ok((id.volume, id.file))
}
