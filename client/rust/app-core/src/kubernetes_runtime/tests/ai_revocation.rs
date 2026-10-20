use super::*;

struct PausedProvider {
    base: String,
    reached: CancellationToken,
    release: CancellationToken,
    disconnected: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for PausedProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl PausedProvider {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let reached = CancellationToken::new();
        let release = CancellationToken::new();
        let disconnected = CancellationToken::new();
        let seen = reached.clone();
        let proceed = release.clone();
        let closed = disconnected.clone();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                header.push(byte[0]);
                assert!(header.len() < 32768);
            }
            let header = String::from_utf8(header).unwrap();
            let size = header
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .unwrap()
                .1
                .trim()
                .parse::<usize>()
                .unwrap();
            assert!(size < 512 * 1024);
            let mut body = vec![0; size];
            socket.read_exact(&mut body).await.unwrap();
            seen.cancel();
            tokio::select! {
                _ = proceed.cancelled() => {},
                _ = socket.read(&mut byte) => {
                    closed.cancel();
                    return;
                },
            }
            let response = json!({
                "id":"fixture","object":"chat.completion","created":0,"model":"fixture",
                "choices":[{"index":0,"message":{"role":"assistant","content":"Late private cluster answer"},
                    "finish_reason":"stop"}]
            })
            .to_string();
            let _ = socket
                .write_all(
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len())
                        .as_bytes(),
                )
                .await;
        });
        Self {
            base,
            reached,
            release,
            disconnected,
            task,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_revision_revokes_active_transport_and_queued_cluster_context() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let model = PausedProvider::start().await;
    let mut provider = ai_provider(&app, &model.base).await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let kind = app
        .kube_discover(session.id.clone())
        .await
        .unwrap()
        .resources
        .into_iter()
        .find(|kind| kind.resource == "pods")
        .unwrap();
    let mut question = ai_question();
    question.selected = Some(ResourceRef {
        api: kind,
        namespace: Some("default".into()),
        name: "web".into(),
    });
    let review = app
        .kube_ai_prepare(session.id.clone(), question)
        .await
        .unwrap();
    let request = app
        .kube_ai_start(session.id.clone(), review.ticket, false)
        .await
        .unwrap();
    // Reaching the provider proves selected-object context was already queued.
    // No UI poll has consumed it when the provider revision changes.
    tokio::time::timeout(Duration::from_secs(5), model.reached.cancelled())
        .await
        .unwrap();
    provider.chat_model = "replacement-model".into();
    app.unlocked()
        .await
        .unwrap()
        .1
        .writer
        .put(VaultObject::AiProvider(provider))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), model.disconnected.cancelled())
        .await
        .expect("provider edit must cancel the in-flight HTTP request");
    if let Ok(poll) = app.kube_ai_poll(session.id.clone(), request).await {
        assert!(poll.closed);
        assert!(poll.events.is_empty(), "revoked context must not reach UI");
    }
    assert!(app
        .kube_ai_history(context.id.to_string())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(fixture.writes(), 0);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_during_pending_poll_discards_late_model_answer_and_history() {
    let fixture = Fixture::start(false).await;
    let (_dir, app, context) = fixture.app().await;
    let model = PausedProvider::start().await;
    ai_provider(&app, &model.base).await;
    let session = app.kube_connect(context.id.to_string()).await.unwrap();
    let review = app
        .kube_ai_prepare(session.id.clone(), ai_question())
        .await
        .unwrap();
    let request = app
        .kube_ai_start(session.id.clone(), review.ticket, false)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), model.reached.cancelled())
        .await
        .unwrap();
    let pending = app.kube_ai_poll(session.id, request);
    tokio::pin!(pending);
    // Poll the future into its pending receiver wait before revoking the vault.
    assert!(futures::poll!(&mut pending).is_pending());
    app.lock().await.unwrap();
    model.release.cancel();
    if let Ok(poll) = pending.await {
        assert!(poll.closed);
        assert!(
            poll.events.is_empty(),
            "no late answer may cross vault lock"
        );
    }
    app.unlock_with_passphrase("synthetic-runtime-test-passphrase".into())
        .await
        .unwrap();
    assert!(app
        .kube_ai_history(context.id.to_string())
        .await
        .unwrap()
        .is_empty());
    app.shutdown().await.unwrap();
}
