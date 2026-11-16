//! Draft observer-lifetime tests. Actual account evidence comes from ApiClient
//! and real Synced AppCore fixtures; these constructors grant no account access.
use super::*;

#[test]
fn transition_entry_cancels_old_scope_and_nested_pending_blocks_new_capture() {
    let tracker = ReaderAuthTransitions::default();
    let old = tracker.capture().unwrap();
    let first = tracker.begin();
    assert!(!old.is_current());
    assert!(tracker.capture().is_err());
    let second = tracker.begin();
    drop(first);
    assert!(tracker.capture().is_err());
    drop(second);
    assert!(tracker.capture().unwrap().is_current());
    assert!(!old.is_current());
}

#[test]
fn current_callback_can_observe_liveness_without_recursive_mutex() {
    let tracker = ReaderAuthTransitions::default();
    let captured = tracker.capture().unwrap();
    captured
        .with_current(|| {
            assert!(captured.is_current());
            Ok(())
        })
        .unwrap();
    let transition = tracker.begin();
    assert!(captured
        .with_current::<()>(|| panic!("stale callback invoked"))
        .is_err());
    drop(transition);
}

#[test]
fn generation_exhaustion_cancels_and_cannot_wrap_into_new_authority() {
    let tracker = ReaderAuthTransitions::default();
    tracker.0.generation.store(u64::MAX, Ordering::SeqCst);
    let old = tracker.capture().unwrap();
    drop(tracker.begin());
    assert!(!old.is_current());
    assert!(tracker.capture().is_err());
    assert_eq!(tracker.0.generation.load(Ordering::SeqCst), u64::MAX);
}
