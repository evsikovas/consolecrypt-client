use crate::{
    manager::SessionState, tls, types::validate_dimensions, CertificateInfo, ConnectConfig, Frame,
    Input, MouseButton, RdpError, SessionStatus,
};
use ironrdp::{
    connector::{
        self, connection_activation::ConnectionActivationState, ClientConnector,
        ClientConnectorState, Config, Credentials, Sequence,
    },
    core::WriteBuf,
    displaycontrol::client::DisplayControlClient,
    dvc::DrdynvcClient,
    graphics::image_processing::PixelFormat,
    input::{Database, MousePosition, Operation, Scancode, WheelRotations},
    pdu::{
        self,
        gcc::KeyboardType,
        rdp::{
            capability_sets::MajorPlatformType,
            client_info::{CompressionType, PerformanceFlags, TimezoneInfo},
        },
        PduHint,
    },
    session::{image::DecodedImage, ActiveStageBuilder, ActiveStageOutput},
};
use secrecy::{ExposeSecret, SecretString};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};
use zeroize::{Zeroize, Zeroizing};

const MAX_PDU: usize = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Unlike the upstream framed helper this checks hinted length BEFORE reserving memory.
struct BoundedIo<S> {
    stream: Option<S>,
    buffered: Vec<u8>,
    stopped: Arc<AtomicBool>,
}
impl<S> Drop for BoundedIo<S> {
    fn drop(&mut self) {
        self.buffered.zeroize();
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> BoundedIo<S> {
    fn new(stream: S, stopped: Arc<AtomicBool>) -> Self {
        Self {
            stream: Some(stream),
            buffered: Vec::new(),
            stopped,
        }
    }
    async fn fill(&mut self) -> Result<(), RdpError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(RdpError::SessionNotFound);
        }
        if self.buffered.len() >= MAX_PDU {
            return Err(RdpError::Protocol);
        }
        let mut chunk = Zeroizing::new([0; 8192]);
        let max = chunk.len().min(MAX_PDU - self.buffered.len());
        let n = self
            .stream
            .as_mut()
            .ok_or(RdpError::Connection)?
            .read(&mut chunk[..max])
            .await
            .map_err(|_| RdpError::Connection)?;
        if n == 0 {
            return Err(RdpError::Connection);
        }
        self.buffered.extend_from_slice(&chunk[..n]);
        Ok(())
    }
    fn take(&mut self, n: usize) -> Vec<u8> {
        let tail = self.buffered.split_off(n);
        std::mem::replace(&mut self.buffered, tail)
    }
    async fn by_hint(&mut self, hint: &dyn PduHint) -> Result<Vec<u8>, RdpError> {
        loop {
            match hint
                .find_size(&self.buffered)
                .map_err(|_| RdpError::Protocol)?
            {
                Some((matched, n)) => {
                    if n == 0 || n > MAX_PDU {
                        return Err(RdpError::Protocol);
                    }
                    while self.buffered.len() < n {
                        self.fill().await?;
                    }
                    let packet = self.take(n);
                    if matched {
                        return Ok(packet);
                    }
                }
                None => self.fill().await?,
            }
        }
    }
    async fn pdu(&mut self) -> Result<(pdu::Action, Zeroizing<Vec<u8>>), RdpError> {
        loop {
            if let Some(info) = pdu::find_size(&self.buffered).map_err(|_| RdpError::Protocol)? {
                if info.length == 0 || info.length > MAX_PDU {
                    return Err(RdpError::Protocol);
                }
                while self.buffered.len() < info.length {
                    self.fill().await?;
                }
                return Ok((info.action, Zeroizing::new(self.take(info.length))));
            }
            self.fill().await?;
        }
    }
    async fn exact(&mut self, n: usize) -> Result<Vec<u8>, RdpError> {
        if n > MAX_PDU {
            return Err(RdpError::Protocol);
        }
        while self.buffered.len() < n {
            self.fill().await?;
        }
        Ok(self.take(n))
    }
    async fn write(&mut self, data: &[u8]) -> Result<(), RdpError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(RdpError::SessionNotFound);
        }
        if data.len() > MAX_PDU {
            return Err(RdpError::Protocol);
        }
        tokio::time::timeout(WRITE_TIMEOUT, async {
            let stream = self.stream.as_mut().ok_or(RdpError::Connection)?;
            stream
                .write_all(data)
                .await
                .map_err(|_| RdpError::Connection)?;
            stream.flush().await.map_err(|_| RdpError::Connection)
        })
        .await
        .map_err(|_| RdpError::Timeout)?
    }
    fn into_stream(mut self) -> Result<S, RdpError> {
        if self.buffered.is_empty() {
            self.stream.take().ok_or(RdpError::Connection)
        } else {
            Err(RdpError::Protocol)
        }
    }
}

fn config(settings: &ConnectConfig) -> Config {
    Config {
        // The connector must never retain a password or clone it into its activation factory.
        credentials: Credentials::UsernamePassword {
            username: settings.username.clone(),
            password: String::new(),
        },
        domain: settings.domain.clone(),
        enable_tls: false,
        enable_credssp: true,
        desktop_size: connector::DesktopSize {
            width: settings.width,
            height: settings.height,
        },
        desktop_scale_factor: 100,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        bitmap: None,
        dig_product_id: String::new(),
        client_build: 0,
        client_name: "ConsoleCrypt".into(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".into(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        platform: MajorPlatformType::UNIX,
        hardware_id: None,
        request_data: None,
        autologon: false,
        enable_audio_playback: false,
        performance_flags: PerformanceFlags::default(),
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        compression_type: Some(CompressionType::Rdp61),
        // Upstream server-pointer cache does not enforce negotiated cache bounds.
        // The frontend uses its local basic cursor until a bounded pointer backend exists.
        enable_server_pointer: false,
        pointer_software_rendering: true,
        multitransport_flags: None,
    }
}

async fn step<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut BoundedIo<S>,
    sequence: &mut dyn Sequence,
) -> Result<(), RdpError> {
    let packet = if let Some(hint) = sequence.next_pdu_hint() {
        Some(Zeroizing::new(io.by_hint(hint).await?))
    } else {
        None
    };
    let mut out = WriteBuf::new();
    let written = match packet {
        Some(ref packet) => sequence.step(packet, &mut out),
        None => sequence.step_no_input(&mut out),
    }
    .map_err(|_| RdpError::Protocol)?;
    if written.size().is_some() {
        io.write(out.filled()).await?;
    }
    Ok(())
}

