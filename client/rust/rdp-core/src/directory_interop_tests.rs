//! Protocol-facing regressions for ordinary Windows directory and file operations.
use super::*;
use crate::permissions::{RedirectState, SessionPermissions};
use std::sync::{Arc, Mutex};

fn fixture() -> (tempfile::TempDir, SharedRedirect, DirectoryBackend) {
    let temp = tempfile::tempdir().unwrap();
    let root = Arc::new(Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap());
    let state = Arc::new(Mutex::new(RedirectState::new(
        SessionPermissions {
            directory_grant_id: Some(uuid::Uuid::new_v4().to_string()),
            directory_writable: true,
            ..Default::default()
        },
        Some(root),
    )));
    let backend = DirectoryBackend::new(state.clone());
    (temp, state, backend)
}
fn header(file_id: u32, major_function: MajorFunction) -> DeviceIoRequest {
    DeviceIoRequest {
        device_id: DRIVE_ID,
        file_id,
        completion_id: 7,
        major_function,
        minor_function: MinorFunction::from(0),
    }
}
fn create(path: &str, disposition: CreateDisposition, directory: bool) -> DeviceCreateRequest {
    DeviceCreateRequest {
        device_io_request: header(0, MajorFunction::Create),
        desired_access: DesiredAccess::GENERIC_READ
            | DesiredAccess::GENERIC_WRITE
            | DesiredAccess::DELETE,
        allocation_size: 0,
        file_attributes: FileAttributes::FILE_ATTRIBUTE_NORMAL,
        shared_access: SharedAccess::FILE_SHARE_READ
            | SharedAccess::FILE_SHARE_WRITE
            | SharedAccess::FILE_SHARE_DELETE,
        create_disposition: disposition,
        create_options: if directory {
            CreateOptions::FILE_DIRECTORY_FILE
        } else {
            CreateOptions::FILE_NON_DIRECTORY_FILE
        },
        path: path.into(),
    }
}
fn reply(backend: &mut DirectoryBackend, request: impl Into<ServerDriveIoRequest>) -> Vec<u8> {
    let messages = backend.handle_drive_io_request(request.into()).unwrap();
    assert_eq!(messages.len(), 1);
    messages[0].encode_unframed_pdu().unwrap()
}
fn reply_status(bytes: &[u8]) -> NtStatus {
    NtStatus::from(u32::from_le_bytes(bytes[12..16].try_into().unwrap()))
}
fn open(backend: &mut DirectoryBackend, request: DeviceCreateRequest) -> u32 {
    let bytes = reply(backend, request);
    assert_eq!(reply_status(&bytes), NtStatus::SUCCESS);
    u32::from_le_bytes(bytes[16..20].try_into().unwrap())
}
fn query(file: u32, initial: u8, path: &str) -> ServerDriveQueryDirectoryRequest {
    let mut device_io_request = header(file, MajorFunction::DirectoryControl);
    device_io_request.minor_function = MinorFunction::IRP_MN_QUERY_DIRECTORY;
    ServerDriveQueryDirectoryRequest {
        device_io_request,
        file_info_class_lvl: FileInformationClassLevel::FILE_NAMES_INFORMATION,
        initial_query: initial,
        path: path.into(),
    }
}

#[test]
fn initial_empty_or_unmatched_enumeration_differs_from_exhaustion() {
    // MS-RDPEFS 2.2.3.3.10: initial query with no match is NO_SUCH_FILE;
    // subsequent exhaustion is NO_MORE_FILES, and its Path must be ignored.
    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpefs/458019d2-5d5a-4fd4-92ef-8c05f8d7acb1
    let (temp, _, mut backend) = fixture();
    let id = open(&mut backend, create("", CreateDisposition::FILE_OPEN, true));
    assert_eq!(
        reply_status(&reply(&mut backend, query(id, 1, "\\*"))),
        NtStatus::NO_SUCH_FILE
    );
    assert_eq!(
        reply_status(&reply(&mut backend, query(id, 0, "ignored\\..\\path"))),
        NtStatus::NO_MORE_FILES
    );
    std::fs::write(temp.path().join("present.txt"), b"synthetic").unwrap();
    assert_eq!(
        reply_status(&reply(&mut backend, query(id, 1, "\\absent.*"))),
        NtStatus::NO_SUCH_FILE
    );
    assert_eq!(
        reply_status(&reply(&mut backend, query(id, 1, "\\*.txt"))),
        NtStatus::SUCCESS
    );
    assert_eq!(
        reply_status(&reply(&mut backend, query(id, 0, ""))),
        NtStatus::NO_MORE_FILES
    );
}

