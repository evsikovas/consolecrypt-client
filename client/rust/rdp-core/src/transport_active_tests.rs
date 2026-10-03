//! Drives the actual active actor with framed peer traffic, without NLA/accounts.
use super::*;
use ironrdp::{core::encode_vec, svc::SvcMessage};
use sha2::{Digest, Sha256};
pub(super) fn bulk_reply() -> (Vec<u8>, Vec<u8>) {
    use ironrdp::rdpdr::pdu::{efs::*, RdpdrPdu};
    let seed = *uuid::Uuid::new_v4().as_bytes();
    let payload: Vec<_> = (0..MAX_PDU)
        .map(|i| (i as u8).wrapping_add(seed[i % 16]))
        .collect();
    let msg = SvcMessage::from(RdpdrPdu::DeviceReadResponse(DeviceReadResponse {
        device_io_reply: DeviceIoResponse::new(
            DeviceIoRequest {
                device_id: 1,
                file_id: 1,
                completion_id: 1,
                major_function: MajorFunction::Read,
                minor_function: MinorFunction::from(0),
            },
            NtStatus::SUCCESS,
        ),
        read_data: payload.clone(),
    }));
    (
        ironrdp::svc::client_encode_svc_messages(vec![msg], 1004, 1002).unwrap(),
        payload,
    )
}
fn bitmap() -> Vec<u8> {
    use pdu::{
        bitmap::{BitmapData, BitmapUpdateData, Compression},
        fast_path::{
            EncryptionFlags, FastPathHeader, FastPathUpdatePdu, Fragmentation, UpdateCode,
        },
        geometry::InclusiveRectangle,
    };
    let body = encode_vec(&BitmapUpdateData {
        rectangles: vec![BitmapData {
            rectangle: InclusiveRectangle {
                left: 0,
                top: 0,
                right: 31,
                bottom: 31,
            },
            width: 32,
            height: 32,
            bits_per_pixel: 24,
            compression_flags: Compression::empty(),
            compressed_data_header: None,
            bitmap_data: &[0, 0, 255].repeat(32 * 32),
        }],
    })
    .unwrap();
    let update = encode_vec(&FastPathUpdatePdu {
        fragmentation: Fragmentation::Single,
        update_code: UpdateCode::Bitmap,
        compression_flags: None,
        compression_type: None,
        data: &body,
    })
    .unwrap();
    let mut packet =
        encode_vec(&FastPathHeader::new(EncryptionFlags::empty(), update.len())).unwrap();
    packet.extend(update);
    packet
}
fn runtime(initial_output: Vec<Zeroizing<Vec<u8>>>) -> ActiveRuntime {
    let settings = ConnectConfig {
        address: "127.0.0.1".into(),
        port: 3389,
        username: "runtime-fixture".into(),
        domain: None,
        width: 320,
        height: 200,
        accepted_certificate_sha256: [1; 32],
    };
    ActiveRuntime {
        active: ActiveStageBuilder {
            static_channels: Default::default(),
            user_channel_id: 1002,
            io_channel_id: 1003,
            message_channel_id: None,
            share_id: 1,
            compression_type: None,
            enable_server_pointer: false,
            pointer_software_rendering: true,
        }
        .build(),
        image: DecodedImage::new(PixelFormat::RgbA32, 320, 200),
        activation_factory: connector::connection_activation::ConnectionActivationFactory::new(
            config(&settings),
            1003,
            1002,
        ),
        activation_context: ActivationContext {
            io_channel_id: 1003,
            user_channel_id: 1002,
            share_id: 1,
            enable_server_pointer: false,
            pointer_software_rendering: true,
        },
        io_channel_id: 1003,
        message_channel_id: None,
        drive_channel_id: None,
        dvc_id: None,
        clipboard_channel_id: None,
        initial_permissions: Default::default(),
        initial_drive_id: 1,
        initial_output,
    }
}
#[tokio::test]
async fn graphics_exceeding_input_cap_and_keyboard_are_serviced_before_file_last() {
    let (reply, payload) = bulk_reply();
    let (client, server) = tokio::io::duplex(1024);
    let stopped = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(SessionState {
        status: SessionStatus::Connected,
        frame: None,
        sequence: 0,
        resize_available: false,
        diagnostics: Default::default(),
    }));
    let redirects = Arc::new(Mutex::new(crate::permissions::RedirectState::new(
        Default::default(),
        None,
    )));
    let (commands, rx) = mpsc::channel(128);
    let actor_state = state.clone();
    let actor = tokio::spawn(async move {
        run_active(
            BoundedIo::new(client, stopped),
            rx,
            actor_state,
            redirects,
            Arc::new(tokio::sync::Notify::new()),
            runtime(vec![Zeroizing::new(reply)]),
        )
        .await
    });
    let (read, mut write) = tokio::io::split(server);
    let mut peer = BoundedIo::new(read, Arc::new(AtomicBool::new(false)));
    let (_, first) = peer.pdu().await.unwrap();
    let mut wire = first.to_vec();
    let graphic = bitmap();
    let count = MAX_PDU / graphic.len() + 16;
    assert!(count * graphic.len() > MAX_PDU);
    // Peer does not read more file bytes until its graphics burst is drained.
    // The former read-ahead-only actor overflowed instead of decoding it.
    for _ in 0..count {
        write.write_all(&graphic).await.unwrap();
    }
    commands
        .send(SessionCommand::Inputs(vec![Input::UnicodeText(
            "runtime input".into(),
        )]))
        .await
        .unwrap();
    let mut assembled = Vec::new();
    let mut key_before_last = false;
    loop {
        let info = pdu::find_size(&wire).unwrap().unwrap();
        if info.action == pdu::Action::FastPath {
            key_before_last = true;
        } else {
            let data =
                ironrdp::core::decode::<pdu::x224::X224<pdu::mcs::SendDataRequest<'_>>>(&wire)
                    .unwrap()
                    .0;
            assert_eq!(data.channel_id, 1004);
            let flags = u32::from_le_bytes(data.user_data[4..8].try_into().unwrap());
            assembled.extend_from_slice(&data.user_data[8..]);
            if flags & 2 != 0 {
                break;
            }
        }
        wire = peer.pdu().await.unwrap().1.to_vec();
    }
    assert!(
        key_before_last,
        "keyboard output must not wait for file LAST"
    );
    assert_eq!(Sha256::digest(&assembled[20..]), Sha256::digest(&payload));
    assert_eq!(&assembled[20..], payload);
    let state = state.lock().unwrap();
    assert!(
        state.sequence >= count as u64,
        "all graphics decoded, latest mailbox retained"
    );
    assert!(state
        .frame
        .as_ref()
        .is_some_and(|frame| frame.rgba.len() == 320 * 200 * 4));
    drop(state);
    actor.abort();
}
#[test]
fn fragmented_drive_completion_reserves_full_reply_and_input_snapshot_preserves_keys() {
    use std::borrow::Cow;
    let svc = [0u8, 0, 0, 0, 2, 0, 0, 0, 0];
    let packet = encode_vec(&pdu::x224::X224(pdu::mcs::SendDataIndication {
        initiator_id: 1002,
        channel_id: 1004,
        user_data: Cow::Borrowed(&svc),
    }))
    .unwrap();
    assert!(drive_reply_budget(pdu::Action::X224, &packet, Some(1004)) > MAX_PDU);
    let mut db = Database::new();
    let _ = input_events(
        &mut db,
        &Input::Scancode {
            code: 0x1d,
            down: true,
            extended: true,
        },
    );
    let mut copy = copy_input_database(&db);
    let _ = input_events(&mut copy, &Input::ReleaseAll);
    assert!(db.is_key_pressed(Scancode::from_u8(true, 0x1d)));
    assert!(!copy.is_key_pressed(Scancode::from_u8(true, 0x1d)));
}