async fn begin(
    settings: &ConnectConfig,
    stopped: Arc<AtomicBool>,
) -> Result<(ClientConnector, TcpStream), RdpError> {
    if stopped.load(Ordering::Acquire) {
        return Err(RdpError::SessionNotFound);
    }
    let tcp = TcpStream::connect((settings.address.as_str(), settings.port))
        .await
        .map_err(|_| RdpError::Connection)?;
    tcp.set_nodelay(true).map_err(|_| RdpError::Connection)?;
    let local = tcp.local_addr().map_err(|_| RdpError::Connection)?;
    let mut io = BoundedIo::new(tcp, stopped);
    let mut connector = ClientConnector::new(config(settings), local);
    while !connector.should_perform_security_upgrade() {
        step(&mut io, &mut connector).await?;
    }
    Ok((connector, io.into_stream()?))
}

pub(crate) async fn probe(address: &str, port: u16) -> Result<CertificateInfo, RdpError> {
    let settings = ConnectConfig {
        address: address.into(),
        port,
        username: "certificate-probe".into(),
        domain: None,
        width: 800,
        height: 600,
        accepted_certificate_sha256: [1; 32],
    };
    settings.validate()?;
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        let (_, tcp) = begin(&settings, Arc::new(AtomicBool::new(false))).await?;
        let (_, _, info) = tls::upgrade(tcp, address, None).await?;
        Ok(info)
    })
    .await
    .map_err(|_| RdpError::Timeout)?
}

#[derive(Debug)]
struct CredsspHint;
impl PduHint for CredsspHint {
    fn find_size(&self, bytes: &[u8]) -> ironrdp::core::DecodeResult<Option<(bool, usize)>> {
        use connector::sspi::credssp::TsRequest;
        match TsRequest::read_length(bytes) {
            Ok(n) => Ok(Some((true, n))),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(_) => Err(ironrdp::core::other_err!("invalid CredSSP length")),
        }
    }
}

/// Construct SSPI's zeroizing Secret directly. No plaintext password enters IronRDP Config/Debug.
async fn nla<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut BoundedIo<S>,
    connector: &mut ClientConnector,
    settings: &ConnectConfig,
    password: SecretString,
    public_key: Vec<u8>,
) -> Result<(), RdpError> {
    use connector::sspi::{
        self,
        credssp::{ClientMode, ClientState, CredSspClient, CredSspMode, TsRequest},
        generator::GeneratorState,
    };
    if io.stopped.load(Ordering::Acquire) {
        return Err(RdpError::SessionNotFound);
    }
    let selected = match connector.state {
        ClientConnectorState::Credssp {
            selected_protocol, ..
        } => selected_protocol,
        _ => return Err(RdpError::Authentication),
    };
    let username = sspi::Username::new(&settings.username, settings.domain.as_deref())
        .map_err(|_| RdpError::InvalidConfig)?;
    let identity = sspi::AuthIdentity {
        username,
        password: sspi::Secret::new(password.expose_secret().to_owned()),
    };
    drop(password);
    let mut client = CredSspClient::new(
        public_key,
        identity.into(),
        CredSspMode::WithCredentials,
        ClientMode::Ntlm(sspi::ntlm::NtlmConfig::default()),
        format!("TERMSRV/{}", settings.address),
    )
    .map_err(|_| RdpError::Authentication)?;
    let mut request = TsRequest::default();
    loop {
        let result = {
            let mut generator = client.process(request);
            match generator.start() {
                GeneratorState::Completed(result) => {
                    result.map_err(|_| RdpError::Authentication)?
                }
                // No external KDC/HTTP authentication requests or automatic new-target credentials.
                GeneratorState::Suspended(_) => return Err(RdpError::Authentication),
            }
        };
        let (reply, done) = match result {
            ClientState::ReplyNeeded(reply) => (reply, false),
            ClientState::FinalMessage(reply) => (reply, true),
        };
        let mut encoded = Zeroizing::new(vec![
            0;
            usize::from(
                reply.buffer_len().map_err(|_| RdpError::Protocol)?
            )
        ]);
        if encoded.len() > MAX_PDU {
            return Err(RdpError::Protocol);
        }
        reply
            .encode_ts_request(encoded.as_mut_slice())
            .map_err(|_| RdpError::Authentication)?;
        io.write(&encoded).await?;
        if done {
            if selected.contains(pdu::nego::SecurityProtocol::HYBRID_EX) {
                let result = io
                    .exact(sspi::credssp::EARLY_USER_AUTH_RESULT_PDU_SIZE)
                    .await?;
                if !matches!(
                    sspi::credssp::EarlyUserAuthResult::from_buffer(result.as_slice())
                        .map_err(|_| RdpError::Authentication)?,
                    sspi::credssp::EarlyUserAuthResult::Success
                ) {
                    return Err(RdpError::Authentication);
                }
            }
            break;
        }
        let mut packet = io.by_hint(&CredsspHint).await?;
        request = TsRequest::from_buffer(&packet).map_err(|_| RdpError::Authentication)?;
        packet.zeroize();
    }
    connector.mark_credssp_as_done();
    Ok(())
}

