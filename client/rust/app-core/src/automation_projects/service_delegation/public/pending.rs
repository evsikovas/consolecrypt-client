//! One bounded cleanup tied to the existing review's original pending Node.
//! No registry, grant, reconnect, credential resolver or retained Unlocked.
use super::*;
use cc_automation_core::HelperPeer;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

pub(super) struct PendingCleanup {
    peer: Arc<HelperPeer>,
    schedule: Uuid,
    armed: AtomicBool,
    disarmed: Arc<AtomicBool>,
    dropped: CancellationToken,
    runtime: tokio::runtime::Handle,
}
async fn cancel(peer: Arc<HelperPeer>, schedule: Uuid) {
    // Node checks exact original schedule + !enrollment_attempted atomically.
    // Errors/lost ACK are never projected as cleanup or revocation success.
    let _ = peer
        .request(
            "service_cancel_pending",
            json!({"schedule_id":schedule}),
            Duration::from_secs(30),
        )
        .await;
}
impl PendingCleanup {
    pub(super) fn new(peer: Arc<HelperPeer>, schedule: Uuid) -> Arc<Self> {
        Arc::new(Self {
            peer,
            schedule,
            armed: AtomicBool::new(false),
            disarmed: Arc::new(AtomicBool::new(false)),
            dropped: CancellationToken::new(),
            runtime: tokio::runtime::Handle::current(),
        })
    }
    pub(super) fn arm(
        &self,
        deadline: Instant,
        stopped: CancellationToken,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> AppResult<()> {
        if deadline <= Instant::now()
            || deadline > Instant::now() + Duration::from_secs(60)
            || self.armed.swap(true, Ordering::AcqRel)
        {
            return Err(unavailable());
        }
        let peer = self.peer.clone();
        let schedule = self.schedule;
        let disarmed = self.disarmed.clone();
        let dropped = self.dropped.clone();
        tokio::spawn(async move {
            if !*shutdown.borrow() && !stopped.is_cancelled() {
                tokio::select! {biased;
                    _=dropped.cancelled()=>{},
                    _=stopped.cancelled()=>{},
                    _=shutdown.changed()=>{},
                    _=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>{},
                }
            }
            if !disarmed.load(Ordering::Acquire) {
                cancel(peer, schedule).await;
            }
        });
        Ok(())
    }
    pub(super) fn disarm(&self) {
        self.disarmed.store(true, Ordering::Release);
        self.dropped.cancel();
    }
}
impl Drop for PendingCleanup {
    fn drop(&mut self) {
        if self.disarmed.load(Ordering::Acquire) {
            return;
        }
        if self.armed.load(Ordering::Acquire) {
            self.dropped.cancel();
        } else {
            // Final review may be dropped by an FFI cancellation thread outside
            // Tokio; schedule only on the ORIGINAL captured runtime.
            self.runtime.spawn(cancel(self.peer.clone(), self.schedule));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader};
    async fn fixture() -> (
        Arc<HelperPeer>,
        BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
        tokio::io::WriteHalf<tokio::io::DuplexStream>,
    ) {
        let (left, right) = tokio::io::duplex(8192);
        let (read, write) = tokio::io::split(left);
        let (server, reply) = tokio::io::split(right);
        (
            HelperPeer::connect(read, write),
            BufReader::new(server),
            reply,
        )
    }
    async fn receive(
        reader: &mut BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
        writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
        schedule: Uuid,
    ) {
        let raw = tokio::time::timeout(
            Duration::from_secs(2),
            cc_automation_core::read_frame(reader),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        let request: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(request["operation"], "service_cancel_pending");
        assert_eq!(request["body"], json!({"schedule_id":schedule}));
        let mut response = serde_json::to_vec(
            &json!({"version":1,"type":"response","request_id":request["request_id"],
            "result":{"cancellation_requested":true},"error":null}),
        )
        .unwrap();
        response.push(b'\n');
        writer.write_all(&response).await.unwrap();
    }
    #[tokio::test]
    async fn actual_original_peer_gets_one_cleanup_on_deadline_final_drop_and_lock() {
        for trigger in 0..4 {
            let (peer, mut reader, mut writer) = fixture().await;
            let schedule = Uuid::new_v4();
            let mut guard = Some(PendingCleanup::new(peer.clone(), schedule));
            let (lock, shutdown) = tokio::sync::watch::channel(false);
            if trigger != 3 {
                guard
                    .as_ref()
                    .unwrap()
                    .arm(
                        Instant::now() + Duration::from_millis(if trigger == 0 { 20 } else { 500 }),
                        CancellationToken::new(),
                        shutdown,
                    )
                    .unwrap();
            }
            // Deadline/lock cases retain the owner across actual observation;
            // final-drop and pre-consent-abort cases drop it before the frame.
            if trigger == 1 {
                drop(guard.take());
            }
            if trigger == 3 {
                let owned = guard.take();
                std::thread::spawn(move || drop(owned)).join().unwrap();
            }
            if trigger == 2 {
                lock.send_replace(true);
            }
            receive(&mut reader, &mut writer, schedule).await;
            drop(guard);
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(30),
                    cc_automation_core::read_frame(&mut reader)
                )
                .await
                .is_err()
            );
            peer.close().await;
        }
    }
    #[tokio::test]
    async fn acknowledged_disarm_never_sends_pending_cleanup_and_arm_is_one_shot() {
        let (peer, mut reader, _writer) = fixture().await;
        let guard = PendingCleanup::new(peer.clone(), Uuid::new_v4());
        let (_lock, shutdown) = tokio::sync::watch::channel(false);
        let stop = CancellationToken::new();
        guard
            .arm(
                Instant::now() + Duration::from_millis(20),
                stop.clone(),
                shutdown.clone(),
            )
            .unwrap();
        assert!(
            guard
                .arm(Instant::now() + Duration::from_secs(1), stop, shutdown)
                .is_err()
        );
        guard.disarm();
        drop(guard);
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                cc_automation_core::read_frame(&mut reader)
            )
            .await
            .is_err()
        );
        peer.close().await;
    }
}
