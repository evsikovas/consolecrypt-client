//! Route-wide original-generation accounting, never an authority or reset API.
use cc_agent_node_wire::{Error, Result, TunnelLimits};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
struct State {
    started: Instant,
    last: Instant,
    high: Instant,
    stop: bool,
    next: u64,
    total: u64,
    queued: u64,
    frames: u64,
    channels: BTreeMap<u64, (u64, u64, u64)>,
}
#[derive(Clone)]
pub(super) struct Budget {
    limits: TunnelLimits,
    state: Arc<Mutex<State>>,
    cancel: CancellationToken,
}
pub(super) struct Channel {
    budget: Budget,
    id: u64,
}
pub(super) struct Frame {
    budget: Budget,
    id: u64,
    capacity: u64,
}
impl Budget {
    pub fn new(limits: TunnelLimits, cancel: CancellationToken) -> Self {
        let now = Instant::now();
        Self {
            limits,
            state: Arc::new(Mutex::new(State {
                started: now,
                last: now,
                high: now,
                stop: false,
                next: 0,
                total: 0,
                queued: 0,
                frames: 0,
                channels: BTreeMap::new(),
            })),
            cancel,
        }
    }
    fn live(&self, s: &mut State) -> Result<()> {
        let now = Instant::now();
        if s.stop
            || self.cancel.is_cancelled()
            || now < s.high
            || now.duration_since(s.started) >= Duration::from_secs(self.limits.max_ttl_seconds)
            || now.duration_since(s.last) >= Duration::from_secs(self.limits.idle_timeout_seconds)
        {
            s.stop = true;
            self.cancel.cancel();
            return Err(Error::Expired);
        }
        s.high = now;
        Ok(())
    }
    pub fn check(&self) -> Result<()> {
        let mut s = self.state.lock().map_err(|_| Error::Storage)?;
        self.live(&mut s)
    }
    pub fn deadline(&self) -> Result<Instant> {
        let mut state = self.state.lock().map_err(|_| Error::Storage)?;
        self.live(&mut state)?;
        let ttl = state
            .started
            .checked_add(Duration::from_secs(self.limits.max_ttl_seconds))
            .ok_or(Error::Quota)?;
        let idle = state
            .last
            .checked_add(Duration::from_secs(self.limits.idle_timeout_seconds))
            .ok_or(Error::Quota)?;
        Ok(ttl.min(idle))
    }
    pub async fn cancelled(&self) {
        loop {
            let Ok(deadline) = self.deadline() else {
                return;
            };
            tokio::select! {biased;_=self.cancel.cancelled()=>return,_=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>{}}
            // Activity on another live channel may move last+idle forward.
            // TTL remains the original started+TTL, never recaptured/reset.
        }
    }
    pub fn stop(&self) {
        self.cancel.cancel();
        if let Ok(mut s) = self.state.lock() {
            s.stop = true;
        }
    }
    pub fn open(&self) -> Result<Channel> {
        let mut s = self.state.lock().map_err(|_| Error::Storage)?;
        self.live(&mut s)?;
        if s.channels.len() as u64 >= self.limits.max_channels {
            return Err(Error::Quota);
        }
        s.next = s.next.checked_add(1).ok_or(Error::Quota)?;
        let id = s.next;
        s.channels.insert(id, (0, 0, 0));
        Ok(Channel {
            budget: self.clone(),
            id,
        })
    }
}
impl Channel {
    /// Reserve before allocating/reading application plaintext. TLS records and
    /// fixed transport overhead are separate; no whole-process RAM claim.
    pub fn reserve(&self) -> Result<Frame> {
        let b = &self.budget;
        let mut s = b.state.lock().map_err(|_| Error::Storage)?;
        b.live(&mut s)?;
        let (charged, queued, frames) = *s.channels.get(&self.id).ok_or(Error::WrongState)?;
        let n = b
            .limits
            .max_frame_bytes
            .min(b.limits.max_bytes_per_channel.saturating_sub(charged))
            .min(b.limits.max_bytes_total.saturating_sub(s.total))
            .min(b.limits.max_queued_bytes_per_channel.saturating_sub(queued))
            .min(b.limits.max_queued_bytes_total.saturating_sub(s.queued));
        if n == 0
            || frames >= b.limits.max_pending_frames_per_channel
            || s.frames >= b.limits.max_pending_frames_total
        {
            return Err(Error::Quota);
        }
        s.channels.get_mut(&self.id).unwrap().1 += n;
        s.channels.get_mut(&self.id).unwrap().2 += 1;
        s.queued += n;
        s.frames += 1;
        Ok(Frame {
            budget: b.clone(),
            id: self.id,
            capacity: n,
        })
    }
}
impl Frame {
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }
    pub fn charge(&self, n: usize) -> Result<()> {
        let b = &self.budget;
        let mut s = b.state.lock().map_err(|_| Error::Storage)?;
        b.live(&mut s)?;
        let n = n as u64;
        let charged = s.channels.get(&self.id).ok_or(Error::WrongState)?.0;
        if n > self.capacity
            || charged
                .checked_add(n)
                .filter(|v| *v <= b.limits.max_bytes_per_channel)
                .is_none()
            || s.total
                .checked_add(n)
                .filter(|v| *v <= b.limits.max_bytes_total)
                .is_none()
        {
            return Err(Error::Quota);
        }
        s.channels.get_mut(&self.id).unwrap().0 += n;
        s.total += n;
        if n > 0 {
            s.last = Instant::now();
        }
        Ok(())
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        if let Ok(mut s) = self.budget.state.lock() {
            if let Some(c) = s.channels.get_mut(&self.id) {
                c.1 = c.1.saturating_sub(self.capacity);
                c.2 = c.2.saturating_sub(1);
            }
            s.queued = s.queued.saturating_sub(self.capacity);
            s.frames = s.frames.saturating_sub(1);
        }
    }
}
impl Drop for Channel {
    fn drop(&mut self) {
        if let Ok(mut s) = self.budget.state.lock() {
            if s.channels
                .get(&self.id)
                .is_some_and(|c| c.1 == 0 && c.2 == 0)
            {
                s.channels.remove(&self.id);
            } else {
                s.stop = true;
                self.budget.cancel.cancel();
            }
        }
    }
}
