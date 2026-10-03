//! RDPDR selected-directory backend. All remote paths are descriptor-relative,
//! every component is no-follow, and no ambient filesystem operation uses a remote path.
use crate::permissions::SharedRedirect;
#[cfg(unix)]
use cap_fs_ext::OpenOptionsSyncExt;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions, ReadDir};
use ironrdp::{
    core::impl_as_any,
    pdu::PduResult,
    rdpdr::{
        pdu::{
            efs::*,
            esc::{ScardCall, ScardIoCtlCode},
            RdpdrPdu,
        },
        RdpdrBackend,
    },
    svc::SvcMessage,
};
use std::{
    collections::HashMap,
    io::{self, Read, Seek, SeekFrom, Write},
    time::UNIX_EPOCH,
};
use zeroize::Zeroize;

pub(crate) const DRIVE_ID: u32 = 1;
const MAX_HANDLES: usize = 128;
const MAX_IO: usize = 64 * 1024;
const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;
const MAX_ENUMERATED: usize = 10_000;
const MAX_SESSION_WRITTEN: u64 = 512 * 1024 * 1024;
const MAX_CREATED: usize = 4096;

fn denied() -> io::Error {
    io::Error::from(io::ErrorKind::PermissionDenied)
}
fn relative(path: &str) -> io::Result<Vec<String>> {
    if path.len() > 4096 || path.starts_with("\\\\") || path.starts_with("//") {
        return Err(denied());
    }
    let normalized = path.replace('\\', "/");
    let normalized = normalized.strip_prefix('/').unwrap_or(&normalized);
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    let parts: Vec<_> = normalized.split('/').map(str::to_owned).collect();
    if parts.len() > 64 || parts.iter().any(|s| !safe_name(s)) {
        return Err(denied());
    }
    Ok(parts)
}
fn safe_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 255
        || matches!(name, "." | "..")
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c < ' ' || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}
fn parent(root: &Dir, parts: &[String]) -> io::Result<(Dir, String)> {
    let (leaf, ancestors) = parts.split_last().ok_or_else(denied)?;
    let mut dir = root.try_clone()?;
    for component in ancestors {
        dir = dir.open_dir_nofollow(component)?;
        check_dir(&dir)?;
    }
    Ok((dir, leaf.clone()))
}
fn check_dir(dir: &Dir) -> io::Result<()> {
    let m = dir.dir_metadata()?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(denied());
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(denied());
        }
    }
    Ok(())
}
fn open_dir(root: &Dir, parts: &[String]) -> io::Result<Dir> {
    let mut dir = root.try_clone()?;
    for component in parts {
        dir = dir.open_dir_nofollow(component)?;
        check_dir(&dir)?;
    }
    Ok(dir)
}
fn safe_file(file: &File) -> io::Result<Metadata> {
    let m = file.metadata()?;
    if !m.is_file() || m.file_type().is_symlink() || m.nlink() != 1 {
        return Err(denied());
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(denied());
        }
    }
    Ok(m)
}
fn status(e: io::Error) -> NtStatus {
    match e.kind() {
        io::ErrorKind::PermissionDenied => NtStatus::ACCESS_DENIED,
        io::ErrorKind::NotFound => NtStatus::NO_SUCH_FILE,
        io::ErrorKind::AlreadyExists => NtStatus::OBJECT_NAME_COLLISION,
        _ => NtStatus::UNSUCCESSFUL,
    }
}
fn time(value: io::Result<cap_std::time::SystemTime>) -> i64 {
    let duration = value
        .map(|t| t.into_std())
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(duration.as_nanos() / 100)
        .unwrap_or(i64::MAX)
        .saturating_add(116_444_736_000_000_000)
}
fn attributes(m: &Metadata, writable: bool) -> FileAttributes {
    let mut flags = if m.is_dir() {
        FileAttributes::FILE_ATTRIBUTE_DIRECTORY
    } else {
        FileAttributes::FILE_ATTRIBUTE_ARCHIVE
    };
    if !writable || m.permissions().readonly() {
        flags |= FileAttributes::FILE_ATTRIBUTE_READONLY;
    }
    flags
}

enum HandleData {
    File(File),
    Directory(Dir),
}
struct Handle {
    data: HandleData,
    parts: Vec<String>,
    writable: bool,
    enumeration: Option<ReadDir>,
    pattern: String,
    seen: usize,
    delete_pending: bool,
}
impl Handle {
    fn metadata(&self) -> io::Result<Metadata> {
        match &self.data {
            HandleData::File(f) => f.metadata(),
            HandleData::Directory(d) => d.dir_metadata(),
        }
    }
}