#[test]
fn filtered_search_beyond_1024_entries_honors_the_documented_total_quota() {
    let (temp, _, mut backend) = fixture();
    for i in 0..1500 {
        std::fs::write(temp.path().join(format!("entry-{i:04}.txt")), b"synthetic").unwrap();
    }
    let id = open(&mut backend, create("", CreateDisposition::FILE_OPEN, true));
    // Use this immutable directory's enumeration order to pick a late exact
    // match, rather than assuming a filesystem returns names alphabetically.
    let HandleData::Directory(dir) = &backend.handles.get(&id).unwrap().data else {
        panic!("expected directory fixture");
    };
    let late_name = dir
        .entries()
        .unwrap()
        .nth(1300)
        .unwrap()
        .unwrap()
        .file_name();
    let late_name = late_name.to_str().unwrap();
    let matching = reply(&mut backend, query(id, 1, &format!("\\{late_name}")));
    assert_eq!(reply_status(&matching), NtStatus::SUCCESS);
    assert!(backend.handles.get(&id).unwrap().seen > 1024);
    let missing = reply(&mut backend, query(id, 1, "\\*.missing"));
    assert_eq!(reply_status(&missing), NtStatus::NO_SUCH_FILE);
    assert_eq!(backend.handles.get(&id).unwrap().seen, 1500);
    // Searching beyond 1,024 must retain the overall 10,000-entry enumeration cap.
    // Exercise its exact remaining boundary without creating 10,000 fixtures.
    backend.handles.get_mut(&id).unwrap().seen = MAX_ENUMERATED;
    let handle = backend.handles.get_mut(&id).unwrap();
    let HandleData::Directory(dir) = &handle.data else {
        panic!("expected directory fixture");
    };
    handle.enumeration = Some(dir.entries().unwrap());
    let over_quota = reply(&mut backend, query(id, 0, "ignored"));
    assert_eq!(reply_status(&over_quota), NtStatus::ACCESS_DENIED);
    assert_eq!(backend.handles.get(&id).unwrap().seen, MAX_ENUMERATED + 1);
}

#[test]
fn overwrite_if_reports_overwrite_and_truncates_existing_file() {
    // MS-RDPEFS 2.2.1.5.1: FILE_OVERWRITE_IF reports FILE_OVERWRITTEN.
    // https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpefs/99e5fca5-b37a-41e4-bc69-8d7da7860f76
    let (temp, _, mut backend) = fixture();
    std::fs::write(temp.path().join("existing.txt"), b"original synthetic data").unwrap();
    let bytes = reply(
        &mut backend,
        create("existing.txt", CreateDisposition::FILE_OVERWRITE_IF, false),
    );
    assert_eq!(reply_status(&bytes), NtStatus::SUCCESS);
    assert_eq!(bytes[20], Information::FILE_OVERWRITTEN.bits());
    assert_eq!(
        std::fs::metadata(temp.path().join("existing.txt"))
            .unwrap()
            .len(),
        0
    );
    let created = reply(
        &mut backend,
        create("new.txt", CreateDisposition::FILE_CREATE, false),
    );
    assert_eq!(reply_status(&created), NtStatus::SUCCESS);
    assert_eq!(created[20], Information::FILE_SUPERSEDED.bits());
}

#[test]
fn larger_file_roundtrips_through_bounded_protocol_chunks_without_loss() {
    let (temp, _, mut backend) = fixture();
    let id = open(
        &mut backend,
        create("large.bin", CreateDisposition::FILE_CREATE, false),
    );
    let expected: Vec<u8> = (0..150 * 1024).map(|i| (i % 251) as u8).collect();
    for (i, chunk) in expected.chunks(MAX_IO).enumerate() {
        let response = reply(
            &mut backend,
            DeviceWriteRequest {
                device_io_request: header(id, MajorFunction::Write),
                offset: (i * MAX_IO) as u64,
                write_data: chunk.into(),
            },
        );
        assert_eq!(reply_status(&response), NtStatus::SUCCESS);
        assert_eq!(
            u32::from_le_bytes(response[16..20].try_into().unwrap()) as usize,
            chunk.len()
        );
    }
    let mut actual = Vec::new();
    while actual.len() < expected.len() {
        let response = reply(
            &mut backend,
            DeviceReadRequest {
                device_io_request: header(id, MajorFunction::Read),
                length: MAX_IO as u32,
                offset: actual.len() as u64,
            },
        );
        assert_eq!(reply_status(&response), NtStatus::SUCCESS);
        assert!(!response[20..].is_empty());
        actual.extend_from_slice(&response[20..]);
    }
    assert!(actual == expected);
    assert!(std::fs::read(temp.path().join("large.bin")).unwrap() == expected);
    let eof = reply(
        &mut backend,
        DeviceReadRequest {
            device_io_request: header(id, MajorFunction::Read),
            length: 1,
            offset: actual.len() as u64,
        },
    );
    assert_eq!(reply_status(&eof), NtStatus::SUCCESS);
    assert!(eof[20..].is_empty());
}

