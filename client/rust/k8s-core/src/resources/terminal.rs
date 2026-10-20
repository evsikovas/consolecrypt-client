use super::*;
use futures::SinkExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{AttachParams, Request as KubeRequest, TerminalSize},
    Api,
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PodTerminalRequest {
    pub namespace: String,
    pub pod: String,
    pub container: Option<String>,
    /// None attaches to the existing process; Some executes an argv vector.
    /// Shell metacharacters are never interpreted on the local machine.
    pub command: Option<Vec<String>>,
    #[serde(default = "yes")]
    pub tty: bool,
}

pub enum PodTerminalInput {
    Stdin(Zeroizing<Vec<u8>>),
    Resize { columns: u16, rows: u16 },
    Eof,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum PodTerminalOutput {
    Connected,
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exited { code: Option<i32> },
}

redacted_debug!(PodTerminalRequest, PodTerminalInput, PodTerminalOutput);

#[derive(Debug, Clone, Serialize)]
pub struct ForwardUpdate {
    pub active_connections: usize,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub failed_connections: u64,
}

pub struct ForwardListener {
    listener: TcpListener,
    api: ResourceApi,
    namespace: String,
    pod: String,
    remote_port: u16,
    network: bool,
    stop: CancellationToken,
}
redacted_debug!(ForwardListener);

impl Drop for ForwardListener {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl ResourceApi {
    pub async fn terminal(
        &self,
        request: PodTerminalRequest,
        mut input: mpsc::Receiver<PodTerminalInput>,
        output: mpsc::Sender<PodTerminalOutput>,
        stop: CancellationToken,
    ) -> Result<(), ResourceError> {
        validate_terminal(&request)?;
        self.resolve("", "v1", "pods")?;
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &request.namespace);
        let params = AttachParams {
            container: request.container,
            stdin: true,
            stdout: true,
            stderr: !request.tty,
            tty: request.tty,
            max_stdin_buf_size: Some(64 * 1024),
            max_stdout_buf_size: Some(64 * 1024),
            max_stderr_buf_size: Some(64 * 1024),
        };
        let closed = self.guard.cancellation();
        tokio::select! {
            biased;
            _=closed.cancelled()=>Err(ResourceError::Closed),
            _=stop.cancelled()=>Ok(()),
            _=output.closed()=>Ok(()),
            result=async {
                let mut process=tokio::time::timeout(Duration::from_secs(30),async {
                    match request.command {
                        Some(command)=>pods.exec(&request.pod,command,&params).await,
                        None=>pods.attach(&request.pod,&params).await,
                    }
                }).await.map_err(|_|ResourceError::Timeout)?.map_err(|_|ResourceError::Transport)?;
                output.send(PodTerminalOutput::Connected).await.map_err(|_|ResourceError::Closed)?;
                let mut stdin=process.stdin();
                let mut stdout=process.stdout();
                let mut stderr=process.stderr();
                let mut resize=process.terminal_size();
                let status=process.take_status().ok_or(ResourceError::Transport)?;
                tokio::pin!(status);
                let mut exit=None;
                let mut input_open=true;
                let mut out_buffer=Zeroizing::new(vec![0;16*1024]);
                let mut err_buffer=Zeroizing::new(vec![0;16*1024]);
                loop {
                    if exit.is_some() && stdout.is_none() && stderr.is_none() { break; }
                    tokio::select! {
                        status=&mut status, if exit.is_none()=>{
                            let code=status.as_ref().and_then(|status| {
                                if status.status.as_deref()==Some("Success") { return Some(0); }
                                status.details.as_ref()?.causes.as_ref()?.iter()
                                    .find(|cause| cause.reason.as_deref()==Some("ExitCode"))?.message.as_ref()?.parse().ok()
                            });
                            exit=Some(code);
                            stdin=None;
                            input_open=false;
                        }
                        read=async {
                            match stdout.as_mut() {
                                Some(reader)=>reader.read(&mut out_buffer).await,
                                None=>std::future::pending().await,
                            }
                        },if stdout.is_some()=>{
                            let size=read.map_err(|_|ResourceError::Transport)?;
                            if size==0 { stdout=None; }
                            else { output.send(PodTerminalOutput::Stdout(out_buffer[..size].to_vec())).await.map_err(|_|ResourceError::Closed)?; }
                        }
                        read=async {
                            match stderr.as_mut() {
                                Some(reader)=>reader.read(&mut err_buffer).await,
                                None=>std::future::pending().await,
                            }
                        },if stderr.is_some()=>{
                            let size=read.map_err(|_|ResourceError::Transport)?;
                            if size==0 { stderr=None; }
                            else { output.send(PodTerminalOutput::Stderr(err_buffer[..size].to_vec())).await.map_err(|_|ResourceError::Closed)?; }
                        }
                        value=input.recv(),if input_open=>{
                            match value {
                                Some(PodTerminalInput::Stdin(bytes))=>{
                                    if bytes.len()>64*1024 { return Err(ResourceError::TooLarge); }
                                    if let Some(writer)=stdin.as_mut() {
                                        writer.write_all(&bytes).await.map_err(|_|ResourceError::Transport)?;
                                    }
                                }
                                Some(PodTerminalInput::Resize {columns,rows})=>{
                                    if columns==0 || rows==0 || columns>1000 || rows>1000 { return Err(ResourceError::InvalidRequest); }
                                    if let Some(sender)=resize.as_mut() {
                                        sender.send(TerminalSize {width:columns,height:rows}).await.map_err(|_|ResourceError::Transport)?;
                                    }
                                }
                                Some(PodTerminalInput::Eof)|None=>{
                                    if let Some(mut writer)=stdin.take() { let _=writer.shutdown().await; }
                                    input_open=false;
                                }
                            }
                        }
                    }
                }
                process.join().await.map_err(|_|ResourceError::Transport)?;
                output.send(PodTerminalOutput::Exited {code:exit.flatten()}).await.map_err(|_|ResourceError::Closed)?;
                Ok(())
            }=>result,
        }
    }

