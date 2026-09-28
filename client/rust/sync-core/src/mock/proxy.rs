//! Loopback TCP proxy in front of the mock server, used to simulate network
//! partitions (refuse / drop connections) and lost responses.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

pub(crate) struct Proxy {
    offline: AtomicBool,
    kill: broadcast::Sender<()>,
    pub(crate) addr: SocketAddr,
}

impl Proxy {
    pub(crate) async fn start(
        upstream: SocketAddr,
    ) -> std::io::Result<(Arc<Self>, JoinHandle<()>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let (kill, _) = broadcast::channel(16);
        let proxy = Arc::new(Self {
            offline: AtomicBool::new(false),
            kill,
            addr: listener.local_addr()?,
        });
        let p = proxy.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut inbound, _)) = listener.accept().await else {
                    continue;
                };
                if p.offline.load(Ordering::SeqCst) {
                    drop(inbound);
                    continue;
                }
                let mut kill = p.kill.subscribe();
                tokio::spawn(async move {
                    let Ok(mut outbound) = TcpStream::connect(upstream).await else {
                        return;
                    };
                    tokio::select! {
                        biased;
                        _ = kill.recv() => {}
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => {}
                    }
                });
            }
        });
        Ok((proxy, task))
    }

    pub(crate) fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::SeqCst);
        if offline {
            self.kill_all();
        }
    }

    pub(crate) fn kill_all(&self) {
        let _ = self.kill.send(());
    }
}
