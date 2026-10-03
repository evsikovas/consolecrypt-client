//! Explicit opt-in native paste receipt acceptance; no process or command is executed.
use super::RdpManager;
use crate::{ConnectConfig, Input, SessionPermissions, SessionStatus};
use secrecy::SecretString;
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
struct PrivateConfig {
    address: String,
    port: u16,
    username: String,
    domain: Option<String>,
    password: String,
    certificate_sha256: String,
    width: u16,
    height: u16,
}
impl Drop for PrivateConfig {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}
fn read_config() -> Result<(PrivateConfig, [u8; 32]), &'static str> {
    let path =
        PathBuf::from(std::env::var_os("CC_RDP_TEST_CONFIG").ok_or("configuration_missing")?);
    let meta = std::fs::metadata(&path).map_err(|_| "configuration_unavailable")?;
    if !meta.is_file() || meta.len() > 16_384 {
        return Err("configuration_invalid");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("configuration_permissions");
        }
    }
    let bytes = Zeroizing::new(std::fs::read(path).map_err(|_| "configuration_unavailable")?);
    let config: PrivateConfig =
        serde_json::from_slice(&bytes).map_err(|_| "configuration_invalid")?;
    let fingerprint = config.certificate_sha256.replace(':', "");
    if fingerprint.len() != 64 || !fingerprint.is_ascii() {
        return Err("invalid_fingerprint");
    }
    let mut pin = [0u8; 32];
    for (i, byte) in pin.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&fingerprint[i * 2..i * 2 + 2], 16)
            .map_err(|_| "invalid_fingerprint")?;
    }
    Ok((config, pin))
}
fn key(code: u8, down: bool, extended: bool) -> Input {
    Input::Scancode {
        code,
        down,
        extended,
    }
}
pub(super) fn ctrl_key(manager: &RdpManager, id: &str, code: u8) -> Result<(), &'static str> {
    manager
        .send_input(
            id,
            vec![
                key(0x1d, true, false),
                key(code, true, false),
                key(code, false, false),
                key(0x1d, false, false),
            ],
        )
        .map_err(|e| e.code())
}
pub(super) fn reset_modifier_state(manager: &RdpManager, id: &str) -> Result<(), &'static str> {
    let mut reset = vec![Input::ReleaseAll];
    for (code, extended) in [
        (0x1d, false),
        (0x38, false),
        (0x2a, false),
        (0x36, false),
        (0x5b, true),
        (0x5c, true),
    ] {
        reset.extend([key(code, true, extended), key(code, false, extended)]);
    }
    reset.extend([key(0x01, true, false), key(0x01, false, false)]);
    manager.send_input(id, reset).map_err(|e| e.code())
}
fn clipboard_metadata(
    manager: &RdpManager,
    id: &str,
) -> Result<(bool, [u64; 5], [u64; 4]), &'static str> {
    let inner = manager.inner.lock().map_err(|_| "test_state")?;
    let state = inner
        .sessions
        .get(id)
        .ok_or("test_state")?
        .redirects
        .lock()
        .map_err(|_| "test_state")?;
    Ok((
        state.ready && state.enabled(),
        state.clipboard_counts,
        state.clipboard_send_counts,
    ))
}
async fn observe(
    manager: &RdpManager,
    id: &str,
    label: &'static str,
) -> Result<(bool, Option<(u16, u16)>), &'static str> {
    let mut poll = manager.poll(id).map_err(|e| e.code())?;
    match poll.status {
        SessionStatus::Failed(error) => return Err(error.code()),
        SessionStatus::Disconnected => return Err("disconnected"),
        _ => {}
    }
    let has_frame = poll.frame.is_some();
    let run_edit = poll
        .frame
        .as_ref()
        .and_then(super::live_redirect_tests::run_dialog_edit);
    if let Some(frame) = &mut poll.frame {
        if let Some(output) = std::env::var_os("CC_RDP_TEST_OUTPUT_DIR") {
            let dir = PathBuf::from(output);
            std::fs::create_dir_all(&dir).map_err(|_| "test_output_unavailable")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| "test_output_unavailable")?;
            }
            std::fs::write(dir.join(format!("clipboard-{label}.rgba")), &frame.rgba)
                .map_err(|_| "test_output_unavailable")?;
            std::fs::write(
                dir.join(format!("clipboard-{label}.json")),
                format!(
                    "{{\"width\":{},\"height\":{},\"sequence\":{}}}",
                    frame.width, frame.height, frame.sequence
                ),
            )
            .map_err(|_| "test_output_unavailable")?;
        }
        frame.rgba.zeroize();
    }
    Ok((has_frame, run_edit))
}
async fn window_settle(
    manager: &RdpManager,
    id: &str,
    label: &'static str,
) -> Result<(), &'static str> {
    // Window animations are distinct from CLIPRDR ordering: receipt-to-commit below has no sleep.
    let until = tokio::time::Instant::now() + Duration::from_millis(700);
    while tokio::time::Instant::now() < until {
        observe(manager, id, label).await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}
