//! One bounded, cancel-safe writer driven by the same actor as the RDP decoder.
//! Only complete wire PDUs may be interleaved; an SVC message reserves its channel
//! through LAST. No task or unbounded message queue owns plaintext here.
use super::{LARGE_WRITE_BUDGET, LARGE_WRITE_THRESHOLD, MAX_WRITE_BATCH, WRITE_TIMEOUT};
use crate::{clipboard_offers::PasteFence, permissions::SharedRedirect, RdpError};
use ironrdp::{core::decode, input::Database, pdu};
use std::collections::HashMap;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::oneshot,
    time::Instant,
};
use zeroize::Zeroizing;

// A 64KiB UTF-8 clipboard text can occupy 128KiB UTF-16 plus SVC framing.
pub(super) const INTERACTIVE_RESERVE: usize = 192 * 1024;
const MAX_BATCHES: usize = 32;
const MAX_FRAMES: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WireFrame {
    end: usize,
    channel: Option<u16>,
    first: bool,
    last: bool,
}

pub(super) enum Purpose {
    Protocol,
    Drive {
        drive_id: u32,
        denied: Zeroizing<Vec<u8>>,
    },
    Input {
        pdus: u64,
        database: Option<Database>,
    },
    Clipboard {
        generation: u64,
        sequence: u64,
        advertisement: Option<crate::clipboard::Advertisement>,
        request: bool,
    },
    Paste {
        fence: PasteFence,
        done: oneshot::Sender<Result<(), RdpError>>,
        pdus: u64,
        database: Option<Database>,
    },
}

impl Purpose {
    fn authorized(&self, s: &crate::permissions::RedirectState) -> bool {
        match self {
            Self::Clipboard {
                generation,
                sequence,
                advertisement,
                request,
            } => {
                !s.closed
                    && s.generation == *generation
                    && (!request
                        || (s.enabled()
                            && s.ready
                            && s.remote_unicode
                            && s.pending_request.is_none()
                            && s.prepared_request == Some((*generation, *sequence))))
                    && if advertisement.is_some() {
                        s.offers.sequence_matches(*sequence)
                    } else {
                        s.offers.content_matches(*sequence)
                    }
            }
            Self::Drive { drive_id, .. } => !s.closed && s.drive_id == *drive_id,
            Self::Paste { fence, done, .. } => {
                !done.is_closed()
                    && s.offers
                        .dispatch_authorized(*fence, s.generation, s.enabled())
            }
            _ => true,
        }
    }
}

struct Batch {
    id: u64,
    bytes: Zeroizing<Vec<u8>>,
    frames: Vec<WireFrame>,
    frame: usize,
    offset: usize,
    flush: bool,
    started: Option<Instant>,
    progress: Option<Instant>,
    urgent: bool,
    purpose: Purpose,
    advertisement_started: bool,
}
impl Batch {
    fn deadline(&self) -> Option<Instant> {
        let started = self.started?;
        let absolute = started
            + if self.bytes.len() > LARGE_WRITE_THRESHOLD {
                LARGE_WRITE_BUDGET
            } else {
                WRITE_TIMEOUT
            };
        Some(absolute.min(self.progress.unwrap_or(started) + WRITE_TIMEOUT))
    }
}

pub(super) struct Completed {
    pub purpose: Purpose,
    pub accepted: usize,
}

pub(super) struct Writer<W> {
    stream: W,
    batches: Vec<Batch>,
    bytes: usize,
    next_id: u64,
    selected: Option<u64>,
    // A reservation belongs to the original message, not a temporary frame.
    channels: HashMap<u16, u64>,
    urgent_run: usize,
    io_channel: u16,
    message_channel: Option<u16>,
}

