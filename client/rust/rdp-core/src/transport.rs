use crate::{
    manager::{SessionCommand, SessionState},
    tls,
    types::validate_dimensions,
    CertificateInfo, ConnectConfig, Frame, Input, MouseButton, RdpError, SessionStatus,
};
use ironrdp::{
    connector::{
        self,
        connection_activation::{ConnectionActivationSequence, ConnectionActivationState},
        ClientConnector, ClientConnectorState, Config, Credentials, Sequence,
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
    collections::VecDeque,
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
// Writes contain a batch of framed SVC fragments, rather than one inbound PDU.
// A bounded 1MiB drive reply has additional RDPDR/MCS/SVC framing overhead.
const MAX_WRITE_BATCH: usize = 2 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const LARGE_WRITE_THRESHOLD: usize = 64 * 1024;
const LARGE_WRITE_BUDGET: Duration = Duration::from_secs(120);
#[path = "transport_writer.rs"]
mod ordered_writer;

#[cfg(test)]
fn record_write_progress(
    state: &Option<Arc<Mutex<SessionState>>>,
    f: impl FnOnce(&mut crate::SessionDiagnostics),
) {
    if let Some(state) = state {
        if let Ok(mut state) = state.lock() {
            f(&mut state.diagnostics);
        }
    }
}

#[cfg(test)]
fn write_read_packet_metadata(
    bytes: &[u8],
    context: Option<(u16, Option<u16>)>,
) -> Vec<crate::types::WriteReadPacketDiagnostic> {
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() && packets.len() < 8 {
        let Ok(Some(info)) = pdu::find_size(&bytes[offset..]) else {
            break;
        };
        if info.length == 0 || info.length > bytes.len() - offset {
            break;
        }
        let packet = &bytes[offset..offset + info.length];
        offset += info.length;
        let mut entry = crate::types::WriteReadPacketDiagnostic {
            action: match info.action {
                pdu::Action::FastPath => "fastpath",
                pdu::Action::X224 => "x224",
            },
            length: info.length,
            channel: None,
            channel_kind: "none",
            control: "none",
            static_flags: 0,
        };
        if info.action == pdu::Action::X224 {
            if let Ok(data) = pdu::mcs::decode_send_data_indication(packet) {
                entry.channel = Some(data.channel_id);
                let data_bytes = data.user_data;
                match context {
                    Some((io, _)) if data.channel_id == io => {
                        entry.channel_kind = "io";
                        if data_bytes.len() >= 6
                            && usize::from(u16::from_le_bytes([data_bytes[0], data_bytes[1]]))
                                == data_bytes.len()
                        {
                            let control = u16::from_le_bytes([data_bytes[2], data_bytes[3]]) & 0xf;
                            entry.control = match control {
                                1 => "demand_active",
                                3 => "confirm_active",
                                6 => "deactivate_all",
                                10 => "redirect",
                                7 if data_bytes.len() >= 18 => match data_bytes[14] {
                                    2 => "update",
                                    0x14 => "control",
                                    0x1b => "pointer",
                                    0x1f => "synchronize",
                                    0x26 => "session_info",
                                    0x29 => "keyboard_indicators",
                                    0x2f => "error_info",
                                    0x38 => "frame_ack",
                                    _ => "other_data",
                                },
                                _ => "other_share",
                            };
                        } else {
                            entry.control = "non_share";
                        }
                    }
                    Some((_, Some(message))) if data.channel_id == message => {
                        entry.channel_kind = "message";
                        if data_bytes.len() >= 10 {
                            entry.control = match u16::from_le_bytes([data_bytes[8], data_bytes[9]])
                            {
                                0x0001 | 0x1001 => "rtt_request",
                                0x0014 | 0x0114 | 0x1014 => "bandwidth_start",
                                2 => "bandwidth_payload",
                                0x002b | 0x0429 | 0x0629 => "bandwidth_stop",
                                0x0840 | 0x0880 | 0x08c0 => "network_characteristics",
                                _ => "other_message",
                            };
                        }
                    }
                    Some(_) => {
                        entry.channel_kind = "static";
                        if data_bytes.len() >= 8 {
                            // Only known header bits: never retain the header/body bytes.
                            entry.static_flags =
                                u32::from_le_bytes(data_bytes[4..8].try_into().unwrap())
                                    & 0x00e0_00f3;
                        }
                    }
                    None => {
                        entry.channel_kind = "unknown";
                    }
                }
            }
        }
        packets.push(entry);
    }
    packets
}

/// Unlike the upstream framed helper this checks hinted length BEFORE reserving memory.
struct BoundedIo<S> {
    stream: Option<S>,
    buffered: Vec<u8>,
    max_buffer: usize,
    stopped: Arc<AtomicBool>,
    read_ahead: bool,
    #[cfg(test)]
    write_diagnostics: Option<Arc<Mutex<SessionState>>>,
    #[cfg(test)]
    write_metadata_context: Option<(u16, Option<u16>)>,
}
impl<S> Drop for BoundedIo<S> {
    fn drop(&mut self) {
        self.buffered.zeroize();
    }
}
impl<S: AsyncRead + Unpin> BoundedIo<S> {
    fn new(stream: S, stopped: Arc<AtomicBool>) -> Self {
        Self {
            stream: Some(stream),
            buffered: Vec::new(),
            max_buffer: MAX_PDU,
            stopped,
            read_ahead: false,
            #[cfg(test)]
            write_diagnostics: None,
            #[cfg(test)]
            write_metadata_context: None,
        }
    }
    async fn fill(&mut self) -> Result<(), RdpError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(RdpError::SessionNotFound);
        }
        if self.buffered.len() >= self.max_buffer {
            return Err(RdpError::Protocol);
        }
        let mut chunk = Zeroizing::new([0; 8192]);
        let max = chunk.len().min(self.max_buffer - self.buffered.len());
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
                if info.length == 0 || info.length > self.max_buffer {
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
}
impl<S: AsyncRead + AsyncWrite + Unpin> BoundedIo<S> {
    fn into_active(
        mut self,
    ) -> Result<(BoundedIo<tokio::io::ReadHalf<S>>, tokio::io::WriteHalf<S>), RdpError> {
        let stream = self.stream.take().ok_or(RdpError::Connection)?;
        let (reader, writer) = tokio::io::split(stream);
        let mut input = BoundedIo::new(reader, self.stopped.clone());
        input.buffered = std::mem::take(&mut self.buffered);
        Ok((input, writer))
    }
    async fn write(&mut self, data: &[u8]) -> Result<(), RdpError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(RdpError::SessionNotFound);
        }
        if data.len() > MAX_WRITE_BATCH {
            return Err(RdpError::Protocol);
        }
        if !self.read_ahead {
            // X224 negotiation must not consume bytes for the following TLS
            // handshake. Keep the upgrade boundary write-only and empty.
            return tokio::time::timeout(WRITE_TIMEOUT, async {
                let stream = self.stream.as_mut().ok_or(RdpError::Connection)?;
                stream
                    .write_all(data)
                    .await
                    .map_err(|_| RdpError::Connection)?;
                stream.flush().await.map_err(|_| RdpError::Connection)
            })
            .await
            .map_err(|_| RdpError::Timeout)?;
        }
        #[cfg(test)]
        let diagnostic = self.write_diagnostics.clone();
        #[cfg(test)]
        let started = std::time::Instant::now();
        #[cfg(test)]
        record_write_progress(&diagnostic, |d| {
            d.last_write_attempted = data.len();
            d.last_write_accepted = 0;
            d.last_write_read_ahead = 0;
            d.last_write_flush_started = false;
            d.last_write_progress_samples = [0; 3];
            d.last_write_last_progress_ms = 0;
            d.last_write_elapsed_ms = 0;
            d.last_write_read_packets.clear();
        });
        let stopped = self.stopped.clone();
        let total_budget = if data.len() > LARGE_WRITE_THRESHOLD {
            LARGE_WRITE_BUDGET
        } else {
            WRITE_TIMEOUT
        };
        let result = tokio::time::timeout(total_budget, async {
            let stream = self.stream.as_mut().ok_or(RdpError::Connection)?;
            // A peer may need to finish graphics/channel output before receiving
            // this reply. A write-only await can fill both TCP directions and
            // deadlock. Split only this borrowed stream; retain the sole ordered
            // writer and preserve read-ahead in the existing bounded input buffer.
            let (mut reader, mut writer) = tokio::io::split(stream);
            let writing = async {
                let mut offset = 0;
                while offset < data.len() {
                    if stopped.load(Ordering::Acquire) {
                        return Err(RdpError::SessionNotFound);
                    }
                    // Large framed replies may progress slowly on a working
                    // peer. Only accepted output advances this idle budget;
                    // incoming traffic never extends it or the absolute cap.
                    let n = tokio::time::timeout(WRITE_TIMEOUT, writer.write(&data[offset..]))
                        .await
                        .map_err(|_| RdpError::Timeout)?
                        .map_err(|_| RdpError::Connection)?;
                    if n == 0 {
                        return Err(RdpError::Connection);
                    }
                    offset += n;
                    #[cfg(test)]
                    record_write_progress(&diagnostic, |d| {
                        d.last_write_accepted = offset;
                        d.last_write_last_progress_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    });
                }
                #[cfg(test)]
                record_write_progress(&diagnostic, |d| d.last_write_flush_started = true);
                tokio::time::timeout(WRITE_TIMEOUT, writer.flush())
                    .await
                    .map_err(|_| RdpError::Timeout)?
                    .map_err(|_| RdpError::Connection)
            };
            tokio::pin!(writing);
            #[cfg(test)]
            let mut sampling = tokio::time::interval(Duration::from_secs(1));
            loop {
                if stopped.load(Ordering::Acquire) {
                    return Err(RdpError::SessionNotFound);
                }
                let mut chunk = Zeroizing::new([0u8; 8192]);
                let available = chunk.len().min(MAX_PDU.saturating_sub(self.buffered.len()));
                #[cfg(test)]
                let sample_tick = sampling.tick();
                #[cfg(not(test))]
                let sample_tick = std::future::pending::<tokio::time::Instant>();
                tokio::select! {
                    biased;
                    result = &mut writing => return result,
                    result = reader.read(&mut chunk[..available]), if available > 0 => {
                        let n = result.map_err(|_| RdpError::Connection)?;
                        if n == 0 { return Err(RdpError::Connection); }
                        self.buffered.extend_from_slice(&chunk[..n]);
                        #[cfg(test)]
                        record_write_progress(&diagnostic, |d| d.last_write_read_ahead += n);
                    },
                    _ = std::future::ready(()), if available == 0 => return Err(RdpError::Protocol),
                    _ = sample_tick => {
                        #[cfg(test)]
                        {
                        let sample = match started.elapsed().as_secs() { 1 => Some(0), 5 => Some(1), 9 => Some(2), _ => None };
                        if let Some(index) = sample {
                            record_write_progress(&diagnostic, |d| d.last_write_progress_samples[index] = d.last_write_accepted);
                        }
                        }
                    },
                }
            }
        })
        .await
        .unwrap_or(Err(RdpError::Timeout));
        #[cfg(test)]
        {
            let packets = write_read_packet_metadata(&self.buffered, self.write_metadata_context);
            record_write_progress(&diagnostic, |d| {
                d.last_write_elapsed_ms =
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                d.last_write_read_packets = packets;
            });
        }
        result
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
    receiver: mpsc::Receiver<SessionCommand>,
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
            #[cfg(test)]
            {
                io.write_diagnostics = Some(state.clone());
            }
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
    // Only activated TLS sessions need duplex service/graphics progress.
    io.read_ahead = true;
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
    #[cfg(test)]
    {
        io.write_metadata_context = Some((io_channel_id, message_channel_id));
    }
    let image = DecodedImage::new(
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
    let clipboard_channel_id = result
        .static_channels
        .get_channel_id_by_type::<ironrdp::cliprdr::CliprdrClient>();
    let active = ActiveStageBuilder {
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
    run_active(
        io,
        receiver,
        state,
        redirects,
        notify,
        ActiveRuntime {
            active,
            image,
            activation_factory,
            activation_context,
            io_channel_id,
            message_channel_id,
            drive_channel_id,
            dvc_id,
            clipboard_channel_id,
            initial_permissions,
            initial_drive_id,
            #[cfg(test)]
            initial_output: Vec::new(),
        },
    )
    .await
}

struct ActiveRuntime {
    active: ironrdp::session::ActiveStage,
    image: DecodedImage,
    activation_factory: connector::connection_activation::ConnectionActivationFactory,
    activation_context: ActivationContext,
    io_channel_id: u16,
    message_channel_id: Option<u16>,
    drive_channel_id: Option<u16>,
    dvc_id: Option<u16>,
    clipboard_channel_id: Option<u16>,
    initial_permissions: crate::SessionPermissions,
    initial_drive_id: u32,
    #[cfg(test)]
    initial_output: Vec<Zeroizing<Vec<u8>>>,
}

async fn run_active<S: AsyncRead + AsyncWrite + Unpin>(
    io: BoundedIo<S>,
    mut receiver: mpsc::Receiver<SessionCommand>,
    state: Arc<Mutex<SessionState>>,
    redirects: crate::permissions::SharedRedirect,
    notify: Arc<tokio::sync::Notify>,
    runtime: ActiveRuntime,
) -> Result<(), RdpError> {
    let ActiveRuntime {
        mut active,
        mut image,
        activation_factory,
        activation_context,
        io_channel_id,
        message_channel_id,
        drive_channel_id,
        dvc_id,
        clipboard_channel_id,
        initial_permissions,
        initial_drive_id,
        #[cfg(test)]
        initial_output,
    } = runtime;
    let mut announced_permissions = initial_permissions;
    let mut announced_drive_id = initial_drive_id;
    let mut database = Database::new();
    let mut limits =
        crate::limits::DecoderLimits::with_dvc_channel(dvc_id).with_drive_channel(drive_channel_id);
    let (mut input, output) = io.into_active()?;
    let mut writer = ordered_writer::Writer::new(output, io_channel_id, message_channel_id);
    #[cfg(test)]
    for bytes in initial_output {
        writer.enqueue(bytes, ordered_writer::Purpose::Protocol)?;
    }
    let mut commands = VecDeque::new();
    let mut deferred = VecDeque::<(pdu::Action, Zeroizing<Vec<u8>>)>::new();
    let mut deferred_bytes = 0usize;
    let mut drive_request_prefix = Vec::with_capacity(24);
    let mut activation: Option<(ConnectionActivationSequence, tokio::time::Instant)> = None;
    loop {
        if input.stopped.load(Ordering::Acquire) {
            return Err(RdpError::SessionNotFound);
        }
        writer.check_deadlines()?;
        if activation
            .as_ref()
            .is_some_and(|(_, deadline)| tokio::time::Instant::now() >= *deadline)
        {
            return Err(RdpError::Timeout);
        }
        state
            .lock()
            .map_err(|_| RdpError::Connection)?
            .resize_available = activation.is_none()
            && active
                .get_dvc::<DisplayControlClient>()
                .is_some_and(|channel| channel.channel_id().is_some());

        // Encode inputs only at an output boundary, with a separately prepared
        // Database committed only after the writer actually accepts plaintext.
        if activation.is_none() && writer.can_generate_input() {
            if let Some(command) = commands.pop_front() {
                let mut next_database = copy_input_database(&database);
                let mut bytes = Zeroizing::new(Vec::new());
                let mut pdus = 0u64;
                let mut event_count = 0u64;
                let purpose = match command {
                    SessionCommand::Inputs(inputs) => {
                        for input in &inputs {
                            match input {
                                Input::Resize { width, height } => {
                                    if let Some(frame) = active.encode_resize(
                                        u32::from(*width),
                                        u32::from(*height),
                                        Some(100),
                                        None,
                                    ) {
                                        bytes.extend_from_slice(
                                            &frame.map_err(|_| RdpError::Protocol)?,
                                        );
                                        pdus += 1;
                                    }
                                }
                                _ => {
                                    let events = input_events(&mut next_database, input);
                                    event_count += events.len() as u64;
                                    encode_input_frames(
                                        &mut active,
                                        &mut image,
                                        &events,
                                        &mut bytes,
                                        &mut pdus,
                                    )?;
                                }
                            }
                        }
                        ordered_writer::Purpose::Input {
                            pdus,
                            database: Some(next_database),
                        }
                    }
                    SessionCommand::ConfirmedPaste { ticket, done } => {
                        if done.is_closed() {
                            redirects
                                .lock()
                                .map_err(|_| RdpError::Connection)?
                                .offers
                                .cancel_ticket(&ticket);
                            continue;
                        }
                        let fence = {
                            let mut s = redirects.lock().map_err(|_| RdpError::Connection)?;
                            let generation = s.generation;
                            let enabled = s.enabled();
                            s.offers.consume_paste(&ticket, generation, enabled)
                        };
                        let fence = match fence {
                            Ok(fence) => fence,
                            Err(error) => {
                                let _ = done.send(Err(error));
                                continue;
                            }
                        };
                        let mut events = input_events(&mut next_database, &Input::ReleaseAll);
                        for (code, down) in
                            [(0x1d, true), (0x2f, true), (0x2f, false), (0x1d, false)]
                        {
                            events.extend(input_events(
                                &mut next_database,
                                &Input::Scancode {
                                    code,
                                    down,
                                    extended: false,
                                },
                            ));
                        }
                        event_count = events.len() as u64;
                        encode_input_frames(
                            &mut active,
                            &mut image,
                            &events,
                            &mut bytes,
                            &mut pdus,
                        )?;
                        ordered_writer::Purpose::Paste {
                            fence,
                            done,
                            pdus,
                            database: Some(next_database),
                        }
                    }
                };
                if bytes.len() > ordered_writer::INTERACTIVE_RESERVE {
                    return Err(RdpError::Protocol);
                }
                writer.enqueue(bytes, purpose)?;
                let mut s = state.lock().map_err(|_| RdpError::Connection)?;
                s.diagnostics.input_batches = s.diagnostics.input_batches.saturating_add(1);
                s.diagnostics.input_events = s.diagnostics.input_events.saturating_add(event_count);
            }
        }
        // Clipboard metadata and snapshots are prepared only with sufficient
        // headroom. The writer opens the ACK slot at actual dispatch.
        if writer.at_boundary()
            && !writer.has_clipboard()
            && writer.channel_available(clipboard_channel_id)
            && writer.remaining() >= ordered_writer::INTERACTIVE_RESERVE
        {
            for (generation, bytes, advertisement, sequence, request) in
                crate::clipboard::flush(&mut active, &redirects)?
            {
                writer.enqueue(
                    bytes,
                    ordered_writer::Purpose::Clipboard {
                        generation,
                        sequence,
                        advertisement,
                        request,
                    },
                )?;
            }
        }
        let permissions = {
            let s = redirects.lock().map_err(|_| RdpError::Connection)?;
            s.drive_ready.then(|| (s.permissions.clone(), s.drive_id))
        };
        if writer.at_boundary()
            && writer.channel_available(drive_channel_id)
            && writer.remaining() >= ordered_writer::INTERACTIVE_RESERVE
        {
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
                        let bytes = Zeroizing::new(active.process_svc_processor_messages(ironrdp::svc::SvcProcessorMessages::<ironrdp::rdpdr::Rdpdr>::new(messages)).map_err(|_| RdpError::Protocol)?);
                        writer.enqueue(bytes, ordered_writer::Purpose::Protocol)?;
                    }
                    announced_permissions = permissions;
                    announced_drive_id = drive_id;
                }
            }
        }

        // Only drive requests whose response would exceed the aggregate output
        // budget wait; other service channels, graphics and commands keep running.
        let mut packet = None;
        let mut packet_from_deferred = false;
        if deferred.front().is_some_and(|(action, bytes)| {
            drive_reply_budget(*action, bytes, drive_channel_id)
                + ordered_writer::INTERACTIVE_RESERVE
                <= writer.remaining()
        }) {
            let next = deferred.pop_front().expect("front checked");
            deferred_bytes -= next.1.len();
            packet = Some(next);
            packet_from_deferred = true;
        }
        if packet.is_none() {
            input.max_buffer = MAX_PDU.saturating_sub(deferred_bytes);
            let deadline = writer
                .next_deadline()
                .into_iter()
                .chain(activation.as_ref().map(|(_, deadline)| *deadline))
                .min();
            tokio::select! {
                result = writer.progress(&redirects, &mut database), if !writer.is_empty() => {
                    if let Some(completed) = result? {
                        match completed.purpose {
                            ordered_writer::Purpose::Input { pdus, .. } => {
                                if completed.accepted > 0 { state.lock().map_err(|_| RdpError::Connection)?.diagnostics.input_pdus_written += pdus; }
                            },
                            ordered_writer::Purpose::Paste { done, pdus, .. } => {
                                if completed.accepted > 0 {
                                    state.lock().map_err(|_| RdpError::Connection)?.diagnostics.input_pdus_written += pdus;
                                    let _ = done.send(Ok(()));
                                } else { let _ = done.send(Err(RdpError::ClipboardUnavailable)); }
                            },
                            _ => {},
                        }
                    }
                    continue;
                },
                result = input.pdu() => { packet = Some(result?); },
                command = receiver.recv(), if commands.len() < 128 => {
                    let Some(command) = command else { return Ok(()); };
                    commands.push_back(command);
                    continue;
                },
                _ = notify.notified() => continue,
                _ = async { match deadline { Some(deadline) => tokio::time::sleep_until(deadline).await, None => std::future::pending().await } } => { return Err(RdpError::Timeout); },
            }
        }
        let (action, packet) = packet.expect("selected packet");
        let is_drive = drive_channel_id.is_some_and(|channel| {
            action == pdu::Action::X224
                && pdu::mcs::decode_send_data_indication(&packet)
                    .is_ok_and(|data| data.channel_id == channel)
        });
        if is_drive
            && !packet_from_deferred
            && (!deferred.is_empty()
                || drive_reply_budget(action, &packet, drive_channel_id)
                    + ordered_writer::INTERACTIVE_RESERVE
                    > writer.remaining())
        {
            if deferred.len() >= 128
                || packet.len() > MAX_PDU.saturating_sub(deferred_bytes + input.buffered.len())
            {
                return Err(RdpError::Protocol);
            }
            deferred_bytes += packet.len();
            deferred.push_back((action, packet));
            continue;
        }
        set_stage(&state, "active_read")?;
        observe_drive_handshake(action, &packet, drive_channel_id, &redirects)?;
        reject_redirect(action, &packet, io_channel_id)?;
        limits.check(action, &packet, io_channel_id, message_channel_id)?;
        let read_response =
            classify_drive_read(action, &packet, drive_channel_id, &mut drive_request_prefix);
        let outputs = if let Some((sequence, _)) = activation.as_mut() {
            let service_channel = action == pdu::Action::X224
                && pdu::mcs::decode_send_data_indication(&packet)
                    .is_ok_and(|d| d.channel_id != io_channel_id);
            let matched = sequence.next_pdu_hint().is_some_and(|hint| {
                hint.find_size(&packet)
                    .is_ok_and(|size| size.is_some_and(|(matched, _)| matched))
            });
            if service_channel || !matched {
                active
                    .process(&mut image, action, &packet)
                    .map_err(|e| decode_failure(&state, &e))?
            } else {
                let mut out = WriteBuf::new();
                let written = sequence
                    .step(&packet, &mut out)
                    .map_err(|_| RdpError::Protocol)?;
                if written.size().is_some() {
                    writer.enqueue(
                        Zeroizing::new(out.filled().to_vec()),
                        ordered_writer::Purpose::Protocol,
                    )?;
                }
                while sequence.next_pdu_hint().is_none()
                    && !matches!(
                        sequence.connection_activation_state(),
                        ConnectionActivationState::Finalized { .. }
                    )
                {
                    let mut out = WriteBuf::new();
                    let written = sequence
                        .step_no_input(&mut out)
                        .map_err(|_| RdpError::Protocol)?;
                    if written.size().is_some() {
                        writer.enqueue(
                            Zeroizing::new(out.filled().to_vec()),
                            ordered_writer::Purpose::Protocol,
                        )?;
                    }
                }
                if let ConnectionActivationState::Finalized {
                    desktop_size,
                    share_id,
                    enable_server_pointer,
                    pointer_software_rendering,
                } = sequence.connection_activation_state()
                {
                    let next = ActivationContext {
                        io_channel_id: sequence.io_channel_id(),
                        user_channel_id: sequence.user_channel_id(),
                        share_id,
                        enable_server_pointer,
                        pointer_software_rendering,
                    };
                    if activation_context != next {
                        return Err(RdpError::Protocol);
                    }
                    image = resized_image(desktop_size.width, desktop_size.height)?;
                    active.set_share_id(share_id);
                    active.set_enable_server_pointer(enable_server_pointer);
                    state
                        .lock()
                        .map_err(|_| RdpError::Connection)?
                        .diagnostics
                        .reactivations_completed += 1;
                    activation = None;
                }
                Vec::new()
            }
        } else {
            set_stage(&state, "active_decode")?;
            active
                .process(&mut image, action, &packet)
                .map_err(|error| decode_failure(&state, &error))?
        };
        for output in outputs {
            match output {
                ActiveStageOutput::ResponseFrame(bytes) => {
                    let bytes = Zeroizing::new(bytes);
                    let purpose = drive_response_purpose(&bytes, drive_channel_id, read_response)?;
                    writer.enqueue(bytes, purpose)?;
                }
                ActiveStageOutput::GraphicsUpdate(_) => publish_frame(&state, &image)?,
                ActiveStageOutput::Terminate(_) => return Ok(()),
                ActiveStageOutput::DeactivateAll => {
                    if activation.is_some() {
                        return Err(RdpError::Protocol);
                    }
                    limits.require_reactivation_boundary()?;
                    state
                        .lock()
                        .map_err(|_| RdpError::Connection)?
                        .diagnostics
                        .reactivations_started += 1;
                    activation = Some((
                        activation_factory.create(),
                        tokio::time::Instant::now() + CONNECT_TIMEOUT,
                    ));
                }
                ActiveStageOutput::MultitransportRequest(_) => return Err(RdpError::Protocol),
                _ => {}
            }
        }
    }
}

