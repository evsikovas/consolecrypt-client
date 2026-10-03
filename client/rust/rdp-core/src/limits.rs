//! Guard stateful upstream accumulators before decoded PDUs enter ActiveStage.
use crate::RdpError;
use ironrdp::{
    core::{decode_cursor, ReadCursor},
    pdu::{
        self,
        fast_path::{FastPathHeader, FastPathUpdatePdu, Fragmentation},
        rdp::headers::CompressionFlags,
    },
};
use std::collections::HashMap;

const MAX_FASTPATH_ACCUMULATED: usize = 64 * 1024 * 1024;
const MAX_FASTPATH_FRAGMENTS: usize = 1024;
const MAX_CHANNEL_MESSAGE: usize = 1024 * 1024;
// Only the negotiated RDPDR channel needs room for a bounded file chunk plus
// its request headers. Clipboard, DVC and unassigned channels keep the old cap.
const MAX_DRIVE_CHANNEL_MESSAGE: usize = crate::directory::MAX_IO + 64 * 1024;
const MAX_STATIC_CHANNELS: usize = 16;
// ironrdp-bulk 0.1.1 uses a fixed 65,536-byte decompression output buffer.
const BULK_FRAGMENT_BOUND: usize = 65_536;

#[derive(Debug, Default)]
pub(crate) struct DecoderLimits {
    fragments: Option<(usize, usize)>,
    channels: HashMap<u16, ChannelMessage>,
    dvc_channel: Option<u16>,
    drive_channel: Option<u16>,
}
#[derive(Debug, Default)]
struct ChannelMessage {
    bytes: usize,
    prefix: [u8; 9],
    prefix_len: usize,
    dvc_header_checked: bool,
}

impl DecoderLimits {
    pub(crate) fn with_dvc_channel(dvc_channel: Option<u16>) -> Self {
        Self {
            dvc_channel,
            ..Default::default()
        }
    }
    pub(crate) fn with_drive_channel(mut self, drive_channel: Option<u16>) -> Self {
        // An ambiguous assignment must not relax the DVC guard.
        self.drive_channel = drive_channel.filter(|id| Some(*id) != self.dvc_channel);
        self
    }
    /// FastPath fragments cannot straddle a graphics activation: the retained codec
    /// must not carry an unbounded unfinished update past a freshly reset guard.
    /// Static/DVC channel accumulators and their guards continue across a resize.
    pub(crate) fn require_reactivation_boundary(&self) -> Result<(), RdpError> {
        if self.fragments.is_some() {
            Err(RdpError::Protocol)
        } else {
            Ok(())
        }
    }