#[test]
fn fallback_tracks_read_request_type_including_fragmented_header() {
    use std::borrow::Cow;
    fn inbound(body: &[u8], flags: u32) -> Vec<u8> {
        let mut user = vec![56, 0, 0, 0];
        user.extend(flags.to_le_bytes());
        user.extend(body);
        encode_vec(&pdu::x224::X224(pdu::mcs::SendDataIndication {
            initiator_id: 1002,
            channel_id: 1004,
            user_data: Cow::Borrowed(&user),
        }))
        .unwrap()
    }
    let mut read = vec![0u8; 24];
    read[..4].copy_from_slice(&[0x72, 0x44, 0x52, 0x49]);
    read[16..20].copy_from_slice(&3u32.to_le_bytes());
    let mut prefix = Vec::new();
    assert!(!classify_drive_read(
        pdu::Action::X224,
        &inbound(&read[..10], 1),
        Some(1004),
        &mut prefix
    ));
    assert!(classify_drive_read(
        pdu::Action::X224,
        &inbound(&read[10..], 2),
        Some(1004),
        &mut prefix
    ));
    assert!(prefix.is_empty());
    read[16..20].copy_from_slice(&4u32.to_le_bytes());
    assert!(!classify_drive_read(
        pdu::Action::X224,
        &inbound(&read, 3),
        Some(1004),
        &mut prefix
    ));
    let (reply, _) = bulk_reply();
    assert!(matches!(
        drive_response_purpose(&reply, Some(1004), false).unwrap(),
        ordered_writer::Purpose::Protocol
    ));
    assert!(matches!(
        drive_response_purpose(&reply, Some(1004), true).unwrap(),
        ordered_writer::Purpose::Drive { .. }
    ));
}
