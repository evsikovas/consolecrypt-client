//! Original-registration custody for the AWX facade. No HTTP, credentials,
//! latest-owner lookup, persisted authority or cancel-before-begin tombstone.
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_OPERATIONS: usize = 32;
const IDLE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    Running,
    Joined,
}

struct Control {
    phase: Mutex<Phase>,
    changed: watch::Sender<Phase>,
    cancel: CancellationToken,
    registered: Instant,
    idle_cleanup: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Control {
    fn clear_body(&self) {
        let cleanup = self
            .idle_cleanup
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(cleanup) = cleanup {
            cleanup();
        }
    }
    // Caller holds phase. Clear the registered body BEFORE publishing Joined.
    fn finish_idle(&self, phase: &mut Phase) {
        self.clear_body();
        self.cancel.cancel();
        *phase = Phase::Joined;
        self.changed.send_replace(Phase::Joined);
    }
}

#[derive(Default)]
pub(super) struct Operations {
    entries: Mutex<HashMap<Uuid, Weak<Control>>>,
    closed: AtomicBool,
}

/// Native returns this exact, non-serializable handle before body admission.
/// A delayed start can only consume this registration, never create another.
pub(super) struct Registered {
    id: Uuid,
    runtime: Arc<Operations>,
    control: Arc<Control>,
}

/// Retained in the actual request future, including semaphore admission and
/// response postcheck. Dropping UI custody does not report premature joined.
pub(super) struct Running {
    id: Uuid,
    runtime: Arc<Operations>,
    control: Arc<Control>,
}

impl Operations {
    pub(super) fn register(self: &Arc<Self>) -> Result<Registered, &'static str> {
        self.register_with_cleanup(|| {})
    }
    pub(super) fn register_with_cleanup(
        self: &Arc<Self>,
        cleanup: impl FnOnce() + Send + 'static,
    ) -> Result<Registered, &'static str> {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        if self.closed.load(Ordering::Acquire) {
            return Err("awx_runtime_closed");
        }
        entries.retain(|_, weak| {
            weak.upgrade().is_some_and(|control| {
                let mut phase = control.phase.lock().unwrap_or_else(|p| p.into_inner());
                if *phase == Phase::Idle && control.registered.elapsed() >= IDLE_TTL {
                    control.finish_idle(&mut phase);
                }
                *phase != Phase::Joined
            })
        });
        if entries.len() >= MAX_OPERATIONS {
            return Err("awx_operation_budget");
        }
        let id = Uuid::new_v4();
        let control = Arc::new(Control {
            phase: Mutex::new(Phase::Idle),
            changed: watch::channel(Phase::Idle).0,
            cancel: CancellationToken::new(),
            registered: Instant::now(),
            idle_cleanup: Mutex::new(Some(Box::new(cleanup))),
        });
        entries.insert(id, Arc::downgrade(&control));
        Ok(Registered {
            id,
            runtime: self.clone(),
            control,
        })
    }

    fn retire(&self, id: Uuid, expected: &Arc<Control>) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        if entries
            .get(&id)
            .and_then(Weak::upgrade)
            .is_some_and(|present| Arc::ptr_eq(&present, expected))
        {
            entries.remove(&id);
        }
    }

    pub(super) fn cancel_all(&self) {
        self.closed.store(true, Ordering::Release);
        // Never call retire while holding the map. The same lock order is used
        // by register: map first, then control; start/cancel release control
        // before attempting retirement.
        let entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        for control in entries.values().filter_map(Weak::upgrade) {
            let mut phase = control.phase.lock().unwrap_or_else(|p| p.into_inner());
            if *phase == Phase::Idle {
                control.finish_idle(&mut phase);
            } else {
                control.cancel.cancel();
            }
        }
    }
}

impl Registered {
    pub(super) fn id(&self) -> Uuid {
        self.id
    }