    pub(crate) fn check(
        &mut self,
        action: pdu::Action,
        packet: &[u8],
        io_channel_id: u16,
        message_channel_id: Option<u16>,
    ) -> Result<(), RdpError> {
        match action {
            pdu::Action::FastPath => self.fastpath(packet),
            pdu::Action::X224 => {
                if let Ok(data) = pdu::mcs::decode_send_data_indication(packet) {
                    if data.channel_id != io_channel_id
                        && Some(data.channel_id) != message_channel_id
                    {
                        self.static_channel(data.channel_id, data.user_data)?;
                    }
                }
                Ok(())
            }
        }
    }
    fn fastpath(&mut self, packet: &[u8]) -> Result<(), RdpError> {
        let mut cursor = ReadCursor::new(packet);
        let header =
            decode_cursor::<FastPathHeader>(&mut cursor).map_err(|_| RdpError::Protocol)?;
        if !header.flags.is_empty() || header.data_length != cursor.len() {
            return Err(RdpError::Protocol);
        }
        while !cursor.is_empty() {
            let update = decode_cursor::<FastPathUpdatePdu<'_>>(&mut cursor)
                .map_err(|_| RdpError::Protocol)?;
            let bound = if update.compression_flags.is_some_and(|flags| {
                flags.intersects(CompressionFlags::COMPRESSED | CompressionFlags::FLUSHED)
            }) {
                BULK_FRAGMENT_BOUND
            } else {
                update.data.len()
            };
            self.fragment(update.fragmentation, bound)?;
        }
        Ok(())
    }
    fn fragment(&mut self, fragmentation: Fragmentation, bound: usize) -> Result<(), RdpError> {
        if bound > MAX_FASTPATH_ACCUMULATED {
            return Err(RdpError::FrameLimit);
        }
        match fragmentation {
            Fragmentation::Single => self.fragments = None,
            Fragmentation::First => self.fragments = Some((bound, 1)),
            Fragmentation::Next | Fragmentation::Last => {
                let (bytes, count) = self.fragments.ok_or(RdpError::Protocol)?;
                let bytes = bytes.checked_add(bound).ok_or(RdpError::FrameLimit)?;
                let count = count.checked_add(1).ok_or(RdpError::FrameLimit)?;
                if bytes > MAX_FASTPATH_ACCUMULATED || count > MAX_FASTPATH_FRAGMENTS {
                    return Err(RdpError::FrameLimit);
                }
                self.fragments = if fragmentation == Fragmentation::Last {
                    None
                } else {
                    Some((bytes, count))
                };
            }
        }
        Ok(())
    }
    fn static_channel(&mut self, id: u16, packet: &[u8]) -> Result<(), RdpError> {
        if packet.len() < 8 {
            return Err(RdpError::Protocol);
        }
        let declared =
            u32::from_le_bytes(packet[0..4].try_into().map_err(|_| RdpError::Protocol)?) as usize;
        let flags = u32::from_le_bytes(packet[4..8].try_into().map_err(|_| RdpError::Protocol)?);
        let maximum = if self.drive_channel == Some(id) {
            MAX_DRIVE_CHANNEL_MESSAGE
        } else {
            MAX_CHANNEL_MESSAGE
        };
        if declared > maximum {
            return Err(RdpError::FrameLimit);
        }
        if !self.channels.contains_key(&id) && self.channels.len() >= MAX_STATIC_CHANNELS {
            return Err(RdpError::FrameLimit);
        }
        let state = self.channels.entry(id).or_default();
        let data = &packet[8..];
        // Match upstream SVC accumulation: FIRST does not clear it; only LAST completes it.
        state.bytes = state
            .bytes
            .checked_add(data.len())
            .ok_or(RdpError::FrameLimit)?;
        if state.bytes > maximum {
            return Err(RdpError::FrameLimit);
        }
        let copied = data.len().min(state.prefix.len() - state.prefix_len);
        state.prefix[state.prefix_len..state.prefix_len + copied].copy_from_slice(&data[..copied]);
        state.prefix_len += copied;
        let last = flags & 2 != 0;
        if self.dvc_channel == Some(id) {
            state.check_dvc_header(last)?;
        }
        if last {
            self.channels.remove(&id);
        }
        Ok(())
    }
}

