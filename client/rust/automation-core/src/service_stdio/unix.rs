//! Fixed private local transport for the SAME dispatcher; no listener creation.
use super::*;
use cap_std::fs::{Dir, FileTypeExt, Metadata, MetadataExt, PermissionsExt};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{Mutex, Semaphore},
    task::JoinSet,
};
#[path = "inherited.rs"]
mod inherited;
const CONNECTIONS: usize = 4;
const PENDING: usize = 4;
struct Pin {
    parent: Dir,
    path: PathBuf,
    directory: Metadata,
    socket: Metadata,
    uid: u32,
}
fn parent(path: &Path) -> Result<Dir> {
    let fd = rustix::fs::openat2(
        rustix::fs::CWD,
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::NO_SYMLINKS | rustix::fs::ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(std::io::Error::from)?;
    Ok(Dir::from_std_file(std::fs::File::from(fd)))
}
fn same(a: &Metadata, b: &Metadata) -> Result<()> {
    if a.ino() == 0 || a.ino() != b.ino() || a.dev() != b.dev() {
        return Err(denied());
    }
    Ok(())
}
impl Pin {
    fn capture(id: Uuid, listener: &std::os::unix::net::UnixListener) -> Result<Self> {
        let path = Path::new("/run/consolecrypt-service").join(id.to_string());
        let actual = listener.local_addr()?;
        let expected = path.join("control.sock");
        if actual.as_pathname().map(Path::as_os_str) != Some(expected.as_os_str()) {
            return Err(denied());
        }
        let parent = parent(&path)?;
        let uid = rustix::process::geteuid().as_raw();
        let result = Self {
            directory: parent.dir_metadata()?,
            socket: parent.symlink_metadata("control.sock")?,
            parent,
            path,
            uid,
        };
        result.recheck()?;
        Ok(result)
    }
    fn recheck(&self) -> Result<()> {
        let current = parent(&self.path)?;
        let directory = current.dir_metadata()?;
        if self.uid == 0
            || !directory.is_dir()
            || directory.uid() != self.uid
            || directory.permissions().mode() & 0o7777 != 0o700
        {
            return Err(denied());
        }
        same(&self.directory, &directory)?;
        same(&self.directory, &self.parent.dir_metadata()?)?;
        let socket = self.parent.symlink_metadata("control.sock")?;
        if !socket.file_type().is_socket()
            || socket.uid() != self.uid
            || socket.nlink() != 1
            || socket.permissions().mode() & 0o7777 != 0o600
        {
            return Err(denied());
        }
        same(&self.socket, &socket)
    }
}
struct Rate {
    start: Instant,
    count: u32,
}
impl Rate {
    fn take(&mut self) -> Result<()> {
        if self.start.elapsed() >= Duration::from_secs(60) {
            self.start = Instant::now();
            self.count = 0;
        }
        if self.count >= 240 {
            return Err(denied());
        }
        self.count += 1;
        Ok(())
    }
}
async fn frame(
    reader: &mut tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
) -> Result<Option<Vec<u8>>> {
    crate::read_frame(reader).await
}

async fn connection_frame(
    reader: &mut tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
    idle: Duration,
    partial: Duration,
) -> Result<Option<Vec<u8>>> {
    // Empty metadata transport is not an authorization deadline. Once any byte
    // arrives, SAME existing bounded reader has one absolute non-renewable window.
    let empty = tokio::time::timeout(idle, reader.fill_buf())
        .await
        .map_err(|_| AutomationError::Timeout)??
        .is_empty();
    if empty {
        return Ok(None);
    }
    tokio::time::timeout(partial, frame(reader))
        .await
        .map_err(|_| AutomationError::Timeout)?
}
async fn connection_frame_until_finished(
    reader: &mut tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
    idle: Duration,
    partial: Duration,
    finished: &tokio_util::sync::CancellationToken,
) -> Result<Option<Vec<u8>>> {
    let result = tokio::select! {biased;
        _=finished.cancelled()=>return Ok(None),
        result=connection_frame(reader,idle,partial)=>result,
    };
    if finished.is_cancelled() { Ok(None) } else { result }
}
async fn send(writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>, bytes: Vec<u8>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut held = writer.lock().await;
        held.write_all(&bytes).await?;
        held.flush().await
    })
    .await
    .map_err(|_| AutomationError::Timeout)??;
    Ok(())
}
type ConnectionBootstrap = Arc<std::sync::Mutex<Option<Uuid>>>;
fn remember_bootstrap(
    owned: &ConnectionBootstrap,
    request: &Request,
    result: &Result<Value>,
) -> Result<()> {
    if request.operation == "service_bootstrap"
        && result
            .as_ref()
            .is_ok_and(|value| value.get("bootstrapped") == Some(&Value::Bool(true)))
    {
        let raw = request
            .body
            .get("schedule")
            .and_then(|v| v.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(denied)?;
        let id = Uuid::parse_str(raw).map_err(|_| denied())?;
        if id.is_nil() || id.to_string() != raw {
            return Err(denied());
        }
        let mut held = owned.lock().map_err(|_| denied())?;
        if held.is_some_and(|previous| previous != id) {
            return Err(denied());
        }
        *held = Some(id);
    }
    Ok(())
}
async fn connection(
    stream: UnixStream,
    dispatch: Arc<dispatch::Dispatcher>,
    pin: Arc<Pin>,
    rate: Arc<std::sync::Mutex<Rate>>,
) -> Result<()> {
    // No caller fields are read before actual kernel peer UID and namespace checks.
    if stream.peer_cred()?.uid() != pin.uid {
        return Err(denied());
    }
    pin.recheck()?;
    let (read, write) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read);
    let writer = Arc::new(Mutex::new(write));
    let normal = Arc::new(Semaphore::new(PENDING));
    let rejected = Arc::new(Semaphore::new(8));
    let statuses = Arc::new(Semaphore::new(2));
    let mut seen = HashSet::new();
    let mut pending = JoinSet::new();
    let bootstrap: ConnectionBootstrap = Arc::new(std::sync::Mutex::new(None));
    let result: Result<()> = async {
        loop {
            while let Some(completed) = pending.try_join_next() {
                completed.map_err(|_| denied())??;
            }
            let raw = connection_frame_until_finished(
                &mut reader,
                Duration::from_secs(300),
                Duration::from_secs(30),
                &dispatch.finished,
            )
            .await?;
            let Some(raw) = raw else {
                break;
            };
            pin.recheck()?;
            if dispatch.finished.is_cancelled() { break; }
            let request = parse_request(&raw, &mut seen)?;
            // Dedicated status capacity is reserved BEFORE signature/cancel.
            // Normal traffic cannot exhaust it. Once observed/accepted, delivery
            // is ALWAYS retained through original persistence; never drop it in
            // a later rate/full-pending branch after cancelling original material.
            let status = request.operation == "service_owner_status";
            let slots = if status {
                statuses.clone()
            } else {
                normal.clone()
            };
            let rate_allowed = if status {
                true
            } else {
                let mut held = rate.lock().map_err(|_| denied())?;
                held.take().is_ok()
            };
            let admitted = if rate_allowed {
                slots.try_acquire_owned().ok()
            } else {
                None
            };
            let Some(permit) = admitted else {
                let permit = rejected.clone().try_acquire_owned().map_err(|_| denied())?;
                let bytes = reply(&request, Err(denied()))?;
                let writer = writer.clone();
                pending.spawn(async move {
                    let _permit = permit;
                    send(&writer, bytes).await
                });
                continue;
            };
            let prepared = if status {
                Some(dispatch.admit_status(request.body.clone()).await)
            } else {
                None
            };
            let child_dispatch = dispatch.clone();
            let child_bootstrap = bootstrap.clone();
            let writer = writer.clone();
            pending.spawn(async move {
                let _permit = permit;
                let result = match prepared {
                    Some(Ok(delivery)) => child_dispatch.persist_status(delivery).await,
                    Some(Err(error)) => Err(error),
                    None => child_dispatch.dispatch(&request).await,
                };
                // Record actual successful bootstrap BEFORE reply delivery. A lost
                // ACK must not strand this connection's un-enrolled pending state.
                remember_bootstrap(&child_bootstrap, &request, &result)?;
                send(&writer, reply(&request, result)?).await
            });
            if dispatch.closed() {
                break;
            }
        }
        Ok(())
    }
    .await;
    // Losing a client's response is unknown to that client, not a rollback. Already
    // admitted work retains original Node/body through completion; no retry/cache.
    let mut failed = false;
    while let Some(completed) = pending.join_next().await {
        if !matches!(completed, Ok(Ok(()))) {
            failed = true;
        }
    }
    // Drain enrollment first: it may have durably committed despite a lost reply.
    // The SAME original dispatcher gate then refuses attempted/enrolled/restored.
    let original = *bootstrap.lock().map_err(|_| denied())?;
    if let Some(schedule) = original {
        dispatch.cancel_connection_pending(schedule).await;
    }
    if failed { Err(denied()) } else { result }
}
pub(super) fn run(launch: HelperLaunch, identity: crate::ServiceIdentity) -> Result<()> {
    if !matches!(launch, HelperLaunch::Native { .. }) {
        return Err(denied());
    }
    let inherited = inherited::take()?;
    let pin = Arc::new(Pin::capture(identity.facts().identity_id, &inherited)?);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = UnixListener::from_std(inherited)?;
        let dispatch =
            dispatch::Dispatcher::new(launch, ServiceNode::with_identity(identity)?).await?;
        let slots = Arc::new(Semaphore::new(CONNECTIONS));
        let rate = Arc::new(std::sync::Mutex::new(Rate {
            start: Instant::now(),
            count: 0,
        }));
        let mut clients = JoinSet::new();
        let result:Result<()>=async {
   loop {
    while let Some(_completed)=clients.try_join_next() {} // Bad client is not service revocation.
    tokio::select! {biased;
     _=dispatch.finished.cancelled()=>break,
     accepted=listener.accept()=>{
      let (stream,_)=accepted?;pin.recheck()?;
      if stream.peer_cred()?.uid()!=pin.uid {continue;}
      let permit=match slots.clone().try_acquire_owned(){Ok(v)=>v,Err(_)=>{continue;}};
      let dispatch=dispatch.clone();let pin=pin.clone();let rate=rate.clone();
      clients.spawn(async move {let _permit=permit;connection(stream,dispatch,pin,rate).await});
     }
    }
   }
   Ok(())
  }.await;
        drop(listener);
        // Stop only the original service, not a newly resolved node. Client EOF never
        // reaches this path; explicit service_close/original expiry/OS failure does.
        tokio::time::timeout(Duration::from_secs(180), async {
            dispatch.finish(true).await?;
            while clients.join_next().await.is_some() {}
            Ok::<_, AutomationError>(())
        })
        .await
        .map_err(|_| AutomationError::Timeout)??;
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;
    #[tokio::test]
    async fn original_finished_wakes_open_idle_and_partial_read_without_admitting_frame() {
        use std::future::Future;
        use std::task::Poll;
        for partial in [false, true] {
            let (left, right) = UnixStream::pair().unwrap();
            let (read, _) = left.into_split();
            let (_, mut write) = right.into_split();
            if partial { write.write_all(b"{\"version\":1").await.unwrap(); }
            let mut reader = tokio::io::BufReader::new(read);
            let finished = tokio_util::sync::CancellationToken::new();
            let waiting = connection_frame_until_finished(&mut reader,
                Duration::from_secs(300),Duration::from_secs(30),&finished);
            tokio::pin!(waiting);
            std::future::poll_fn(|cx| {
                assert!(matches!(waiting.as_mut().poll(cx),Poll::Pending));
                Poll::Ready(())
            }).await;
            finished.cancel();
            assert!(tokio::time::timeout(Duration::from_secs(1),waiting)
                .await.unwrap().unwrap().is_none());
        }
        let (left,right)=UnixStream::pair().unwrap();drop(right);
        let (read,_)=left.into_split();let mut reader=tokio::io::BufReader::new(read);
        let finished=tokio_util::sync::CancellationToken::new();
        assert!(connection_frame_until_finished(&mut reader,Duration::from_secs(300),
            Duration::from_secs(30),&finished).await.unwrap().is_none());
        assert!(!finished.is_cancelled(),"client EOF must not stop the shared service");
    }
    #[tokio::test]
    async fn original_finished_drains_actual_admitted_reply_before_connection_returns() {
        use tokio::io::AsyncReadExt;
        // Real named socket/UID and actual closed control dispatcher. No policy,
        // publisher, service enrollment or execution witness is synthesized.
        let own=tempfile::tempdir().unwrap();let path=own.path().join("peer");
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o700)).unwrap();
        let listener=UnixListener::bind(path.join("control.sock")).unwrap();
        std::fs::set_permissions(path.join("control.sock"),std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut client=UnixStream::connect(path.join("control.sock")).await.unwrap();
        let (server,_)=listener.accept().await.unwrap();
        let held=parent(&path).unwrap();let pin=Arc::new(Pin{directory:held.dir_metadata().unwrap(),
            socket:held.symlink_metadata("control.sock").unwrap(),parent:held,path,
            uid:rustix::process::geteuid().as_raw()});pin.recheck().unwrap();
        // Hold the ACK in the actual kernel send buffer while the original
        // service finishes. Drain its exact known filler before parsing ACK.
        server.writable().await.unwrap();let filler=[b'x';8192];let mut bytes=0;
        loop {match server.try_write(&filler){Ok(n)=>{assert!(n>0);bytes+=n;assert!(bytes<=4*1024*1024);},
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>break,Err(e)=>panic!("{e}")}}
        assert!(bytes>0);
        let dispatch=dispatch::Dispatcher::new(HelperLaunch::Native{python:"/unopened".into(),
            module_dir:"/unopened".into(),root:"/unopened".into()},ServiceNode::new().unwrap()).await.unwrap();
        let task=tokio::spawn(connection(server,dispatch.clone(),pin,
            Arc::new(std::sync::Mutex::new(Rate{start:Instant::now(),count:0}))));
        let id=Uuid::new_v4();let mut raw=serde_json::to_vec(&json!({"version":1,"request_id":id,
            "operation":"service_close","body":{}})).unwrap();raw.push(b'\n');client.write_all(&raw).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2),dispatch.finished.cancelled()).await.unwrap();
        assert!(!task.is_finished(),"admitted ACK must remain owned despite finished token");
        let mut drained=vec![0;bytes];client.read_exact(&mut drained).await.unwrap();assert!(drained.iter().all(|v|*v==b'x'));
        let mut reader=tokio::io::BufReader::new(client);let response=crate::read_frame(&mut reader).await.unwrap().unwrap();
        let response:Value=serde_json::from_slice(&response).unwrap();assert_eq!(response["request_id"],id.to_string());
        assert_eq!(response["result"],json!({"cancellation_requested":true}));assert!(response["error"].is_null());
        tokio::time::timeout(Duration::from_secs(2),task).await.unwrap().unwrap().unwrap();
        dispatch.finish(false).await.unwrap();
    }
    #[tokio::test]
    async fn bootstrap_transport_ownership_is_retained_before_lost_reply_only_on_success() {
        // Transport bookkeeping, not a fabricated enrollment/approval witness.
        let schedule = Uuid::new_v4();
        let owned: ConnectionBootstrap = Arc::new(std::sync::Mutex::new(None));
        let request = Request {
            version: 1,
            request_id: Uuid::new_v4(),
            operation: "service_bootstrap".into(),
            body: json!({"schedule":{"id":schedule}}),
        };
        remember_bootstrap(&owned, &request, &Err(denied())).unwrap();
        assert!(owned.lock().unwrap().is_none());
        let result = Ok(json!({"bootstrapped":true}));
        remember_bootstrap(&owned, &request, &result).unwrap();
        let (left, right) = UnixStream::pair().unwrap();
        drop(right);
        let (_, write) = left.into_split();
        assert!(
            send(
                &Arc::new(Mutex::new(write)),
                reply(&request, result).unwrap()
            )
            .await
            .is_err()
        );
        assert_eq!(*owned.lock().unwrap(), Some(schedule));
    }
    #[test]
    fn finite_rate_window_recovers_without_resetting_service_state() {
        let mut rate = Rate {
            start: Instant::now(),
            count: 0,
        };
        for _ in 0..240 {
            rate.take().unwrap();
        }
        assert!(rate.take().is_err());
        rate.start = Instant::now() - Duration::from_secs(60);
        rate.take().unwrap();
        assert_eq!(rate.count, 1);
    }
    #[test]
    fn actual_named_socket_permissions_and_directory_replacement_fail_closed() {
        let own = tempfile::tempdir().unwrap();
        let path = own.path().join("original");
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(path.join("control.sock")).unwrap();
        std::fs::set_permissions(
            path.join("control.sock"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let held = parent(&path).unwrap();
        let pin = Pin {
            directory: held.dir_metadata().unwrap(),
            socket: held.symlink_metadata("control.sock").unwrap(),
            parent: held,
            path: path.clone(),
            uid: rustix::process::geteuid().as_raw(),
        };
        pin.recheck().unwrap();
        std::fs::set_permissions(
            path.join("control.sock"),
            std::fs::Permissions::from_mode(0o660),
        )
        .unwrap();
        assert!(pin.recheck().is_err());
        std::fs::set_permissions(
            path.join("control.sock"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        pin.recheck().unwrap();
        std::fs::rename(&path, own.path().join("retained")).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(pin.recheck().is_err());
        drop(listener);
    }
    #[tokio::test]
    async fn actual_peer_credentials_are_kernel_observation() {
        let (left, right) = UnixStream::pair().unwrap();
        let uid = rustix::process::geteuid().as_raw();
        assert_eq!(left.peer_cred().unwrap().uid(), uid);
        assert_eq!(right.peer_cred().unwrap().uid(), uid);
    }
    #[tokio::test]
    async fn real_stream_frame_bound_and_eof_do_not_dispatch_or_approve() {
        let (left, right) = UnixStream::pair().unwrap();
        let (read, _) = left.into_split();
        let mut reader = tokio::io::BufReader::new(read);
        let (_, mut write) = right.into_split();
        let writer = tokio::spawn(async move {
            write.write_all(&vec![b'x'; MAX_FRAME_BYTES]).await.unwrap();
            write.write_all(b"\n").await.unwrap();
        });
        assert!(
            tokio::time::timeout(Duration::from_secs(5), frame(&mut reader))
                .await
                .unwrap()
                .is_err()
        );
        drop(reader);
        let _ = writer.await;
        let (left, right) = UnixStream::pair().unwrap();
        drop(right);
        let (read, _) = left.into_split();
        assert!(
            frame(&mut tokio::io::BufReader::new(read))
                .await
                .unwrap()
                .is_none()
        );
    }
    #[tokio::test]
    async fn metadata_idle_survives_partial_budget_then_accepts_exact_late_frame() {
        let (left, right) = UnixStream::pair().unwrap();
        let (read, _) = left.into_split();
        let (_, mut write) = right.into_split();
        let mut reader = tokio::io::BufReader::new(read);
        let waiting = connection_frame(
            &mut reader,
            Duration::from_millis(300),
            Duration::from_millis(20),
        );
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(60), waiting.as_mut())
                .await
                .is_err(),
            "no-byte human review must survive partial-frame budget"
        );
        write.write_all(b"{}\n").await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(100), waiting)
                .await
                .unwrap()
                .unwrap(),
            Some(b"{}\n".to_vec())
        );
    }
    #[tokio::test]
    async fn first_partial_byte_starts_short_absolute_deadline_without_idle_renewal() {
        let (left, right) = UnixStream::pair().unwrap();
        let (read, _) = left.into_split();
        let (_, mut write) = right.into_split();
        write.write_all(b"{").await.unwrap();
        let mut reader = tokio::io::BufReader::new(read);
        let result = tokio::time::timeout(
            Duration::from_millis(150),
            connection_frame(
                &mut reader,
                Duration::from_millis(300),
                Duration::from_millis(20),
            ),
        )
        .await
        .unwrap();
        assert!(matches!(result, Err(AutomationError::Timeout)));
    }
}