    /// Invoke only for a user-confirmed forwarding action. The listener is
    /// loopback by default, and each accepted connection receives a single-use
    /// native confirmation bound to this fixed target and this session.
    pub async fn forward_listener(
        &self,
        namespace: &str,
        pod: &str,
        remote_port: u16,
        local_port: u16,
    ) -> Result<ForwardListener, ResourceError> {
        self.forward_listener_on(
            namespace,
            pod,
            remote_port,
            SocketAddr::from((Ipv4Addr::LOCALHOST, local_port)),
            false,
        )
        .await
    }

    /// The bind address is a literal IP. Network access requires an explicit
    /// acknowledgement and a desktop non-production policy before any bind.
    pub async fn forward_listener_on(
        &self,
        namespace: &str,
        pod: &str,
        remote_port: u16,
        address: SocketAddr,
        approve_network: bool,
    ) -> Result<ForwardListener, ResourceError> {
        validate_forward(namespace, pod, remote_port)?;
        let network = !address.ip().is_loopback();
        if address.ip().is_multicast() || address.ip() == IpAddr::V4(Ipv4Addr::BROADCAST) {
            return Err(ResourceError::InvalidRequest);
        }
        if network {
            self.guard
                .allow_network_forward(approve_network)
                .map_err(|_| ResourceError::Transport)?;
        }
        self.resolve("", "v1", "pods")?;
        let req = forward_request(namespace, pod, remote_port)?;
        self.guard
            .confirm_port_forward(req.method().clone(), req.uri().clone())
            .map_err(|_| ResourceError::Transport)?;
        let listener = TcpListener::bind(address)
            .await
            .map_err(|_| ResourceError::Transport)?;
        if self.guard.cancellation().is_cancelled() {
            return Err(ResourceError::Closed);
        }
        Ok(ForwardListener {
            listener,
            api: self.clone(),
            namespace: namespace.into(),
            pod: pod.into(),
            remote_port,
            network,
            stop: CancellationToken::new(),
        })
    }
}

impl ForwardListener {
    pub fn address(&self) -> Result<SocketAddr, ResourceError> {
        self.listener
            .local_addr()
            .map_err(|_| ResourceError::Transport)
    }
    pub fn cancellation(&self) -> CancellationToken {
        self.stop.clone()
    }

    pub async fn serve(self, updates: mpsc::Sender<ForwardUpdate>) -> Result<(), ResourceError> {
        let closed = self.api.guard.cancellation();
        let mut jobs = JoinSet::new();
        let sent = Arc::new(AtomicU64::new(0));
        let received = Arc::new(AtomicU64::new(0));
        let mut failures = 0_u64;
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                biased;
                _=closed.cancelled()=>return Err(ResourceError::Closed),
                _=self.stop.cancelled()=>return Ok(()),
                _=updates.closed()=>return Ok(()),
                result=jobs.join_next(),if !jobs.is_empty()=>{
                    if !matches!(result,Some(Ok(Ok(())))) { failures=failures.saturating_add(1); }
                }
                _=interval.tick()=>{
                    // A slow UI must not stall active connections.
                    let _=updates.try_send(ForwardUpdate {active_connections:jobs.len(),bytes_sent:sent.load(Ordering::Relaxed),bytes_received:received.load(Ordering::Relaxed),failed_connections:failures});
                }
                accepted=self.listener.accept()=>{
                    let (socket,address)=accepted.map_err(|_|ResourceError::Transport)?;
                    if (!self.network && !address.ip().is_loopback()) || jobs.len()>=32 { drop(socket); continue; }
                    let api=self.api.clone();
                    let namespace=self.namespace.clone();
                    let pod=self.pod.clone();
                    let port=self.remote_port;
                    let sent=sent.clone();
                    let received=received.clone();
                    let stop=self.stop.clone();
                    jobs.spawn(async move {
                        let closed=api.guard.cancellation();
                        tokio::select! {
                            biased;
                            _=closed.cancelled()=>Err(ResourceError::Closed),
                            _=stop.cancelled()=>Ok(()),
                            result=forward_connection(&api,&namespace,&pod,port,socket,sent,received)=>result,
                        }
                    });
                }
            }
        }
    }
}