pub(crate) struct DirectoryBackend {
    state: SharedRedirect,
    generation: u64,
    next_id: u32,
    handles: HashMap<u32, Handle>,
    written: u64,
    created: usize,
}
impl std::fmt::Debug for DirectoryBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DirectoryBackend(<redacted>)")
    }
}
impl_as_any!(DirectoryBackend);
impl DirectoryBackend {
    pub fn new(state: SharedRedirect) -> Self {
        Self {
            state,
            generation: 0,
            next_id: 1,
            handles: HashMap::new(),
            written: 0,
            created: 0,
        }
    }
    fn create(
        &mut self,
        root: &Dir,
        writable: bool,
        req: &DeviceCreateRequest,
    ) -> io::Result<(u32, Information)> {
        if self.handles.len() >= MAX_HANDLES || req.allocation_size > MAX_FILE_SIZE {
            return Err(denied());
        }
        let parts = relative(&req.path)?;
        let write_access = req.desired_access.intersects(
            DesiredAccess::FILE_WRITE_DATA_OR_FILE_ADD_FILE
                | DesiredAccess::FILE_APPEND_DATA_OR_FILE_ADD_SUBDIRECTORY
                | DesiredAccess::FILE_WRITE_EA
                | DesiredAccess::FILE_WRITE_ATTRIBUTES
                | DesiredAccess::FILE_DELETE_CHILD
                | DesiredAccess::DELETE
                | DesiredAccess::WRITE_DAC
                | DesiredAccess::WRITE_OWNER
                | DesiredAccess::GENERIC_WRITE
                | DesiredAccess::GENERIC_ALL,
        );
        if !writable
            && (write_access
                || req.create_disposition != CreateDisposition::FILE_OPEN
                || req
                    .create_options
                    .contains(CreateOptions::FILE_DELETE_ON_CLOSE))
        {
            return Err(denied());
        }
        if req.create_options.intersects(
            CreateOptions::FILE_OPEN_BY_FILE_ID | CreateOptions::FILE_OPEN_REPARSE_POINT,
        ) {
            return Err(denied());
        }
        let dir_request = req
            .create_options
            .contains(CreateOptions::FILE_DIRECTORY_FILE);
        let existing = if parts.is_empty() {
            Some(root.dir_metadata()?)
        } else {
            let (p, n) = parent(root, &parts)?;
            p.symlink_metadata(n).ok()
        };
        if existing
            .as_ref()
            .is_some_and(|m| m.file_type().is_symlink() || (!m.is_dir() && !m.is_file()))
        {
            return Err(denied());
        }
        if existing.is_none() && self.created >= MAX_CREATED {
            return Err(denied());
        }
        let is_dir = dir_request || existing.as_ref().is_some_and(Metadata::is_dir);
        let data = if is_dir {
            if req
                .create_options
                .contains(CreateOptions::FILE_NON_DIRECTORY_FILE)
            {
                return Err(denied());
            }
            if existing.is_none() {
                if !writable
                    || !matches!(
                        req.create_disposition,
                        CreateDisposition::FILE_CREATE | CreateDisposition::FILE_OPEN_IF
                    )
                {
                    return Err(io::ErrorKind::NotFound.into());
                }
                let (p, n) = parent(root, &parts)?;
                p.create_dir(n)?;
            } else if req.create_disposition == CreateDisposition::FILE_CREATE {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            HandleData::Directory(open_dir(root, &parts)?)
        } else {
            let (p, n) = parent(root, &parts)?;
            let mut options = OpenOptions::new();
            options
                .read(true)
                .write(writable && write_access)
                .follow(FollowSymlinks::No);
            #[cfg(unix)]
            options.nonblock(true);
            match req.create_disposition {
                CreateDisposition::FILE_OPEN => {}
                CreateDisposition::FILE_CREATE => {
                    options.create_new(true);
                }
                CreateDisposition::FILE_OPEN_IF
                | CreateDisposition::FILE_OVERWRITE_IF
                | CreateDisposition::FILE_SUPERSEDE => {
                    options.create(true).write(true);
                }
                CreateDisposition::FILE_OVERWRITE => {
                    options.write(true);
                }
                _ => return Err(denied()),
            }
            let file = p.open_with(n, &options)?;
            safe_file(&file)?;
            if matches!(
                req.create_disposition,
                CreateDisposition::FILE_OVERWRITE
                    | CreateDisposition::FILE_OVERWRITE_IF
                    | CreateDisposition::FILE_SUPERSEDE
            ) {
                file.set_len(0)?;
            }
            HandleData::File(file)
        };
        if existing.is_none() {
            self.created += 1;
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or_else(denied)?;
        self.handles.insert(
            id,
            Handle {
                data,
                parts,
                writable: writable && write_access,
                enumeration: None,
                pattern: String::new(),
                seen: 0,
                delete_pending: req
                    .create_options
                    .contains(CreateOptions::FILE_DELETE_ON_CLOSE),
            },
        );
        Ok((
            id,
            if existing.is_some() && req.create_disposition == CreateDisposition::FILE_OVERWRITE_IF
            {
                Information::FILE_OVERWRITTEN
            } else if existing.is_some() {
                Information::FILE_OPENED
            } else {
                Information::FILE_SUPERSEDED
            },
        ))
    }
    fn read(&mut self, req: &DeviceReadRequest) -> io::Result<Vec<u8>> {
        if req.length as usize > MAX_IO
            || req
                .offset
                .checked_add(u64::from(req.length))
                .is_none_or(|end| end > MAX_FILE_SIZE)
        {
            return Err(denied());
        }
        let handle = self
            .handles
            .get_mut(&req.device_io_request.file_id)
            .ok_or(io::ErrorKind::NotFound)?;
        let HandleData::File(file) = &mut handle.data else {
            return Err(denied());
        };
        safe_file(file)?;
        file.seek(SeekFrom::Start(req.offset))?;
        let mut bytes = vec![0; req.length as usize];
        let count = file.read(&mut bytes)?;
        bytes.truncate(count);
        Ok(bytes)
    }
    fn write(&mut self, req: &DeviceWriteRequest) -> io::Result<usize> {
        if req.write_data.len() > MAX_IO
            || req
                .offset
                .checked_add(req.write_data.len() as u64)
                .is_none_or(|n| n > MAX_FILE_SIZE)
        {
            return Err(denied());
        }
        let next_written = self
            .written
            .checked_add(req.write_data.len() as u64)
            .ok_or_else(denied)?;
        if next_written > MAX_SESSION_WRITTEN {
            return Err(denied());
        }
        let handle = self
            .handles
            .get_mut(&req.device_io_request.file_id)
            .ok_or(io::ErrorKind::NotFound)?;
        if !handle.writable {
            return Err(denied());
        }
        let HandleData::File(file) = &mut handle.data else {
            return Err(denied());
        };
        safe_file(file)?;
        file.seek(SeekFrom::Start(req.offset))?;
        // Charge attempted writes as well: a partial I/O failure cannot bypass the budget.
        self.written = next_written;
        file.write_all(&req.write_data)?;
        file.flush()?;
        Ok(req.write_data.len())
    }
    fn query(
        &self,
        req: &ServerDriveQueryInformationRequest,
        writable: bool,
    ) -> io::Result<Option<FileInformationClass>> {
        let h = self
            .handles
            .get(&req.device_io_request.file_id)
            .ok_or(io::ErrorKind::NotFound)?;
        let m = h.metadata()?;
        let flags = attributes(&m, writable);
        Ok(
            if req.file_info_class_lvl == FileInformationClassLevel::FILE_BASIC_INFORMATION {
                Some(FileInformationClass::Basic(FileBasicInformation {
                    creation_time: time(m.created()),
                    last_access_time: time(m.accessed()),
                    last_write_time: time(m.modified()),
                    change_time: time(m.modified()),
                    file_attributes: flags,
                }))
            } else if req.file_info_class_lvl
                == FileInformationClassLevel::FILE_STANDARD_INFORMATION
            {
                Some(FileInformationClass::Standard(FileStandardInformation {
                    allocation_size: i64::try_from(m.len()).unwrap_or(i64::MAX),
                    end_of_file: i64::try_from(m.len()).unwrap_or(i64::MAX),
                    number_of_links: 1,
                    delete_pending: if h.delete_pending {
                        Boolean::True
                    } else {
                        Boolean::False
                    },
                    directory: if m.is_dir() {
                        Boolean::True
                    } else {
                        Boolean::False
                    },
                }))
            } else if req.file_info_class_lvl
                == FileInformationClassLevel::FILE_ATTRIBUTE_TAG_INFORMATION
            {
                Some(FileInformationClass::AttributeTag(
                    FileAttributeTagInformation {
                        file_attributes: flags,
                        reparse_tag: 0,
                    },
                ))
            } else {
                None
            },
        )
    }
    fn enumerate(
        &mut self,
        req: &ServerDriveQueryDirectoryRequest,
        writable: bool,
    ) -> io::Result<Option<FileInformationClass>> {
        let h = self
            .handles
            .get_mut(&req.device_io_request.file_id)
            .ok_or(io::ErrorKind::NotFound)?;
        let HandleData::Directory(dir) = &h.data else {
            return Err(denied());
        };
        if req.initial_query != 0 {
            let path = req.path.replace('\\', "/");
            let (prefix, pattern) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
            if relative(prefix)? != h.parts || pattern.len() > 255 || pattern.contains([':', '\0'])
            {
                return Err(denied());
            }
            h.pattern = if pattern.is_empty() || pattern == "*.*" {
                "*".to_owned()
            } else {
                pattern.to_owned()
            };
            h.enumeration = Some(dir.entries()?);
            h.seen = 0;
        }
        let entries = h.enumeration.as_mut().ok_or_else(denied)?;
        // A filtered search may need to inspect the whole permitted directory.
        // One extra iteration detects exhaustion or enforces the overall quota.
        for _ in 0..=MAX_ENUMERATED {
            let Some(entry) = entries.next() else {
                return Ok(None);
            };
            h.seen += 1;
            if h.seen > MAX_ENUMERATED {
                return Err(denied());
            }
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !safe_name(name) || !wildcard(&h.pattern, name) {
                continue;
            }
            let m = entry.metadata()?;
            if m.file_type().is_symlink() || (!m.is_file() && !m.is_dir()) {
                continue;
            }
            let flags = attributes(&m, writable);
            let size = i64::try_from(m.len()).unwrap_or(i64::MAX);
            let c = time(m.created());
            let a = time(m.accessed());
            let w = time(m.modified());
            return Ok(
                if req.file_info_class_lvl
                    == FileInformationClassLevel::FILE_BOTH_DIRECTORY_INFORMATION
                {
                    Some(FileInformationClass::BothDirectory(
                        FileBothDirectoryInformation::new(c, a, w, w, size, flags, name.to_owned()),
                    ))
                } else if req.file_info_class_lvl
                    == FileInformationClassLevel::FILE_DIRECTORY_INFORMATION
                {
                    Some(FileInformationClass::Directory(
                        FileDirectoryInformation::new(c, a, w, w, size, flags, name.to_owned()),
                    ))
                } else if req.file_info_class_lvl
                    == FileInformationClassLevel::FILE_FULL_DIRECTORY_INFORMATION
                {
                    Some(FileInformationClass::FullDirectory(
                        FileFullDirectoryInformation::new(c, a, w, w, size, flags, name.to_owned()),
                    ))
                } else if req.file_info_class_lvl
                    == FileInformationClassLevel::FILE_NAMES_INFORMATION
                {
                    Some(FileInformationClass::Names(FileNamesInformation::new(
                        name.to_owned(),
                    )))
                } else {
                    return Err(io::ErrorKind::Unsupported.into());
                },
            );
        }
        Err(denied())
    }
    fn set_info(&mut self, root: &Dir, req: &ServerDriveSetInformationRequest) -> io::Result<()> {
        let h = self
            .handles
            .get_mut(&req.device_io_request.file_id)
            .ok_or(io::ErrorKind::NotFound)?;
        if !h.writable {
            return Err(denied());
        }
        match &req.set_buffer {
            FileInformationClass::EndOfFile(info) => {
                if info.end_of_file < 0 || info.end_of_file as u64 > MAX_FILE_SIZE {
                    return Err(denied());
                }
                let HandleData::File(file) = &h.data else {
                    return Err(denied());
                };
                let size = safe_file(file)?.len();
                let growth = (info.end_of_file as u64).saturating_sub(size);
                let next = self.written.checked_add(growth).ok_or_else(denied)?;
                if next > MAX_SESSION_WRITTEN {
                    return Err(denied());
                }
                self.written = next;
                file.set_len(info.end_of_file as u64)
            }
            FileInformationClass::Allocation(info) => {
                if info.allocation_size < 0 || info.allocation_size as u64 > MAX_FILE_SIZE {
                    return Err(denied());
                }
                Ok(())
            }
            FileInformationClass::Disposition(info) => {
                h.delete_pending = info.delete_pending != 0;
                Ok(())
            }
            FileInformationClass::Rename(info) => {
                let target = relative(&info.file_name)?;
                let (from, name) = parent(root, &h.parts)?;
                let (to, dest) = parent(root, &target)?;
                if info.replace_if_exists == Boolean::False && to.symlink_metadata(&dest).is_ok() {
                    return Err(io::ErrorKind::AlreadyExists.into());
                }
                from.rename(name, &to, dest)?;
                h.parts = target;
                Ok(())
            }
            // Copying files commonly supplies original timestamps. No permissions/ACL are changed.
            FileInformationClass::Basic(_) => Ok(()),
            _ => Err(io::ErrorKind::Unsupported.into()),
        }
    }
    fn close(&mut self, root: &Dir, id: u32) -> io::Result<()> {
        if let Some(h) = self.handles.remove(&id) {
            if h.delete_pending {
                let (p, n) = parent(root, &h.parts)?;
                match h.data {
                    HandleData::Directory(_) => p.remove_dir(n)?,
                    HandleData::File(_) => p.remove_file(n)?,
                }
            }
        }
        Ok(())
    }
}
fn wildcard(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut i, mut j, mut star, mut mark) = (0, 0, None, 0);
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i].eq_ignore_ascii_case(&n[j])) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            i += 1;
            mark = j;
        } else if let Some(k) = star {
            i = k + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}
