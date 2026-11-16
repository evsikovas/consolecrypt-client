//! Exact production state transitions with a generic retained payload, never
//! a fabricated VerifiedDependencyPreview/Ready/archive witness.
use super::*;
fn view() -> DependencyView {
    DependencyView {
        version: 1,
        operation_id: Uuid::new_v4(),
        phase: Phase::CandidatePreview,
        error: None,
        cleanup: Cleanup::NotStarted,
        archive_verified: false,
        base_digest: Some("b".repeat(64)),
        candidate_digest: Some("c".repeat(64)),
        review_digest: Some("a".repeat(64)),
        change_count: 0,
        changes: vec![],
        changes_truncated: false,
    }
}
struct Payload {
    drops: Arc<std::sync::atomic::AtomicUsize>,
    _slot: OwnedSemaphorePermit,
}
impl Drop for Payload {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
fn state() -> (
    Arc<Mutex<State<Payload>>>,
    Arc<Semaphore>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let slot = Arc::new(Semaphore::new(1));
    let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let state = Arc::new(Mutex::new(State::Ready(
        view(),
        Arc::new(Payload {
            drops: drops.clone(),
            _slot: slot.clone().try_acquire_owned().unwrap(),
        }),
    )));
    (state, slot, drops)
}
#[test]
fn one_use_registry_custody_reuses_existing_owner_and_lifecycle_not_an_extra_holder() {
    let (state, slot, drops) = state();
    let nonce = accept_state(&state, &"a".repeat(64), || true).unwrap();
    assert!(retain_accepted_state(&state, &"a".repeat(64), nonce, || false).is_err());
    assert!(retain_accepted_state(&state, &"a".repeat(64), Uuid::new_v4(), || true).is_err());
    let result = retain_accepted_state(&state, &"a".repeat(64), nonce, || true).unwrap();
    assert!(retain_accepted_state(&state, &"a".repeat(64), nonce, || true).is_err());
    assert_eq!(slot.available_permits(), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    // Consumed external owner Drop must not revoke registry's retained state.
    release_accepted(
        &state,
        nonce,
        || panic!("detached external owner cancelled registry custody"),
        drop,
    );
    assert_eq!(slot.available_permits(), 0);
    let json = serde_json::to_value(result).unwrap();
    assert!(json.get("nonce").is_none() && json.get("registry_owned").is_none());
    retire(&state, Phase::Cancelled, "dependency_cancelled", drop);
    assert_eq!(slot.available_permits(), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    release_accepted(
        &state,
        nonce,
        || panic!("late external Drop resurrected cleanup"),
        drop,
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retain_racing_close_or_expiry_never_resurrects_or_releases_admission_early() {
    for _ in 0..16 {
        let (state, slot, drops) = state();
        let nonce = accept_state(&state, &"a".repeat(64), || true).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let closed = Arc::new(AtomicBool::new(false));
        let accept = tokio::task::spawn_blocking({
            let state = state.clone();
            let barrier = barrier.clone();
            let closed = closed.clone();
            move || {
                barrier.wait();
                retain_accepted_state(&state, &"a".repeat(64), nonce, || {
                    !closed.load(Ordering::Acquire)
                })
            }
        });
        let close = tokio::task::spawn_blocking({
            let state = state.clone();
            move || {
                barrier.wait();
                closed.store(true, Ordering::Release);
                retire(&state, Phase::Expired, "dependency_expired", drop)
            }
        });
        let _ = accept.await.unwrap();
        close.await.unwrap();
        assert!(matches!(
            *state.lock().unwrap(),
            State::Terminal(Phase::Expired, _)
        ));
        assert_eq!(slot.available_permits(), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
