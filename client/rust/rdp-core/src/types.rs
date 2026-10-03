use std::fmt;

pub const MAX_SESSIONS: usize = 4;
pub const MAX_WIDTH: u16 = 4096;
pub const MAX_HEIGHT: u16 = 2160;

#[derive(Clone, Debug)]
pub struct ConnectConfig {
    pub address: String,
    pub port: u16,
    pub username: String,
    pub domain: Option<String>,
    pub width: u16,
    pub height: u16,
    /// Exact SHA-256 of the target's DER certificate, confirmed by the user.
    pub accepted_certificate_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertificateInfo {
    pub sha256: [u8; 32],
    /// Displayable hexadecimal SHA-256. This is public certificate metadata.
    pub fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Connecting,
    Connected,
    Disconnected,
    Failed(RdpError),
}

pub struct Frame {
    pub sequence: u64,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("sequence", &self.sequence)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rgba_bytes", &self.rgba.len())
            .finish()
    }
}

#[derive(Debug)]
pub struct PollResult {
    pub status: SessionStatus,
    pub folder_status: crate::FolderStatus,
    pub frame: Option<Frame>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionDiagnostics {
    pub input_batches: u64,
    pub input_events: u64,
    pub input_pdus_written: u64,
    /// Closed vocabulary assigned by the transport, never a remote payload/error.
    pub last_stage: &'static str,
    pub reactivations_started: u64,
    pub reactivations_completed: u64,
    pub decode_kind: &'static str,
    pub decode_site: &'static str,
    pub decode_subkind: &'static str,
    #[cfg(test)]
    pub last_write_attempted: usize,
    #[cfg(test)]
    pub last_write_accepted: usize,
    #[cfg(test)]
    pub last_write_read_ahead: usize,
    #[cfg(test)]
    pub last_write_flush_started: bool,
    #[cfg(test)]
    pub last_write_progress_samples: [usize; 3],
    #[cfg(test)]
    pub last_write_last_progress_ms: u64,
    #[cfg(test)]
    pub last_write_elapsed_ms: u64,
    #[cfg(test)]
    pub last_write_read_packets: Vec<WriteReadPacketDiagnostic>,
}

/// Test-only protocol header metadata. Never includes a body or remote string.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteReadPacketDiagnostic {
    pub action: &'static str,
    pub length: usize,
    pub channel: Option<u16>,
    pub channel_kind: &'static str,
    pub control: &'static str,
    pub static_flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Back,
    Forward,
}

#[derive(Clone)]
pub enum Input {
    UnicodeText(String),
    Scancode {
        code: u8,
        down: bool,
        extended: bool,
    },
    Pointer {
        x: u16,
        y: u16,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    Wheel {
        units: i16,
        horizontal: bool,
    },
    /// Release held keys/buttons when the view loses focus.
    ReleaseAll,
    Resize {
        width: u16,
        height: u16,
    },
}

impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnicodeText(text) => f.debug_tuple("UnicodeText").field(&text.len()).finish(),
            Self::Scancode {
                code,
                down,
                extended,
            } => f
                .debug_struct("Scancode")
                .field("code", code)
                .field("down", down)
                .field("extended", extended)
                .finish(),
            Self::Pointer { x, y } => f
                .debug_struct("Pointer")
                .field("x", x)
                .field("y", y)
                .finish(),
            Self::Button { button, down } => f
                .debug_struct("Button")
                .field("button", button)
                .field("down", down)
                .finish(),
            Self::Wheel { units, horizontal } => f
                .debug_struct("Wheel")
                .field("units", units)
                .field("horizontal", horizontal)
                .finish(),
            Self::ReleaseAll => f.write_str("ReleaseAll"),
            Self::Resize { width, height } => f
                .debug_struct("Resize")
                .field("width", width)
                .field("height", height)
                .finish(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RdpError {
    #[error("Invalid remote desktop settings")]
    InvalidConfig,
    #[error("Remote desktop redirection permission is not granted")]
    PermissionDenied,
    #[error("Remote desktop feature is unavailable on this platform")]
    UnsupportedPlatform,
    #[error("Remote desktop directory grant is unavailable")]
    DirectoryGrantUnavailable,
    #[error("Remote desktop clipboard is not ready")]
    ClipboardUnavailable,
    #[error("Remote desktop clipboard text exceeds the limit")]
    ClipboardLimit,
    #[error("Remote desktop session limit reached")]
    SessionLimit,
    #[error("Remote desktop session not found")]
    SessionNotFound,
    #[error("Remote desktop connection failed")]
    Connection,
    #[error("Remote desktop certificate does not match the confirmed certificate")]
    CertificateMismatch,
    #[error("Remote desktop TLS verification failed")]
    Tls,
    #[error("Remote desktop authentication failed")]
    Authentication,
    #[error("Remote desktop protocol error")]
    Protocol,
    #[error("Remote desktop operation timed out")]
    Timeout,
    #[error("Remote desktop input queue is full")]
    InputQueueFull,
    #[error("Remote desktop resize is unavailable")]
    ResizeUnavailable,
    #[error("Remote desktop dimensions exceed the supported limit")]
    FrameLimit,
    #[error("Remote desktop server redirection is not permitted")]
    RedirectRejected,
}

impl RdpError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfig => "invalid_config",
            Self::PermissionDenied => "permission_denied",
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::DirectoryGrantUnavailable => "directory_grant_unavailable",
            Self::ClipboardUnavailable => "clipboard_unavailable",
            Self::ClipboardLimit => "clipboard_limit",
            Self::SessionLimit => "session_limit",
            Self::SessionNotFound => "session_not_found",
            Self::Connection => "connection",
            Self::CertificateMismatch => "certificate_mismatch",
            Self::Tls => "tls",
            Self::Authentication => "authentication",
            Self::Protocol => "protocol",
            Self::Timeout => "timeout",
            Self::InputQueueFull => "input_queue_full",
            Self::ResizeUnavailable => "resize_unavailable",
            Self::FrameLimit => "frame_limit",
            Self::RedirectRejected => "redirect_rejected",
        }
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        if let Self::UnicodeText(text) = self {
            zeroize::Zeroize::zeroize(text);
        }
    }
}

pub(crate) fn validate_dimensions(width: u16, height: u16) -> Result<(), RdpError> {
    if !(200..=MAX_WIDTH).contains(&width)
        || !(200..=MAX_HEIGHT).contains(&height)
        || !width.is_multiple_of(2)
    {
        return Err(RdpError::FrameLimit);
    }
    Ok(())
}

impl ConnectConfig {
    pub(crate) fn validate(&self) -> Result<(), RdpError> {
        validate_dimensions(self.width, self.height)?;
        if self.address.is_empty()
            || self.address.len() > 253
            || self.port == 0
            || self
                .address
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\' | '@'))
            || self.username.is_empty()
            || self.username.len() > 256
            || self.username.chars().any(char::is_control)
            || self
                .domain
                .as_ref()
                .is_some_and(|s| s.len() > 253 || s.chars().any(char::is_control))
            || self.accepted_certificate_sha256 == [0; 32]
        {
            return Err(RdpError::InvalidConfig);
        }
        Ok(())
    }
}