#[test]
fn nested_unicode_enumeration_rename_and_delete_use_protocol_responses() {
    let (temp, _, mut backend) = fixture();
    let directory = open(
        &mut backend,
        create("папка", CreateDisposition::FILE_CREATE, true),
    );
    let id = open(
        &mut backend,
        create("папка\\пример.txt", CreateDisposition::FILE_CREATE, false),
    );
    let names = reply(&mut backend, query(directory, 1, "\\папка\\*"));
    assert_eq!(reply_status(&names), NtStatus::SUCCESS);
    assert_eq!(
        String::from_utf16(
            &names[32..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>()
        )
        .unwrap(),
        "пример.txt"
    );
    let renamed = reply(
        &mut backend,
        ServerDriveSetInformationRequest {
            device_io_request: header(id, MajorFunction::SetInformation),
            set_buffer: FileInformationClass::Rename(FileRenameInformation {
                replace_if_exists: Boolean::False,
                file_name: "\\папка\\новое.txt".into(),
            }),
        },
    );
    assert_eq!(reply_status(&renamed), NtStatus::SUCCESS);
    assert!(!temp.path().join("папка/пример.txt").exists());
    assert!(temp.path().join("папка/новое.txt").is_file());
    let deletion = reply(
        &mut backend,
        ServerDriveSetInformationRequest {
            device_io_request: header(id, MajorFunction::SetInformation),
            set_buffer: FileInformationClass::Disposition(FileDispositionInformation {
                delete_pending: 1,
            }),
        },
    );
    assert_eq!(reply_status(&deletion), NtStatus::SUCCESS);
    assert!(temp.path().join("папка/новое.txt").exists());
    let closed = reply(
        &mut backend,
        DeviceCloseRequest {
            device_io_request: header(id, MajorFunction::Close),
        },
    );
    assert_eq!(reply_status(&closed), NtStatus::SUCCESS);
    assert!(!temp.path().join("папка/новое.txt").exists());
    assert!(temp.path().join("папка").is_dir());
}

#[test]
fn replaced_folder_rejects_old_open_file_and_allocates_new_handle() {
    let (old, state, mut backend) = fixture();
    std::fs::write(old.path().join("same.txt"), b"old synthetic file").unwrap();
    let id = open(
        &mut backend,
        create("same.txt", CreateDisposition::FILE_OPEN, false),
    );
    let replacement = tempfile::tempdir().unwrap();
    std::fs::write(
        replacement.path().join("same.txt"),
        b"replacement synthetic file",
    )
    .unwrap();
    {
        let mut state = state.lock().unwrap();
        state.generation += 1;
        state.drive_id += 1;
        state.folder = Some(Arc::new(
            Dir::open_ambient_dir(replacement.path(), cap_std::ambient_authority()).unwrap(),
        ));
        state.permissions.directory_grant_id = Some(uuid::Uuid::new_v4().to_string());
    }
    let current_device = state.lock().unwrap().drive_id;
    // Use the current device with the old FileId: denial must follow handle
    // invalidation, rather than merely rejecting the former device ID.
    let mut stale_header = header(id, MajorFunction::Read);
    stale_header.device_id = current_device;
    let stale = reply(
        &mut backend,
        DeviceReadRequest {
            device_io_request: stale_header,
            length: 64,
            offset: 0,
        },
    );
    assert_eq!(reply_status(&stale), NtStatus::NO_SUCH_FILE);
    let mut fresh_request = create("same.txt", CreateDisposition::FILE_OPEN, false);
    fresh_request.device_io_request.device_id = current_device;
    let fresh = open(&mut backend, fresh_request);
    assert_ne!(id, fresh);
    let mut fresh_header = header(fresh, MajorFunction::Read);
    fresh_header.device_id = current_device;
    let bytes = reply(
        &mut backend,
        DeviceReadRequest {
            device_io_request: fresh_header,
            length: 64,
            offset: 0,
        },
    );
    assert_eq!(reply_status(&bytes), NtStatus::SUCCESS);
    assert_eq!(&bytes[20..], b"replacement synthetic file");
}