// Track only the fixed public RDPDR IO header, not a file payload. This prevents
// ambiguous READ/WRITE/query response layouts from selecting a wrong fallback.
fn classify_drive_read(
    action: pdu::Action,
    packet: &[u8],
    drive: Option<u16>,
    prefix: &mut Vec<u8>,
) -> bool {
    if action != pdu::Action::X224 {
        return false;
    }
    let Ok(data) = pdu::mcs::decode_send_data_indication(packet) else {
        return false;
    };
    if Some(data.channel_id) != drive || data.user_data.len() < 8 {
        return false;
    }
    let b = data.user_data;
    let n = (24usize.saturating_sub(prefix.len())).min(b.len() - 8);
    prefix.extend_from_slice(&b[8..8 + n]);
    if u32::from_le_bytes(b[4..8].try_into().unwrap()) & 2 == 0 {
        return false;
    }
    let read = prefix.len() >= 24
        && prefix[..4] == [0x72, 0x44, 0x52, 0x49]
        && u32::from_le_bytes(prefix[16..20].try_into().unwrap()) == 3;
    prefix.clear();
    read
}

fn drive_response_purpose(
    bytes: &[u8],
    drive: Option<u16>,
    read_response: bool,
) -> Result<ordered_writer::Purpose, RdpError> {
    if !read_response {
        return Ok(ordered_writer::Purpose::Protocol);
    }
    let Some(drive) = drive else {
        return Ok(ordered_writer::Purpose::Protocol);
    };
    let Some(info) = pdu::find_size(bytes).map_err(|_| RdpError::Protocol)? else {
        return Err(RdpError::Protocol);
    };
    if info.action != pdu::Action::X224 {
        return Ok(ordered_writer::Purpose::Protocol);
    }
    let data = ironrdp::core::decode::<pdu::x224::X224<pdu::mcs::SendDataRequest<'_>>>(
        &bytes[..info.length],
    )
    .map_err(|_| RdpError::Protocol)?
    .0;
    let b = data.user_data.as_ref();
    if data.channel_id != drive || b.len() < 28 || b[8..12] != [0x72, 0x44, 0x43, 0x49] {
        return Ok(ordered_writer::Purpose::Protocol);
    }
    let length = u32::from_le_bytes(b[..4].try_into().unwrap());
    let body_length = u32::from_le_bytes(b[24..28].try_into().unwrap());
    if length != body_length.saturating_add(20) || b[20..24] != [0, 0, 0, 0] {
        return Ok(ordered_writer::Purpose::Protocol);
    }
    use ironrdp::rdpdr::pdu::{
        efs::{DeviceIoResponse, DeviceReadResponse, NtStatus},
        RdpdrPdu,
    };
    let drive_id = u32::from_le_bytes(b[12..16].try_into().unwrap());
    let denied = ironrdp::svc::client_encode_svc_messages(
        vec![ironrdp::svc::SvcMessage::from(
            RdpdrPdu::DeviceReadResponse(DeviceReadResponse {
                device_io_reply: DeviceIoResponse {
                    device_id: drive_id,
                    completion_id: u32::from_le_bytes(b[16..20].try_into().unwrap()),
                    io_status: NtStatus::ACCESS_DENIED,
                },
                read_data: Vec::new(),
            }),
        )],
        drive,
        data.initiator_id,
    )
    .map_err(|_| RdpError::Protocol)?;
    Ok(ordered_writer::Purpose::Drive {
        drive_id,
        denied: Zeroizing::new(denied),
    })
}