impl ChannelMessage {
    fn check_dvc_header(&mut self, last: bool) -> Result<(), RdpError> {
        if self.dvc_header_checked {
            return Ok(());
        }
        if self.prefix_len == 0 {
            return if last {
                Err(RdpError::Protocol)
            } else {
                Ok(())
            };
        }
        let header = self.prefix[0];
        if header >> 4 != 2 {
            self.dvc_header_checked = true;
            return Ok(());
        }
        fn field_size(bits: u8) -> Result<usize, RdpError> {
            match bits {
                0 => Ok(1),
                1 => Ok(2),
                2 => Ok(4),
                _ => Err(RdpError::Protocol),
            }
        }
        let id_size = field_size(header & 3)?;
        let len_size = field_size((header >> 2) & 3)?;
        let end = 1 + id_size + len_size;
        if self.prefix_len < end {
            return if last {
                Err(RdpError::Protocol)
            } else {
                Ok(())
            };
        }
        let mut encoded = [0; 4];
        encoded[..len_size].copy_from_slice(&self.prefix[1 + id_size..end]);
        if u32::from_le_bytes(encoded) as usize > MAX_CHANNEL_MESSAGE {
            return Err(RdpError::FrameLimit);
        }
        self.dvc_header_checked = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reactivation_rejects_unfinished_graphics_but_retains_static_channel_limits() {
        let mut limits = DecoderLimits::default();
        limits.fragment(Fragmentation::First, 1).unwrap();
        assert_eq!(
            limits.require_reactivation_boundary(),
            Err(RdpError::Protocol)
        );
        limits.fragment(Fragmentation::Last, 1).unwrap();
        let half = vec![0; MAX_CHANNEL_MESSAGE / 2];
        limits
            .static_channel(1004, &svc(MAX_CHANNEL_MESSAGE as u32, 1, &half))
            .unwrap();
        limits.require_reactivation_boundary().unwrap();
        limits
            .static_channel(1004, &svc(MAX_CHANNEL_MESSAGE as u32, 0, &half))
            .unwrap();
        assert_eq!(
            limits.static_channel(1004, &svc(MAX_CHANNEL_MESSAGE as u32, 0, &[0])),
            Err(RdpError::FrameLimit)
        );
    }
    #[test]
    fn encoded_compressed_fragment_headers_exhaust_budget_before_decoder() {
        use ironrdp::{
            core::encode_vec,
            pdu::{
                fast_path::{EncryptionFlags, UpdateCode},
                rdp::client_info::CompressionType,
            },
        };
        fn packet(fragmentation: Fragmentation) -> Vec<u8> {
            let body = encode_vec(&FastPathUpdatePdu {
                fragmentation,
                update_code: UpdateCode::Bitmap,
                compression_flags: Some(CompressionFlags::COMPRESSED),
                compression_type: Some(CompressionType::Rdp61),
                data: &[],
            })
            .unwrap();
            let mut out =
                encode_vec(&FastPathHeader::new(EncryptionFlags::empty(), body.len())).unwrap();
            out.extend_from_slice(&body);
            out
        }
        let first = packet(Fragmentation::First);
        let next = packet(Fragmentation::Next);
        let mut limits = DecoderLimits::default();
        limits
            .check(pdu::Action::FastPath, &first, 1003, None)
            .unwrap();
        for _ in 1..MAX_FASTPATH_FRAGMENTS {
            limits
                .check(pdu::Action::FastPath, &next, 1003, None)
                .unwrap();
        }
        assert_eq!(
            limits.check(pdu::Action::FastPath, &next, 1003, None),
            Err(RdpError::FrameLimit)
        );
    }
    #[test]
    fn unterminated_fastpath_fragments_are_bounded_before_upstream_append() {
        let mut limits = DecoderLimits::default();
        limits
            .fragment(Fragmentation::First, BULK_FRAGMENT_BOUND)
            .unwrap();
        for _ in 1..MAX_FASTPATH_FRAGMENTS {
            limits
                .fragment(Fragmentation::Next, BULK_FRAGMENT_BOUND)
                .unwrap();
        }
        assert_eq!(
            limits.fragment(Fragmentation::Next, BULK_FRAGMENT_BOUND),
            Err(RdpError::FrameLimit)
        );
    }
    #[test]
    fn completed_fragments_reset_and_unanchored_fragments_reject() {
        let mut limits = DecoderLimits::default();
        assert_eq!(
            limits.fragment(Fragmentation::Next, 2),
            Err(RdpError::Protocol)
        );
        limits.fragment(Fragmentation::First, 2).unwrap();
        limits.fragment(Fragmentation::Last, 2).unwrap();
        assert!(limits.fragments.is_none());
        limits.fragment(Fragmentation::First, 2).unwrap();
        limits.fragment(Fragmentation::Single, 2).unwrap();
        assert!(limits.fragments.is_none());
    }
    fn svc(length: u32, flags: u32, data: &[u8]) -> Vec<u8> {
        let mut out = length.to_le_bytes().to_vec();
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(data);
        out
    }
    #[test]
    fn static_chunks_do_not_reset_budget_with_forged_first_flags() {
        let mut limits = DecoderLimits::default();
        let half = vec![0; MAX_CHANNEL_MESSAGE / 2];
        limits
            .static_channel(1004, &svc(MAX_CHANNEL_MESSAGE as u32, 1, &half))
            .unwrap();
        limits
            .static_channel(1004, &svc(MAX_CHANNEL_MESSAGE as u32, 1, &half))
            .unwrap();
        assert_eq!(
            limits.static_channel(1004, &svc(MAX_CHANNEL_MESSAGE as u32, 1, &[0])),
            Err(RdpError::FrameLimit)
        );
    }
    #[test]
    fn only_negotiated_drive_channel_accepts_one_mib_file_chunk_with_headers() {
        let mut limits = DecoderLimits::with_dvc_channel(Some(1004)).with_drive_channel(Some(1005));
        let size = crate::directory::MAX_IO + 56;
        let half = vec![0; size / 2];
        let tail = vec![0; size - half.len()];
        limits
            .static_channel(1005, &svc(size as u32, 1, &half))
            .unwrap();
        limits
            .static_channel(1005, &svc(size as u32, 2, &tail))
            .unwrap();
        for other in [1004, 1006] {
            assert_eq!(
                limits.static_channel(other, &svc(size as u32, 3, &[0])),
                Err(RdpError::FrameLimit)
            );
        }
        assert_eq!(
            limits.static_channel(1005, &svc((MAX_DRIVE_CHANNEL_MESSAGE + 1) as u32, 3, &[0])),
            Err(RdpError::FrameLimit)
        );
        let mut unnegotiated = DecoderLimits::default();
        assert_eq!(
            unnegotiated.static_channel(1005, &svc(size as u32, 3, &[0])),
            Err(RdpError::FrameLimit)
        );
        let mut ambiguous =
            DecoderLimits::with_dvc_channel(Some(1005)).with_drive_channel(Some(1005));
        assert_eq!(
            ambiguous.static_channel(1005, &svc(size as u32, 3, &[0])),
            Err(RdpError::FrameLimit)
        );
    }
    #[test]
    fn drive_channel_fragments_remain_bounded_despite_forged_first_or_length() {
        let mut limits = DecoderLimits::default().with_drive_channel(Some(1005));
        let half = vec![0; MAX_DRIVE_CHANNEL_MESSAGE / 2];
        // Match upstream behavior: FIRST never clears unfinished accumulation.
        for _ in 0..2 {
            limits.static_channel(1005, &svc(1, 1, &half)).unwrap();
        }
        assert_eq!(
            limits.static_channel(1005, &svc(1, 1, &[0])),
            Err(RdpError::FrameLimit)
        );
    }
    #[test]
    fn dynamic_declared_length_is_bounded_even_if_header_spans_static_chunks() {
        // DYNVC_DATA_FIRST: u8 channel ID, u32 total message length.
        let mut limits = DecoderLimits::with_dvc_channel(Some(1004));
        let mut data = vec![0x28, 1];
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        limits.static_channel(1004, &svc(6, 1, &data[..2])).unwrap();
        assert_eq!(
            limits.static_channel(1004, &svc(6, 2, &data[2..])),
            Err(RdpError::FrameLimit)
        );
    }
    #[test]
    fn concurrent_static_channel_guard_states_are_bounded() {
        let mut limits = DecoderLimits::default();
        for id in 0..MAX_STATIC_CHANNELS as u16 {
            limits.static_channel(id, &svc(1, 1, &[0])).unwrap();
        }
        assert_eq!(
            limits.static_channel(1004, &svc(1, 1, &[0])),
            Err(RdpError::FrameLimit)
        );
    }
    #[test]
    fn clipboard_and_drive_bytes_are_not_misinterpreted_as_dvc_headers() {
        let mut limits = DecoderLimits::with_dvc_channel(Some(1004));
        let mut payload = vec![0x28, 1];
        payload.extend_from_slice(&u32::MAX.to_le_bytes());
        limits.static_channel(1005, &svc(6, 3, &payload)).unwrap();
        assert_eq!(
            limits.static_channel(1004, &svc(6, 3, &payload)),
            Err(RdpError::FrameLimit)
        );
    }
}
