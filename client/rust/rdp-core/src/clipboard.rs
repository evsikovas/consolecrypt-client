//! CF_UNICODETEXT only, explicit user Send/Request, no OS clipboard access or file formats.
use crate::{
    permissions::{ClipboardAction, SharedRedirect, MAX_CLIPBOARD_TEXT},
    RdpError,
};
use ironrdp::{
    cliprdr::{backend::CliprdrBackend, pdu::*, CliprdrClient},
    core::{impl_as_any, IntoOwned},
    session::ActiveStage,
};
use zeroize::Zeroizing;

pub(crate) struct TextBackend {
    state: SharedRedirect,
}
impl std::fmt::Debug for TextBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TextBackend(<redacted>)")
    }
}
impl_as_any!(TextBackend);
impl TextBackend {
    pub fn new(state: SharedRedirect) -> Self {
        Self { state }
    }
}
impl CliprdrBackend for TextBackend {
    fn temporary_directory(&self) -> &str {
        "."
    }
    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
    }
    fn on_ready(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            s.ready = true;
        }
    }
    fn on_request_format_list(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            let _ = s.enqueue(ClipboardAction::Advertise);
        }
    }
    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}
    fn on_format_list_response(&mut self, ok: bool) {
        if let Ok(mut s) = self.state.lock() {
            let generation = s.generation;
            let enabled = s.enabled();
            s.offers.acknowledge(ok, generation, enabled);
        }
    }
    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        if let Ok(mut s) = self.state.lock() {
            s.offers.content_changed();
            s.prepared_request = None;
            s.clipboard_counts[0] = s.clipboard_counts[0].saturating_add(1);
            s.remote_unicode = formats.iter().any(|f| {
                f.id == ClipboardFormatId::CF_UNICODETEXT
                    && f.name.as_ref().is_none_or(|name| {
                        !matches!(name.value(), "FileGroupDescriptorW" | "FileGroupDescriptor")
                    })
            });
            // A remote clipboard change invalidates a pending user request and old result.
            if s.pending_request.is_some() {
                s.pending_request = Some(0);
            }
            s.received_text = None;
        }
    }
    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        if let Ok(mut s) = self.state.lock() {
            #[cfg(test)]
            {
                let index = usize::from(request.format != ClipboardFormatId::CF_UNICODETEXT);
                s.clipboard_send_counts[index] = s.clipboard_send_counts[index].saturating_add(1);
            }
            let generation = s.generation;
            let _ = s.enqueue(ClipboardAction::Respond(
                generation,
                request.format == ClipboardFormatId::CF_UNICODETEXT,
            ));
        }
    }
    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        if let Ok(mut s) = self.state.lock() {
            s.clipboard_counts[2] = s.clipboard_counts[2].saturating_add(1);
            if s.pending_request.take() != Some(s.generation) || !s.enabled() || response.is_error()
            {
                s.clipboard_counts[3] = s.clipboard_counts[3].saturating_add(1);
                return;
            }
            s.received_text = decode_text(response.data());
            if s.received_text.is_none() {
                s.clipboard_counts[4] = s.clipboard_counts[4].saturating_add(1);
            }
        }
    }
    fn on_file_contents_request(&mut self, _: FileContentsRequest) {}
    fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}
    fn on_lock(&mut self, _: LockDataId) {}
    fn on_unlock(&mut self, _: LockDataId) {}
}