    pub(super) fn start(&self) -> Result<Running, &'static str> {
        let mut phase = self.control.phase.lock().unwrap_or_else(|p| p.into_inner());
        if *phase == Phase::Idle
            && (self.runtime.closed.load(Ordering::Acquire)
                || self.control.cancel.is_cancelled()
                || self.control.registered.elapsed() >= IDLE_TTL)
        {
            self.control.finish_idle(&mut phase);
            drop(phase);
            self.runtime.retire(self.id, &self.control);
            return Err("awx_registration_unavailable");
        }
        if *phase != Phase::Idle {
            return Err("awx_registration_unavailable");
        }
        *phase = Phase::Running;
        self.control.changed.send_replace(Phase::Running);
        Ok(Running {
            id: self.id,
            runtime: self.runtime.clone(),
            control: self.control.clone(),
        })
    }

    pub(super) fn cancel(&self) {
        let idle = {
            let mut phase = self.control.phase.lock().unwrap_or_else(|p| p.into_inner());
            if *phase == Phase::Idle {
                self.control.finish_idle(&mut phase);
                true
            } else {
                self.control.cancel.cancel();
                false
            }
        };
        if idle {
            self.runtime.retire(self.id, &self.control);
        }
    }

    pub(super) fn observe(&self) -> watch::Receiver<Phase> {
        self.control.changed.subscribe()
    }

    pub(super) async fn joined(&self) {
        let mut changed = self.observe();
        loop {
            if *changed.borrow_and_update() == Phase::Joined {
                return;
            }
            if changed.changed().await.is_err() {
                return;
            }
        }
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Running {
    pub(super) fn cancel_token(&self) -> CancellationToken {
        self.control.cancel.clone()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        {
            let mut phase = self.control.phase.lock().unwrap_or_else(|p| p.into_inner());
            self.control.clear_body();
            self.control.cancel.cancel();
            *phase = Phase::Joined;
            self.control.changed.send_replace(Phase::Joined);
        }
        self.runtime.retire(self.id, &self.control);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_before_delayed_start_never_creates_a_body() {
        let runtime = Arc::new(Operations::default());
        let registration = runtime.register().unwrap();
        assert!(!registration.id().is_nil());
        registration.cancel();
        registration.joined().await;
        assert!(registration.start().is_err());
        assert!(runtime.entries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canceled_running_request_is_not_joined_until_actual_worker_drop() {
        let runtime = Arc::new(Operations::default());
        let registration = runtime.register().unwrap();
        let running = registration.start().unwrap();
        let observer = registration.observe();
        registration.cancel();
        assert!(running.cancel_token().is_cancelled());
        assert_eq!(*observer.borrow(), Phase::Running);
        assert_eq!(runtime.entries.lock().unwrap().len(), 1);
        assert!(registration.start().is_err());
        drop(running);
        registration.joined().await;
        assert!(runtime.entries.lock().unwrap().is_empty());
    }

    #[test]
    fn caller_drop_keeps_actual_worker_custody_and_budget() {
        let runtime = Arc::new(Operations::default());
        let mut registrations = Vec::new();
        for _ in 0..MAX_OPERATIONS {
            registrations.push(runtime.register().unwrap());
        }
        assert!(runtime.register().is_err());
        let first = registrations.pop().unwrap();
        let running = first.start().unwrap();
        drop(first);
        assert!(running.cancel_token().is_cancelled());
        assert!(runtime.register().is_err());
        drop(running);
        assert!(runtime.register().is_ok());
    }

    #[tokio::test]
    async fn shutdown_retires_idle_but_preserves_running_join_boundary() {
        let runtime = Arc::new(Operations::default());
        let idle = runtime.register().unwrap();
        let active = runtime.register().unwrap();
        let running = active.start().unwrap();
        runtime.cancel_all();
        assert!(runtime.register().is_err());
        idle.joined().await;
        assert!(idle.start().is_err());
        assert_eq!(*active.observe().borrow(), Phase::Running);
        assert!(running.cancel_token().is_cancelled());
        drop(running);
        active.joined().await;
        assert!(active.start().is_err());
    }

    #[test]
    fn shutdown_fence_denies_start_even_before_cancel_all_reaches_the_map() {
        let runtime = Arc::new(Operations::default());
        let registration = Arc::new(runtime.register().unwrap());
        let observer = registration.observe();
        let map_guard = runtime.entries.lock().unwrap();
        let shutdown_runtime = runtime.clone();
        let shutdown = std::thread::spawn(move || shutdown_runtime.cancel_all());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !runtime.closed.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let closed = runtime.closed.load(Ordering::Acquire);
        let delayed = registration.clone();
        let body = std::thread::spawn(move || delayed.start().is_err());
        while *observer.borrow() == Phase::Idle && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let phase_before_map_release = *observer.borrow();
        drop(map_guard);
        shutdown.join().unwrap();
        let denied = body.join().unwrap();
        assert!(closed);
        assert_eq!(phase_before_map_release, Phase::Joined);
        assert!(denied);
    }
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    #[tokio::test]
    async fn idle_cancel_and_shutdown_clear_body_before_joined_is_observable() {
        for shutdown in [false, true] {
            let runtime = Arc::new(Operations::default());
            let cleared = Arc::new(AtomicBool::new(false));
            let marker = cleared.clone();
            let registration = runtime
                .register_with_cleanup(move || {
                    marker.store(true, Ordering::Release);
                })
                .unwrap();
            if shutdown {
                runtime.cancel_all();
            } else {
                registration.cancel();
            }
            registration.joined().await;
            assert!(cleared.load(Ordering::Acquire));
            assert!(registration.start().is_err());
        }
    }
}