pub(crate) async fn run(
    settings: ConnectConfig,
    password: SecretString,
    mut receiver: mpsc::Receiver<Vec<Input>>,
    state: Arc<Mutex<SessionState>>,
    stopped: Arc<AtomicBool>,
    redirects: crate::permissions::SharedRedirect,
    notify: Arc<tokio::sync::Notify>,
) -> Result<(), RdpError> {
    let (mut io, result, initial_permissions, initial_drive_id) =
        tokio::time::timeout(CONNECT_TIMEOUT, async {
            let (mut connector, tcp) = begin(&settings, stopped.clone()).await?;
            let (stream, public_key, _) = tls::upgrade(
                tcp,
                &settings.address,
                Some(settings.accepted_certificate_sha256),
            )
            .await?;
            connector.mark_security_upgrade_as_done();
            let mut io = BoundedIo::new(stream, stopped);
            nla(&mut io, &mut connector, &settings, password, public_key).await?;
            connector.attach_static_channel(ironrdp::cliprdr::CliprdrClient::new(Box::new(
                crate::clipboard::TextBackend::new(redirects.clone()),
            )));
            let (initial_permissions, initial_drive_id) = {
                let s = redirects.lock().map_err(|_| RdpError::Connection)?;
                (s.permissions.clone(), s.drive_id)
            };
            let initial_drive = initial_permissions
                .directory_grant_id
                .as_ref()
                .map(|_| vec![(initial_drive_id, "ConsoleCrypt".to_owned())]);
            connector.attach_static_channel(
                ironrdp::rdpdr::Rdpdr::new(
                    Box::new(crate::directory::DirectoryBackend::new(redirects.clone())),
                    "ConsoleCrypt".into(),
                )
                .with_drives(initial_drive),
            );
            // MS-RDPEFS Appendix A §2.1: Windows does not start RDPDR without RDPSND.
            // Empty formats/no-op handler enables the device handshake without audio capture/output.
            connector.attach_static_channel(ironrdp::rdpsnd::client::Rdpsnd::new(Box::new(
                ironrdp::rdpsnd::client::NoopRdpsndBackend,
            )));
            connector.attach_static_channel(
                DrdynvcClient::new()
                    .with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new()))),
            );
            loop {
                step(&mut io, &mut connector).await?;
                if matches!(connector.state, ClientConnectorState::Connected { .. }) {
                    if let ClientConnectorState::Connected { result } =
                        std::mem::replace(&mut connector.state, ClientConnectorState::Consumed)
                    {
                        break Ok((io, result, initial_permissions, initial_drive_id));
                    }
                }
            }
        })
        .await
        .map_err(|_| RdpError::Timeout)??;
    validate_dimensions(result.desktop_size.width, result.desktop_size.height)?;
    let activation_context = ActivationContext {
        io_channel_id: result.io_channel_id,
        user_channel_id: result.user_channel_id,
        share_id: result.share_id,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    };
    let activation_factory = result.activation_factory;
    let io_channel_id = result.io_channel_id;
    let message_channel_id = result.message_channel_id;
    let mut image = DecodedImage::new(
        PixelFormat::RgbA32,
        result.desktop_size.width,
        result.desktop_size.height,
    );
    let drive_channel_id = result
        .static_channels
        .get_channel_id_by_type::<ironrdp::rdpdr::Rdpdr>();
    let dvc_id = result
        .static_channels
        .get_channel_id_by_type::<DrdynvcClient>();
    let mut active = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();
    state.lock().map_err(|_| RdpError::Connection)?.status = SessionStatus::Connected;
    let mut announced_permissions = initial_permissions;
    let mut announced_drive_id = initial_drive_id;
    let mut database = Database::new();
    let mut limits = crate::limits::DecoderLimits::with_dvc_channel(dvc_id);
    loop {
        state
            .lock()
            .map_err(|_| RdpError::Connection)?
            .resize_available = active
            .get_dvc::<DisplayControlClient>()
            .is_some_and(|channel| channel.channel_id().is_some());
        let (outputs, from_input) = tokio::select! {
            packet=io.pdu()=>{
                set_stage(&state, "active_read")?;
                let (action,packet)=packet?;
                observe_drive_handshake(action, &packet, drive_channel_id, &redirects)?;
                set_stage(&state, "redirect_guard")?;
                reject_redirect(action,&packet,io_channel_id)?;
                set_stage(&state, "decoder_guard")?;
                limits.check(action,&packet,io_channel_id,message_channel_id)?;
                set_stage(&state, "active_decode")?;
                (active.process(&mut image,action,&packet).map_err(|error| decode_failure(&state, &error))?,false)
            },
            _=notify.notified()=>{(Vec::new(),false)},
            inputs=receiver.recv()=>{
                set_stage(&state, "input_encode")?;
                let Some(inputs)=inputs else { return Ok(()); };
                let mut outputs=Vec::new();
                let mut event_count=0u64;
                for input in &inputs {
                    match input {
                        Input::Resize{width,height}=>{
                            if let Some(frame)=active.encode_resize(u32::from(*width),u32::from(*height),Some(100),None) {
                                outputs.push(ActiveStageOutput::ResponseFrame(frame.map_err(|_|RdpError::Protocol)?));
                            }
                        },
                        _=>{
                            let events=input_events(&mut database,input);
                            event_count=event_count.saturating_add(events.len() as u64);
                            // FastPathInput has a four-bit event count, so never emit >15 per PDU.
                            for chunk in events.chunks(15) { outputs.extend(active.process_fastpath_input(&mut image,chunk).map_err(|_|RdpError::Protocol)?); }
                        }
                    }
                }
                {let mut state=state.lock().map_err(|_|RdpError::Connection)?;state.diagnostics.input_batches=state.diagnostics.input_batches.saturating_add(1);state.diagnostics.input_events=state.diagnostics.input_events.saturating_add(event_count);}
                (outputs,true)
            }
        };
        for output in outputs {
            match output {
                ActiveStageOutput::ResponseFrame(bytes) => {
                    set_stage(&state, "response_write")?;
                    io.write(&Zeroizing::new(bytes)).await?;
                    if from_input {
                        let mut state = state.lock().map_err(|_| RdpError::Connection)?;
                        state.diagnostics.input_pdus_written =
                            state.diagnostics.input_pdus_written.saturating_add(1);
                    }
                }
                ActiveStageOutput::GraphicsUpdate(_) => publish_frame(&state, &image)?,
                ActiveStageOutput::Terminate(_) => return Ok(()),
                ActiveStageOutput::DeactivateAll => {
                    {
                        let mut state = state.lock().map_err(|_| RdpError::Connection)?;
                        state.diagnostics.last_stage = "reactivation";
                        state.diagnostics.reactivations_started =
                            state.diagnostics.reactivations_started.saturating_add(1);
                    }
                    limits.require_reactivation_boundary()?;
                    let mut activation = activation_factory.create();
                    tokio::time::timeout(CONNECT_TIMEOUT, async {
                        loop {
                            // Activation hints describe only the main RDP sequence. Service
                            // channel packets may interleave; dispatch them instead of dropping
                            // them while waiting for DemandActive/finalization packets.
                            let mut out = WriteBuf::new();
                            let written = if let Some(hint) = activation.next_pdu_hint() {
                                let (action, packet) = io.pdu().await?;
                                let service_channel = action == pdu::Action::X224
                                    && pdu::mcs::decode_send_data_indication(&packet)
                                        .is_ok_and(|d| d.channel_id != io_channel_id);
                                let matched = hint
                                    .find_size(&packet)
                                    .map_err(|_| RdpError::Protocol)?
                                    .is_some_and(|(matched, _)| matched);
                                if service_channel || !matched {
                                    reject_redirect(action, &packet, io_channel_id)?;
                                    limits.check(
                                        action,
                                        &packet,
                                        io_channel_id,
                                        message_channel_id,
                                    )?;
                                    let outputs = active
                                        .process(&mut image, action, &packet)
                                        .map_err(|e| decode_failure(&state, &e))?;
                                    for output in outputs {
                                        match output {
                                            ActiveStageOutput::ResponseFrame(bytes) => {
                                                io.write(&Zeroizing::new(bytes)).await?
                                            }
                                            ActiveStageOutput::GraphicsUpdate(_) => {
                                                publish_frame(&state, &image)?
                                            }
                                            ActiveStageOutput::Terminate(_) => {
                                                return Err(RdpError::Connection)
                                            }
                                            ActiveStageOutput::MultitransportRequest(_) => {
                                                return Err(RdpError::Protocol)
                                            }
                                            _ => {}
                                        }
                                    }
                                    for (generation, bytes) in
                                        crate::clipboard::flush(&mut active, &redirects)?
                                    {
                                        let current = {
                                            let s = redirects
                                                .lock()
                                                .map_err(|_| RdpError::Connection)?;
                                            !s.closed && s.generation == generation
                                        };
                                        if current && !bytes.is_empty() {
                                            io.write(&bytes).await?;
                                        }
                                    }
                                    continue;
                                }
                                activation.step(&packet, &mut out)
                            } else {
                                activation.step_no_input(&mut out)
                            }
                            .map_err(|_| RdpError::Protocol)?;
                            if written.size().is_some() {
                                io.write(out.filled()).await?;
                            }
                            if let ConnectionActivationState::Finalized {
                                desktop_size,
                                share_id,
                                enable_server_pointer,
                                pointer_software_rendering,
                            } = activation.connection_activation_state()
                            {
                                let next_context = ActivationContext {
                                    io_channel_id: activation.io_channel_id(),
                                    user_channel_id: activation.user_channel_id(),
                                    share_id,
                                    enable_server_pointer,
                                    pointer_software_rendering,
                                };
                                if activation_context != next_context {
                                    set_stage(&state, "reactivation_context_changed")?;
                                    return Err(RdpError::Protocol);
                                }
                                // Keep the existing FastPath codec and bulk histories. A resize
                                // changes the image, not the transport compression dictionary.
                                image = resized_image(desktop_size.width, desktop_size.height)?;
                                active.set_share_id(share_id);
                                active.set_enable_server_pointer(enable_server_pointer);
                                {
                                    let mut state =
                                        state.lock().map_err(|_| RdpError::Connection)?;
                                    state.diagnostics.last_stage = "reactivated";
                                    state.diagnostics.reactivations_completed =
                                        state.diagnostics.reactivations_completed.saturating_add(1);
                                }
                                break Ok::<_, RdpError>(());
                            }
                        }
                    })
                    .await
                    .map_err(|_| RdpError::Timeout)??;
                }
                ActiveStageOutput::MultitransportRequest(_) => return Err(RdpError::Protocol),
                ActiveStageOutput::PointerDefault
                | ActiveStageOutput::PointerHidden
                | ActiveStageOutput::PointerPosition { .. }
                | ActiveStageOutput::PointerBitmap(_)
                | ActiveStageOutput::AutoDetect(_) => {}
            }
        }
        for (generation, bytes) in crate::clipboard::flush(&mut active, &redirects)? {
            let authorized = {
                let current = redirects.lock().map_err(|_| RdpError::Connection)?;
                !current.closed && current.generation == generation
            };
            if authorized && !bytes.is_empty() {
                io.write(&bytes).await?;
            }
        }
        let permissions = {
            let s = redirects.lock().map_err(|_| RdpError::Connection)?;
            if s.drive_ready {
                Some((s.permissions.clone(), s.drive_id))
            } else {
                None
            }
        };
        if let Some((permissions, drive_id)) = permissions {
            if needs_drive_update(
                &permissions,
                drive_id,
                &announced_permissions,
                announced_drive_id,
            ) {
                let mut messages = Vec::new();
                if let Some(channel) = active.get_svc_processor_mut::<ironrdp::rdpdr::Rdpdr>() {
                    if let Some(removed) = channel.remove_device(announced_drive_id) {
                        messages.push(ironrdp::svc::SvcMessage::from(
                            ironrdp::rdpdr::pdu::RdpdrPdu::ClientDeviceListRemove(removed),
                        ));
                    }
                    if permissions.directory_grant_id.is_some() {
                        messages.push(ironrdp::svc::SvcMessage::from(
                            ironrdp::rdpdr::pdu::RdpdrPdu::ClientDeviceListAnnounce(
                                channel.add_drive(drive_id, "ConsoleCrypt".into()),
                            ),
                        ));
                    }
                }
                if !messages.is_empty() {
                    let bytes = Zeroizing::new(
                        active
                            .process_svc_processor_messages(ironrdp::svc::SvcProcessorMessages::<
                                ironrdp::rdpdr::Rdpdr,
                            >::new(
                                messages
                            ))
                            .map_err(|_| RdpError::Protocol)?,
                    );
                    io.write(&bytes).await?;
                }
                announced_permissions = permissions;
                announced_drive_id = drive_id;
            }
        }
    }
}

