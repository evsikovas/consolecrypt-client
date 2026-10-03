//! RDP bridge. Pixel buffers cross FRB as raw bytes, never JSON/base64.
//! Password buffers are owned by the call and wrapped before any await.
use crate::api::error::BridgeError;
use crate::state::{self, with_core};
use cc_app_core::{AppError, RdpConnectConfig, RdpInput, RdpMouseButton, RdpSessionStatus};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Clone)]
pub struct RdpCertificate {
    pub address: String,
    pub port: u16,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct RdpConnection {
    pub address: String,
    pub port: u16,
    pub username: String,
    pub domain: String,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Default)]
pub struct RdpPermissions {
    pub clipboard_enabled: bool,
    pub directory_grant_id: Option<String>,
    pub directory_writable: bool,
}
impl From<RdpPermissions> for cc_app_core::RdpSessionPermissions {
    fn from(value: RdpPermissions) -> Self {
        Self {
            clipboard_enabled: value.clipboard_enabled,
            directory_grant_id: value.directory_grant_id,
            directory_writable: value.directory_writable,
        }
    }
}
impl From<cc_app_core::RdpSessionPermissions> for RdpPermissions {
    fn from(value: cc_app_core::RdpSessionPermissions) -> Self {
        Self {
            clipboard_enabled: value.clipboard_enabled,
            directory_grant_id: value.directory_grant_id,
            directory_writable: value.directory_writable,
        }
    }
}
#[derive(Debug, Clone)]
pub struct RdpDirectoryGrant {
    pub id: String,
    pub name: String,
}
#[derive(Debug, Clone)]
pub struct RdpCapabilities {
    pub clipboard_supported: bool,
    pub folder_supported: bool,
}

#[derive(Debug, Clone)]
pub struct RdpSessionInfo {
    pub id: String,
    pub width: u16,
    pub height: u16,
}

pub struct RdpPixels {
    pub sequence: u64,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}
impl std::fmt::Debug for RdpPixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RdpPixels(<redacted>)")
    }
}

#[derive(Debug)]
pub struct RdpPoll {
    /// connecting / connected / disconnected / failed.
    pub phase: String,
    pub error_code: Option<String>,
    /// disabled / pending / ready / denied / unavailable; independent of connection phase.
    pub folder_status: String,
    pub frame: Option<RdpPixels>,
}

/// Flat, bounded input DTO to avoid transporting pixel data or secrets in JSON.
pub struct RdpInputMessage {
    /// unicode / scancode / pointer / wheel / resize / release_all.
    pub kind: String,
    pub text: String,
    pub code: u16,
    pub down: bool,
    pub extended: bool,
    pub x: u16,
    pub y: u16,
    /// empty / left / middle / right. Position precedes the button event.
    pub button: String,
    pub vertical: i16,
    pub horizontal: i16,
    pub width: u16,
    pub height: u16,
}
impl std::fmt::Debug for RdpInputMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RdpInputMessage(<redacted>)")
    }
}

fn error(e: AppError) -> BridgeError {
    match e {
        AppError::Rdp(e) => BridgeError::new(e.code(), e.to_string()),
        other => other.into(),
    }
}

fn fingerprint(value: &str) -> Result<[u8; 32], BridgeError> {
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(BridgeError::invalid("certificate", "expected SHA-256"));
    }
    let mut out = [0; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[2 * i..2 * i + 2], 16)
            .map_err(|_| BridgeError::invalid("certificate", "expected SHA-256"))?;
    }
    Ok(out)
}

pub async fn rdp_probe_certificate(
    address: String,
    port: u16,
) -> Result<RdpCertificate, BridgeError> {
    with_core(move |c| async move {
        let info = c
            .rdp_probe_certificate(address.clone(), port)
            .await
            .map_err(error)?;
        Ok(RdpCertificate {
            address,
            port,
            sha256: info.fingerprint.replace(':', "").to_lowercase(),
        })
    })
    .await
}

pub async fn rdp_connect(
    options: RdpConnection,
    password: Vec<u8>,
    certificate_sha256: String,
) -> Result<RdpSessionInfo, BridgeError> {
    rdp_connect_with_permissions(
        options,
        password,
        certificate_sha256,
        RdpPermissions::default(),
    )
    .await
}