async fn roundtrip(manager: &RdpManager, id: &str) -> Result<(), &'static str> {
    let until = tokio::time::Instant::now() + Duration::from_secs(50);
    let mut frame_ready = false;
    loop {
        frame_ready |= observe(manager, id, "connected").await?.0;
        if frame_ready && clipboard_metadata(manager, id)?.0 {
            break;
        }
        if tokio::time::Instant::now() >= until {
            return Err("clipboard_startup_timeout");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!("RDP confirmed paste acceptance: connected_and_clipboard_ready");
    // The disposable target reuses a Windows session. Reset only modifier keys,
    // including remote state not known to this fresh local input database.
    reset_modifier_state(manager, id)?;
    window_settle(manager, id, "neutral").await?;
    // Open one bounded disposable Run edit field. Never press Enter or launch a process.
    manager
        .send_input(
            id,
            vec![
                Input::ReleaseAll,
                key(0x5b, true, true),
                key(0x13, true, false),
                key(0x13, false, false),
                key(0x5b, false, true),
            ],
        )
        .map_err(|e| e.code())?;
    let until = tokio::time::Instant::now() + Duration::from_secs(15);
    let (edit_x, edit_y) = loop {
        if let Some(edit) = observe(manager, id, "run-open").await?.1 {
            break edit;
        }
        if tokio::time::Instant::now() >= until {
            return Err("test_run_window_not_visible");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    println!("RDP confirmed paste acceptance: observed_run_window_ready");
    // This pointer is grounded in the observed default 1280x720 disposable Run
    // dialog edit rectangle, not an arbitrary desktop control or user window.
    manager
        .send_input(
            id,
            vec![
                Input::Pointer {
                    x: edit_x,
                    y: edit_y,
                },
                Input::Button {
                    button: crate::MouseButton::Left,
                    down: true,
                },
                Input::Button {
                    button: crate::MouseButton::Left,
                    down: false,
                },
            ],
        )
        .map_err(|e| e.code())?;
    ctrl_key(manager, id, 0x1e)?;
    manager
        .send_input(id, vec![Input::UnicodeText("CC-input-probe".to_owned())])
        .map_err(|e| e.code())?;
    window_settle(manager, id, "input-probe").await?;
    let baseline = copy_back(manager, id).await?;
    if baseline.as_str() != "CC-input-probe" {
        return Err("test_input_probe_mismatch");
    }
    println!("RDP confirmed paste acceptance: input_and_remote_copy_probe_pass");
    ctrl_key(manager, id, 0x1e)?;
    window_settle(manager, id, "before-paste").await?;
    let expected = Zeroizing::new(format!("CC-RDP-PASTE / Привет / {}", uuid::Uuid::new_v4()));
    let previous_send = clipboard_metadata(manager, id)?.2;
    let ticket = manager
        .offer_clipboard_text_confirmed(id, expected.to_string())
        .await
        .map_err(|e| e.code())?;
    println!("RDP confirmed paste acceptance: exact_offer_acknowledged");
    // The only paste injection is this explicit caller action after exact receipt.
    manager
        .commit_clipboard_paste(id, ticket.clone())
        .await
        .map_err(|e| e.code())?;
    if manager.commit_clipboard_paste(id, ticket).await
        != Err(crate::RdpError::ClipboardUnavailable)
    {
        return Err("paste_ticket_replay");
    }
    println!("RDP confirmed paste acceptance: explicit_commit_written_and_replay_denied");
    let until = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let counters = clipboard_metadata(manager, id)?.2;
        if counters[3] > previous_send[3] {
            return Err("paste_format_response_denied");
        }
        if counters[0] > previous_send[0] && counters[2] > previous_send[2] {
            break;
        }
        observe(manager, id, "paste-dispatch").await?;
        if tokio::time::Instant::now() >= until {
            return Err("paste_format_request_timeout");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!("RDP confirmed paste acceptance: windows_fetched_current_text");
    window_settle(manager, id, "after-paste").await?;
    let received = copy_back(manager, id).await?;
    if received.as_str() != expected.as_str() {
        return Err("paste_copyback_mismatch");
    }
    println!("RDP confirmed paste acceptance: exact_cyrillic_roundtrip_pass");
    Ok(())
}
async fn copy_back(manager: &RdpManager, id: &str) -> Result<Zeroizing<String>, &'static str> {
    ctrl_key(manager, id, 0x1e)?;
    let previous_formats = clipboard_metadata(manager, id)?.1[0];
    ctrl_key(manager, id, 0x2e)?;
    let until = tokio::time::Instant::now() + Duration::from_secs(20);
    while clipboard_metadata(manager, id)?.1[0] <= previous_formats {
        observe(manager, id, "after-copy").await?;
        if tokio::time::Instant::now() >= until {
            return Err("remote_copy_format_timeout");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    manager.request_clipboard_text(id).map_err(|e| e.code())?;
    let received = loop {
        if let Some(text) = manager.take_clipboard_text(id).map_err(|e| e.code())? {
            break text;
        }
        observe(manager, id, "after-copy").await?;
        if tokio::time::Instant::now() >= until {
            return Err("remote_copy_response_timeout");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    Ok(received)
}

#[tokio::test]
#[ignore = "requires the sole owner-authorized Windows session and a private mode-0600 config"]
async fn live_confirmed_ack_ticket_paste_real_windows() {
    let result: Result<(), &'static str> = async {
        if let Some(output) = std::env::var_os("CC_RDP_TEST_OUTPUT_DIR") {
            let dir = PathBuf::from(output);
            for label in ["connected", "neutral", "run-open", "input-probe", "before-paste", "after-paste", "after-copy", "paste-dispatch", "closed"] {
                for extension in ["rgba", "json", "png"] {
                    let _ = std::fs::remove_file(dir.join(format!("clipboard-{label}.{extension}")));
                }
            }
        }
        let (mut config, pin) = read_config()?;
        if config.width != 1280 || config.height != 720 {
            return Err("test_viewport_unsupported");
        }
        let manager = RdpManager::new();
        let id = manager.connect_with_permissions(
            ConnectConfig {
                address: config.address.clone(), port: config.port, username: config.username.clone(),
                domain: config.domain.clone(), width: config.width, height: config.height,
                accepted_certificate_sha256: pin,
            },
            SecretString::from(std::mem::take(&mut config.password)),
            SessionPermissions { clipboard_enabled: true, ..Default::default() },
        ).map_err(|e| e.code())?;
        let result = roundtrip(&manager, &id).await;
        // Close only the created Run dialog, including on assertion failure.
        let _ = manager.send_input(&id, vec![Input::ReleaseAll, key(0x01, true, false), key(0x01, false, false)]);
        let _ = window_settle(&manager, &id, "closed").await;
        if result.is_err() {
            if let Ok((_, counters, _)) = clipboard_metadata(&manager, &id) {
                println!("RDP confirmed paste counters: formats={} requests={} responses={} discarded={} invalid={}", counters[0], counters[1], counters[2], counters[3], counters[4]);
            }
            if let Ok(inner) = manager.inner.lock() {
                if let Some(session) = inner.sessions.get(&id) {
                    if let Ok(state) = session.redirects.lock() {
                        let c = state.clipboard_send_counts;
                        println!("RDP confirmed paste send counters: unicode_requests={} other_requests={} text_responses={} error_responses={}", c[0], c[1], c[2], c[3]);
                    }
                }
            }
        }
        manager.shutdown();
        result
    }.await;
    if let Err(code) = result {
        panic!("RDP confirmed paste acceptance failed: {code}");
    }
}