fn needs_drive_update(
    current: &crate::SessionPermissions,
    current_id: u32,
    announced: &crate::SessionPermissions,
    announced_id: u32,
) -> bool {
    current.directory_grant_id != announced.directory_grant_id
        || current.directory_writable != announced.directory_writable
        || current_id != announced_id
}

// Only protocol handshake categories are retained; never variable packet fields or bodies.
fn observe_drive_handshake(
    action: pdu::Action,
    packet: &[u8],
    channel_id: Option<u16>,
    redirects: &crate::permissions::SharedRedirect,
) -> Result<(), RdpError> {
    if action != pdu::Action::X224 {
        return Ok(());
    }
    let Ok(data) = pdu::mcs::decode_send_data_indication(packet) else {
        return Ok(());
    };
    if Some(data.channel_id) != channel_id || data.user_data.len() < 12 {
        return Ok(());
    }
    let bytes = data.user_data;
    if u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| RdpError::Protocol)?) & 1 == 0
        || bytes[8..10] != [0x72, 0x44]
    {
        return Ok(());
    }
    let bit = match u16::from_le_bytes([bytes[10], bytes[11]]) {
        0x496e => 1,
        0x5350 => 2,
        0x4343 => 4,
        0x554c => 8,
        0x6472 => 16,
        _ => 0,
    };
    redirects
        .lock()
        .map_err(|_| RdpError::Connection)?
        .drive_handshake |= bit;
    Ok(())
}