fn copy_input_database(database: &Database) -> Database {
    let mut copy = Database::new();
    let mut operations = vec![Operation::MouseMove(database.mouse_position())];
    for idx in 0..512 {
        let code = Scancode::from_u8(idx >= 256, (idx % 256) as u8);
        if database.is_key_pressed(code) {
            operations.push(Operation::KeyPressed(code));
        }
    }
    for idx in 0..5 {
        let button = ironrdp::input::MouseButton::from_idx(idx).expect("bounded button");
        if database.is_mouse_button_pressed(button) {
            operations.push(Operation::MouseButtonPressed(button));
        }
    }
    // Every Unicode scalar is one down/up transaction in input_events(), so no
    // Unicode key remains held between input batches.
    let _ = copy.apply(operations);
    copy
}

fn encode_input_frames(
    active: &mut ironrdp::session::ActiveStage,
    image: &mut DecodedImage,
    events: &[pdu::input::fast_path::FastPathInputEvent],
    bytes: &mut Vec<u8>,
    pdus: &mut u64,
) -> Result<(), RdpError> {
    for chunk in events.chunks(15) {
        for output in active
            .process_fastpath_input(image, chunk)
            .map_err(|_| RdpError::Protocol)?
        {
            match output {
                ActiveStageOutput::ResponseFrame(frame) => {
                    bytes.extend_from_slice(&frame);
                    *pdus += 1;
                }
                ActiveStageOutput::GraphicsUpdate(_) => {}
                _ => return Err(RdpError::Protocol),
            }
        }
    }
    Ok(())
}