pub async fn rdp_connect_with_permissions(
    options: RdpConnection,
    password: Vec<u8>,
    certificate_sha256: String,
    permissions: RdpPermissions,
) -> Result<RdpSessionInfo, BridgeError> {
    let mut password = password;
    if password.is_empty() || password.len() > 4096 {
        password.zeroize();
        return Err(BridgeError::invalid("password", "invalid length"));
    }
    let password = cc_vault_core::SecretString::from(state::secret_string("password", password)?);
    let pin = fingerprint(&certificate_sha256)?;
    with_core(move |c| async move {
        let width = options.width;
        let height = options.height;
        let config = RdpConnectConfig {
            address: options.address,
            port: options.port,
            username: options.username,
            domain: (!options.domain.is_empty()).then_some(options.domain),
            width,
            height,
            accepted_certificate_sha256: pin,
        };
        let id = c
            .rdp_connect_with_permissions(config, password, permissions.into())
            .await
            .map_err(error)?;
        Ok(RdpSessionInfo { id, width, height })
    })
    .await
}

pub async fn rdp_capabilities() -> Result<RdpCapabilities, BridgeError> {
    with_core(|c| async move {
        let caps = c.rdp_capabilities().await.map_err(error)?;
        Ok(RdpCapabilities {
            clipboard_supported: caps.clipboard_supported,
            folder_supported: caps.folder_supported,
        })
    })
    .await
}
pub async fn rdp_pick_directory() -> Result<Option<RdpDirectoryGrant>, BridgeError> {
    with_core(|c| async move {
        Ok(c.rdp_pick_directory()
            .await
            .map_err(error)?
            .map(|grant| RdpDirectoryGrant {
                id: grant.id,
                name: grant.name,
            }))
    })
    .await
}
pub async fn rdp_release_directory_grant(grant_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { c.rdp_release_directory_grant(grant_id).await.map_err(error) })
        .await
}
pub async fn rdp_set_permissions(
    session_id: String,
    permissions: RdpPermissions,
) -> Result<(), BridgeError> {
    with_core(move |c| async move {
        c.rdp_set_permissions(session_id, permissions.into())
            .await
            .map_err(error)
    })
    .await
}
pub async fn rdp_permissions(session_id: String) -> Result<RdpPermissions, BridgeError> {
    with_core(
        move |c| async move { Ok(c.rdp_permissions(session_id).await.map_err(error)?.into()) },
    )
    .await
}
pub async fn rdp_offer_clipboard_text(session_id: String, text: String) -> Result<(), BridgeError> {
    let text = Zeroizing::new(text);
    with_core(move |c| async move {
        c.rdp_offer_clipboard_text(session_id, text)
            .await
            .map_err(error)
    })
    .await
}
pub async fn rdp_request_clipboard_text(session_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move {
        c.rdp_request_clipboard_text(session_id)
            .await
            .map_err(error)
    })
    .await
}
pub async fn rdp_take_clipboard_text(session_id: String) -> Result<Option<String>, BridgeError> {
    with_core(move |c| async move {
        Ok(c.rdp_take_clipboard_text(session_id)
            .await
            .map_err(error)?
            .map(|text| text.to_string()))
    })
    .await
}

pub async fn rdp_poll(session_id: String) -> Result<RdpPoll, BridgeError> {
    with_core(move |c| async move {
        let result = c.rdp_poll(session_id).await.map_err(error)?;
        let (phase, error_code) = match result.status {
            RdpSessionStatus::Connecting => ("connecting", None),
            RdpSessionStatus::Connected => ("connected", None),
            RdpSessionStatus::Disconnected => ("disconnected", None),
            RdpSessionStatus::Failed(e) => ("failed", Some(e.code().to_owned())),
        };
        Ok(RdpPoll {
            phase: phase.to_owned(),
            error_code,
            folder_status: result.folder_status.code().to_owned(),
            frame: result.frame.map(|f| RdpPixels {
                sequence: f.sequence,
                width: f.width,
                height: f.height,
                rgba: f.rgba,
            }),
        })
    })
    .await
}