async fn forward_connection(
    api: &ResourceApi,
    namespace: &str,
    pod: &str,
    remote_port: u16,
    socket: TcpStream,
    sent: Arc<AtomicU64>,
    received: Arc<AtomicU64>,
) -> Result<(), ResourceError> {
    let request = forward_request(namespace, pod, remote_port)?;
    let expected_method = request.method().clone();
    let expected_uri = request.uri().clone();
    let approval = api
        .guard
        .confirm_port_forward(expected_method.clone(), expected_uri.clone())
        .map_err(|_| ResourceError::Transport)?;
    let inner = api.client.clone();
    let approval = Arc::new(std::sync::Mutex::new(Some(approval)));
    // kube's Portforwarder constructor is deliberately private. This one-shot
    // service decorates exactly the request Api<Pod>::portforward generates,
    // while forwarding through the original guarded client.
    let decorated = Client::new(
        tower::service_fn(move |mut request: Request<Body>| {
            let inner = inner.clone();
            let approval = if request.method() == expected_method && request.uri() == &expected_uri
            {
                approval.lock().ok().and_then(|mut value| value.take())
            } else {
                None
            };
            async move {
                let approval = approval
                    .ok_or_else(|| kube::Error::Service(Box::new(ResourceError::InvalidRequest)))?;
                request.extensions_mut().insert(approval);
                inner.send(request).await
            }
        }),
        namespace,
    );
    let pods: Api<Pod> = Api::namespaced(decorated, namespace);
    let forward = tokio::time::timeout(
        Duration::from_secs(30),
        pods.portforward(pod, &[remote_port]),
    )
    .await
    .map_err(|_| ResourceError::Timeout)?
    .map_err(|_| ResourceError::Transport)?;
    let mut forward = AbortForward(forward);
    let mut remote = forward
        .0
        .take_stream(remote_port)
        .ok_or(ResourceError::Transport)?;
    let mut socket = CountedSocket {
        socket,
        sent,
        received,
    };
    tokio::io::copy_bidirectional(&mut socket, &mut remote)
        .await
        .map_err(|_| ResourceError::Transport)?;
    Ok(())
}

struct CountedSocket {
    socket: TcpStream,
    sent: Arc<AtomicU64>,
    received: Arc<AtomicU64>,
}
impl tokio::io::AsyncRead for CountedSocket {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buffer.filled().len();
        let result = std::pin::Pin::new(&mut self.socket).poll_read(cx, buffer);
        if matches!(result, std::task::Poll::Ready(Ok(()))) {
            self.sent
                .fetch_add((buffer.filled().len() - before) as u64, Ordering::Relaxed);
        }
        result
    }
}
impl tokio::io::AsyncWrite for CountedSocket {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let result = std::pin::Pin::new(&mut self.socket).poll_write(cx, buffer);
        if let std::task::Poll::Ready(Ok(count)) = result {
            self.received.fetch_add(count as u64, Ordering::Relaxed);
        }
        result
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.socket).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}

struct AbortForward(kube::api::Portforwarder);
impl Drop for AbortForward {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn forward_request(
    namespace: &str,
    pod: &str,
    port: u16,
) -> Result<Request<Vec<u8>>, ResourceError> {
    KubeRequest::new(format!("/api/v1/namespaces/{namespace}/pods"))
        .portforward(pod, &[port])
        .map_err(|_| ResourceError::InvalidRequest)
}
fn validate_forward(namespace: &str, pod: &str, port: u16) -> Result<(), ResourceError> {
    if !dns_name(namespace, 63) || !dns_name(pod, 253) || port == 0 {
        return Err(ResourceError::InvalidRequest);
    }
    Ok(())
}
fn validate_terminal(request: &PodTerminalRequest) -> Result<(), ResourceError> {
    if !dns_name(&request.namespace, 63)
        || !dns_name(&request.pod, 253)
        || request
            .container
            .as_deref()
            .is_some_and(|c| !dns_name(c, 63))
        || request.command.as_ref().is_some_and(|command| {
            command.is_empty()
                || command.len() > 64
                || command.iter().map(String::len).sum::<usize>() > 64 * 1024
                || command.iter().any(|argument| argument.contains('\0'))
                || command[0].is_empty()
        })
    {
        return Err(ResourceError::InvalidRequest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_argv_and_forward_target_reject_ambiguous_input() {
        let mut request = PodTerminalRequest {
            namespace: "apps".into(),
            pod: "web".into(),
            container: None,
            command: Some(vec!["/bin/sh".into()]),
            tty: true,
        };
        assert!(validate_terminal(&request).is_ok());
        request.namespace = "../apps".into();
        assert!(validate_terminal(&request).is_err());
        request.namespace = "apps".into();
        request.command = Some(Vec::new());
        assert!(validate_terminal(&request).is_err());
        assert!(validate_forward("apps", "web", 0).is_err());
        assert!(validate_forward("apps", "web?x=1", 80).is_err());
    }
}