fn drive_reply_budget(action: pdu::Action, packet: &[u8], drive: Option<u16>) -> usize {
    if action != pdu::Action::X224 {
        return 0;
    }
    let Ok(data) = pdu::mcs::decode_send_data_indication(packet) else {
        return 0;
    };
    if Some(data.channel_id) != drive {
        return 0;
    }
    let b = data.user_data;
    if b.len() >= 8 {
        let flags = u32::from_le_bytes(b[4..8].try_into().unwrap());
        if flags & 3 == 2 {
            // LAST may complete a fragmented READ whose header is retained by
            // the channel decoder. Reserve its maximum response before decode.
            let inner = crate::directory::MAX_IO + 20;
            return inner + inner.div_ceil(1600) * 23;
        }
        if flags & 2 == 0 {
            return 0;
        }
    }
    // Single-fragment DR_DEVICE_IOREQUEST/IRP_MJ_READ: reserve the entire bounded
    // reply before ActiveStage can read/allocate its plaintext.
    if b.len() >= 36
        && u32::from_le_bytes(b[4..8].try_into().unwrap()) & 3 == 3
        && b[8..12] == [0x72, 0x44, 0x52, 0x49]
        && u32::from_le_bytes(b[24..28].try_into().unwrap()) == 3
    {
        let len = usize::try_from(u32::from_le_bytes(b[32..36].try_into().unwrap()))
            .unwrap_or(usize::MAX)
            .min(crate::directory::MAX_IO);
        let inner = len + 20;
        inner + inner.div_ceil(1600) * 23
    } else {
        4096
    }
}