fn set_stage(state: &Mutex<SessionState>, stage: &'static str) -> Result<(), RdpError> {
    state
        .lock()
        .map_err(|_| RdpError::Connection)?
        .diagnostics
        .last_stage = stage;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ActivationContext {
    io_channel_id: u16,
    user_channel_id: u16,
    share_id: u32,
    enable_server_pointer: bool,
    pointer_software_rendering: bool,
}

fn resized_image(width: u16, height: u16) -> Result<DecodedImage, RdpError> {
    validate_dimensions(width, height)?;
    Ok(DecodedImage::new(PixelFormat::RgbA32, width, height))
}

fn decode_failure(state: &Mutex<SessionState>, error: &ironrdp::session::SessionError) -> RdpError {
    use ironrdp::{
        core::{DecodeError, DecodeErrorKind},
        session::SessionErrorKind,
    };
    use std::error::Error;
    if let Ok(mut state) = state.lock() {
        let diagnostics = &mut state.diagnostics;
        diagnostics.decode_kind = match error.kind() {
            SessionErrorKind::Pdu(_) => "pdu",
            SessionErrorKind::Encode(_) => "encode",
            SessionErrorKind::Decode(_) => "decode",
            SessionErrorKind::Reason(_) => "reason",
            SessionErrorKind::General => "general",
            SessionErrorKind::Custom => "custom",
            _ => "other",
        };
        // Upstream's static source location is classified into a closed vocabulary.
        // Do not format context, Reason, source errors, paths or remote packet fields.
        let file = error.location().file();
        diagnostics.decode_site = if file.ends_with("/fast_path.rs") {
            "fast_path"
        } else if file.ends_with("/image.rs") {
            "image"
        } else if file.ends_with("/rfx.rs") {
            "rfx"
        } else if file.contains("/x224/") {
            "x224"
        } else {
            "other"
        };
        diagnostics.decode_subkind = "none";
        let mut source = error.source();
        for _ in 0..4 {
            let Some(current) = source else {
                break;
            };
            if let Some(decode) = current.downcast_ref::<DecodeError>() {
                diagnostics.decode_subkind = match decode.kind() {
                    DecodeErrorKind::NotEnoughBytes { .. } => "underflow",
                    DecodeErrorKind::InvalidField { .. } => "invalid_field",
                    DecodeErrorKind::UnexpectedMessageType { .. } => "message_type",
                    DecodeErrorKind::UnsupportedVersion { .. } => "version",
                    DecodeErrorKind::UnsupportedValue { .. } => "unsupported_value",
                    _ => "other",
                };
                break;
            }
            source = current.source();
        }
    }
    RdpError::Protocol
}

fn reject_redirect(action: pdu::Action, packet: &[u8], io_channel_id: u16) -> Result<(), RdpError> {
    if action == pdu::Action::X224 {
        if let Ok(data) = pdu::mcs::decode_send_data_indication(packet) {
            if data.channel_id != io_channel_id {
                return Ok(());
            }
            let user = data.user_data;
            if user.len() >= 2 {
                let header = u16::from_le_bytes([user[0], user[1]]);
                // SEC_REDIRECTION_PKT or TS_SHARECONTROLHEADER type ServerRedirect.
                if (header == 0x0400 && user.get(2..4) == Some(&[0, 0]))
                    || (user.len() >= 4
                        && u16::from_le_bytes([user[2], user[3]]) & 0x000f == 0x000a)
                {
                    return Err(RdpError::RedirectRejected);
                }
            }
        }
    }
    Ok(())
}

fn publish_frame(state: &Mutex<SessionState>, image: &DecodedImage) -> Result<(), RdpError> {
    validate_dimensions(image.width(), image.height())?;
    let mut state = state.lock().map_err(|_| RdpError::Connection)?;
    state.clear_frame();
    state.sequence = state.sequence.checked_add(1).ok_or(RdpError::Protocol)?;
    state.frame = Some(Frame {
        sequence: state.sequence,
        width: image.width(),
        height: image.height(),
        rgba: image.data().to_vec(),
    });
    Ok(())
}

pub(crate) fn validate_inputs(inputs: &[Input]) -> Result<(), RdpError> {
    if inputs.is_empty() || inputs.len() > 128 {
        return Err(RdpError::InvalidConfig);
    }
    let mut text_bytes = 0;
    for input in inputs {
        match input {
            Input::UnicodeText(text) => {
                text_bytes += text.len();
                if text_bytes > 8192 {
                    return Err(RdpError::InvalidConfig);
                }
            }
            Input::Resize { width, height } => validate_dimensions(*width, *height)?,
            Input::Pointer { x, y } if *x >= crate::MAX_WIDTH || *y >= crate::MAX_HEIGHT => {
                return Err(RdpError::InvalidConfig)
            }
            Input::Wheel { units, .. } if !(-255..=255).contains(units) => {
                return Err(RdpError::InvalidConfig)
            }
            _ => {}
        }
    }
    Ok(())
}

fn input_events(
    database: &mut Database,
    input: &Input,
) -> Vec<pdu::input::fast_path::FastPathInputEvent> {
    let ops = match input {
        Input::UnicodeText(text) => text
            .chars()
            .flat_map(|c| {
                [
                    Operation::UnicodeKeyPressed(c),
                    Operation::UnicodeKeyReleased(c),
                ]
            })
            .collect(),
        Input::Scancode {
            code,
            down,
            extended,
        } => {
            let code = Scancode::from_u8(*extended, *code);
            vec![if *down {
                Operation::KeyPressed(code)
            } else {
                Operation::KeyReleased(code)
            }]
        }
        Input::Pointer { x, y } => vec![Operation::MouseMove(MousePosition { x: *x, y: *y })],
        Input::Button { button, down } => {
            let button = match button {
                MouseButton::Left => ironrdp::input::MouseButton::Left,
                MouseButton::Middle => ironrdp::input::MouseButton::Middle,
                MouseButton::Right => ironrdp::input::MouseButton::Right,
                MouseButton::Back => ironrdp::input::MouseButton::X1,
                MouseButton::Forward => ironrdp::input::MouseButton::X2,
            };
            vec![if *down {
                Operation::MouseButtonPressed(button)
            } else {
                Operation::MouseButtonReleased(button)
            }]
        }
        Input::Wheel { units, horizontal } => vec![Operation::WheelRotations(WheelRotations {
            is_vertical: !*horizontal,
            rotation_units: *units,
        })],
        Input::ReleaseAll => return database.release_all().into_vec(),
        Input::Resize { .. } => return Vec::new(),
    };
    database.apply(ops).into_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};

    fn test_active(compression_type: Option<CompressionType>) -> ironrdp::session::ActiveStage {
        ActiveStageBuilder {
            static_channels: Default::default(),
            user_channel_id: 1002,
            io_channel_id: 1003,
            message_channel_id: None,
            share_id: 1,
            compression_type,
            enable_server_pointer: false,
            pointer_software_rendering: true,
        }
        .build()
    }

    #[test]
    fn coalesced_folder_permissions_still_require_new_announcement() {
        let read = crate::SessionPermissions {
            directory_grant_id: Some(uuid::Uuid::new_v4().to_string()),
            ..Default::default()
        };
        assert!(!needs_drive_update(&read, 1, &read, 1));
        // readonly→write→readonly may coalesce while transport is busy with a frame.
        assert!(needs_drive_update(&read, 3, &read, 1));
        let clipboard_only = crate::SessionPermissions {
            clipboard_enabled: true,
            ..read.clone()
        };
        assert!(!needs_drive_update(&clipboard_only, 1, &read, 1));
    }
    #[test]
    fn reactivated_processor_decodes_negotiated_bulk_compressed_bitmap() {
        use ironrdp::core::encode_vec;
        use pdu::{
            bitmap::{BitmapData, BitmapUpdateData, Compression},
            fast_path::{
                EncryptionFlags, FastPathHeader, FastPathUpdatePdu, Fragmentation, UpdateCode,
            },
            geometry::InclusiveRectangle,
            rdp::headers::CompressionFlags,
        };
        // Runtime synthetic solid-red BGR pixels; no remote captures or secret fixtures.
        let pixels = [0, 0, 255].repeat(32 * 32);
        let bitmap = encode_vec(&BitmapUpdateData {
            rectangles: vec![BitmapData {
                rectangle: InclusiveRectangle {
                    left: 0,
                    top: 0,
                    right: 31,
                    bottom: 31,
                },
                width: 32,
                height: 32,
                bits_per_pixel: 24,
                compression_flags: Compression::empty(),
                compressed_data_header: None,
                bitmap_data: &pixels,
            }],
        })
        .unwrap();
        let mut encoder =
            ironrdp_bulk::BulkCompressor::new(ironrdp_bulk::CompressionType::Rdp61).unwrap();
        let (length, flags) = encoder.compress(&bitmap).unwrap();
        assert_ne!(flags & ironrdp_bulk::flags::PACKET_COMPRESSED, 0);
        let body = encode_vec(&FastPathUpdatePdu {
            fragmentation: Fragmentation::Single,
            update_code: UpdateCode::Bitmap,
            compression_flags: Some(CompressionFlags::from_bits_retain(flags as u8 & 0xf0)),
            compression_type: Some(CompressionType::Rdp61),
            data: encoder.compressed_data(length),
        })
        .unwrap();
        let mut packet =
            encode_vec(&FastPathHeader::new(EncryptionFlags::empty(), body.len())).unwrap();
        packet.extend_from_slice(&body);
        let mut image = DecodedImage::new(PixelFormat::RgbA32, 1920, 1080);
        // The previous reactivation builder omitted the decoder and fails this real encoded PDU.
        let mut missing = test_active(None);
        assert!(missing
            .process(&mut image, pdu::Action::FastPath, &packet)
            .is_err());
        let mut processor = test_active(Some(CompressionType::Rdp61));
        let updates = processor
            .process(&mut image, pdu::Action::FastPath, &packet)
            .unwrap();
        assert!(updates
            .iter()
            .any(|u| matches!(u, ActiveStageOutput::GraphicsUpdate(_))));
        assert_eq!(&image.data()[..3], &[255, 0, 0]);
    }

    #[test]
    fn resize_retains_bulk_history_required_by_the_next_compressed_frame() {
        use ironrdp::core::encode_vec;
        use pdu::{
            bitmap::{BitmapData, BitmapUpdateData, Compression},
            fast_path::{
                EncryptionFlags, FastPathHeader, FastPathUpdatePdu, Fragmentation, UpdateCode,
            },
            geometry::InclusiveRectangle,
            rdp::headers::CompressionFlags,
        };
        let pixels = [7, 71, 197].repeat(32 * 32);
        let bitmap = encode_vec(&BitmapUpdateData {
            rectangles: vec![BitmapData {
                rectangle: InclusiveRectangle {
                    left: 0,
                    top: 0,
                    right: 31,
                    bottom: 31,
                },
                width: 32,
                height: 32,
                bits_per_pixel: 24,
                compression_flags: Compression::empty(),
                compressed_data_header: None,
                bitmap_data: &pixels,
            }],
        })
        .unwrap();
        for (pdu_type, bulk_type) in [
            (CompressionType::K64, ironrdp_bulk::CompressionType::Rdp5),
            (CompressionType::Rdp61, ironrdp_bulk::CompressionType::Rdp61),
        ] {
            let mut encoder = ironrdp_bulk::BulkCompressor::new(bulk_type).unwrap();
            let mut packets = Vec::new();
            for index in 0..2 {
                let (length, flags) = encoder.compress(&bitmap).unwrap();
                assert_ne!(flags & ironrdp_bulk::flags::PACKET_COMPRESSED, 0);
                if index == 1 {
                    assert_eq!(flags & ironrdp_bulk::flags::PACKET_FLUSHED, 0);
                }
                let body = encode_vec(&FastPathUpdatePdu {
                    fragmentation: Fragmentation::Single,
                    update_code: UpdateCode::Bitmap,
                    compression_flags: Some(CompressionFlags::from_bits_retain(flags as u8 & 0xf0)),
                    compression_type: Some(pdu_type),
                    data: encoder.compressed_data(length),
                })
                .unwrap();
                let mut packet =
                    encode_vec(&FastPathHeader::new(EncryptionFlags::empty(), body.len())).unwrap();
                packet.extend_from_slice(&body);
                packets.push(packet);
            }
            let mut active = test_active(Some(pdu_type));
            let mut image = resized_image(1280, 720).unwrap();
            active
                .process(&mut image, pdu::Action::FastPath, &packets[0])
                .unwrap();
            assert_eq!(&image.data()[..3], &[197, 71, 7]);
            // Only the image changes at reactivation. Compression history is transport state.
            image = resized_image(1920, 1080).unwrap();
            active
                .process(&mut image, pdu::Action::FastPath, &packets[1])
                .unwrap();
            assert_eq!(&image.data()[..3], &[197, 71, 7]);
            let mut fresh = test_active(Some(pdu_type));
            let mut wrong_image = resized_image(1920, 1080).unwrap();
            let result = fresh.process(&mut wrong_image, pdu::Action::FastPath, &packets[1]);
            assert!(result.is_err() || wrong_image.data()[..3] != [197, 71, 7]);
        }
    }

    #[test]
    fn decode_diagnostics_never_include_remote_error_details() {
        use ironrdp::session::SessionErrorExt;
        let state = Mutex::new(SessionState {
            status: SessionStatus::Connected,
            frame: None,
            sequence: 0,
            resize_available: false,
            diagnostics: Default::default(),
        });
        let error = ironrdp::session::SessionError::reason(
            "remote supplied context",
            "remote supplied reason",
        );
        assert_eq!(decode_failure(&state, &error), RdpError::Protocol);
        let diagnostics = state.lock().unwrap().diagnostics.clone();
        assert_eq!(diagnostics.decode_kind, "reason");
        assert_eq!(diagnostics.decode_site, "other");
        assert_eq!(diagnostics.decode_subkind, "none");
        assert!(!format!("{diagnostics:?}").contains("remote supplied"));
    }

    struct BufferedWriter {
        buffered: Vec<u8>,
        committed: Arc<Mutex<Vec<u8>>>,
    }
    impl AsyncRead for BufferedWriter {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Pending
        }
    }
    impl AsyncWrite for BufferedWriter {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            bytes: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            self.get_mut().buffered.extend_from_slice(bytes);
            std::task::Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let this = self.get_mut();
            let pending = std::mem::take(&mut this.buffered);
            this.committed.lock().unwrap().extend_from_slice(&pending);
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            self.poll_flush(cx)
        }
    }
    #[tokio::test]
    async fn input_write_flushes_buffered_transport_before_reporting_success() {
        let committed = Arc::new(Mutex::new(Vec::new()));
        let stream = BufferedWriter {
            buffered: Vec::new(),
            committed: committed.clone(),
        };
        let stopped = Arc::new(AtomicBool::new(false));
        let mut io = BoundedIo::new(stream, stopped.clone());
        io.write(&[1, 2, 3]).await.unwrap();
        assert_eq!(*committed.lock().unwrap(), vec![1, 2, 3]);
        stopped.store(true, Ordering::Release);
        assert_eq!(io.write(&[4]).await, Err(RdpError::SessionNotFound));
        assert_eq!(*committed.lock().unwrap(), vec![1, 2, 3]);
    }

    #[derive(Debug)]
    struct OversizedHint;
    impl PduHint for OversizedHint {
        fn find_size(&self, _: &[u8]) -> ironrdp::core::DecodeResult<Option<(bool, usize)>> {
            Ok(Some((true, MAX_PDU + 1)))
        }
    }
    #[tokio::test]
    async fn oversized_hint_is_rejected_before_allocation_or_read() {
        let (a, _) = tokio::io::duplex(8);
        let mut io = BoundedIo::new(a, Arc::new(AtomicBool::new(false)));
        assert_eq!(io.by_hint(&OversizedHint).await, Err(RdpError::Protocol));
        assert_eq!(io.buffered.capacity(), 0);
    }
    #[tokio::test]
    async fn interrupted_pdu_read_keeps_bytes_for_the_next_poll() {
        let (a, mut b) = tokio::io::duplex(32);
        let mut io = BoundedIo::new(a, Arc::new(AtomicBool::new(false)));
        b.write_all(&[0, 3]).await.unwrap();
        assert!(tokio::time::timeout(Duration::from_millis(10), io.pdu())
            .await
            .is_err());
        b.write_all(&[0x42]).await.unwrap();
        let (action, packet) = io.pdu().await.unwrap();
        assert_eq!(action, pdu::Action::FastPath);
        assert_eq!(packet.as_slice(), &[0, 3, 0x42]);
    }
    #[test]
    fn unicode_encodes_cyrillic_and_surrogate_pairs_with_releases() {
        let mut db = Database::new();
        let events = input_events(&mut db, &Input::UnicodeText("Я😀".into()));
        let codes: Vec<_> = events
            .iter()
            .filter_map(|event| {
                if let FastPathInputEvent::UnicodeKeyboardEvent(flags, code) = event {
                    Some((*flags, *code))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            codes,
            vec![
                (KeyboardFlags::empty(), 0x42f),
                (KeyboardFlags::RELEASE, 0x42f),
                (KeyboardFlags::empty(), 0xd83d),
                (KeyboardFlags::empty(), 0xde00),
                (KeyboardFlags::RELEASE, 0xd83d),
                (KeyboardFlags::RELEASE, 0xde00)
            ]
        );
        assert!(db.release_all().is_empty());
    }
    #[test]
    fn focus_loss_releases_held_scancodes_and_mouse_buttons() {
        let mut db = Database::new();
        input_events(
            &mut db,
            &Input::Scancode {
                code: 0x5b,
                down: true,
                extended: true,
            },
        );
        input_events(
            &mut db,
            &Input::Button {
                button: MouseButton::Left,
                down: true,
            },
        );
        assert_eq!(input_events(&mut db, &Input::ReleaseAll).len(), 2);
        assert!(input_events(&mut db, &Input::ReleaseAll).is_empty());
    }
    #[test]
    fn mailbox_retains_only_latest_frame_and_poll_consumes_it() {
        let state = Mutex::new(SessionState {
            status: SessionStatus::Connected,
            frame: None,
            sequence: 0,
            resize_available: false,
            diagnostics: crate::SessionDiagnostics::default(),
        });
        let image = DecodedImage::new(PixelFormat::RgbA32, 800, 600);
        publish_frame(&state, &image).unwrap();
        publish_frame(&state, &image).unwrap();
        let mut state = state.lock().unwrap();
        let frame = state.frame.take().unwrap();
        assert_eq!(frame.sequence, 2);
        assert_eq!(frame.rgba.len(), 800 * 600 * 4);
        assert!(state.frame.is_none());
    }
    #[test]
    fn resource_limits_reject_oversized_frames_and_input_batches() {
        assert!(validate_dimensions(4096, 2160).is_ok());
        assert!(validate_dimensions(4098, 2160).is_err());
        assert!(validate_dimensions(800, 2161).is_err());
        assert!(validate_inputs(&vec![Input::ReleaseAll; 129]).is_err());
        assert!(validate_inputs(&[Input::UnicodeText("x".repeat(8193))]).is_err());
        assert!(validate_inputs(&[Input::Wheel {
            units: 256,
            horizontal: false
        }])
        .is_err());
    }
    #[test]
    fn connector_never_contains_password_in_its_cloned_config() {
        let settings = ConnectConfig {
            address: "localhost".into(),
            port: 3389,
            username: "fixture".into(),
            domain: None,
            width: 800,
            height: 600,
            accepted_certificate_sha256: [1; 32],
        };
        let cfg = config(&settings);
        assert!(
            matches!(cfg.credentials,Credentials::UsernamePassword{ref password,..} if password.is_empty())
        );
        assert!(!cfg.enable_tls && cfg.enable_credssp && !cfg.autologon);
    }
}
