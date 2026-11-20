//! Exact production ownership decision plus real Core framed-transfer denials.
//! Unit payloads contain no fabricated archive/preview/Ready authority witness.
use super::*;
use tokio::io::AsyncReadExt;

fn view() -> DependencyView {
    DependencyView {
        version: 1,
        operation_id: Uuid::new_v4(),
        phase: Phase::CandidatePreview,
        error: None,
        cleanup: Cleanup::NotStarted,
        archive_verified: false,
        base_digest: None,
        candidate_digest: None,
        review_digest: Some("a".repeat(64)),
        change_count: 0,
        changes: vec![],
        changes_truncated: false,
    }
}

#[tokio::test]
async fn failed_or_cancelled_pre_handoff_never_remembers_retained_transport() {
    for phase in [
        Phase::Failed,
        Phase::Cancelled,
        Phase::ContextChanged,
        Phase::Expired,
    ] {
        let (reader, hold) = tokio::io::duplex(64);
        let peer = cc_automation_core::HelperPeer::connect(reader, tokio::io::sink());
        let state = Mutex::new(State::<()>::Terminal(phase, "unit_failed_before_handoff"));
        let retained = AtomicBool::new(false);
        let weak = Mutex::new(Arc::downgrade(&peer));
        cancellation_peer(&state, &retained, &weak)
            .unwrap()
            .close()
            .await;
        assert!(peer.is_closed());
        assert!(!retained.load(Ordering::Acquire));
        drop(hold);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_terminal_causes_leave_healthy_peer_for_bounded_owned_close() {
    for mode in ["reject", "drop", "cancel", "lock", "expiry"] {
        let (reader, hold) = tokio::io::duplex(64);
        let peer = cc_automation_core::HelperPeer::connect(reader, tokio::io::sink());
        let weak = Mutex::new(Arc::downgrade(&peer));
        let retained = AtomicBool::new(true);
        let state = Mutex::new(State::Ready(view(), Arc::new(())));
        if mode == "drop" {
            let nonce = accept_state(&state, &"a".repeat(64), || true).unwrap();
            release_accepted(&state, nonce, || {}, drop);
        } else {
            let phase = match mode {
                "reject" => Phase::Rejected,
                "lock" => Phase::ContextChanged,
                "expiry" => Phase::Expired,
                _ => Phase::Cancelled,
            };
            retire(&state, phase, "unit_retired", drop);
        }
        assert!(matches!(*state.lock().unwrap(), State::Terminal(..)));
        assert!(accept_state(&state, &"a".repeat(64), || false).is_err());
        assert!(cancellation_peer(&state, &retained, &weak).is_none());
        assert!(
            !peer.is_closed(),
            "watcher must not preempt owned dispose: {mode}"
        );
        // A real peer and reservation remain owned until close actually ends.
        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().try_acquire_owned().unwrap();
        let (finish, wait) = tokio::sync::oneshot::channel();
        let owner = tokio::spawn({
            let peer = peer.clone();
            async move {
                let _permit = permit;
                tokio::time::timeout(Duration::from_secs(1), wait)
                    .await
                    .unwrap()
                    .unwrap();
                peer.close().await;
            }
        });
        tokio::task::yield_now().await;
        assert_eq!(slots.available_permits(), 0);
        assert!(!owner.is_finished());
        assert!(!peer.is_closed());
        finish.send(()).unwrap();
        owner.await.unwrap();
        assert!(peer.is_closed());
        assert_eq!(slots.available_permits(), 1);
        drop(hold);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handoff_observation_cannot_see_worker_ownership_between_ready_state_and_flag() {
    let (reader, hold) = tokio::io::duplex(64);
    let peer = cc_automation_core::HelperPeer::connect(reader, tokio::io::sink());
    let weak = Arc::new(Mutex::new(Arc::downgrade(&peer)));
    let state = Arc::new(Mutex::new(State::<()>::Running(Phase::Preparing)));
    let retained = Arc::new(AtomicBool::new(false));
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (finish, wait) = std::sync::mpsc::sync_channel::<()>(0);
    let producer = tokio::task::spawn_blocking({
        let state = state.clone();
        let retained = retained.clone();
        move || {
            let mut state = state.lock().unwrap();
            // Same ordering/lock as op.ready; no actual witness is synthesized.
            retained.store(true, Ordering::Release);
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            *state = State::Ready(view(), Arc::new(()));
        }
    });
    ready.await.unwrap();
    let observer = tokio::task::spawn_blocking({
        let state = state.clone();
        let retained = retained.clone();
        let weak = weak.clone();
        move || cancellation_peer(&state, &retained, &weak)
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        !observer.is_finished(),
        "observation must share Ready handoff fence"
    );
    finish.send(()).unwrap();
    producer.await.unwrap();
    assert!(observer.await.unwrap().is_none());
    assert!(!peer.is_closed());
    peer.close().await;
    drop(hold);
}

async fn partial_case(mode: &'static str) {
    // Actual public receiver uses production ClosePeer framing. No response or
    // archive is forged; the tiny real pipe blocks the first request mid-frame.
    let (reader, response_hold) = tokio::io::duplex(64);
    let (writer, mut wire) = tokio::io::duplex(8);
    let peer = cc_automation_core::HelperPeer::connect(reader, writer);
    let weak = Mutex::new(Arc::downgrade(&peer));
    let retained = AtomicBool::new(false);
    let state = Mutex::new(State::<()>::Running(Phase::Preparing));
    let cancel = CancellationToken::new();
    let (start_timeout, timeout_start) = tokio::sync::oneshot::channel();
    let transfer = tokio::spawn({
        let peer = peer.clone();
        let cancel = cancel.clone();
        async move {
            let source = DependencySourceSelection::new(
                cc_automation_core::DependencyKind::Role,
                "owned_unit".into(),
                "1.2.3".into(),
                cc_automation_core::DependencyArtifactSource {
                    url: "https://owned-unit.example.invalid/role.tar.gz".into(),
                    sha256: "a".repeat(64),
                },
                0,
            )
            .unwrap();
            let digest = "b".repeat(64);
            let mut receive = Box::pin(cc_automation_core::receive_dependency_archive(
                &peer,
                &source,
                Uuid::new_v4(),
                &digest,
                &cancel,
            ));
            if mode == "timeout" {
                // Start the outer deadline only after the reader observed a partial
                // frame; scheduler load cannot turn this into a pre-send timeout.
                tokio::select! {
                    result=&mut receive=>result.is_err(),
                    started=timeout_start=>{
                        started.unwrap();
                        tokio::time::timeout(Duration::from_millis(40),receive).await.map_or(true,|r|r.is_err())
                    }
                }
            } else {
                receive.await.is_err()
            }
        }
    });
    let mut prefix = [0; 4];
    tokio::time::timeout(Duration::from_secs(1), wire.read_exact(&mut prefix))
        .await
        .unwrap()
        .unwrap();
    let mut watcher = None;
    if mode == "watcher_first" {
        let abort_peer = cancellation_peer(&state, &retained, &weak).unwrap();
        watcher = Some(tokio::spawn(async move { abort_peer.close().await }));
        tokio::time::timeout(Duration::from_secs(1), async {
            while !peer.is_closed() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // Caller cancellation then releases its blocked write for close join.
        cancel.cancel();
    } else {
        // Independent causal cases: NO watcher close before receiver fencing.
        assert!(!peer.is_closed());
        match mode {
            "abort" => transfer.abort(),
            "cancel" => cancel.cancel(),
            "timeout" => start_timeout.send(()).unwrap(),
            _ => panic!("unsupported unit mode"),
        }
    }
    let outcome = tokio::time::timeout(Duration::from_secs(2), transfer)
        .await
        .unwrap();
    if mode == "abort" {
        assert!(outcome.unwrap_err().is_cancelled());
    } else {
        assert!(outcome.unwrap());
    }
    assert!(
        peer.is_closed(),
        "receiver must fence before watcher cleanup: {mode}"
    );
    if let Some(watcher) = watcher {
        tokio::time::timeout(Duration::from_secs(1), watcher)
            .await
            .unwrap()
            .unwrap();
    } else {
        tokio::time::timeout(
            Duration::from_secs(1),
            cancellation_peer(&state, &retained, &weak).unwrap().close(),
        )
        .await
        .unwrap();
    }
    let mut transcript = prefix.to_vec();
    tokio::time::timeout(Duration::from_secs(1), wire.read_to_end(&mut transcript))
        .await
        .unwrap()
        .unwrap();
    assert!(transcript.len() <= 12);
    assert!(!transcript.contains(&b'\n'));
    assert!(!String::from_utf8_lossy(&transcript).contains("cancel_request"));
    drop(response_hold);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_partial_transfer_abort_cancel_and_timeout_independently_fence_before_watcher_cleanup(
) {
    for mode in ["abort", "cancel", "timeout"] {
        partial_case(mode).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watcher_first_still_fences_partial_transfer_then_joins_after_caller_cancel() {
    partial_case("watcher_first").await;
}
