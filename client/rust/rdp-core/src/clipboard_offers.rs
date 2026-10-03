//! Correlates the ID-less CLIPRDR acknowledgement with exactly one serialized offer.
use crate::RdpError;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use zeroize::Zeroizing;

pub(crate) const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10);
#[derive(Clone, Copy)]
pub(crate) struct PasteFence {
    generation: u64,
    sequence: u64,
}

struct Waiter {
    id: u64,
    generation: u64,
    sequence: u64,
    done: oneshot::Sender<Result<String, RdpError>>,
}
struct Flight {
    confirmed_id: Option<u64>,
    generation: u64,
    sequence: u64,
}
struct Ticket {
    offer_id: u64,
    value: String,
    generation: u64,
    sequence: u64,
    expires: Instant,
    queued: bool,
}
pub(crate) struct ClipboardOffers {
    sequence: u64,
    next_id: u64,
    waiter: Option<Waiter>,
    flight: Option<Flight>,
    ticket: Option<Ticket>,
    pub advertised_text: Option<Zeroizing<String>>,
}
impl Default for ClipboardOffers {
    fn default() -> Self {
        Self {
            sequence: 1,
            next_id: 1,
            waiter: None,
            flight: None,
            ticket: None,
            advertised_text: None,
        }
    }
}
impl ClipboardOffers {
    fn advance(&mut self) -> Result<(), RdpError> {
        self.sequence = self.sequence.checked_add(1).ok_or(RdpError::Protocol)?;
        self.ticket = None;
        Ok(())
    }
    pub fn ordinary_changed(&mut self) -> Result<(), RdpError> {
        if self.waiter.is_some() {
            return Err(RdpError::InputQueueFull);
        }
        self.advance()
    }
    pub fn begin_confirmation(
        &mut self,
        generation: u64,
        done: oneshot::Sender<Result<String, RdpError>>,
    ) -> Result<u64, RdpError> {
        if self.waiter.is_some() {
            return Err(RdpError::InputQueueFull);
        }
        self.advance()?;
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(RdpError::Protocol)?;
        self.waiter = Some(Waiter {
            id,
            generation,
            sequence: self.sequence,
            done,
        });
        Ok(id)
    }
    pub fn in_flight(&self) -> bool {
        self.flight.is_some()
    }
    pub fn begin_advertisement(
        &mut self,
        generation: u64,
        enabled: bool,
        text: Option<&Zeroizing<String>>,
        confirmed_id: Option<u64>,
    ) -> bool {
        if self.flight.is_some() {
            return false;
        }
        if let Some(id) = confirmed_id {
            let valid = self.waiter.as_ref().is_some_and(|waiter| {
                waiter.id == id
                    && waiter.generation == generation
                    && waiter.sequence == self.sequence
                    && !waiter.done.is_closed()
                    && enabled
            });
            if !valid {
                return false;
            }
        }
        self.advertised_text = enabled.then(|| text.cloned()).flatten();
        self.flight = Some(Flight {
            confirmed_id,
            generation,
            sequence: self.sequence,
        });
        true
    }
    pub fn acknowledge(&mut self, ok: bool, generation: u64, enabled: bool) {
        let Some(flight) = self.flight.take() else {
            return;
        };
        let Some(id) = flight.confirmed_id else {
            return;
        };
        if !self.waiter.as_ref().is_some_and(|waiter| waiter.id == id) {
            return; // Timed-out/cancelled flight drains; it never confirms its successor.
        }
        let waiter = self.waiter.take().expect("waiter checked above");
        if !ok
            || !enabled
            || flight.generation != generation
            || flight.sequence != self.sequence
            || waiter.generation != generation
            || waiter.sequence != self.sequence
            || waiter.done.is_closed()
        {
            let _ = waiter.done.send(Err(if enabled {
                RdpError::ClipboardUnavailable
            } else {
                RdpError::PermissionDenied
            }));
            return;
        }
        let value = uuid::Uuid::new_v4().to_string();
        self.ticket = Some(Ticket {
            offer_id: id,
            value: value.clone(),
            generation,
            sequence: self.sequence,
            expires: Instant::now() + CONFIRM_TIMEOUT,
            queued: false,
        });
        if waiter.done.send(Ok(value)).is_err() {
            self.ticket = None;
        }
    }
    pub fn invalidate(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            let _ = waiter.done.send(Err(RdpError::ClipboardUnavailable));
        }
        self.ticket = None;
    }
    pub fn cancel_interaction(&mut self) {
        self.invalidate();
        self.sequence = self.sequence.saturating_add(1);
    }
    pub fn content_changed(&mut self) {
        self.cancel_interaction();
        self.advertised_text = None;
    }
    pub fn cancel_confirmation(&mut self, id: u64) -> bool {
        if self.waiter.as_ref().is_some_and(|waiter| waiter.id == id) {
            self.content_changed();
            return true;
        }
        if self
            .ticket
            .as_ref()
            .is_some_and(|ticket| ticket.offer_id == id)
        {
            self.ticket = None;
        }
        false
    }
    fn ticket_valid(&self, value: &str, generation: u64, enabled: bool) -> bool {
        enabled
            && self.ticket.as_ref().is_some_and(|ticket| {
                ticket.value == value
                    && ticket.generation == generation
                    && ticket.sequence == self.sequence
                    && Instant::now() < ticket.expires
            })
    }
    pub fn queue_paste(
        &mut self,
        value: &str,
        generation: u64,
        enabled: bool,
    ) -> Result<(), RdpError> {
        if !self.ticket_valid(value, generation, enabled) {
            return Err(RdpError::ClipboardUnavailable);
        }
        let ticket = self.ticket.as_mut().expect("ticket validated above");
        if ticket.queued {
            return Err(RdpError::InputQueueFull);
        }
        ticket.queued = true;
        Ok(())
    }
    pub fn consume_paste(
        &mut self,
        value: &str,
        generation: u64,
        enabled: bool,
    ) -> Result<PasteFence, RdpError> {
        if !self.ticket_valid(value, generation, enabled)
            || !self.ticket.as_ref().is_some_and(|ticket| ticket.queued)
        {
            return Err(if enabled {
                RdpError::ClipboardUnavailable
            } else {
                RdpError::PermissionDenied
            });
        }
        self.ticket = None;
        Ok(PasteFence {
            generation,
            sequence: self.sequence,
        })
    }
    pub fn dispatch_authorized(&self, fence: PasteFence, generation: u64, enabled: bool) -> bool {
        enabled && generation == fence.generation && self.sequence == fence.sequence
    }
    pub fn cancel_ticket(&mut self, value: &str) {
        if self
            .ticket
            .as_ref()
            .is_some_and(|ticket| ticket.value == value)
        {
            self.ticket = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn confirmation(
        offers: &mut ClipboardOffers,
        generation: u64,
    ) -> (u64, oneshot::Receiver<Result<String, RdpError>>) {
        let (done, receiver) = oneshot::channel();
        (
            offers.begin_confirmation(generation, done).unwrap(),
            receiver,
        )
    }
    fn acknowledged(offers: &mut ClipboardOffers, generation: u64) -> String {
        let (id, mut receiver) = confirmation(offers, generation);
        let text = Zeroizing::new("synthetic / Привет".to_string());
        assert!(offers.begin_advertisement(generation, true, Some(&text), Some(id)));
        offers.acknowledge(true, generation, true);
        receiver.try_recv().unwrap().unwrap()
    }

    #[test]
    fn initial_and_empty_ack_cannot_confirm_a_new_offer() {
        let mut offers = ClipboardOffers::default();
        assert!(offers.begin_advertisement(1, true, None, None));
        let (id, mut receiver) = confirmation(&mut offers, 1);
        let text = Zeroizing::new("synthetic".to_owned());
        assert!(!offers.begin_advertisement(1, true, Some(&text), Some(id)));
        offers.acknowledge(true, 1, true);
        assert_eq!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );
        assert!(offers.begin_advertisement(1, true, Some(&text), Some(id)));
        offers.acknowledge(true, 1, true);
        let ticket = receiver.try_recv().unwrap().unwrap();
        offers.queue_paste(&ticket, 1, true).unwrap();
        offers.consume_paste(&ticket, 1, true).unwrap();
        assert!(offers.consume_paste(&ticket, 1, true).is_err());
    }

    #[test]
    fn old_offer_body_is_frozen_until_its_ack_drains() {
        let mut offers = ClipboardOffers::default();
        let old = Zeroizing::new("old synthetic".to_owned());
        let new = Zeroizing::new("new synthetic".to_owned());
        assert!(offers.begin_advertisement(1, true, Some(&old), None));
        let (id, mut receiver) = confirmation(&mut offers, 1);
        assert!(!offers.begin_advertisement(1, true, Some(&new), Some(id)));
        assert_eq!(
            offers.advertised_text.as_deref().map(|s| s.as_str()),
            Some("old synthetic")
        );
        offers.acknowledge(true, 1, true);
        assert_eq!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );
        assert!(offers.begin_advertisement(1, true, Some(&new), Some(id)));
        assert_eq!(
            offers.advertised_text.as_deref().map(|s| s.as_str()),
            Some("new synthetic")
        );
        offers.acknowledge(true, 1, true);
        assert!(receiver.try_recv().unwrap().is_ok());
    }

    #[test]
    fn cancelled_and_timed_out_ack_is_a_draining_tombstone() {
        let mut offers = ClipboardOffers::default();
        let text = Zeroizing::new("synthetic".to_owned());
        let (old, mut old_receiver) = confirmation(&mut offers, 1);
        assert!(offers.begin_advertisement(1, true, Some(&text), Some(old)));
        assert!(offers.cancel_confirmation(old));
        assert!(old_receiver.try_recv().unwrap().is_err());
        let (new, mut new_receiver) = confirmation(&mut offers, 1);
        assert!(!offers.begin_advertisement(1, true, Some(&text), Some(new)));
        offers.acknowledge(true, 1, true);
        assert_eq!(
            new_receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );
        assert!(offers.begin_advertisement(1, true, Some(&text), Some(new)));
        offers.acknowledge(true, 1, true);
        assert!(new_receiver.try_recv().unwrap().is_ok());
    }

    #[test]
    fn dropped_waiter_failed_ack_revoke_and_remote_copy_never_grant_a_ticket() {
        for mode in 0..4 {
            let mut offers = ClipboardOffers::default();
            let (id, receiver) = confirmation(&mut offers, 1);
            let text = Zeroizing::new("synthetic".to_owned());
            assert!(offers.begin_advertisement(1, true, Some(&text), Some(id)));
            match mode {
                0 => drop(receiver),
                1 => {
                    offers.acknowledge(false, 1, true);
                    assert!(receiver.blocking_recv().unwrap().is_err());
                }
                2 => {
                    offers.content_changed();
                    assert!(receiver.blocking_recv().unwrap().is_err());
                }
                _ => {
                    offers.acknowledge(true, 2, false);
                    assert!(receiver.blocking_recv().unwrap().is_err());
                }
            }
            offers.acknowledge(true, 1, true);
            assert!(offers.ticket.is_none());
        }
    }

    #[test]
    fn newer_send_generation_remote_copy_expiry_and_replay_block_commit() {
        for mode in 0..5 {
            let mut offers = ClipboardOffers::default();
            let ticket = acknowledged(&mut offers, 1);
            match mode {
                0 => offers.ordinary_changed().unwrap(),
                1 => {}
                2 => offers.content_changed(),
                3 => {
                    offers.ticket.as_mut().unwrap().expires =
                        Instant::now() - Duration::from_secs(1)
                }
                _ => {
                    offers.queue_paste(&ticket, 1, true).unwrap();
                    offers.consume_paste(&ticket, 1, true).unwrap();
                }
            }
            assert!(offers
                .queue_paste(&ticket, if mode == 1 { 2 } else { 1 }, true)
                .is_err());
        }
    }

    #[test]
    fn queued_paste_rechecks_at_dispatch_and_consumes_once() {
        let mut offers = ClipboardOffers::default();
        let ticket = acknowledged(&mut offers, 1);
        offers.queue_paste(&ticket, 1, true).unwrap();
        assert_eq!(
            offers.queue_paste(&ticket, 1, true),
            Err(RdpError::InputQueueFull)
        );
        let fence = offers.consume_paste(&ticket, 1, true).unwrap();
        assert!(offers.dispatch_authorized(fence, 1, true));
        offers.content_changed();
        assert!(!offers.dispatch_authorized(fence, 1, true));
        assert!(offers.consume_paste(&ticket, 1, true).is_err());
    }

    #[test]
    fn ordinary_send_cannot_replace_unconfirmed_body() {
        let mut offers = ClipboardOffers::default();
        let (_id, _receiver) = confirmation(&mut offers, 1);
        assert_eq!(offers.ordinary_changed(), Err(RdpError::InputQueueFull));
    }
}