#[cfg(test)]
#[path = "transport_active_tests.rs"]
mod actor_tests;

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

    #[tokio::test]
    async fn bounded_large_drive_response_including_headers_can_be_written() {
        use ironrdp::{
            rdpdr::pdu::{
                efs::{
                    DeviceIoRequest, DeviceIoResponse, DeviceReadResponse, MajorFunction,
                    MinorFunction, NtStatus,
                },
                RdpdrPdu,
            },
            svc::SvcMessage,
        };
        let payload = vec![17; 1024 * 1024];
        let message = SvcMessage::from(RdpdrPdu::DeviceReadResponse(DeviceReadResponse {
            device_io_reply: DeviceIoResponse::new(
                DeviceIoRequest {
                    device_id: 1,
                    file_id: 1,
                    completion_id: 1,
                    major_function: MajorFunction::Read,
                    minor_function: MinorFunction::from(0),
                },
                NtStatus::SUCCESS,
            ),
            read_data: payload,
        }));
        let frame = ironrdp::svc::client_encode_svc_messages(vec![message], 1004, 1002).unwrap();
        assert!(frame.len() > MAX_PDU);
        assert!(frame.len() <= MAX_WRITE_BATCH);
        let mut offset = 0;
        let mut pdus = 0;
        let mut assembled = Vec::new();
        while offset < frame.len() {
            let info = pdu::find_size(&frame[offset..]).unwrap().unwrap();
            assert!(info.length > 0 && info.length <= MAX_PDU);
            assert_eq!(info.action, pdu::Action::X224);
            let request = ironrdp::core::decode::<pdu::x224::X224<pdu::mcs::SendDataRequest<'_>>>(
                &frame[offset..offset + info.length],
            )
            .unwrap()
            .0;
            assert_eq!(request.channel_id, 1004);
            assert_eq!(request.initiator_id, 1002);
            assert!(request.user_data.len() <= ironrdp::svc::CHANNEL_CHUNK_LENGTH + 8);
            let whole_length =
                u32::from_le_bytes(request.user_data[0..4].try_into().unwrap()) as usize;
            assert_eq!(whole_length, MAX_PDU + 20);
            let flags = u32::from_le_bytes(request.user_data[4..8].try_into().unwrap());
            assert_eq!(flags & 1 != 0, pdus == 0);
            assert_eq!(flags & 2 != 0, offset + info.length == frame.len());
            assert_eq!(
                flags & 0x00e0_0000,
                0,
                "no compressed flag on uncompressed body"
            );
            assembled.extend_from_slice(&request.user_data[8..]);
            offset += info.length;
            assert!(offset <= frame.len());
            pdus += 1;
        }
        assert_eq!(offset, frame.len());
        assert!(pdus > 1);
        assert_eq!(assembled.len(), MAX_PDU + 20);
        assert_eq!(
            u32::from_le_bytes(assembled[16..20].try_into().unwrap()) as usize,
            MAX_PDU
        );
        assert!(assembled[20..].iter().all(|byte| *byte == 17));
        let committed = Arc::new(Mutex::new(Vec::new()));
        let stream = BufferedWriter {
            buffered: Vec::new(),
            committed: committed.clone(),
        };
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        io.write(&frame).await.unwrap();
        assert_eq!(committed.lock().unwrap().as_slice(), frame.as_slice());
        assert_eq!(
            io.write(&vec![0; MAX_WRITE_BATCH + 1]).await,
            Err(RdpError::Protocol)
        );
        assert_eq!(committed.lock().unwrap().len(), frame.len());
    }

    #[tokio::test]
    async fn large_write_makes_progress_while_peer_input_is_drained_and_preserved() {
        // Model a duplex peer which must finish its pending graphics/channel
        // bytes before it can receive the large drive response. Both directions
        // exceed the bounded socket buffers; a write-only await deadlocks.
        let (stream, mut peer) = tokio::io::duplex(4096);
        let outgoing = vec![17u8; 1024 * 1024];
        let incoming = vec![29u8; 64 * 1024];
        let expected = outgoing.clone();
        let peer_bytes = incoming.clone();
        let task = tokio::spawn(async move {
            peer.write_all(&peer_bytes).await.unwrap();
            let mut actual = vec![0; expected.len()];
            peer.read_exact(&mut actual).await.unwrap();
            assert_eq!(actual, expected);
        });
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), io.write(&outgoing)).await,
            Ok(Ok(()))
        );
        assert_eq!(io.exact(incoming.len()).await.unwrap(), incoming);
        task.await.unwrap();
    }

    #[test]
    fn write_read_header_metadata_is_bounded_and_never_retains_remote_body() {
        use std::borrow::Cow;
        let body = uuid::Uuid::new_v4().to_string();
        let mut channel = Vec::new();
        channel.extend_from_slice(&(body.len() as u32).to_le_bytes());
        channel.extend_from_slice(&0x23u32.to_le_bytes());
        channel.extend_from_slice(body.as_bytes());
        let frame = ironrdp::core::encode_vec(&pdu::x224::X224(pdu::mcs::SendDataIndication {
            initiator_id: 1002,
            channel_id: 1004,
            user_data: Cow::Borrowed(&channel),
        }))
        .unwrap();
        let packets = write_read_packet_metadata(&frame.repeat(16), Some((1003, Some(1005))));
        assert_eq!(packets.len(), 8);
        assert!(packets
            .iter()
            .all(|packet| packet.channel_kind == "static" && packet.static_flags == 0x23));
        assert!(!format!("{packets:?}").contains(&body));
        assert!(write_read_packet_metadata(&frame[..3], Some((1003, None))).is_empty());
    }

    #[tokio::test]
    async fn unicode_fastpath_split_preserves_all_events_and_exact_wire_lengths() {
        use pdu::input::fast_path::FastPathInput;
        let mut database = Database::new();
        let events = input_events(&mut database, &Input::UnicodeText("CC-input-probe".into()));
        assert_eq!(events.len(), 28);
        let mut decoded = Vec::new();
        let mut frames = Vec::new();
        for (index, events) in events.chunks(15).enumerate() {
            let frame =
                ironrdp::core::encode_vec(&FastPathInput::new(events.to_vec()).unwrap()).unwrap();
            assert_eq!(frame.len(), [47, 41][index]);
            assert_eq!(pdu::find_size(&frame).unwrap().unwrap().length, frame.len());
            let packet = ironrdp::core::decode::<FastPathInput>(&frame).unwrap();
            assert_eq!(packet.input_events().len(), [15, 13][index]);
            decoded.extend_from_slice(packet.input_events());
            frames.push(frame);
        }
        assert_eq!(decoded, events);
        let expected = frames.concat();
        let committed = Arc::new(Mutex::new(Vec::new()));
        let stream = BufferedWriter {
            buffered: Vec::new(),
            committed: committed.clone(),
        };
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        for frame in frames {
            io.write(&frame).await.unwrap();
        }
        assert_eq!(committed.lock().unwrap().as_slice(), expected.as_slice());
    }

    #[tokio::test]
    async fn tls_large_write_progresses_with_bidirectional_bounded_backpressure() {
        use rustls::pki_types::{PrivatePkcs8KeyDer, ServerName};
        use tokio_rustls::{TlsAcceptor, TlsConnector};
        let generated = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = generated.cert.der().clone();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.clone()],
                PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der()).into(),
            )
            .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let client = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let (client_stream, server_stream) = tokio::io::duplex(1024);
        let incoming = vec![29u8; 64 * 1024];
        let peer_bytes = incoming.clone();
        let outgoing = vec![17u8; MAX_PDU];
        let expected = outgoing.clone();
        let task = tokio::spawn(async move {
            let mut stream = TlsAcceptor::from(Arc::new(server))
                .accept(server_stream)
                .await
                .unwrap();
            stream.write_all(&peer_bytes).await.unwrap();
            stream.flush().await.unwrap();
            let mut actual = vec![0; expected.len()];
            stream.read_exact(&mut actual).await.unwrap();
            assert_eq!(actual, expected);
        });
        let stream = TlsConnector::from(Arc::new(client))
            .connect(ServerName::try_from("localhost").unwrap(), client_stream)
            .await
            .unwrap();
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), io.write(&outgoing)).await,
            Ok(Ok(()))
        );
        assert_eq!(io.exact(incoming.len()).await.unwrap(), incoming);
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn duplex_write_stalled_peer_keeps_original_deadline() {
        let (stream, _peer) = tokio::io::duplex(64);
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        let began = tokio::time::Instant::now();
        assert_eq!(io.write(&vec![17; MAX_PDU]).await, Err(RdpError::Timeout));
        assert_eq!(began.elapsed(), WRITE_TIMEOUT);
        assert!(io.buffered.is_empty());
    }

    struct PacedIo {
        next_write: std::pin::Pin<Box<tokio::time::Sleep>>,
        next_read: Option<std::pin::Pin<Box<tokio::time::Sleep>>>,
        every: Duration,
        chunk: usize,
        accepted: usize,
        stalled_flush: bool,
    }
    impl PacedIo {
        fn new(every: Duration, chunk: usize, incoming: bool) -> Self {
            Self {
                next_write: Box::pin(tokio::time::sleep(every)),
                next_read: incoming.then(|| Box::pin(tokio::time::sleep(Duration::from_secs(1)))),
                every,
                chunk,
                accepted: 0,
                stalled_flush: false,
            }
        }
    }
    impl AsyncRead for PacedIo {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buffer: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let Some(next) = self.next_read.as_mut() else {
                return std::task::Poll::Pending;
            };
            if std::future::Future::poll(next.as_mut(), cx).is_pending() {
                return std::task::Poll::Pending;
            }
            buffer.put_slice(&[29]);
            next.as_mut()
                .reset(tokio::time::Instant::now() + Duration::from_secs(1));
            std::task::Poll::Ready(Ok(()))
        }
    }
    impl AsyncWrite for PacedIo {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            bytes: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if self.chunk == 0
                || std::future::Future::poll(self.next_write.as_mut(), cx).is_pending()
            {
                return std::task::Poll::Pending;
            }
            let n = self.chunk.min(bytes.len());
            self.accepted += n;
            let until = tokio::time::Instant::now() + self.every;
            self.next_write.as_mut().reset(until);
            std::task::Poll::Ready(Ok(n))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.stalled_flush {
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(Ok(()))
            }
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn slow_large_write_completes_beyond_ten_seconds_with_real_progress() {
        let stream = PacedIo::new(Duration::from_secs(5), LARGE_WRITE_THRESHOLD, false);
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        let began = tokio::time::Instant::now();
        assert_eq!(io.write(&vec![17; LARGE_WRITE_THRESHOLD * 3]).await, Ok(()));
        assert_eq!(began.elapsed(), Duration::from_secs(15));
        assert_eq!(
            io.stream.as_ref().unwrap().accepted,
            LARGE_WRITE_THRESHOLD * 3
        );
    }

    #[tokio::test(start_paused = true)]
    async fn incoming_traffic_never_extends_stalled_output_idle_deadline() {
        let stream = PacedIo::new(Duration::from_secs(1), 0, true);
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        let began = tokio::time::Instant::now();
        assert_eq!(
            io.write(&vec![17; LARGE_WRITE_THRESHOLD + 1]).await,
            Err(RdpError::Timeout)
        );
        assert_eq!(began.elapsed(), WRITE_TIMEOUT);
        assert!(io.buffered.len() >= 9);
        assert_eq!(io.stream.as_ref().unwrap().accepted, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn hostile_output_trickle_cannot_extend_large_absolute_cap() {
        let stream = PacedIo::new(Duration::from_secs(9), 1, true);
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        let began = tokio::time::Instant::now();
        assert_eq!(
            io.write(&vec![17; LARGE_WRITE_THRESHOLD + 1]).await,
            Err(RdpError::Timeout)
        );
        assert_eq!(began.elapsed(), LARGE_WRITE_BUDGET);
        assert_eq!(io.stream.as_ref().unwrap().accepted, 13);
    }

    #[tokio::test(start_paused = true)]
    async fn small_write_keeps_absolute_budget_even_with_output_progress() {
        let stream = PacedIo::new(Duration::from_secs(5), 16, false);
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        let began = tokio::time::Instant::now();
        assert_eq!(io.write(&[17; 48]).await, Err(RdpError::Timeout));
        assert_eq!(began.elapsed(), WRITE_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn accepted_body_does_not_extend_stalled_flush_idle_budget() {
        let mut stream = PacedIo::new(Duration::from_secs(1), MAX_WRITE_BATCH, false);
        stream.stalled_flush = true;
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        let began = tokio::time::Instant::now();
        assert_eq!(
            io.write(&vec![17; LARGE_WRITE_THRESHOLD + 1]).await,
            Err(RdpError::Timeout)
        );
        assert_eq!(began.elapsed(), Duration::from_secs(11));
        assert_eq!(
            io.stream.as_ref().unwrap().accepted,
            LARGE_WRITE_THRESHOLD + 1
        );
    }

    #[tokio::test]
    async fn negotiation_write_keeps_tls_preface_unread_for_upgrade() {
        let (stream, mut peer) = tokio::io::duplex(64);
        peer.write_all(&[22, 3, 3, 0, 0]).await.unwrap();
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.write(&[1, 2, 3]).await.unwrap();
        assert!(io.buffered.is_empty());
        let mut stream = io.into_stream().unwrap();
        let mut preface = [0; 5];
        stream.read_exact(&mut preface).await.unwrap();
        assert_eq!(preface, [22, 3, 3, 0, 0]);
    }

    #[tokio::test]
    async fn duplex_write_read_ahead_fails_closed_at_existing_input_bound() {
        let (stream, mut peer) = tokio::io::duplex(4096);
        let task = tokio::spawn(async move {
            let _ = peer.write_all(&vec![29; MAX_PDU + 1]).await;
            std::future::pending::<()>().await;
        });
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), io.write(&vec![17; MAX_PDU])).await,
            Ok(Err(RdpError::Protocol))
        );
        assert_eq!(io.buffered.len(), MAX_PDU);
        assert!(io.buffered.capacity() <= MAX_PDU);
        task.abort();
    }

    #[tokio::test]
    async fn duplex_write_closed_peer_fails_without_claiming_success() {
        let (stream, peer) = tokio::io::duplex(64);
        drop(peer);
        let mut io = BoundedIo::new(stream, Arc::new(AtomicBool::new(false)));
        io.read_ahead = true;
        assert_eq!(io.write(&[17]).await, Err(RdpError::Connection));
        assert!(io.buffered.is_empty());
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