fn input(mut messages: Vec<RdpInputMessage>) -> Result<Vec<RdpInput>, BridgeError> {
    if messages.len() > 128 || messages.iter().map(|e| e.text.len()).sum::<usize>() > 16384 {
        for e in &mut messages {
            e.text.zeroize();
        }
        return Err(BridgeError::invalid("input", "batch too large"));
    }
    // Validate the complete batch before moving any text into the runtime, so
    // malformed trailing events cannot leave earlier text in abandoned buffers.
    let invalid = messages.iter().any(|e| match e.kind.as_str() {
        "unicode" => false,
        "scancode" => e.code > u8::MAX as u16,
        "pointer" => !matches!(e.button.as_str(), "" | "left" | "middle" | "right"),
        "wheel" | "resize" | "release_all" => false,
        _ => true,
    });
    if invalid {
        for e in &mut messages {
            e.text.zeroize();
        }
        return Err(BridgeError::invalid("input", "invalid event"));
    }
    let mut events = Vec::with_capacity(messages.len());
    for mut e in messages {
        match e.kind.as_str() {
            "unicode" => events.push(RdpInput::UnicodeText(std::mem::take(&mut e.text))),
            "scancode" => events.push(RdpInput::Scancode {
                code: e.code as u8,
                down: e.down,
                extended: e.extended,
            }),
            "pointer" => {
                events.push(RdpInput::Pointer { x: e.x, y: e.y });
                let button = match e.button.as_str() {
                    "" => None,
                    "left" => Some(RdpMouseButton::Left),
                    "middle" => Some(RdpMouseButton::Middle),
                    "right" => Some(RdpMouseButton::Right),
                    _ => unreachable!("validated button"),
                };
                if let Some(button) = button {
                    events.push(RdpInput::Button {
                        button,
                        down: e.down,
                    });
                }
            }
            "wheel" => {
                events.push(RdpInput::Pointer { x: e.x, y: e.y });
                if e.vertical != 0 {
                    events.push(RdpInput::Wheel {
                        units: e.vertical,
                        horizontal: false,
                    });
                }
                if e.horizontal != 0 {
                    events.push(RdpInput::Wheel {
                        units: e.horizontal,
                        horizontal: true,
                    });
                }
            }
            "resize" => events.push(RdpInput::Resize {
                width: e.width,
                height: e.height,
            }),
            "release_all" => events.push(RdpInput::ReleaseAll),
            _ => unreachable!("validated input kind"),
        }
        e.text.zeroize();
    }
    Ok(events)
}

pub async fn rdp_send_input(
    session_id: String,
    messages: Vec<RdpInputMessage>,
) -> Result<(), BridgeError> {
    let events = input(messages)?;
    with_core(move |c| async move { c.rdp_send_input(session_id, events).await.map_err(error) })
        .await
}

pub async fn rdp_disconnect(session_id: String) -> Result<(), BridgeError> {
    with_core(move |c| async move { c.rdp_disconnect(session_id).await.map_err(error) }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(kind: &str) -> RdpInputMessage {
        RdpInputMessage {
            kind: kind.into(),
            text: String::new(),
            code: 0,
            down: false,
            extended: false,
            x: 0,
            y: 0,
            button: String::new(),
            vertical: 0,
            horizontal: 0,
            width: 0,
            height: 0,
        }
    }
    #[test]
    fn input_preserves_unicode_and_pointer_event_order() {
        let mut text = message("unicode");
        text.text = "Hello / Привет / 😀".into();
        let mut pointer = message("pointer");
        pointer.x = 100;
        pointer.y = 50;
        pointer.button = "left".into();
        pointer.down = true;
        let events = input(vec![text, pointer, message("release_all")]).unwrap();
        assert!(matches!(&events[0], RdpInput::UnicodeText(s) if s == "Hello / Привет / 😀"));
        assert!(matches!(events[1], RdpInput::Pointer { x: 100, y: 50 }));
        assert!(matches!(
            events[2],
            RdpInput::Button {
                button: RdpMouseButton::Left,
                down: true
            }
        ));
        assert!(matches!(events[3], RdpInput::ReleaseAll));
    }
    #[test]
    fn malformed_or_oversized_input_is_rejected_as_a_whole() {
        let mut scan = message("scancode");
        scan.code = 256;
        assert!(input(vec![message("unicode"), scan]).is_err());
        let mut pointer = message("pointer");
        pointer.button = "unrecognized".into();
        assert!(input(vec![pointer]).is_err());
        assert!(input(vec![message("unknown")]).is_err());
        assert!(input((0..129).map(|_| message("release_all")).collect()).is_err());
        let mut text = message("unicode");
        text.text = "x".repeat(16385);
        assert!(input(vec![text]).is_err());
    }
    #[test]
    fn certificate_pin_is_exact_and_bounded() {
        assert_eq!(fingerprint(&"ab".repeat(32)).unwrap(), [0xab; 32]);
        assert!(fingerprint(&"x".repeat(64)).is_err());
        assert!(fingerprint(&"ab".repeat(31)).is_err());
        assert!(fingerprint(&"😀".repeat(16)).is_err());
    }
    #[test]
    fn debug_never_includes_screen_bytes() {
        let frame = RdpPixels {
            sequence: 1,
            width: 1,
            height: 1,
            rgba: b"1234".to_vec(),
        };
        assert_eq!(format!("{frame:?}"), "RdpPixels(<redacted>)");
    }
}