impl<W: AsyncWrite + Unpin> Writer<W> {
    pub fn new(stream: W, io_channel: u16, message_channel: Option<u16>) -> Self {
        Self {
            stream,
            batches: Vec::new(),
            bytes: 0,
            next_id: 1,
            selected: None,
            channels: HashMap::new(),
            urgent_run: 0,
            io_channel,
            message_channel,
        }
    }
    pub fn remaining(&self) -> usize {
        MAX_WRITE_BATCH - self.bytes
    }
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }
    pub fn has_clipboard(&self) -> bool {
        self.batches
            .iter()
            .any(|b| matches!(b.purpose, Purpose::Clipboard { .. }))
    }
    pub fn at_boundary(&self) -> bool {
        self.selected.is_none()
    }
    pub fn channel_available(&self, channel: Option<u16>) -> bool {
        channel.is_none_or(|channel| !self.channels.contains_key(&channel))
    }
    pub fn can_generate_input(&self) -> bool {
        self.at_boundary()
            && self.remaining() >= INTERACTIVE_RESERVE
            && !self
                .batches
                .iter()
                .any(|b| matches!(b.purpose, Purpose::Input { .. } | Purpose::Paste { .. }))
    }
    pub fn enqueue(&mut self, bytes: Zeroizing<Vec<u8>>, purpose: Purpose) -> Result<(), RdpError> {
        if bytes.is_empty() {
            return Ok(());
        }
        if self.batches.len() >= MAX_BATCHES || bytes.len() > self.remaining() {
            return Err(RdpError::Protocol);
        }
        let frames = frame_boundaries(&bytes, self.io_channel, self.message_channel)?;
        let urgent = bytes.len() <= LARGE_WRITE_THRESHOLD
            || matches!(
                purpose,
                Purpose::Input { .. } | Purpose::Paste { .. } | Purpose::Clipboard { .. }
            );
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or(RdpError::Protocol)?;
        self.bytes += bytes.len();
        self.batches.push(Batch {
            id,
            bytes,
            frames,
            frame: 0,
            offset: 0,
            flush: false,
            started: None,
            progress: None,
            urgent,
            purpose,
            advertisement_started: false,
        });
        Ok(())
    }
    fn eligible(&self, b: &Batch) -> bool {
        b.frames[b.frame].channel.is_none_or(|channel| {
            self.channels.get(&channel).is_none_or(|id| *id == b.id)
                && !self.batches.iter().any(|older| {
                    older.id < b.id && older.frames[older.frame].channel == Some(channel)
                })
        })
    }
    fn choose(&mut self) -> Option<usize> {
        if let Some(id) = self.selected {
            return self.batches.iter().position(|b| b.id == id);
        }
        let earliest = self
            .batches
            .iter()
            .enumerate()
            .filter(|(_, b)| self.eligible(b))
            .min_by_key(|(_, b)| b.id)
            .map(|(i, _)| i)?;
        let urgent = self
            .batches
            .iter()
            .enumerate()
            .filter(|(_, b)| b.urgent && self.eligible(b))
            .min_by_key(|(_, b)| b.id)
            .map(|(i, _)| i);
        let bulk = self
            .batches
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.urgent && self.eligible(b))
            .min_by_key(|(_, b)| b.id)
            .map(|(i, _)| i);
        let chosen = if self.urgent_run >= 8 {
            bulk.unwrap_or(earliest)
        } else {
            urgent.unwrap_or(earliest)
        };
        if self.batches[chosen].urgent {
            self.urgent_run += 1;
        } else {
            self.urgent_run = 0;
        }
        self.selected = Some(self.batches[chosen].id);
        Some(chosen)
    }
    pub fn next_deadline(&self) -> Option<Instant> {
        self.batches.iter().filter_map(Batch::deadline).min()
    }
    pub fn check_deadlines(&self) -> Result<(), RdpError> {
        if self
            .next_deadline()
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            Err(RdpError::Timeout)
        } else {
            Ok(())
        }
    }
    fn cancel_unsent(
        &mut self,
        index: usize,
        redirects: &SharedRedirect,
    ) -> Result<Option<Completed>, RdpError> {
        self.selected = None;
        if let Purpose::Drive { denied, .. } = &mut self.batches[index].purpose {
            let denied = std::mem::take(denied);
            let batch = &mut self.batches[index];
            self.bytes -= batch.bytes.len();
            batch.bytes = denied;
            self.bytes += batch.bytes.len();
            batch.frames = frame_boundaries(&batch.bytes, self.io_channel, self.message_channel)?;
            batch.purpose = Purpose::Protocol;
            batch.urgent = true;
            return Ok(None);
        }
        let batch = self.batches.remove(index);
        self.bytes -= batch.bytes.len();
        if let Purpose::Clipboard {
            generation,
            sequence,
            request: true,
            ..
        } = &batch.purpose
        {
            let mut state = redirects.lock().map_err(|_| RdpError::Connection)?;
            if state.prepared_request == Some((*generation, *sequence)) {
                state.prepared_request = None;
            }
        }
        if batch.advertisement_started {
            redirects
                .lock()
                .map_err(|_| RdpError::Connection)?
                .offers
                .abandon_unsent_advertisement();
        }
        Ok(Some(Completed {
            purpose: batch.purpose,
            accepted: 0,
        }))
    }
    // Selection/offset survives cancellation of this future by an incoming PDU.
    // A canceled write() future cannot create an untracked partial wire frame.
    pub async fn progress(
        &mut self,
        redirects: &SharedRedirect,
        database: &mut Database,
    ) -> Result<Option<Completed>, RdpError> {
        self.check_deadlines()?;
        let Some(index) = self.choose() else {
            std::future::pending::<()>().await;
            unreachable!()
        };
        if self.batches[index].offset == 0
            && !self.batches[index]
                .purpose
                .authorized(&*redirects.lock().map_err(|_| RdpError::Connection)?)
        {
            return self.cancel_unsent(index, redirects);
        }
        let batch = &mut self.batches[index];
        let now = Instant::now();
        batch.started.get_or_insert(now);
        batch.progress.get_or_insert(now);
        let frame = batch.frames[batch.frame];
        let deadline = batch.deadline().expect("batch started");
        if batch.flush {
            tokio::time::timeout_at(deadline, self.stream.flush())
                .await
                .map_err(|_| RdpError::Timeout)?
                .map_err(|_| RdpError::Connection)?;
            batch.flush = false;
            if frame.last {
                if let Some(channel) = frame.channel {
                    self.channels.remove(&channel);
                }
            }
            batch.frame += 1;
            self.selected = None;
            if batch.frame == batch.frames.len() {
                let batch = self.batches.remove(index);
                self.bytes -= batch.bytes.len();
                return Ok(Some(Completed {
                    accepted: batch.offset,
                    purpose: batch.purpose,
                }));
            }
        } else {
            let writing = std::future::poll_fn(|cx| {
                let mut guard = match redirects.lock() {
                    Ok(guard) => guard,
                    Err(_) => {
                        return std::task::Poll::Ready(Err(std::io::Error::other(
                            "redirect state unavailable",
                        )))
                    }
                };
                if batch.offset == 0 {
                    if !batch.purpose.authorized(&guard) {
                        return std::task::Poll::Ready(Ok(None));
                    }
                    if let Purpose::Clipboard {
                        generation,
                        advertisement: Some(advertisement),
                        ..
                    } = &batch.purpose
                    {
                        if !guard.offers.can_begin_advertisement(
                            *generation,
                            advertisement.enabled,
                            advertisement.confirmed_id,
                        ) {
                            return std::task::Poll::Ready(Ok(None));
                        }
                    }
                }
                let written = std::pin::Pin::new(&mut self.stream)
                    .poll_write(cx, &batch.bytes[batch.offset..frame.end]);
                if let std::task::Poll::Ready(Ok(n)) = written {
                    if n > 0 && batch.offset == 0 {
                        if let Purpose::Clipboard {
                            generation,
                            request: true,
                            ..
                        } = &batch.purpose
                        {
                            guard.prepared_request = None;
                            guard.pending_request = Some(*generation);
                            guard.clipboard_counts[1] = guard.clipboard_counts[1].saturating_add(1);
                        }
                        if let Purpose::Clipboard {
                            generation,
                            advertisement: Some(advertisement),
                            ..
                        } = &batch.purpose
                        {
                            if !guard.offers.begin_advertisement(
                                *generation,
                                advertisement.enabled,
                                advertisement.text.as_ref(),
                                advertisement.confirmed_id,
                            ) {
                                return std::task::Poll::Ready(Err(std::io::Error::other(
                                    "clipboard dispatch state changed",
                                )));
                            }
                            batch.advertisement_started = true;
                        }
                    }
                }
                written.map(|result| result.map(Some))
            });
            let n = tokio::time::timeout_at(deadline, writing)
                .await
                .map_err(|_| RdpError::Timeout)?
                .map_err(|_| RdpError::Connection)?;
            let Some(n) = n else {
                return self.cancel_unsent(index, redirects);
            };
            if n == 0 {
                return Err(RdpError::Connection);
            }
            if batch.offset == 0 {
                match &mut batch.purpose {
                    Purpose::Input {
                        database: pending, ..
                    }
                    | Purpose::Paste {
                        database: pending, ..
                    } => {
                        if let Some(pending) = pending.take() {
                            *database = pending;
                        }
                    }
                    _ => {}
                }
            }
            if frame.first {
                if let Some(channel) = frame.channel {
                    self.channels.insert(channel, batch.id);
                }
            }
            batch.offset += n;
            batch.progress = Some(Instant::now());
            if batch.offset == frame.end {
                batch.flush = true;
            }
        }
        Ok(None)
    }
}