fn request_header(req: &ServerDriveIoRequest) -> &DeviceIoRequest {
    match req {
        ServerDriveIoRequest::ServerCreateDriveRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::ServerDriveQueryInformationRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::DeviceCloseRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::ServerDriveQueryDirectoryRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::ServerDriveNotifyChangeDirectoryRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::ServerDriveQueryVolumeInformationRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::DeviceControlRequest(r) => &r.header,
        ServerDriveIoRequest::DeviceReadRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::DeviceWriteRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::ServerDriveSetInformationRequest(r) => &r.device_io_request,
        ServerDriveIoRequest::ServerDriveLockControlRequest(r) => &r.device_io_request,
    }
}
fn one(pdu: RdpdrPdu) -> PduResult<Vec<SvcMessage>> {
    Ok(vec![SvcMessage::from(pdu)])
}
impl RdpdrBackend for DirectoryBackend {
    fn handle_server_device_announce_response(
        &mut self,
        reply: ServerDeviceAnnounceResponse,
    ) -> PduResult<()> {
        if let Ok(mut s) = self.state.lock() {
            if s.folder.is_some() && reply.device_id == s.drive_id {
                s.folder_status = if reply.result_code == NtStatus::SUCCESS {
                    crate::FolderStatus::Ready
                } else {
                    crate::FolderStatus::Denied
                };
            }
        }
        Ok(())
    }
    fn handle_user_logged_on(
        &mut self,
        _: &mut ironrdp::rdpdr::Rdpdr,
    ) -> PduResult<Vec<SvcMessage>> {
        if let Ok(mut s) = self.state.lock() {
            s.drive_ready = true;
        }
        Ok(Vec::new())
    }
    fn handle_scard_call(
        &mut self,
        _: DeviceControlRequest<ScardIoCtlCode>,
        _: ScardCall,
    ) -> PduResult<()> {
        Ok(())
    }
    fn handle_drive_io_request(&mut self, req: ServerDriveIoRequest) -> PduResult<Vec<SvcMessage>> {
        let shared = self.state.clone();
        let guard = shared.lock().map_err(|_| {
            ironrdp::pdu::decode_err!(io::Error::from(io::ErrorKind::PermissionDenied))
        })?;
        // Directory capabilities have their own epoch. Clipboard-only permission
        // changes must not interrupt a file handle or an unrelated transfer.
        if u64::from(guard.drive_id) != self.generation {
            self.handles.clear();
            self.generation = u64::from(guard.drive_id);
        }
        let header = request_header(&req);
        let root = if !guard.closed && header.device_id == guard.drive_id {
            guard.folder.clone()
        } else {
            None
        };
        let writable = guard.permissions.directory_writable;
        match req {
            ServerDriveIoRequest::ServerCreateDriveRequest(r) => {
                let result = root.as_deref().ok_or_else(denied).and_then(|d| {
                    if r.device_io_request.device_id == guard.drive_id {
                        self.create(d, writable, &r)
                    } else {
                        Err(denied())
                    }
                });
                let (status, id, info) = match result {
                    Ok((id, info)) => (NtStatus::SUCCESS, id, info),
                    Err(e) => (status(e), 0, Information::empty()),
                };
                one(RdpdrPdu::DeviceCreateResponse(DeviceCreateResponse {
                    device_io_reply: DeviceIoResponse::new(r.device_io_request, status),
                    file_id: id,
                    information: info,
                }))
            }
            ServerDriveIoRequest::DeviceReadRequest(r) => {
                let result = if root.is_some() {
                    self.read(&r)
                } else {
                    Err(denied())
                };
                let (status, bytes) = match result {
                    Ok(b) => (NtStatus::SUCCESS, b),
                    Err(e) => (status(e), Vec::new()),
                };
                one(RdpdrPdu::DeviceReadResponse(DeviceReadResponse {
                    device_io_reply: DeviceIoResponse::new(r.device_io_request, status),
                    read_data: bytes,
                }))
            }
            ServerDriveIoRequest::DeviceWriteRequest(mut r) => {
                let result = if root.is_some() && writable {
                    self.write(&r)
                } else {
                    Err(denied())
                };
                let (status, length) = match result {
                    Ok(n) => (NtStatus::SUCCESS, n as u32),
                    Err(e) => (status(e), 0),
                };
                r.write_data.zeroize();
                one(RdpdrPdu::DeviceWriteResponse(DeviceWriteResponse {
                    device_io_reply: DeviceIoResponse::new(r.device_io_request, status),
                    length,
                }))
            }
            ServerDriveIoRequest::DeviceCloseRequest(r) => {
                let status = match root
                    .as_deref()
                    .ok_or_else(denied)
                    .and_then(|d| self.close(d, r.device_io_request.file_id))
                {
                    Ok(()) => NtStatus::SUCCESS,
                    Err(e) => status(e),
                };
                one(RdpdrPdu::DeviceCloseResponse(DeviceCloseResponse {
                    device_io_response: DeviceIoResponse::new(r.device_io_request, status),
                }))
            }
            ServerDriveIoRequest::ServerDriveQueryInformationRequest(r) => {
                let result = if root.is_some() {
                    self.query(&r, writable)
                } else {
                    Err(denied())
                };
                let (status, buffer) = match result {
                    Ok(Some(i)) => (NtStatus::SUCCESS, Some(i)),
                    Ok(None) => (NtStatus::NOT_SUPPORTED, None),
                    Err(e) => (status(e), None),
                };
                one(RdpdrPdu::ClientDriveQueryInformationResponse(
                    ClientDriveQueryInformationResponse {
                        device_io_response: DeviceIoResponse::new(r.device_io_request, status),
                        buffer,
                    },
                ))
            }
            ServerDriveIoRequest::ServerDriveQueryDirectoryRequest(r) => {
                let result = if root.is_some() {
                    self.enumerate(&r, writable)
                } else {
                    Err(denied())
                };
                let (status, buffer) = match result {
                    Ok(Some(i)) => (NtStatus::SUCCESS, Some(i)),
                    // MS-RDPEFS 2.2.3.3.10 distinguishes an empty initial search
                    // from exhaustion of an already-started enumeration.
                    Ok(None) if r.initial_query != 0 => (NtStatus::NO_SUCH_FILE, None),
                    Ok(None) => (NtStatus::NO_MORE_FILES, None),
                    Err(e) => (status(e), None),
                };
                one(RdpdrPdu::ClientDriveQueryDirectoryResponse(
                    ClientDriveQueryDirectoryResponse {
                        device_io_reply: DeviceIoResponse::new(r.device_io_request, status),
                        buffer,
                    },
                ))
            }
            ServerDriveIoRequest::ServerDriveSetInformationRequest(r) => {
                let status = match root.as_deref().ok_or_else(denied).and_then(|d| {
                    if writable {
                        self.set_info(d, &r)
                    } else {
                        Err(denied())
                    }
                }) {
                    Ok(()) => NtStatus::SUCCESS,
                    Err(e) => status(e),
                };
                one(RdpdrPdu::ClientDriveSetInformationResponse(
                    ClientDriveSetInformationResponse::new(&r, status)
                        .map_err(|error| ironrdp::pdu::encode_err!(error))?,
                ))
            }
            ServerDriveIoRequest::ServerDriveQueryVolumeInformationRequest(r) => {
                let buffer = if root.is_none() {
                    None
                } else if r.fs_info_class_lvl
                    == FileSystemInformationClassLevel::FILE_FS_VOLUME_INFORMATION
                {
                    Some(FileSystemInformationClass::FileFsVolumeInformation(
                        FileFsVolumeInformation {
                            volume_creation_time: time(Err(io::ErrorKind::Unsupported.into())),
                            volume_serial_number: 1,
                            supports_objects: Boolean::False,
                            volume_label: "ConsoleCrypt".into(),
                        },
                    ))
                } else if r.fs_info_class_lvl
                    == FileSystemInformationClassLevel::FILE_FS_SIZE_INFORMATION
                {
                    Some(FileSystemInformationClass::FileFsSizeInformation(
                        FileFsSizeInformation {
                            total_alloc_units: (MAX_SESSION_WRITTEN / 4096) as i64,
                            available_alloc_units: ((MAX_SESSION_WRITTEN - self.written) / 4096)
                                as i64,
                            sectors_per_alloc_unit: 8,
                            bytes_per_sector: 512,
                        },
                    ))
                } else if r.fs_info_class_lvl
                    == FileSystemInformationClassLevel::FILE_FS_FULL_SIZE_INFORMATION
                {
                    Some(FileSystemInformationClass::FileFsFullSizeInformation(
                        FileFsFullSizeInformation {
                            total_alloc_units: (MAX_SESSION_WRITTEN / 4096) as i64,
                            caller_available_alloc_units: ((MAX_SESSION_WRITTEN - self.written)
                                / 4096)
                                as i64,
                            actual_available_alloc_units: ((MAX_SESSION_WRITTEN - self.written)
                                / 4096)
                                as i64,
                            sectors_per_alloc_unit: 8,
                            bytes_per_sector: 512,
                        },
                    ))
                } else if r.fs_info_class_lvl
                    == FileSystemInformationClassLevel::FILE_FS_ATTRIBUTE_INFORMATION
                {
                    Some(FileSystemInformationClass::FileFsAttributeInformation(
                        FileFsAttributeInformation {
                            file_system_attributes: FileSystemAttributes::FILE_CASE_SENSITIVE_SEARCH
                                | FileSystemAttributes::FILE_CASE_PRESERVED_NAMES
                                | FileSystemAttributes::FILE_UNICODE_ON_DISK,
                            max_component_name_len: 255,
                            file_system_name: "ConsoleCrypt".into(),
                        },
                    ))
                } else {
                    None
                };
                one(RdpdrPdu::ClientDriveQueryVolumeInformationResponse(
                    ClientDriveQueryVolumeInformationResponse {
                        device_io_reply: DeviceIoResponse::new(
                            r.device_io_request,
                            if buffer.is_some() {
                                NtStatus::SUCCESS
                            } else {
                                NtStatus::NOT_SUPPORTED
                            },
                        ),
                        buffer,
                    },
                ))
            }
            ServerDriveIoRequest::DeviceControlRequest(r) => {
                one(RdpdrPdu::DeviceControlResponse(DeviceControlResponse {
                    device_io_reply: DeviceIoResponse::new(r.header, NtStatus::NOT_SUPPORTED),
                    output_buffer: None,
                }))
            }
            ServerDriveIoRequest::ServerDriveNotifyChangeDirectoryRequest(r) => {
                one(RdpdrPdu::DeviceCloseResponse(DeviceCloseResponse {
                    device_io_response: DeviceIoResponse::new(
                        r.device_io_request,
                        NtStatus::NOT_SUPPORTED,
                    ),
                }))
            }
            ServerDriveIoRequest::ServerDriveLockControlRequest(r) => {
                one(RdpdrPdu::DeviceCloseResponse(DeviceCloseResponse {
                    device_io_response: DeviceIoResponse::new(
                        r.device_io_request,
                        NtStatus::NOT_SUPPORTED,
                    ),
                }))
            }
        }
    }
}