fn decode_text(bytes: &[u8]) -> Option<Zeroizing<String>> {
    if bytes.len() < 2 || bytes.len() > MAX_CLIPBOARD_TEXT * 2 + 2 || !bytes.len().is_multiple_of(2)
    {
        return None;
    }
    let mut units = Zeroizing::new(Vec::with_capacity(bytes.len() / 2));
    for pair in bytes.as_chunks::<2>().0 {
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    if units.pop() != Some(0) || units.contains(&0) {
        return None;
    }
    let text = Zeroizing::new(String::from_utf16(&units).ok()?);
    if text.len() > MAX_CLIPBOARD_TEXT {
        return None;
    }
    Some(text)
}

pub(crate) struct Advertisement {
    pub confirmed_id: Option<u64>,
    pub enabled: bool,
    pub text: Option<Zeroizing<String>>,
}
pub(crate) type ClipboardFrame = (u64, Zeroizing<Vec<u8>>, Option<Advertisement>, u64, bool);

pub(crate) fn flush(
    active: &mut ActiveStage,
    state: &SharedRedirect,
) -> Result<Vec<ClipboardFrame>, RdpError> {
    let mut s = state.lock().map_err(|_| RdpError::Connection)?;
    let Some(channel) = active.get_svc_processor_mut::<CliprdrClient>() else {
        return Ok(Vec::new());
    };
    let mut messages = Vec::new();
    // Process each queued action at most once. An ID-less FormatList ACK must
    // drain before ANY subsequent advertisement, including empty/revoke offers.
    for _ in 0..s.actions.len() {
        let Some(action) = s.actions.pop_front() else {
            break;
        };
        let mut advertisement = None;
        let mut request = false;
        let next = match action {
            ClipboardAction::Advertise | ClipboardAction::AdvertiseConfirmed(_) => {
                if s.offers.in_flight() {
                    s.actions.push_back(action);
                    continue;
                }
                let confirmed_id = match action {
                    ClipboardAction::AdvertiseConfirmed(id) => Some(id),
                    _ => None,
                };
                let enabled = s.enabled();
                let text = s.local_text.clone();
                // The native writer opens the ID-less ACK slot at dispatch,
                // not while this message waits behind a file response.
                advertisement = Some(Advertisement {
                    confirmed_id,
                    enabled,
                    text,
                });
                let formats = if s.enabled() && s.local_text.is_some() {
                    vec![ClipboardFormat {
                        id: ClipboardFormatId::CF_UNICODETEXT,
                        name: None,
                    }]
                } else {
                    Vec::new()
                };
                channel.initiate_copy(&formats)
            }
            ClipboardAction::Request => {
                if !s.enabled()
                    || !s.ready
                    || !s.remote_unicode
                    || s.pending_request.is_some()
                    || s.prepared_request.is_some()
                {
                    continue;
                }
                s.prepared_request = Some((s.generation, s.offers.content_sequence()));
                request = true;
                channel.initiate_paste(ClipboardFormatId::CF_UNICODETEXT)
            }
            ClipboardAction::Respond(generation, unicode) => {
                let response = if unicode && generation == s.generation && s.enabled() {
                    s.offers
                        .advertised_text
                        .as_ref()
                        .map(|text| FormatDataResponse::new_unicode_string(text).into_owned())
                        .unwrap_or_else(|| FormatDataResponse::new_error().into_owned())
                } else {
                    FormatDataResponse::new_error().into_owned()
                };
                #[cfg(test)]
                {
                    let index = if response.is_error() { 3 } else { 2 };
                    s.clipboard_send_counts[index] =
                        s.clipboard_send_counts[index].saturating_add(1);
                }
                channel.submit_format_data(response)
            }
        }
        .map_err(|_| RdpError::Protocol)?;
        messages.push((next, advertisement, request));
        // Bound generated-but-not-yet-dispatched plaintext to one action.
        break;
    }
    let generation = s.generation;
    let sequence = s.offers.sequence();
    let content_sequence = s.offers.content_sequence();
    drop(s);
    messages
        .into_iter()
        .map(|(message, advertisement, request)| {
            let sequence = if advertisement.is_some() {
                sequence
            } else {
                content_sequence
            };
            active
                .process_svc_processor_messages(message)
                .map(|bytes| {
                    (
                        generation,
                        Zeroizing::new(bytes),
                        advertisement,
                        sequence,
                        request,
                    )
                })
                .map_err(|_| RdpError::Protocol)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{RedirectState, SessionPermissions};
    use std::sync::{Arc, Mutex};
    fn state(enabled: bool) -> SharedRedirect {
        Arc::new(Mutex::new(RedirectState::new(
            SessionPermissions {
                clipboard_enabled: enabled,
                ..Default::default()
            },
            None,
        )))
    }
    #[test]
    fn unicode_text_is_strict_and_bounded() {
        let text = "Clipboard / Привет 😀";
        let bytes = FormatDataResponse::new_unicode_string(text);
        assert_eq!(decode_text(bytes.data()).unwrap().as_str(), text);
        assert!(decode_text(&[0]).is_none());
        assert!(decode_text(&[0, 0, 1, 0]).is_none());
        assert!(decode_text(&[0, 0, 0, 0]).is_none());
        assert!(decode_text(&[0, 0xd8, 0, 0]).is_none());
        assert!(decode_text(&vec![0; MAX_CLIPBOARD_TEXT * 2 + 4]).is_none());
    }
    #[test]
    fn disabling_or_replacing_permission_drops_inflight_response() {
        let s = state(true);
        let mut b = TextBackend::new(s.clone());
        s.lock().unwrap().pending_request = Some(1);
        {
            let mut s = s.lock().unwrap();
            s.permissions.clipboard_enabled = false;
            s.clear_text();
        }
        b.on_format_data_response(FormatDataResponse::new_unicode_string("synthetic"));
        assert!(s.lock().unwrap().received_text.is_none());
        {
            let mut s = s.lock().unwrap();
            s.permissions.clipboard_enabled = true;
            s.pending_request = Some(1);
            s.generation = 2;
        }
        b.on_format_data_response(FormatDataResponse::new_unicode_string("synthetic"));
        assert!(s.lock().unwrap().received_text.is_none());
    }
    #[test]
    fn unsolicited_response_and_nontext_formats_never_grant_data() {
        let s = state(true);
        let mut b = TextBackend::new(s.clone());
        b.on_format_data_response(FormatDataResponse::new_unicode_string("synthetic"));
        assert!(s.lock().unwrap().received_text.is_none());
        b.on_remote_copy(&[ClipboardFormat {
            id: ClipboardFormatId::new(2),
            name: None,
        }]);
        assert!(!s.lock().unwrap().remote_unicode);
        let s = state(false);
        let mut b = TextBackend::new(s.clone());
        b.on_format_data_request(FormatDataRequest {
            format: ClipboardFormatId::CF_UNICODETEXT,
        });
        assert!(s.lock().unwrap().local_text.is_none());
    }
    #[test]
    fn cancelled_wire_request_is_drained_before_reenable_or_remote_change() {
        let s = state(true);
        let mut b = TextBackend::new(s.clone());
        {
            let mut s = s.lock().unwrap();
            s.pending_request = Some(1);
            s.clear_text();
            s.generation = 2;
        }
        assert_eq!(s.lock().unwrap().pending_request, Some(0));
        b.on_format_data_response(FormatDataResponse::new_unicode_string("old-synthetic"));
        assert!(s.lock().unwrap().received_text.is_none());
        assert!(s.lock().unwrap().pending_request.is_none());
        {
            let mut s = s.lock().unwrap();
            s.pending_request = Some(2);
        }
        b.on_remote_copy(&[ClipboardFormat {
            id: ClipboardFormatId::CF_UNICODETEXT,
            name: None,
        }]);
        assert_eq!(s.lock().unwrap().pending_request, Some(0));
        b.on_format_data_response(FormatDataResponse::new_unicode_string("previous-clipboard"));
        assert!(s.lock().unwrap().received_text.is_none());
        {
            let mut s = s.lock().unwrap();
            s.pending_request = Some(2);
        }
        b.on_format_data_response(FormatDataResponse::new_unicode_string("new-synthetic"));
        assert_eq!(
            s.lock()
                .unwrap()
                .received_text
                .as_deref()
                .map(|v| v.as_str()),
            Some("new-synthetic")
        );
    }

    #[test]
    fn real_cliprdr_ack_hook_correlates_only_the_serialized_current_offer() {
        use ironrdp::svc::{SvcMessage, SvcProcessor};
        use tokio::sync::oneshot;
        let s = state(true);
        let mut channel = CliprdrClient::new(Box::new(TextBackend::new(s.clone())));
        let (done, mut receiver) = oneshot::channel();
        let id = {
            let mut state = s.lock().unwrap();
            assert!(state.offers.begin_advertisement(1, true, None, None));
            state.offers.begin_confirmation(1, done).unwrap()
        };
        let ack = SvcMessage::from(ClipboardPdu::FormatListResponse(FormatListResponse::Ok))
            .encode_unframed_pdu()
            .unwrap();
        channel.process(&ack).unwrap();
        assert!(s.lock().unwrap().ready);
        assert_eq!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );
        {
            let mut state = s.lock().unwrap();
            let text = Zeroizing::new("synthetic / Привет".to_owned());
            assert!(state
                .offers
                .begin_advertisement(1, true, Some(&text), Some(id)));
        }
        channel.process(&ack).unwrap();
        let ticket = receiver.try_recv().unwrap().unwrap();
        let mut state = s.lock().unwrap();
        state.offers.queue_paste(&ticket, 1, true).unwrap();
        state.offers.consume_paste(&ticket, 1, true).unwrap();
        assert!(state.offers.consume_paste(&ticket, 1, true).is_err());
    }

    #[test]
    fn release_all_after_committed_paste_preserves_the_delayed_wire_response_body() {
        use ironrdp::{
            session::ActiveStageBuilder,
            svc::{StaticChannelSet, SvcMessage, SvcProcessor},
        };
        use std::any::TypeId;
        use tokio::sync::oneshot;
        let s = state(true);
        let mut channel = CliprdrClient::new(Box::new(TextBackend::new(s.clone())));
        let ack = SvcMessage::from(ClipboardPdu::FormatListResponse(FormatListResponse::Ok))
            .encode_unframed_pdu()
            .unwrap();
        let (done, mut receiver) = oneshot::channel();
        let expected = Zeroizing::new("synthetic delayed / Привет".to_owned());
        {
            let mut state = s.lock().unwrap();
            let id = state.offers.begin_confirmation(1, done).unwrap();
            assert!(state
                .offers
                .begin_advertisement(1, true, Some(&expected), Some(id)));
        }
        channel.process(&ack).unwrap();
        {
            let ticket = receiver.try_recv().unwrap().unwrap();
            let mut state = s.lock().unwrap();
            state.offers.queue_paste(&ticket, 1, true).unwrap();
            state.offers.consume_paste(&ticket, 1, true).unwrap();
            state.offers.cancel_interaction(); // Exact manager ReleaseAll behavior.
        }
        let request = SvcMessage::from(ClipboardPdu::FormatDataRequest(FormatDataRequest {
            format: ClipboardFormatId::CF_UNICODETEXT,
        }))
        .encode_unframed_pdu()
        .unwrap();
        channel.process(&request).unwrap();
        let mut channels = StaticChannelSet::new();
        channels.insert(channel);
        channels.attach_channel_id(TypeId::of::<CliprdrClient>(), 1004);
        let mut active = ActiveStageBuilder {
            static_channels: channels,
            user_channel_id: 1002,
            io_channel_id: 1003,
            message_channel_id: None,
            share_id: 1,
            compression_type: None,
            enable_server_pointer: false,
            pointer_software_rendering: true,
        }
        .build();
        let frames = flush(&mut active, &s).unwrap();
        assert_eq!(frames.len(), 1);
        let expected_wire = Zeroizing::new(
            expected
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        assert!(frames[0]
            .1
            .windows(expected_wire.len())
            .any(|part| part == expected_wire.as_slice()));
        assert_eq!(s.lock().unwrap().clipboard_send_counts, [1, 0, 1, 0]);
        s.lock().unwrap().clear_text();
        assert!(s.lock().unwrap().offers.advertised_text.is_none());
    }
}

#[cfg(test)]
mod file_format_confusion_tests {
    use super::*;
    use crate::permissions::{RedirectState, SessionPermissions};
    use std::sync::{Arc, Mutex};
    #[test]
    fn file_list_cannot_impersonate_builtin_text_id() {
        let state = Arc::new(Mutex::new(RedirectState::new(
            SessionPermissions {
                clipboard_enabled: true,
                ..Default::default()
            },
            None,
        )));
        let mut backend = TextBackend::new(state.clone());
        backend.on_remote_copy(&[ClipboardFormat {
            id: ClipboardFormatId::CF_UNICODETEXT,
            name: Some(ClipboardFormatName::FILE_LIST),
        }]);
        assert!(!state.lock().unwrap().remote_unicode);
    }
}