fn frame_boundaries(
    bytes: &[u8],
    io: u16,
    message: Option<u16>,
) -> Result<Vec<WireFrame>, RdpError> {
    let mut offset = 0;
    let mut frames = Vec::new();
    while offset < bytes.len() {
        if frames.len() >= MAX_FRAMES {
            return Err(RdpError::Protocol);
        }
        let info = pdu::find_size(&bytes[offset..])
            .map_err(|_| RdpError::Protocol)?
            .ok_or(RdpError::Protocol)?;
        if info.length == 0 || info.length > bytes.len() - offset {
            return Err(RdpError::Protocol);
        }
        let mut frame = WireFrame {
            end: offset + info.length,
            channel: None,
            first: true,
            last: true,
        };
        if info.action == pdu::Action::X224 {
            let packet = &bytes[offset..frame.end];
            let mcs = decode::<pdu::x224::X224<pdu::mcs::McsMessage<'_>>>(packet)
                .map_err(|_| RdpError::Protocol)?;
            if let pdu::mcs::McsMessage::SendDataRequest(data) = mcs.0 {
                frame.channel = Some(data.channel_id);
                if data.channel_id != io && Some(data.channel_id) != message {
                    if data.user_data.len() < 8 {
                        return Err(RdpError::Protocol);
                    }
                    let flags = u32::from_le_bytes(data.user_data[4..8].try_into().unwrap());
                    frame.first = flags & 1 != 0;
                    frame.last = flags & 2 != 0;
                }
            }
        }
        frames.push(frame);
        offset = frame.end;
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{RedirectState, SessionPermissions};
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll},
    };
    use tokio::{
        io::AsyncReadExt,
        time::{Duration, Sleep},
    };
    fn redirects() -> SharedRedirect {
        Arc::new(Mutex::new(RedirectState::new(
            SessionPermissions::default(),
            None,
        )))
    }
    fn fastpath(size: usize) -> Zeroizing<Vec<u8>> {
        let mut all = Vec::new();
        while all.len() < size {
            let n = (size - all.len()).min(30000);
            let mut p = ironrdp::core::encode_vec(&pdu::fast_path::FastPathHeader::new(
                pdu::fast_path::EncryptionFlags::empty(),
                n,
            ))
            .unwrap();
            p.resize(p.len() + n, 0);
            all.extend(p);
        }
        Zeroizing::new(all)
    }
    struct Paced {
        delay: Duration,
        chunk: usize,
        timer: Option<Pin<Box<Sleep>>>,
        accepted: Arc<Mutex<usize>>,
        flush_stalls: bool,
    }
    impl AsyncWrite for Paced {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            if self.timer.is_none() {
                self.timer = Some(Box::pin(tokio::time::sleep(self.delay)));
            }
            if self.timer.as_mut().unwrap().as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.timer = None;
            let n = self.chunk.min(bytes.len());
            *self.accepted.lock().unwrap() += n;
            Poll::Ready(Ok(n))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            if self.flush_stalls {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    async fn drain<W: AsyncWrite + Unpin>(writer: &mut Writer<W>) -> Result<(), RdpError> {
        let mut db = Database::new();
        while !writer.is_empty() {
            writer.progress(&redirects(), &mut db).await?;
        }
        Ok(())
    }
    #[tokio::test(start_paused = true)]
    async fn paced_large_frames_keep_original_absolute_and_idle_budgets() {
        let accepted = Arc::new(Mutex::new(0));
        let io = Paced {
            delay: Duration::from_secs(5),
            chunk: 65536,
            timer: None,
            accepted: accepted.clone(),
            flush_stalls: false,
        };
        let mut w = Writer::new(io, 1003, None);
        w.enqueue(fastpath(100000), Purpose::Protocol).unwrap();
        let started = Instant::now();
        drain(&mut w).await.unwrap();
        assert_eq!(started.elapsed(), Duration::from_secs(20));
        assert!(*accepted.lock().unwrap() > 100000);
    }
    #[tokio::test(start_paused = true)]
    async fn stalled_and_hostile_trickle_are_bounded() {
        for (delay, chunk, expected) in [
            (Duration::from_secs(11), 1, 10),
            (Duration::from_secs(9), 1, 120),
        ] {
            let io = Paced {
                delay,
                chunk,
                timer: None,
                accepted: Arc::new(Mutex::new(0)),
                flush_stalls: false,
            };
            let mut w = Writer::new(io, 1003, None);
            w.enqueue(fastpath(100000), Purpose::Protocol).unwrap();
            let start = Instant::now();
            assert_eq!(drain(&mut w).await, Err(RdpError::Timeout));
            assert_eq!(start.elapsed(), Duration::from_secs(expected));
        }
    }
    #[tokio::test(start_paused = true)]
    async fn small_batch_and_pending_flush_keep_ten_second_limit() {
        for flush_stalls in [false, true] {
            let io = Paced {
                delay: Duration::from_secs(5),
                chunk: 1,
                timer: None,
                accepted: Arc::new(Mutex::new(0)),
                flush_stalls,
            };
            let mut w = Writer::new(io, 1003, None);
            w.enqueue(fastpath(100), Purpose::Protocol).unwrap();
            let start = Instant::now();
            assert_eq!(drain(&mut w).await, Err(RdpError::Timeout));
            assert_eq!(start.elapsed(), Duration::from_secs(10));
        }
        let io = Paced {
            delay: Duration::from_secs(1),
            chunk: 65536,
            timer: None,
            accepted: Arc::new(Mutex::new(0)),
            flush_stalls: true,
        };
        let mut w = Writer::new(io, 1003, None);
        w.enqueue(fastpath(100000), Purpose::Protocol).unwrap();
        let start = Instant::now();
        assert_eq!(drain(&mut w).await, Err(RdpError::Timeout));
        assert_eq!(start.elapsed(), Duration::from_secs(11));
    }
    #[test]
    fn total_output_and_frame_count_are_bounded() {
        let (stream, _) = tokio::io::duplex(1);
        let mut w = Writer::new(stream, 1003, None);
        w.enqueue(fastpath(1500000), Purpose::Protocol).unwrap();
        assert!(w.enqueue(fastpath(1000000), Purpose::Protocol).is_err());
        assert!(w.bytes <= MAX_WRITE_BATCH);
        assert!(frame_boundaries(&[0], 1003, None).is_err());
    }
    #[tokio::test]
    async fn same_channel_message_cannot_be_overtaken_before_last() {
        use ironrdp::svc::SvcMessage;
        let a = ironrdp::svc::client_encode_svc_messages(
            vec![SvcMessage::from(vec![1u8; 100000])],
            1004,
            1002,
        )
        .unwrap();
        let b = ironrdp::svc::client_encode_svc_messages(
            vec![SvcMessage::from(vec![2u8; 100])],
            1004,
            1002,
        )
        .unwrap();
        let (stream, mut peer) = tokio::io::duplex(64);
        let mut w = Writer::new(stream, 1003, None);
        w.enqueue(Zeroizing::new(a.clone()), Purpose::Protocol)
            .unwrap();
        w.enqueue(Zeroizing::new(b.clone()), Purpose::Protocol)
            .unwrap();
        let read = tokio::spawn(async move {
            let mut received = vec![0; a.len() + b.len()];
            peer.read_exact(&mut received).await.unwrap();
            assert_eq!(received, [a, b].concat());
        });
        drain(&mut w).await.unwrap();
        read.await.unwrap();
    }
    #[tokio::test]
    async fn cancelled_unstarted_paste_does_not_mutate_input_database() {
        let (stream, _) = tokio::io::duplex(64);
        let shared = redirects();
        let (done, mut ack) = oneshot::channel();
        let id = shared
            .lock()
            .unwrap()
            .offers
            .begin_confirmation(1, done)
            .unwrap();
        shared
            .lock()
            .unwrap()
            .offers
            .begin_advertisement(1, true, None, Some(id));
        shared.lock().unwrap().offers.acknowledge(true, 1, true);
        let ticket = ack.try_recv().unwrap().unwrap();
        shared
            .lock()
            .unwrap()
            .offers
            .queue_paste(&ticket, 1, true)
            .unwrap();
        let fence = shared
            .lock()
            .unwrap()
            .offers
            .consume_paste(&ticket, 1, true)
            .unwrap();
        let (done, mut result) = oneshot::channel();
        let mut w = Writer::new(stream, 1003, None);
        w.enqueue(
            fastpath(10),
            Purpose::Paste {
                fence,
                done,
                pdus: 1,
                database: Some(Database::new()),
            },
        )
        .unwrap();
        let ctrl = ironrdp::input::Scancode::from_u8(false, 0x1d);
        let mut db = Database::new();
        let _ = db.apply([ironrdp::input::Operation::KeyPressed(ctrl)]);
        shared.lock().unwrap().offers.cancel_interaction();
        let finished = w.progress(&shared, &mut db).await.unwrap().unwrap();
        assert_eq!(finished.accepted, 0);
        assert!(db.is_key_pressed(ctrl));
        assert!(result.try_recv().is_err());
    }
    struct GateWriter {
        writable: Arc<std::sync::atomic::AtomicBool>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
        accepted: Arc<Mutex<Vec<u8>>>,
    }
    impl AsyncWrite for GateWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if !self.writable.load(std::sync::atomic::Ordering::Acquire) {
                return Poll::Pending;
            }
            self.accepted.lock().unwrap().extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    fn gate() -> (
        GateWriter,
        Arc<std::sync::atomic::AtomicBool>,
        Arc<Mutex<Vec<u8>>>,
    ) {
        let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let accepted = Arc::new(Mutex::new(Vec::new()));
        (
            GateWriter {
                writable: ready.clone(),
                calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                accepted: accepted.clone(),
            },
            ready,
            accepted,
        )
    }
    #[tokio::test]
    async fn pending_first_write_rechecks_paste_and_clipboard_before_accepting_bytes() {
        for paste in [false, true] {
            let (io, ready, accepted) = gate();
            let mut w = Writer::new(io, 1003, None);
            let shared = redirects();
            shared.lock().unwrap().permissions.clipboard_enabled = true;
            let mut db = Database::new();
            let ctrl = ironrdp::input::Scancode::from_u8(false, 0x1d);
            let _ = db.apply([ironrdp::input::Operation::KeyPressed(ctrl)]);
            let mut held_receiver = None;
            let purpose = if paste {
                let (done, mut result) = oneshot::channel();
                let id = shared
                    .lock()
                    .unwrap()
                    .offers
                    .begin_confirmation(1, done)
                    .unwrap();
                shared
                    .lock()
                    .unwrap()
                    .offers
                    .begin_advertisement(1, true, None, Some(id));
                shared.lock().unwrap().offers.acknowledge(true, 1, true);
                let ticket = result.try_recv().unwrap().unwrap();
                shared
                    .lock()
                    .unwrap()
                    .offers
                    .queue_paste(&ticket, 1, true)
                    .unwrap();
                let fence = shared
                    .lock()
                    .unwrap()
                    .offers
                    .consume_paste(&ticket, 1, true)
                    .unwrap();
                let (done, _receiver) = oneshot::channel();
                // Keep caller alive while the pending write is polled.
                held_receiver = Some(_receiver);
                Purpose::Paste {
                    fence,
                    done,
                    pdus: 1,
                    database: Some(Database::new()),
                }
            } else {
                let sequence = shared.lock().unwrap().offers.sequence();
                Purpose::Clipboard {
                    generation: 1,
                    sequence,
                    advertisement: None,
                    request: false,
                }
            };
            w.enqueue(fastpath(10), purpose).unwrap();
            let mut future = Box::pin(w.progress(&shared, &mut db));
            std::future::poll_fn(|cx| {
                assert!(future.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            if paste {
                shared.lock().unwrap().offers.cancel_interaction();
            } else {
                shared.lock().unwrap().offers.content_changed();
            }
            ready.store(true, std::sync::atomic::Ordering::Release);
            let completed = future.await.unwrap().unwrap();
            assert_eq!(completed.accepted, 0);
            assert!(accepted.lock().unwrap().is_empty());
            assert!(db.is_key_pressed(ctrl));
            drop(held_receiver);
        }
    }
    #[tokio::test]
    async fn cancel_confirmed_advertisement_after_preparation_keeps_session_usable() {
        let (io, ready, accepted) = gate();
        ready.store(true, std::sync::atomic::Ordering::Release);
        let mut w = Writer::new(io, 1003, None);
        let shared = redirects();
        let (done, _result) = oneshot::channel();
        let id = shared
            .lock()
            .unwrap()
            .offers
            .begin_confirmation(1, done)
            .unwrap();
        let sequence = shared.lock().unwrap().offers.sequence();
        w.enqueue(
            fastpath(10),
            Purpose::Clipboard {
                generation: 1,
                sequence,
                request: false,
                advertisement: Some(crate::clipboard::Advertisement {
                    confirmed_id: Some(id),
                    enabled: true,
                    text: Some(Zeroizing::new("runtime synthetic".into())),
                }),
            },
        )
        .unwrap();
        shared.lock().unwrap().offers.cancel_interaction();
        let mut db = Database::new();
        assert_eq!(
            w.progress(&shared, &mut db)
                .await
                .unwrap()
                .unwrap()
                .accepted,
            0
        );
        assert!(accepted.lock().unwrap().is_empty());
        w.enqueue(fastpath(10), Purpose::Protocol).unwrap();
        drain(&mut w).await.unwrap();
        assert!(!accepted.lock().unwrap().is_empty());
        assert!(!shared.lock().unwrap().offers.in_flight());
    }
    #[tokio::test]
    async fn pending_unsent_drive_read_revoke_sends_denial_instead_of_old_plaintext() {
        let (reply, payload) = super::super::actor_tests::bulk_reply();
        let purpose = super::super::drive_response_purpose(&reply, Some(1004), true).unwrap();
        assert!(matches!(purpose, Purpose::Drive { .. }));
        let (io, ready, accepted) = gate();
        let mut w = Writer::new(io, 1003, None);
        w.enqueue(Zeroizing::new(reply), purpose).unwrap();
        let shared = redirects();
        let mut db = Database::new();
        let mut future = Box::pin(w.progress(&shared, &mut db));
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        shared.lock().unwrap().drive_id = 2;
        ready.store(true, std::sync::atomic::Ordering::Release);
        assert!(future.await.unwrap().is_none());
        drain(&mut w).await.unwrap();
        let bytes = accepted.lock().unwrap();
        assert!(bytes.len() < 100);
        let data = decode::<pdu::x224::X224<pdu::mcs::SendDataRequest<'_>>>(&bytes)
            .unwrap()
            .0;
        assert_eq!(
            u32::from_le_bytes(data.user_data[20..24].try_into().unwrap()),
            0xc0000022
        );
        assert_eq!(
            u32::from_le_bytes(data.user_data[24..28].try_into().unwrap()),
            0
        );
        assert!(bytes.len() < payload.len());
    }
    #[tokio::test]
    async fn release_all_preserves_queued_committed_body_but_remote_copy_revokes_it() {
        for remote_copy in [false, true] {
            let (io, ready, accepted) = gate();
            let mut w = Writer::new(io, 1003, None);
            let shared = redirects();
            let sequence = shared.lock().unwrap().offers.content_sequence();
            w.enqueue(
                fastpath(10),
                Purpose::Clipboard {
                    generation: 1,
                    sequence,
                    advertisement: None,
                    request: false,
                },
            )
            .unwrap();
            let mut db = Database::new();
            let mut future = Box::pin(w.progress(&shared, &mut db));
            std::future::poll_fn(|cx| {
                assert!(future.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            if remote_copy {
                shared.lock().unwrap().offers.content_changed();
            } else {
                shared.lock().unwrap().offers.cancel_interaction();
            }
            ready.store(true, std::sync::atomic::Ordering::Release);
            let completed = future.await.unwrap();
            if remote_copy {
                assert_eq!(completed.unwrap().accepted, 0);
                assert!(accepted.lock().unwrap().is_empty());
            } else {
                assert!(completed.is_none());
                drain(&mut w).await.unwrap();
                assert!(!accepted.lock().unwrap().is_empty());
            }
        }
    }
    #[tokio::test]
    async fn ack_slot_opens_only_after_actual_first_plaintext_acceptance() {
        let (io, ready, _accepted) = gate();
        let mut w = Writer::new(io, 1003, None);
        let shared = redirects();
        let (done, mut response) = oneshot::channel();
        let id = shared
            .lock()
            .unwrap()
            .offers
            .begin_confirmation(1, done)
            .unwrap();
        let sequence = shared.lock().unwrap().offers.sequence();
        w.enqueue(
            fastpath(10),
            Purpose::Clipboard {
                generation: 1,
                sequence,
                request: false,
                advertisement: Some(crate::clipboard::Advertisement {
                    confirmed_id: Some(id),
                    enabled: true,
                    text: Some(Zeroizing::new("runtime".into())),
                }),
            },
        )
        .unwrap();
        let mut db = Database::new();
        let mut future = Box::pin(w.progress(&shared, &mut db));
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert!(!shared.lock().unwrap().offers.in_flight());
        shared.lock().unwrap().offers.acknowledge(true, 1, true);
        assert!(
            response.try_recv().is_err(),
            "unsent advertisement cannot have matching ACK"
        );
        ready.store(true, std::sync::atomic::Ordering::Release);
        assert!(future.await.unwrap().is_none());
        assert!(shared.lock().unwrap().offers.in_flight());
        shared.lock().unwrap().offers.acknowledge(true, 1, true);
        assert!(response.try_recv().unwrap().is_ok());
        drain(&mut w).await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_unsent_receive_releases_reservation_without_wire_tombstone() {
        use ironrdp::{
            cliprdr::{backend::CliprdrBackend, pdu::*, CliprdrClient},
            session::ActiveStageBuilder,
            svc::{StaticChannelSet, SvcMessage, SvcProcessor},
        };
        use std::any::TypeId;
        for revoke in [false, true] {
            let shared = redirects();
            shared.lock().unwrap().permissions.clipboard_enabled = true;
            let mut backend = crate::clipboard::TextBackend::new(shared.clone());
            let formats = [ClipboardFormat {
                id: ClipboardFormatId::CF_UNICODETEXT,
                name: None,
            }];
            backend.on_remote_copy(&formats);
            let mut channel = CliprdrClient::new(Box::new(backend));
            channel
                .process(
                    &SvcMessage::from(ClipboardPdu::FormatListResponse(FormatListResponse::Ok))
                        .encode_unframed_pdu()
                        .unwrap(),
                )
                .unwrap();
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
            shared
                .lock()
                .unwrap()
                .enqueue(crate::permissions::ClipboardAction::Request)
                .unwrap();
            let (generation, bytes, advertisement, sequence, request) =
                crate::clipboard::flush(&mut active, &shared)
                    .unwrap()
                    .pop()
                    .unwrap();
            assert!(request);
            assert!(shared.lock().unwrap().prepared_request.is_some());
            assert!(shared.lock().unwrap().pending_request.is_none());
            let (io, ready, accepted) = gate();
            let mut writer = Writer::new(io, 1003, None);
            writer
                .enqueue(
                    bytes,
                    Purpose::Clipboard {
                        generation,
                        sequence,
                        advertisement,
                        request,
                    },
                )
                .unwrap();
            let mut db = Database::new();
            let mut future = Box::pin(writer.progress(&shared, &mut db));
            std::future::poll_fn(|cx| {
                assert!(future.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            // An unsolicited reply cannot consume the prepared local reservation.
            crate::clipboard::TextBackend::new(shared.clone()).on_format_data_response(
                FormatDataResponse::new_unicode_string("unsolicited synthetic"),
            );
            assert!(shared.lock().unwrap().received_text.is_none());
            assert!(shared.lock().unwrap().prepared_request.is_some());
            if revoke {
                let mut state = shared.lock().unwrap();
                state.permissions.clipboard_enabled = false;
                state.generation += 1;
                state.clear_text();
                state.permissions.clipboard_enabled = true;
            } else {
                crate::clipboard::TextBackend::new(shared.clone()).on_remote_copy(&formats);
            }
            ready.store(true, std::sync::atomic::Ordering::Release);
            assert_eq!(future.await.unwrap().unwrap().accepted, 0);
            assert!(accepted.lock().unwrap().is_empty());
            assert!(shared.lock().unwrap().prepared_request.is_none());
            assert!(shared.lock().unwrap().pending_request.is_none());
            // A fresh user Receive can proceed without waiting for a nonexistent
            // late response to the request that never reached the peer.
            shared
                .lock()
                .unwrap()
                .enqueue(crate::permissions::ClipboardAction::Request)
                .unwrap();
            let (generation, bytes, advertisement, sequence, request) =
                crate::clipboard::flush(&mut active, &shared)
                    .unwrap()
                    .pop()
                    .unwrap();
            writer
                .enqueue(
                    bytes,
                    Purpose::Clipboard {
                        generation,
                        sequence,
                        advertisement,
                        request,
                    },
                )
                .unwrap();
            while !writer.is_empty() {
                writer.progress(&shared, &mut db).await.unwrap();
            }
            assert!(shared.lock().unwrap().prepared_request.is_none());
            assert_eq!(shared.lock().unwrap().pending_request, Some(generation));
            crate::clipboard::TextBackend::new(shared.clone())
                .on_format_data_response(FormatDataResponse::new_unicode_string("fresh synthetic"));
            assert_eq!(
                shared
                    .lock()
                    .unwrap()
                    .received_text
                    .as_deref()
                    .map(|s| s.as_str()),
                Some("fresh synthetic")
            );
        }
    }
}