#[cfg(test)]
#[path = "directory_interop_tests.rs"]
mod interop_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{RedirectState, SessionPermissions};
    use std::sync::{Arc, Mutex};

    fn fixture(writable: bool) -> (tempfile::TempDir, SharedRedirect, DirectoryBackend) {
        let temp = tempfile::tempdir().unwrap();
        let root =
            Arc::new(Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap());
        let state = Arc::new(Mutex::new(RedirectState::new(
            SessionPermissions {
                directory_grant_id: Some(uuid::Uuid::new_v4().to_string()),
                directory_writable: writable,
                ..Default::default()
            },
            Some(root),
        )));
        let backend = DirectoryBackend::new(state.clone());
        (temp, state, backend)
    }
    fn header(file_id: u32, major: MajorFunction) -> DeviceIoRequest {
        DeviceIoRequest {
            device_id: DRIVE_ID,
            file_id,
            completion_id: 7,
            major_function: major,
            minor_function: MinorFunction::from(0),
        }
    }
    fn create(path: &str, writable: bool, disposition: CreateDisposition) -> DeviceCreateRequest {
        DeviceCreateRequest {
            device_io_request: header(0, MajorFunction::Create),
            desired_access: if writable {
                DesiredAccess::GENERIC_READ | DesiredAccess::GENERIC_WRITE | DesiredAccess::DELETE
            } else {
                DesiredAccess::GENERIC_READ
            },
            allocation_size: 0,
            file_attributes: FileAttributes::FILE_ATTRIBUTE_NORMAL,
            shared_access: SharedAccess::FILE_SHARE_READ
                | SharedAccess::FILE_SHARE_WRITE
                | SharedAccess::FILE_SHARE_DELETE,
            create_disposition: disposition,
            create_options: CreateOptions::FILE_NON_DIRECTORY_FILE,
            path: path.into(),
        }
    }
    fn reply(b: &mut DirectoryBackend, r: ServerDriveIoRequest) -> Vec<u8> {
        let messages = b.handle_drive_io_request(r).unwrap();
        assert_eq!(messages.len(), 1);
        messages[0].encode_unframed_pdu().unwrap()
    }
    fn reply_status(bytes: &[u8]) -> NtStatus {
        NtStatus::from(u32::from_le_bytes(bytes[12..16].try_into().unwrap()))
    }
    fn opened(b: &mut DirectoryBackend, r: DeviceCreateRequest) -> u32 {
        let bytes = reply(b, r.into());
        assert_eq!(reply_status(&bytes), NtStatus::SUCCESS);
        u32::from_le_bytes(bytes[16..20].try_into().unwrap())
    }
    fn read_req(file: u32, len: u32) -> DeviceReadRequest {
        DeviceReadRequest {
            device_io_request: header(file, MajorFunction::Read),
            length: len,
            offset: 0,
        }
    }
    fn write_req(file: u32, data: &[u8]) -> DeviceWriteRequest {
        DeviceWriteRequest {
            device_io_request: header(file, MajorFunction::Write),
            offset: 0,
            write_data: data.into(),
        }
    }

    #[test]
    fn readonly_reads_but_denies_create_write_truncate_delete_and_rename() {
        let (temp, _, mut b) = fixture(false);
        std::fs::write(temp.path().join("sample.txt"), b"synthetic-data").unwrap();
        let id = opened(
            &mut b,
            create("\\sample.txt", false, CreateDisposition::FILE_OPEN),
        );
        let bytes = reply(&mut b, read_req(id, 64).into());
        assert_eq!(reply_status(&bytes), NtStatus::SUCCESS);
        assert_eq!(&bytes[20..], b"synthetic-data");
        for r in [
            create("new.txt", true, CreateDisposition::FILE_CREATE),
            create("sample.txt", true, CreateDisposition::FILE_OVERWRITE),
            create("sample.txt", true, CreateDisposition::FILE_OPEN),
        ] {
            assert_ne!(reply_status(&reply(&mut b, r.into())), NtStatus::SUCCESS);
        }
        assert_eq!(
            reply_status(&reply(&mut b, write_req(id, b"overwrite").into())),
            NtStatus::ACCESS_DENIED
        );
        let set = ServerDriveSetInformationRequest {
            device_io_request: header(id, MajorFunction::SetInformation),
            set_buffer: FileInformationClass::EndOfFile(FileEndOfFileInformation {
                end_of_file: 0,
            }),
        };
        assert_eq!(
            reply_status(&reply(&mut b, set.into())),
            NtStatus::ACCESS_DENIED
        );
        assert_eq!(
            std::fs::read(temp.path().join("sample.txt")).unwrap(),
            b"synthetic-data"
        );
    }
    #[test]
    fn clipboard_only_changes_preserve_open_file_but_folder_epoch_revokes_it() {
        let (temp, state, mut backend) = fixture(false);
        std::fs::write(temp.path().join("test.txt"), b"synthetic").unwrap();
        let id = opened(
            &mut backend,
            create("test.txt", false, CreateDisposition::FILE_OPEN),
        );
        {
            let mut s = state.lock().unwrap();
            s.generation += 1;
            s.permissions.clipboard_enabled = true;
        }
        let bytes = reply(&mut backend, read_req(id, 64).into());
        assert_eq!(reply_status(&bytes), NtStatus::SUCCESS);
        assert_eq!(&bytes[20..], b"synthetic");
        let mut limit = read_req(id, 1);
        limit.offset = MAX_FILE_SIZE;
        assert_eq!(
            reply_status(&reply(&mut backend, limit.into())),
            NtStatus::ACCESS_DENIED
        );
        state.lock().unwrap().drive_id += 1;
        let mut stale = read_req(id, 64);
        stale.device_io_request.device_id = 2;
        assert_ne!(
            reply_status(&reply(&mut backend, stale.into())),
            NtStatus::SUCCESS
        );
    }
    #[test]
    fn writable_roundtrip_and_permission_revocation_invalidate_all_handles() {
        let (temp, s, mut b) = fixture(true);
        let id = opened(
            &mut b,
            create("write.txt", true, CreateDisposition::FILE_CREATE),
        );
        assert_eq!(
            reply_status(&reply(
                &mut b,
                write_req(id, b"roundtrip / synthetic").into()
            )),
            NtStatus::SUCCESS
        );
        let bytes = reply(&mut b, read_req(id, 64).into());
        assert_eq!(&bytes[20..], b"roundtrip / synthetic");
        {
            let mut s = s.lock().unwrap();
            s.generation += 1;
            s.drive_id += 1;
            s.permissions.directory_writable = false;
        }
        assert_ne!(
            reply_status(&reply(&mut b, write_req(id, b"replacement").into())),
            NtStatus::SUCCESS
        );
        assert!(b.handles.is_empty());
        let mut create_new = create("write.txt", false, CreateDisposition::FILE_OPEN);
        create_new.device_io_request.device_id = 2;
        let new = opened(&mut b, create_new);
        {
            let mut s = s.lock().unwrap();
            s.generation += 1;
            s.drive_id += 1;
            s.folder = None;
            s.permissions.directory_grant_id = None;
        }
        assert_ne!(
            reply_status(&reply(&mut b, read_req(new, 64).into())),
            NtStatus::SUCCESS
        );
        assert_eq!(
            std::fs::read(temp.path().join("write.txt")).unwrap(),
            b"roundtrip / synthetic"
        );
    }
    #[test]
    fn traversal_device_aliases_and_resource_limits_reject_without_side_effects() {
        let (temp, _, mut b) = fixture(true);
        for path in [
            "../outside",
            "\\..\\outside",
            "C:\\outside",
            "\\\\server\\share",
            "a//b",
            "a/../b",
            "CON.txt",
            "file:stream",
            "trailing.",
            "NUL",
            "/../x",
        ] {
            assert_ne!(
                reply_status(&reply(
                    &mut b,
                    create(path, true, CreateDisposition::FILE_CREATE).into()
                )),
                NtStatus::SUCCESS
            );
        }
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        let id = opened(
            &mut b,
            create("local.txt", true, CreateDisposition::FILE_CREATE),
        );
        let mut foreign = read_req(id, 1);
        foreign.device_io_request.device_id = 2;
        assert_ne!(
            reply_status(&reply(&mut b, foreign.into())),
            NtStatus::SUCCESS
        );
        assert_ne!(
            reply_status(&reply(&mut b, read_req(id, (MAX_IO + 1) as u32).into())),
            NtStatus::SUCCESS
        );
        let mut huge = write_req(id, b"a");
        huge.offset = MAX_FILE_SIZE;
        assert_ne!(reply_status(&reply(&mut b, huge.into())), NtStatus::SUCCESS);
        b.written = MAX_SESSION_WRITTEN;
        assert_ne!(
            reply_status(&reply(&mut b, write_req(id, b"a").into())),
            NtStatus::SUCCESS
        );
        assert_eq!(
            std::fs::metadata(temp.path().join("local.txt"))
                .unwrap()
                .len(),
            0
        );
    }
    #[test]
    fn handle_and_creation_quotas_are_bounded() {
        let (_temp, _, mut b) = fixture(true);
        for i in 0..MAX_HANDLES {
            opened(
                &mut b,
                create(
                    &format!("file{i}.txt"),
                    true,
                    CreateDisposition::FILE_CREATE,
                ),
            );
        }
        assert_ne!(
            reply_status(&reply(
                &mut b,
                create("extra.txt", true, CreateDisposition::FILE_CREATE).into()
            )),
            NtStatus::SUCCESS
        );
        b.handles.clear();
        b.created = MAX_CREATED;
        assert_ne!(
            reply_status(&reply(
                &mut b,
                create("extra.txt", true, CreateDisposition::FILE_CREATE).into()
            )),
            NtStatus::SUCCESS
        );
    }
    #[test]
    fn volume_budget_is_usable_by_windows_and_unsupported_operations_fail_closed() {
        let (_temp, _, mut b) = fixture(false);
        for class in [
            FileSystemInformationClassLevel::FILE_FS_VOLUME_INFORMATION,
            FileSystemInformationClassLevel::FILE_FS_SIZE_INFORMATION,
            FileSystemInformationClassLevel::FILE_FS_FULL_SIZE_INFORMATION,
            FileSystemInformationClassLevel::FILE_FS_ATTRIBUTE_INFORMATION,
        ] {
            let r = ServerDriveQueryVolumeInformationRequest {
                device_io_request: header(0, MajorFunction::QueryVolumeInformation),
                fs_info_class_lvl: class,
            };
            assert_eq!(reply_status(&reply(&mut b, r.into())), NtStatus::SUCCESS);
        }
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_hardlinks_and_special_files_never_expose_outside_or_block() {
        use std::os::unix::fs::{symlink, FileTypeExt};
        let (temp, _, mut b) = fixture(true);
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("sentinel.txt"), b"outside-synthetic").unwrap();
        symlink(outside.path(), temp.path().join("link")).unwrap();
        symlink(
            outside.path().join("sentinel.txt"),
            temp.path().join("link.txt"),
        )
        .unwrap();
        std::fs::hard_link(
            outside.path().join("sentinel.txt"),
            temp.path().join("hard.txt"),
        )
        .unwrap();
        for path in ["link/sentinel.txt", "link.txt", "hard.txt"] {
            assert_ne!(
                reply_status(&reply(
                    &mut b,
                    create(path, true, CreateDisposition::FILE_OVERWRITE).into()
                )),
                NtStatus::SUCCESS
            );
        }
        assert_eq!(
            std::fs::read(outside.path().join("sentinel.txt")).unwrap(),
            b"outside-synthetic"
        );
        // UnixStream sockets are also rejected without an ambient open or blocking read.
        let socket = std::os::unix::net::UnixListener::bind(temp.path().join("socket")).unwrap();
        assert!(std::fs::symlink_metadata(temp.path().join("socket"))
            .unwrap()
            .file_type()
            .is_socket());
        assert_ne!(
            reply_status(&reply(
                &mut b,
                create("socket", false, CreateDisposition::FILE_OPEN).into()
            )),
            NtStatus::SUCCESS
        );
        drop(socket);
    }
    #[cfg(unix)]
    #[test]
    fn concurrent_symlink_replacement_cannot_escape_descriptor_relative_root() {
        use std::{
            os::unix::fs::symlink,
            sync::atomic::{AtomicBool, Ordering},
        };
        let (temp, _, mut b) = fixture(false);
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("sentinel.txt"), b"outside-synthetic").unwrap();
        let child = temp.path().join("slot");
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let dest = outside.path().to_owned();
        let racing = std::thread::spawn(move || {
            while !stop2.load(Ordering::Acquire) {
                let _ = symlink(&dest, &child);
                let _ = std::fs::remove_file(&child);
            }
        });
        for _ in 0..200 {
            let bytes = reply(
                &mut b,
                create("slot/sentinel.txt", false, CreateDisposition::FILE_OPEN).into(),
            );
            assert_ne!(reply_status(&bytes), NtStatus::SUCCESS);
        }
        stop.store(true, Ordering::Release);
        racing.join().unwrap();
    }
}

#[cfg(test)]
mod folder_status_tests {
    use super::*;
    use crate::permissions::{RedirectState, SessionPermissions};
    use crate::FolderStatus;
    use std::sync::{Arc, Mutex};
    #[test]
    fn device_acknowledgement_is_exact_and_timeout_is_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let root =
            Arc::new(Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap());
        let state = Arc::new(Mutex::new(RedirectState::new(
            SessionPermissions {
                directory_grant_id: Some(uuid::Uuid::new_v4().to_string()),
                ..Default::default()
            },
            Some(root),
        )));
        let mut backend = DirectoryBackend::new(state.clone());
        let mut channel = ironrdp::rdpdr::Rdpdr::new(
            Box::new(DirectoryBackend::new(state.clone())),
            "ConsoleCrypt".into(),
        );
        backend.handle_user_logged_on(&mut channel).unwrap();
        assert!(state.lock().unwrap().drive_ready);
        assert_eq!(
            state.lock().unwrap().current_folder_status(),
            FolderStatus::Pending
        );
        backend
            .handle_server_device_announce_response(ServerDeviceAnnounceResponse {
                device_id: 2,
                result_code: NtStatus::SUCCESS,
            })
            .unwrap();
        assert_eq!(
            state.lock().unwrap().current_folder_status(),
            FolderStatus::Pending
        );
        backend
            .handle_server_device_announce_response(ServerDeviceAnnounceResponse {
                device_id: 1,
                result_code: NtStatus::ACCESS_DENIED,
            })
            .unwrap();
        assert_eq!(
            state.lock().unwrap().current_folder_status(),
            FolderStatus::Denied
        );
        {
            let mut s = state.lock().unwrap();
            s.drive_id = 2;
            s.folder_status = FolderStatus::Pending;
        }
        backend
            .handle_server_device_announce_response(ServerDeviceAnnounceResponse {
                device_id: 1,
                result_code: NtStatus::SUCCESS,
            })
            .unwrap();
        assert_eq!(
            state.lock().unwrap().current_folder_status(),
            FolderStatus::Pending
        );
        backend
            .handle_server_device_announce_response(ServerDeviceAnnounceResponse {
                device_id: 2,
                result_code: NtStatus::SUCCESS,
            })
            .unwrap();
        assert_eq!(
            state.lock().unwrap().current_folder_status(),
            FolderStatus::Ready
        );
        {
            let mut s = state.lock().unwrap();
            s.folder_status = FolderStatus::Pending;
            s.folder_pending_since = std::time::Instant::now() - std::time::Duration::from_secs(21);
        }
        assert_eq!(
            state.lock().unwrap().current_folder_status(),
            FolderStatus::Unavailable
        );
        state.lock().unwrap().close();
        assert_eq!(
            state.lock().unwrap().current_folder_status(),
            FolderStatus::Disabled
        );
    }
}
