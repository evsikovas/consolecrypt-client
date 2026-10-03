//! Opt-in acceptance against an owner-authorized disposable Windows session.
//! The test driver consumes a mode-0600 ignored JSON file; no account or text is logged.
use super::RdpManager;
use crate::{ConnectConfig, Input, SessionPermissions, SessionStatus};
use secrecy::SecretString;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
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
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for group in bytes.chunks(3) {
        let n = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        result.push(TABLE[((n >> 18) & 63) as usize] as char);
        result.push(TABLE[((n >> 12) & 63) as usize] as char);
        result.push(if group.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if group.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
async fn settle(manager: &RdpManager, id: &str, duration: Duration) -> Result<(), &'static str> {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        let mut poll = manager.poll(id).map_err(|e| e.code())?;
        if let Some(frame) = &mut poll.frame {
            record_frame(frame)?;
            frame.rgba.zeroize();
        }
        match poll.status {
            SessionStatus::Failed(e) => {
                if let Ok(diagnostic) = manager.diagnostics(id) {
                    // These fields are source-defined static categories, never
                    // remote diagnostics, paths, command text or file contents.
                    println!(
                        "RDP test-only failed session metadata: stage={} code={} write_attempted={} write_accepted={} write_read_ahead={} write_flush_started={} progress_1s={} progress_5s={} progress_9s={} last_progress_ms={} elapsed_ms={}",
                        diagnostic.last_stage,
                        e.code(),
                        diagnostic.last_write_attempted,
                        diagnostic.last_write_accepted,
                        diagnostic.last_write_read_ahead,
                        diagnostic.last_write_flush_started,
                        diagnostic.last_write_progress_samples[0],
                        diagnostic.last_write_progress_samples[1],
                        diagnostic.last_write_progress_samples[2],
                        diagnostic.last_write_last_progress_ms,
                        diagnostic.last_write_elapsed_ms,
                    );
                    for packet in &diagnostic.last_write_read_packets {
                        println!(
                            "RDP test-only write read packet: action={} length={} channel={} channel_kind={} control={} static_flags={:08x}",
                            packet.action, packet.length,
                            packet.channel.map(u32::from).unwrap_or(0),
                            packet.channel_kind, packet.control, packet.static_flags,
                        );
                    }
                }
                return Err(e.code());
            }
            SessionStatus::Disconnected => return Err("disconnected"),
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}
fn record_frame(frame: &crate::Frame) -> Result<(), &'static str> {
    if let Some(output) = std::env::var_os("CC_RDP_TEST_OUTPUT_DIR") {
        let dir = PathBuf::from(output);
        std::fs::create_dir_all(&dir).map_err(|_| "test_output_unavailable")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "test_output_unavailable")?;
        }
        std::fs::write(dir.join("latest.rgba"), &frame.rgba)
            .map_err(|_| "test_output_unavailable")?;
        std::fs::write(
            dir.join("latest.json"),
            format!(
                "{{\"width\":{},\"height\":{},\"sequence\":{}}}",
                frame.width, frame.height, frame.sequence
            ),
        )
        .map_err(|_| "test_output_unavailable")?;
    }
    Ok(())
}
// Test-only Windows Run detection grounded in the observed standard dialog.
// Match several static chrome points, never its text or arbitrary user controls.
pub(super) fn run_dialog_edit(frame: &crate::Frame) -> Option<(u16, u16)> {
    let width = usize::from(frame.width);
    let height = usize::from(frame.height);
    if width < 800 || height < 600 || frame.rgba.len() != width * height * 4 {
        return None;
    }
    let rgb = |x: usize, y: usize| {
        let offset = (y * width + x) * 4;
        &frame.rgba[offset..offset + 3]
    };
    let white = |pixel: &[u8]| pixel.iter().all(|channel| *channel >= 248);
    let gray = |pixel: &[u8]| {
        pixel.iter().all(|channel| *channel >= 235)
            && pixel.iter().max().unwrap() - pixel.iter().min().unwrap() <= 12
    };
    let blue = |pixel: &[u8]| pixel[0] < 50 && (100..=160).contains(&pixel[1]) && pixel[2] > 180;
    let cyan = |pixel: &[u8]| pixel[0] < 100 && pixel[1] > 150 && pixel[2] > 200;
    for top in 0..height - 232 {
        if gray(rgb(20, top + 21))
            && gray(rgb(430, top + 221))
            && white(rgb(70, top + 46))
            && (white(rgb(100, top + 103)) || blue(rgb(100, top + 103)))
            && (white(rgb(400, top + 119)) || blue(rgb(400, top + 119)))
            && cyan(rgb(33, top + 60))
            && blue(rgb(91, top + 111))
            && (blue(rgb(179, top + 185)) || {
                let pixel = rgb(179, top + 185);
                pixel.iter().all(|channel| *channel >= 180)
                    && pixel.iter().max().unwrap() - pixel.iter().min().unwrap() <= 12
            })
        {
            return Some((230, (top + 111) as u16));
        }
    }
    None
}
fn run_edit_empty(frame: &crate::Frame, edit: (u16, u16)) -> bool {
    let width = usize::from(frame.width);
    let y = usize::from(edit.1);
    (y - 8..y + 8).all(|row| {
        (100..390).all(|x| {
            let offset = (row * width + x) * 4;
            frame.rgba[offset..offset + 3]
                .iter()
                .all(|channel| *channel >= 248)
        })
    })
}
fn run_edit_selected(frame: &crate::Frame, edit: (u16, u16)) -> bool {
    // The observed Run history selection fills the edit interior in Windows
    // blue. Ignore its fixed blue border and isolated antialiased glyph pixels.
    let width = usize::from(frame.width);
    let y = usize::from(edit.1);
    let mut blue = 0;
    for row in y - 4..y + 5 {
        for x in 100..390 {
            let offset = (row * width + x) * 4;
            let pixel = &frame.rgba[offset..offset + 3];
            if pixel[0] < 50 && (100..=160).contains(&pixel[1]) && pixel[2] > 180 {
                blue += 1;
            }
        }
    }
    blue >= 128
}
pub(super) async fn wait_run_selection(manager: &RdpManager, id: &str) -> Result<(), &'static str> {
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        let mut poll = manager.poll(id).map_err(|e| e.code())?;
        let mut selected = false;
        if let Some(frame) = &mut poll.frame {
            record_frame(frame)?;
            selected = run_dialog_edit(frame)
                .is_some_and(|edit| run_edit_empty(frame, edit) || run_edit_selected(frame, edit));
            frame.rgba.zeroize();
        }
        match poll.status {
            SessionStatus::Failed(error) => return Err(error.code()),
            SessionStatus::Disconnected => return Err("disconnected"),
            _ => {}
        }
        if selected {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("run_edit_not_selected")
}
pub(super) async fn wait_run_edit(
    manager: &RdpManager,
    id: &str,
    require_empty: bool,
) -> Result<(u16, u16), &'static str> {
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        let mut poll = manager.poll(id).map_err(|e| e.code())?;
        let mut edit = None;
        if let Some(frame) = &mut poll.frame {
            record_frame(frame)?;
            edit = run_dialog_edit(frame)
                .filter(|edit| !require_empty || run_edit_empty(frame, *edit));
            frame.rgba.zeroize();
        }
        match poll.status {
            SessionStatus::Failed(error) => return Err(error.code()),
            SessionStatus::Disconnected => return Err("disconnected"),
            _ => {}
        }
        if let Some(edit) = edit {
            return Ok(edit);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(if require_empty {
        "run_edit_not_cleared"
    } else {
        "run_dialog_not_observed"
    })
}
#[test]
fn run_readiness_requires_static_dialog_chrome_and_valid_frame() {
    let mut frame = crate::Frame {
        sequence: 1,
        width: 1280,
        height: 720,
        rgba: vec![0; 1280 * 720 * 4],
    };
    assert_eq!(run_dialog_edit(&frame), None);
    let top = 429;
    for (x, y, color) in [
        (20, 21, [247, 243, 247]),
        (430, 221, [247, 243, 247]),
        (70, 46, [255, 255, 255]),
        (100, 103, [255, 255, 255]),
        (400, 119, [255, 255, 255]),
        (33, 60, [58, 198, 247]),
        (91, 111, [0, 121, 214]),
        (179, 185, [0, 121, 214]),
    ] {
        let offset = ((top + y) * 1280 + x) * 4;
        frame.rgba[offset..offset + 3].copy_from_slice(&color);
    }
    assert_eq!(run_dialog_edit(&frame), Some((230, 540)));
    for (x, y) in [(100, 103), (400, 119)] {
        let offset = ((top + y) * 1280 + x) * 4;
        frame.rgba[offset..offset + 3].copy_from_slice(&[0, 121, 214]);
    }
    assert_eq!(
        run_dialog_edit(&frame),
        Some((230, 540)),
        "selected Run history is still ready"
    );
    // A cleared Run field disables OK; its gray chrome is still a valid dialog.
    let ok_offset = ((top + 185) * 1280 + 179) * 4;
    frame.rgba[ok_offset..ok_offset + 3].copy_from_slice(&[214, 214, 214]);
    for row in top + 103..top + 119 {
        for x in 100..390 {
            let offset = (row * 1280 + x) * 4;
            frame.rgba[offset..offset + 3].copy_from_slice(&[255, 255, 255]);
        }
    }
    assert_eq!(run_dialog_edit(&frame), Some((230, 540)));
    assert!(run_edit_empty(&frame, (230, 540)));
    assert!(!run_edit_selected(&frame, (230, 540)));
    let typed_offset = ((top + 110) * 1280 + 120) * 4;
    frame.rgba[typed_offset..typed_offset + 3].copy_from_slice(&[30, 30, 30]);
    assert!(
        !run_edit_empty(&frame, (230, 540)),
        "visible residual text is not a cleared edit"
    );
    assert!(!run_edit_selected(&frame, (230, 540)));
    for row in top + 107..top + 116 {
        for x in 100..130 {
            let offset = (row * 1280 + x) * 4;
            frame.rgba[offset..offset + 3].copy_from_slice(&[0, 121, 214]);
        }
    }
    assert!(run_edit_selected(&frame, (230, 540)));
    let icon_offset = ((top + 60) * 1280 + 33) * 4;
    frame.rgba[icon_offset..icon_offset + 3].copy_from_slice(&[255, 255, 255]);
    assert_eq!(
        run_dialog_edit(&frame),
        None,
        "a generic white panel is not Run readiness"
    );
    frame.rgba.pop();
    assert_eq!(run_dialog_edit(&frame), None);
}
fn snapshot(label: &str) -> Result<(), &'static str> {
    if let Some(output) = std::env::var_os("CC_RDP_TEST_OUTPUT_DIR") {
        let dir = PathBuf::from(output);
        for extension in ["rgba", "json"] {
            let current = dir.join(format!("latest.{extension}"));
            if current.exists() {
                std::fs::copy(current, dir.join(format!("{label}.{extension}")))
                    .map_err(|_| "test_output_unavailable")?;
            }
        }
    }
    Ok(())
}
fn observed_file_stage(folder: &std::path::Path) -> &'static str {
    let path = folder.join("operations.stage");
    if !path.exists() {
        return "missing";
    }
    let Ok(metadata) = std::fs::metadata(&path) else {
        return "invalid";
    };
    if !metadata.is_file() || metadata.len() > 64 {
        return "invalid";
    }
    let Ok(value) = std::fs::read_to_string(path) else {
        return "invalid";
    };
    match value.as_str() {
        "enum" => "enum",
        "nested" => "nested",
        "read" => "read",
        "read_hash" => "read_hash",
        "write" => "write",
        "write_hash" => "write_hash",
        "overwrite" => "overwrite",
        "rename" => "rename",
        "delete" => "delete",
        "complete" => "complete",
        _ => "unknown",
    }
}
#[test]
fn file_stage_diagnostics_emit_only_closed_categories() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("operations.stage");
    assert_eq!(observed_file_stage(folder.path()), "missing");
    std::fs::write(&path, "read_hash").unwrap();
    assert_eq!(observed_file_stage(folder.path()), "read_hash");
    std::fs::write(&path, uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(observed_file_stage(folder.path()), "unknown");
    std::fs::write(&path, [0xff]).unwrap();
    assert_eq!(observed_file_stage(folder.path()), "invalid");
    std::fs::write(&path, [b'x'; 65]).unwrap();
    assert_eq!(observed_file_stage(folder.path()), "invalid");
}
fn scan_key(code: u8, down: bool, extended: bool) -> Input {
    Input::Scancode {
        code,
        down,
        extended,
    }
}
async fn prepare_run(manager: &RdpManager, id: &str) -> Result<(), &'static str> {
    // Reuse the live-proven explicit remote modifier reset. A new local Input
    // Database cannot release keys left down by a previous disconnected session.
    super::clipboard_live_tests::reset_modifier_state(manager, id)?;
    settle(manager, id, Duration::from_millis(700)).await?;
    manager
        .send_input(
            id,
            vec![
                scan_key(0x5b, true, true),
                scan_key(0x13, true, false),
                scan_key(0x13, false, false),
                scan_key(0x5b, false, true),
            ],
        )
        .map_err(|e| e.code())?;
    let (x, y) = wait_run_edit(manager, id, false).await?;
    snapshot("file-run-ready")?;
    manager
        .send_input(
            id,
            vec![
                Input::Pointer { x, y },
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
    // Drain the actual pointer/focus repaint before sending a selection chord.
    // A queued click alone is not evidence that Windows has focused the edit.
    settle(manager, id, Duration::from_millis(700)).await?;
    super::clipboard_live_tests::ctrl_key(manager, id, 0x1e)?;
    wait_run_selection(manager, id).await?;
    manager
        .send_input(
            id,
            vec![scan_key(0x0e, true, false), scan_key(0x0e, false, false)],
        )
        .map_err(|e| e.code())?;
    wait_run_edit(manager, id, true).await?;
    snapshot("file-run-cleared")
}
fn remote_formats(manager: &RdpManager, id: &str) -> Result<u64, &'static str> {
    let inner = manager.inner.lock().map_err(|_| "test_state")?;
    let state = inner
        .sessions
        .get(id)
        .ok_or("test_state")?
        .redirects
        .lock()
        .map_err(|_| "test_state")?;
    Ok(state.clipboard_counts[0])
}
fn sent_text_responses(manager: &RdpManager, id: &str) -> Result<u64, &'static str> {
    let inner = manager.inner.lock().map_err(|_| "test_state")?;
    let state = inner
        .sessions
        .get(id)
        .ok_or("test_state")?
        .redirects
        .lock()
        .map_err(|_| "test_state")?;
    Ok(state.clipboard_send_counts[2])
}
async fn paste_run_command(manager: &RdpManager, id: &str, text: &str) -> Result<(), &'static str> {
    let previous = sent_text_responses(manager, id)?;
    let ticket = manager
        .offer_clipboard_text_confirmed(id, text.to_string())
        .await
        .map_err(|e| e.code())?;
    manager
        .commit_clipboard_paste(id, ticket)
        .await
        .map_err(|e| e.code())?;
    let until = Instant::now() + Duration::from_secs(15);
    while sent_text_responses(manager, id)? <= previous && Instant::now() < until {
        settle(manager, id, Duration::from_millis(100)).await?;
    }
    if sent_text_responses(manager, id)? <= previous {
        return Err("launcher_paste_request_timeout");
    }
    Ok(())
}
async fn verify_run_command(
    manager: &RdpManager,
    id: &str,
    expected: &str,
) -> Result<(), &'static str> {
    // Launcher prerequisite only: copy the command we just typed in the observed
    // disposable Run field, compare in zeroizing memory, never log its text.
    // File-operation assertions themselves remain independent filesystem markers.
    let previous = remote_formats(manager, id)?;
    super::clipboard_live_tests::ctrl_key(manager, id, 0x1e)?;
    super::clipboard_live_tests::ctrl_key(manager, id, 0x2e)?;
    let until = Instant::now() + Duration::from_secs(15);
    while remote_formats(manager, id)? <= previous && Instant::now() < until {
        settle(manager, id, Duration::from_millis(100)).await?;
    }
    if remote_formats(manager, id)? <= previous {
        return Err("launcher_copy_format_timeout");
    }
    let actual = receive(manager, id).await?;
    if actual.as_str() != expected {
        snapshot("file-launcher-mismatch")?;
        return Err("launcher_command_mismatch");
    }
    println!("RDP harness phase: exact_run_command_verified");
    Ok(())
}
async fn command_file(
    manager: &RdpManager,
    id: &str,
    folder: &std::path::Path,
    script: &str,
) -> Result<crate::directory::TestFileProgress, &'static str> {
    let name = format!("cc-{}.ps1", uuid::Uuid::new_v4().simple());
    let progress = crate::directory::TestFileProgress::new(&name).map_err(|_| "folder_fixture")?;
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(folder.join(&name), bytes).map_err(|_| "folder_fixture")?;
    prepare_run(manager, id).await?;
    let diagnostic = if std::env::var_os("CC_RDP_TEST_KEEP_CONSOLE").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        " -NoExit"
    } else {
        ""
    };
    let text = format!(
        r"powershell.exe -NoProfile -STA{diagnostic} -ExecutionPolicy Bypass -File \\tsclient\ConsoleCrypt\{name}"
    );
    let text = if std::env::var_os("CC_RDP_TEST_DIAGNOSE_FILE").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        // Separate the PowerShell script loader from plain Get-Content. This
        // diagnostic mode intentionally stops at the existing bounded result timeout.
        String::from(
            r"powershell.exe -NoProfile -NoExit -Command type \\tsclient\ConsoleCrypt\sentinel.txt",
        )
    } else {
        text
    };
    // Short Run commands stay below the Windows edit limit. Unicode packets
    // avoid changing or depending on the disposable session's current layout.
    paste_run_command(manager, id, &text).await?;
    verify_run_command(manager, id, &text).await?;
    snapshot("file-run-before-enter")?;
    enter(manager, id)?;
    settle(manager, id, Duration::from_secs(5)).await?;
    snapshot("file-command-after-enter")?;
    Ok(progress)
}
async fn command_remote_file(
    manager: &RdpManager,
    id: &str,
    name: &str,
) -> Result<(), &'static str> {
    prepare_run(manager, id).await?;
    let text = format!(
        "powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -Command \"& (Join-Path $env:TEMP '{name}')\""
    );
    paste_run_command(manager, id, &text).await?;
    verify_run_command(manager, id, &text).await?;
    enter(manager, id)?;
    settle(manager, id, Duration::from_secs(5)).await
}
async fn wait_marker(
    manager: &RdpManager,
    id: &str,
    folder: &std::path::Path,
    name: &str,
) -> Result<(), &'static str> {
    let until = Instant::now() + Duration::from_secs(30);
    let path = folder.join(name);
    while Instant::now() < until {
        if let Ok(metadata) = std::fs::metadata(&path) {
            if !metadata.is_file() || metadata.len() > 64 {
                return Err("file_marker_invalid");
            }
            // Empty files are observable briefly between create and write.
            if metadata.len() > 0 {
                let value = std::fs::read_to_string(&path).map_err(|_| "file_marker_invalid")?;
                return if value == "PASS" {
                    Ok(())
                } else {
                    Err("file_marker_mismatch")
                };
            }
        }
        settle(manager, id, Duration::from_millis(100)).await?;
    }
    Err("file_marker_timeout")
}
async fn wait_folder(manager: &RdpManager, id: &str) -> Result<(), &'static str> {
    let until = Instant::now() + Duration::from_secs(25);
    while Instant::now() < until {
        match manager.poll(id).map_err(|e| e.code())?.folder_status {
            crate::FolderStatus::Ready => {
                println!("RDP channel phase: selected_folder_accepted");
                return Ok(());
            }
            crate::FolderStatus::Denied => return Err("folder_denied"),
            crate::FolderStatus::Unavailable => return Err("folder_unavailable"),
            _ => {}
        }
        settle(manager, id, Duration::from_millis(100)).await?;
    }
    Err("folder_ack_timeout")
}
async fn command(manager: &RdpManager, id: &str, script: &str) -> Result<(), &'static str> {
    command_impl(manager, id, script, true).await
}
async fn command_impl(
    manager: &RdpManager,
    id: &str,
    script: &str,
    paste: bool,
) -> Result<(), &'static str> {
    prepare_run(manager, id).await?;
    let utf16: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let short = format!(
        "powershell.exe -NoProfile -STA -EncodedCommand {}",
        base64(&utf16)
    );
    if short.len() < 240 {
        type_text(manager, id, &short).await?;
        snapshot("run-before-enter")?;
        enter(manager, id)?;
        settle(manager, id, Duration::from_secs(4)).await?;
        return Ok(());
    }
    // Run has a small editable buffer on some Windows builds. Open a short command,
    // then enter the bounded synthetic script into the console instead.
    type_text(manager, id, "powershell.exe -NoProfile -STA").await?;
    snapshot("run-before-enter")?;
    enter(manager, id)?;
    settle(manager, id, Duration::from_secs(3)).await?;
    snapshot("console-open")?;
    let text = short;
    if paste {
        // Unicode VK_PACKET entry is unreliable in Windows console/PSReadLine for
        // long synthetic commands. Clipboard Send was independently verified first;
        // paste this bounded test command through that explicit opt-in channel.
        manager
            .offer_clipboard_text(id, text)
            .map_err(|e| e.code())?;
        settle(manager, id, Duration::from_secs(1)).await?;
        manager
            .send_input(
                id,
                vec![
                    Input::Scancode {
                        code: 0x1d,
                        down: true,
                        extended: false,
                    },
                    Input::Scancode {
                        code: 0x2f,
                        down: true,
                        extended: false,
                    },
                ],
            )
            .map_err(|e| e.code())?;
        settle(manager, id, Duration::from_millis(150)).await?;
        manager
            .send_input(id, vec![Input::ReleaseAll])
            .map_err(|e| e.code())?;
        settle(manager, id, Duration::from_secs(1)).await?;
    } else {
        type_ascii_scancodes(manager, id, &text).await?;
    }
    snapshot("console-before-enter")?;
    enter(manager, id)?;
    settle(manager, id, Duration::from_secs(4)).await?;
    snapshot("console-after-enter")?;
    Ok(())
}
async fn type_ascii_scancodes(
    manager: &RdpManager,
    id: &str,
    text: &str,
) -> Result<(), &'static str> {
    const LETTERS: [u8; 26] = [
        0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
    ];
    for chunk in text.as_bytes().chunks(8) {
        let mut events = Vec::new();
        for b in chunk {
            let (code, shift) = match *b {
                b'a'..=b'z' => (LETTERS[usize::from(*b - b'a')], false),
                b'A'..=b'Z' => (LETTERS[usize::from(*b - b'A')], true),
                b'1'..=b'9' => (*b - b'1' + 2, false),
                b'0' => (0x0b, false),
                b' ' => (0x39, false),
                b'.' => (0x34, false),
                b'/' => (0x35, false),
                b'-' => (0x0c, false),
                b'=' => (0x0d, false),
                b'+' => (0x0d, true),
                b'\\' => (0x2b, false),
                _ => return Err("test_command_invalid"),
            };
            if shift {
                events.push(Input::Scancode {
                    code: 0x2a,
                    down: true,
                    extended: false,
                });
            }
            events.push(Input::Scancode {
                code,
                down: true,
                extended: false,
            });
            events.push(Input::Scancode {
                code,
                down: false,
                extended: false,
            });
            if shift {
                events.push(Input::Scancode {
                    code: 0x2a,
                    down: false,
                    extended: false,
                });
            }
        }
        manager.send_input(id, events).map_err(|e| e.code())?;
        settle(manager, id, Duration::from_millis(100)).await?;
    }
    settle(manager, id, Duration::from_millis(300)).await
}
async fn type_text(manager: &RdpManager, id: &str, text: &str) -> Result<(), &'static str> {
    if !text.is_ascii() {
        return Err("test_command_invalid");
    }
    for bytes in text.as_bytes().chunks(8) {
        let chunk = String::from_utf8(bytes.into()).map_err(|_| "test_command_invalid")?;
        manager
            .send_input(id, vec![Input::UnicodeText(chunk)])
            .map_err(|e| e.code())?;
        settle(manager, id, Duration::from_millis(125)).await?;
    }
    settle(manager, id, Duration::from_millis(300)).await
}
fn enter(manager: &RdpManager, id: &str) -> Result<(), &'static str> {
    manager
        .send_input(
            id,
            vec![
                Input::Scancode {
                    code: 0x1c,
                    down: true,
                    extended: false,
                },
                Input::Scancode {
                    code: 0x1c,
                    down: false,
                    extended: false,
                },
            ],
        )
        .map_err(|e| e.code())
}
async fn receive(manager: &RdpManager, id: &str) -> Result<Zeroizing<String>, &'static str> {
    let until = Instant::now() + Duration::from_secs(30);
    let mut requested = false;
    let mut attempts = 0;
    while Instant::now() < until {
        if let Some(text) = manager.take_clipboard_text(id).map_err(|e| e.code())? {
            return Ok(text);
        }
        // Remote clipboard changes invalidate an old wire request. Once its response
        // drains, make another bounded explicit request within this test action.
        if attempts < 8 {
            match manager.request_clipboard_text(id) {
                Ok(()) => {
                    requested = true;
                    attempts += 1;
                }
                Err(crate::RdpError::ClipboardUnavailable | crate::RdpError::InputQueueFull) => {}
                Err(e) => return Err(e.code()),
            }
        }
        settle(manager, id, Duration::from_millis(200)).await?;
    }
    {
        let inner = manager.inner.lock().map_err(|_| "test_state")?;
        let s = inner
            .sessions
            .get(id)
            .ok_or("test_state")?
            .redirects
            .lock()
            .map_err(|_| "test_state")?;
        println!(
            "RDP clipboard counters: formats={} requests={} responses={} discarded={} invalid={}",
            s.clipboard_counts[0],
            s.clipboard_counts[1],
            s.clipboard_counts[2],
            s.clipboard_counts[3],
            s.clipboard_counts[4]
        );
        println!(
            "RDP test-only drive counters: create={} query={} read={} write={} other={} success={} denied={} not_found={} unsupported={} other_status={}",
            s.drive_operation_counts[0], s.drive_operation_counts[1],
            s.drive_operation_counts[2], s.drive_operation_counts[3],
            s.drive_operation_counts[4], s.drive_status_counts[0],
            s.drive_status_counts[1], s.drive_status_counts[2],
            s.drive_status_counts[3], s.drive_status_counts[4]
        );
    }
    Err(if requested {
        "clipboard_response_timeout"
    } else {
        "clipboard_format_timeout"
    })
}

#[tokio::test]
#[ignore = "requires an authorized disposable Windows target and a private mode-0600 JSON config"]
async fn live_clipboard_and_selected_folder_roundtrip() {
    if let Err(code) = run(false, false, false).await {
        panic!("RDP channel acceptance failed: {code}");
    }
}
#[tokio::test]
#[ignore = "requires owner-authorized Windows config and synthetic selected folder only"]
async fn live_selected_folder_real_windows_operations() {
    if let Err(code) = run(true, false, false).await {
        panic!("RDP folder acceptance failed: {code}");
    }
}
#[tokio::test]
#[ignore = "requires owner-authorized Windows config; scoped readonly/write/large-file IO only"]
async fn live_selected_folder_core_windows_operations() {
    if let Err(code) = run(true, true, false).await {
        panic!("RDP core folder acceptance failed: {code}");
    }
}
#[tokio::test]
#[ignore = "requires owner-authorized Windows config; stable writable large-file IO only"]
async fn live_selected_folder_stable_writable_windows_operations() {
    if let Err(code) = run(true, true, true).await {
        panic!("RDP stable-writable folder acceptance failed: {code}");
    }
}
async fn run(files_only: bool, core_only: bool, stable_writable: bool) -> Result<(), &'static str> {
    let path =
        PathBuf::from(std::env::var_os("CC_RDP_TEST_CONFIG").ok_or("configuration_missing")?);
    let meta = std::fs::metadata(&path).map_err(|_| "configuration_unavailable")?;
    if !meta.is_file() || meta.len() > 16384 {
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
    let mut config: PrivateConfig =
        serde_json::from_slice(&bytes).map_err(|_| "configuration_invalid")?;
    drop(bytes);
    let clean = config.certificate_sha256.replace(':', "");
    if clean.len() != 64 || !clean.is_ascii() {
        return Err("invalid_fingerprint");
    }
    let mut pin = [0u8; 32];
    for (i, b) in pin.iter_mut().enumerate() {
        *b = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).map_err(|_| "invalid_fingerprint")?;
    }
    let manager = RdpManager::new();
    let folder = tempfile::tempdir().map_err(|_| "folder_fixture")?;
    let sentinel = format!("CC-RDP-READ-{}", uuid::Uuid::new_v4());
    std::fs::write(folder.path().join("sentinel.txt"), &sentinel).map_err(|_| "folder_fixture")?;
    std::fs::create_dir(folder.path().join("папка")).map_err(|_| "folder_fixture")?;
    std::fs::write(folder.path().join("папка/пример.txt"), &sentinel)
        .map_err(|_| "folder_fixture")?;
    let seed = *uuid::Uuid::new_v4().as_bytes();
    let binary: Vec<u8> = (0..1024 * 1024)
        .map(|i| (i as u8).wrapping_mul(17).wrapping_add(seed[i % 16]))
        .collect();
    std::fs::write(folder.path().join("binary.bin"), &binary).map_err(|_| "folder_fixture")?;
    let readonly_completion =
        crate::directory::TestReadCompletion::new(folder.path()).map_err(|_| "folder_fixture")?;
    let binary_hash = format!("{:X}", Sha256::digest(&binary));
    let grant = manager
        .register_directory(folder.path())
        .map_err(|e| e.code())?;
    let mut permissions = SessionPermissions {
        clipboard_enabled: true,
        directory_grant_id: stable_writable.then(|| grant.id.clone()),
        directory_writable: stable_writable,
    };
    let id = manager
        .connect_with_permissions(
            ConnectConfig {
                address: config.address.clone(),
                port: config.port,
                username: config.username.clone(),
                domain: config.domain.clone(),
                width: config.width,
                height: config.height,
                accepted_certificate_sha256: pin,
            },
            SecretString::from(std::mem::take(&mut config.password)),
            permissions.clone(),
        )
        .map_err(|e| e.code())?;
    let until = Instant::now() + Duration::from_secs(180);
    let mut frame_ready = false;
    let mut observed = (false, false, false);
    let mut observed_handshake = 0;
    let mut graphics_since = None;
    while Instant::now() < until {
        let mut poll = manager.poll(&id).map_err(|e| e.code())?;
        if let Some(frame) = &mut poll.frame {
            frame_ready = true;
            frame.rgba.zeroize();
        }
        if let SessionStatus::Failed(e) = poll.status {
            return Err(e.code());
        }
        let (sequence, clipboard_ready, drive_ready, handshake) = {
            let inner = manager.inner.lock().map_err(|_| "test_state")?;
            let session = inner.sessions.get(&id).ok_or("test_state")?;
            let sequence = session.state.lock().map_err(|_| "test_state")?.sequence;
            let s = session.redirects.lock().map_err(|_| "test_state")?;
            (sequence, s.ready, s.drive_ready, s.drive_handshake)
        };
        frame_ready |= sequence > 0;
        for (index, (current, previous)) in [
            (frame_ready, observed.0),
            (clipboard_ready, observed.1),
            (drive_ready, observed.2),
        ]
        .into_iter()
        .enumerate()
        {
            if current && !previous {
                println!(
                    "RDP channel readiness: {}",
                    ["graphics", "clipboard", "drive"][index]
                );
            }
        }
        for (bit, label) in [
            (1, "announce"),
            (2, "capabilities"),
            (4, "client_confirm"),
            (8, "logged_on"),
            (16, "device_reply"),
        ] {
            if handshake & bit != 0 && observed_handshake & bit == 0 {
                println!("RDP drive handshake: {label}");
            }
        }
        observed_handshake = handshake;
        observed = (frame_ready, clipboard_ready, drive_ready);
        if frame_ready {
            let first = graphics_since.get_or_insert_with(Instant::now);
            if first.elapsed() > Duration::from_secs(30) && (!clipboard_ready || !drive_ready) {
                return Err("channel_initialization_timeout");
            }
        }

        let ready = clipboard_ready && drive_ready;
        if frame_ready && ready {
            break;
        }
        settle(&manager, &id, Duration::from_millis(100)).await?;
    }
    if !frame_ready {
        return Err("frame_timeout");
    }
    if !observed.1 || !observed.2 {
        return Err("channel_initialization_timeout");
    }
    settle(&manager, &id, Duration::from_secs(3)).await?;
    // Harness-only recovery for the explicitly observed post-reboot Shutdown
    // Event Tracker on the disposable 1280x720 Windows fixture. Cancel dismisses
    // the diagnostic dialog; it does not choose a shutdown reason or change settings.
    if std::env::var_os("CC_RDP_TEST_DISMISS_SHUTDOWN_TRACKER").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        if config.width != 1280 || config.height != 720 {
            return Err("fixture_dialog_dimensions");
        }
        manager
            .send_input(
                &id,
                vec![
                    Input::ReleaseAll,
                    Input::Pointer { x: 668, y: 503 },
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
        settle(&manager, &id, Duration::from_secs(1)).await?;
        snapshot("shutdown-tracker-dismissed")?;
        println!("RDP harness phase: observed_shutdown_tracker_cancel_sent");
    }
    println!("RDP channel phase: ready");
    if !files_only {
        command(&manager, &id, "Set-Clipboard 'CC-RDP-REMOTE-READY'").await?;
        if receive(&manager, &id).await?.trim_end() != "CC-RDP-REMOTE-READY" {
            return Err("remote_command_mismatch");
        }
        println!("RDP channel phase: remote_command_and_receive_pass");
    }
    if !files_only {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let text = format!("{} / Привет", &suffix[..8]);
        manager
            .offer_clipboard_text(&id, text.clone())
            .map_err(|e| e.code())?;
        settle(&manager, &id, Duration::from_secs(1)).await?;
        command(
            &manager,
            &id,
            &format!("Set-Clipboard ((Get-Clipboard -Raw) -ceq '{text}')"),
        )
        .await?;
        if receive(&manager, &id).await?.trim_end() != "True" {
            return Err("clipboard_send_mismatch");
        }
        println!("RDP channel phase: clipboard_send_and_receive_pass");
    }
    if !stable_writable {
        permissions.directory_grant_id = Some(grant.id);
        manager
            .set_permissions(&id, permissions.clone())
            .map_err(|e| e.code())?;
    }
    wait_folder(&manager, &id).await?;

    // One PowerShell worker remains in memory across the folder capability
    // transition. Owner nonce go files coordinate phases; there is no repeated
    // Run dialog/Terminal launch or clipboard dependency in the IO assertions.
    let writable_start =
        crate::directory::TestReadCompletion::new(folder.path()).map_err(|_| "folder_fixture")?;
    let written = format!("CC-RDP-WRITE-{} / Привет", uuid::Uuid::new_v4());
    let write_go = format!("cc-{}-write.go", uuid::Uuid::new_v4().simple());
    let operations_go = format!("cc-{}-operations.go", uuid::Uuid::new_v4().simple());
    let write_go_progress =
        crate::directory::TestFileProgress::new(&write_go).map_err(|_| "folder_fixture")?;
    let operations_go_progress =
        crate::directory::TestFileProgress::new(&operations_go).map_err(|_| "folder_fixture")?;
    let remote_temp = format!("cc-rdp-{}.bin", uuid::Uuid::new_v4());
    let operations_script = format!(
        r#"$ErrorActionPreference='Stop';$root='\\tsclient\ConsoleCrypt\';$t=Join-Path $env:TEMP '{remote_temp}';$phase='enum';[IO.File]::WriteAllText($root+'operations.stage',$phase);try{{
        $names=@(Get-ChildItem -LiteralPath $root|Select-Object -ExpandProperty Name);
        if(-not($names -contains 'binary.bin') -or -not($names -contains 'папка')){{throw 'enum'}};
        $phase='nested';[IO.File]::WriteAllText($root+'operations.stage',$phase);if((Get-ChildItem -LiteralPath ($root+'папка')|Select-Object -ExpandProperty Name) -cne 'пример.txt'){{throw 'nested'}};
        $phase='read';[IO.File]::WriteAllText($root+'operations.stage',$phase);
        Copy-Item -LiteralPath ($root+'binary.bin') -Destination $t;
        $phase='read_hash';[IO.File]::WriteAllText($root+'operations.stage',$phase);
        if((Get-FileHash -LiteralPath $t -Algorithm SHA256).Hash -cne '{binary_hash}'){{throw 'read'}};
        $phase='write';[IO.File]::WriteAllText($root+'operations.stage',$phase);Copy-Item -LiteralPath $t -Destination ($root+'returned.bin');
        $phase='write_hash';[IO.File]::WriteAllText($root+'operations.stage',$phase);
        if((Get-FileHash -LiteralPath ($root+'returned.bin') -Algorithm SHA256).Hash -cne '{binary_hash}'){{throw 'write'}};
        $phase='overwrite';[IO.File]::WriteAllText($root+'operations.stage',$phase);[IO.File]::WriteAllText($root+'returned.bin','old');Copy-Item -LiteralPath $t -Destination ($root+'returned.bin') -Force;
        $phase='rename';[IO.File]::WriteAllText($root+'operations.stage',$phase);
        Rename-Item -LiteralPath ($root+'returned.bin') -NewName 'переименован.bin';
        if((Get-FileHash -LiteralPath ($root+'переименован.bin') -Algorithm SHA256).Hash -cne '{binary_hash}'){{throw 'overwrite'}};
        $phase='delete';[IO.File]::WriteAllText($root+'operations.stage',$phase);Copy-Item -LiteralPath ($root+'переименован.bin') -Destination ($root+'delete.bin');
        Remove-Item -LiteralPath ($root+'delete.bin');
        if(Test-Path -LiteralPath ($root+'delete.bin')){{throw 'delete'}};
        [IO.File]::WriteAllText($root+'operations.stage','complete');$result='PASS';
    }}catch{{$result='FAIL_'+$phase}}finally{{Remove-Item -LiteralPath $t -ErrorAction SilentlyContinue}};[IO.File]::WriteAllText($root+'operations.result',$result)"#
    );
    let readonly_script = if stable_writable {
        String::new()
    } else {
        format!(
            r#"$ok=$false;try{{$ok=[IO.File]::ReadAllText($root+'sentinel.txt') -ceq '{sentinel}';try{{[IO.File]::WriteAllText($root+'sentinel.txt','forbidden');$ok=$false}}catch{{}}}}catch{{}};
            if($ok){{$m='{}'}}else{{$m='{}'}};[IO.File]::ReadAllText($root+$m)|Out-Null;"#,
            readonly_completion.pass_name, readonly_completion.fail_name
        )
    };
    let mut worker = format!(
        r#"$ErrorActionPreference='Stop';$root='\\tsclient\ConsoleCrypt\';
        function Wait-Owner($name){{$deadline=[DateTime]::UtcNow.AddSeconds(60);while(-not [IO.File]::Exists($root+$name)){{if([DateTime]::UtcNow -ge $deadline){{throw 'phase_timeout'}};Start-Sleep -Milliseconds 100}};[IO.File]::ReadAllText($root+$name)|Out-Null}};
        {readonly_script}
        Wait-Owner '{write_go}';[IO.File]::ReadAllText($root+'{}')|Out-Null;
        try{{$p=$root+'roundtrip.txt';[IO.File]::WriteAllText($p,'{written}');if([IO.File]::ReadAllText($p) -ceq '{written}'){{$v='PASS'}}else{{$v='FAIL'}};[IO.File]::WriteAllText($root+'writable.result',$v)}}catch{{[IO.File]::WriteAllText($root+'writable.result','FAIL')}};
        Wait-Owner '{operations_go}';"#,
        writable_start.pass_name
    );
    worker.push_str(&operations_script);
    if stable_writable {
        // This mode isolates wire framing/IO: no topology transition or cached
        // negative path lookup is a prerequisite for the large-copy proof.
        std::fs::write(folder.path().join(&write_go), b"1").map_err(|_| "folder_fixture")?;
        std::fs::write(folder.path().join(&operations_go), b"1").map_err(|_| "folder_fixture")?;
        println!("RDP channel phase: stable_writable_from_connection_start");
    }
    let loader = match command_file(&manager, &id, folder.path(), &worker).await {
        Ok(loader) => loader,
        Err(code) => {
            println!(
                "RDP test-only observed file stage: {}",
                observed_file_stage(folder.path())
            );
            return Err(code);
        }
    };
    println!(
        "RDP test-only worker loader: opened={} read={} denied={} other_failure={}",
        loader.opened(),
        loader.read(),
        loader.denied(),
        loader.other_failure()
    );
    if !stable_writable {
        let until = Instant::now() + Duration::from_secs(30);
        while readonly_completion.result().is_none() && Instant::now() < until {
            settle(&manager, &id, Duration::from_millis(100)).await?;
        }
        match readonly_completion.result() {
            Some(true) => println!("RDP channel phase: readonly_read_and_write_denial_pass"),
            Some(false) => return Err("readonly_remote_mismatch"),
            None => return Err("readonly_completion_timeout"),
        }
        if std::fs::read_to_string(folder.path().join("sentinel.txt"))
            .map_err(|_| "folder_fixture")?
            != sentinel
        {
            return Err("readonly_local_changed");
        }
        permissions.directory_writable = true;
        manager
            .set_permissions(&id, permissions.clone())
            .map_err(|e| e.code())?;
        wait_folder(&manager, &id).await?;
        std::fs::write(folder.path().join(&write_go), b"1").map_err(|_| "folder_fixture")?;
    }
    let launch_started = Instant::now();
    while writable_start.result().is_none() && launch_started.elapsed() < Duration::from_secs(60) {
        settle(&manager, &id, Duration::from_millis(100)).await?;
    }
    println!("RDP test-only writable launch: opened={} read={} denied={} other_failure={} body_started={} elapsed_seconds={}", loader.opened(), loader.read(), loader.denied(), loader.other_failure(), writable_start.result() == Some(true), launch_started.elapsed().as_secs());
    println!(
        "RDP test-only write-go progress: opened={} read={} denied={} other_failure={}",
        write_go_progress.opened(),
        write_go_progress.read(),
        write_go_progress.denied(),
        write_go_progress.other_failure()
    );
    snapshot("writable-launch-final")?;
    if writable_start.result() != Some(true) {
        return Err("writable_body_start_timeout");
    }
    wait_marker(&manager, &id, folder.path(), "writable.result").await?;
    if std::fs::read_to_string(folder.path().join("roundtrip.txt"))
        .map_err(|_| "writable_local_missing")?
        != written
    {
        return Err("writable_local_mismatch");
    }
    println!("RDP channel phase: writable_roundtrip_pass");
    if !stable_writable {
        std::fs::write(folder.path().join(&operations_go), b"1").map_err(|_| "folder_fixture")?;
    }
    // The 1MiB fixture performs several full read/hash/write cycles. Preserve
    // that workload while allowing measured slow but progressing channel IO;
    // the transport retains independent idle and absolute write limits.
    let operations_started = Instant::now();
    let until = operations_started + Duration::from_secs(12 * 60);
    let mut last_phase = "unobserved";
    let mut last_report = Instant::now();
    // Create and write are separate RDP requests. An existing empty result is
    // not a completed marker and must not become a spurious "unknown" failure.
    while !std::fs::metadata(folder.path().join("operations.result"))
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        && Instant::now() < until
    {
        let phase = observed_file_stage(folder.path());
        if phase != last_phase {
            println!("RDP test-only operation phase: {phase}");
            last_phase = phase;
        }
        if last_report.elapsed() >= Duration::from_secs(20) {
            if let Ok(diagnostic) = manager.diagnostics(&id) {
                println!(
                    "RDP test-only operation progress: phase={} elapsed_seconds={} transport_stage={} write_attempted={} write_accepted={} write_read_ahead={} last_write_progress_ms={}",
                    phase, operations_started.elapsed().as_secs(), diagnostic.last_stage,
                    diagnostic.last_write_attempted, diagnostic.last_write_accepted,
                    diagnostic.last_write_read_ahead, diagnostic.last_write_last_progress_ms,
                );
            }
            last_report = Instant::now();
        }
        if let Err(code) = settle(&manager, &id, Duration::from_millis(200)).await {
            println!("RDP test-only operations-go progress: opened={} read={} denied={} other_failure={}", operations_go_progress.opened(), operations_go_progress.read(), operations_go_progress.denied(), operations_go_progress.other_failure());
            println!(
                "RDP test-only observed file stage: {}",
                observed_file_stage(folder.path())
            );
            return Err(code);
        }
    }
    if !std::fs::metadata(folder.path().join("operations.result"))
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    {
        println!(
            "RDP test-only observed file stage: {}",
            observed_file_stage(folder.path())
        );
        return Err("operations_completion_timeout");
    }
    if std::fs::metadata(folder.path().join("operations.result"))
        .map_err(|_| "operations_result_missing")?
        .len()
        > 64
    {
        return Err("file_marker_invalid");
    }
    let result = std::fs::read_to_string(folder.path().join("operations.result"))
        .map_err(|_| "operations_result_missing")?;
    if result != "PASS" {
        println!(
            "RDP test-only observed file stage: {}",
            observed_file_stage(folder.path())
        );
        let stage = match result.as_str() {
            "FAIL_enum" => "enum",
            "FAIL_nested" => "nested",
            "FAIL_read" => "read",
            "FAIL_read_hash" => "read_hash",
            "FAIL_write" => "write",
            "FAIL_write_hash" => "write_hash",
            "FAIL_overwrite" => "overwrite",
            "FAIL_rename" => "rename",
            "FAIL_delete" => "delete",
            _ => "unknown",
        };
        println!("RDP test-only file operation failure: stage={stage}");
        return Err("remote_file_operations_failed");
    }
    let returned = std::fs::read(folder.path().join("переименован.bin"))
        .map_err(|_| "binary_result_missing")?;
    if returned != binary || Sha256::digest(&returned) != Sha256::digest(&binary) {
        return Err("binary_roundtrip_mismatch");
    }
    if folder.path().join("delete.bin").exists() {
        return Err("delete_not_applied");
    }
    println!("RDP channel phase: binary_1mib_sha256_nested_enum_overwrite_rename_delete_pass");
    if core_only {
        manager.shutdown();
        if stable_writable {
            println!("RDP stable-writable folder acceptance: PASS (writable, 1MiB SHA256, nested Unicode, overwrite, rename, delete; readonly/mode change/lifecycle separate)");
        } else {
            println!("RDP core folder acceptance: PASS (readonly, writable, 1MiB SHA256, nested Unicode, overwrite, rename, delete; lifecycle separate)");
        }
        return Ok(());
    }
    manager
        .send_input(
            &id,
            vec![Input::Resize {
                width: 1920,
                height: 1080,
            }],
        )
        .map_err(|e| e.code())?;
    settle(&manager, &id, Duration::from_secs(5)).await?;
    command_file(&manager,&id,folder.path(),r"$root='\\tsclient\ConsoleCrypt\';if(Test-Path -LiteralPath ($root+'binary.bin')){$v='PASS'}else{$v='FAIL'};[IO.File]::WriteAllText($root+'resize.result',$v)").await?;
    wait_marker(&manager, &id, folder.path(), "resize.result").await?;
    println!("RDP channel phase: resize_preserves_drive_pass");
    let replacement = tempfile::tempdir().map_err(|_| "folder_fixture")?;
    std::fs::write(replacement.path().join("пример.txt"), &sentinel)
        .map_err(|_| "folder_fixture")?;
    let replacement_grant = manager
        .register_directory(replacement.path())
        .map_err(|e| e.code())?;
    permissions.directory_grant_id = Some(replacement_grant.id);
    permissions.directory_writable = false;
    manager
        .set_permissions(&id, permissions.clone())
        .map_err(|e| e.code())?;
    settle(&manager, &id, Duration::from_secs(3)).await?;
    let replacement_marker = format!("cc-rdp-replacement-{}.result", uuid::Uuid::new_v4());
    let revocation_marker = format!("cc-rdp-revocation-{}.result", uuid::Uuid::new_v4());
    let revocation_script = format!("cc-rdp-revocation-{}.ps1", uuid::Uuid::new_v4());
    command_file(&manager,&id,replacement.path(),&format!("$p='\\\\tsclient\\ConsoleCrypt\\';$ok=([IO.File]::ReadAllText($p+'пример.txt') -ceq '{sentinel}') -and -not [IO.File]::Exists($p+'roundtrip.txt');if($ok){{$v='PASS'}}else{{$v='FAIL'}};[IO.File]::WriteAllText((Join-Path $env:TEMP '{replacement_marker}'),$v)")).await?;
    // Prepare the revocation probe in our own TEMP while access still exists;
    // it must run without loading any script from the revoked directory.
    let script = format!("$ok=$false;try{{[IO.File]::ReadAllText('\\\\tsclient\\ConsoleCrypt\\пример.txt')|Out-Null}}catch{{$ok=$true}};if($ok){{$v='PASS'}}else{{$v='FAIL'}};[IO.File]::WriteAllText((Join-Path $env:TEMP '{revocation_marker}'),$v)");
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(replacement.path().join(&revocation_script), bytes)
        .map_err(|_| "folder_fixture")?;
    permissions.directory_writable = true;
    manager
        .set_permissions(&id, permissions.clone())
        .map_err(|e| e.code())?;
    settle(&manager, &id, Duration::from_secs(3)).await?;
    command_file(&manager,&id,replacement.path(),&format!("$ErrorActionPreference='Stop';$p='\\\\tsclient\\ConsoleCrypt\\';$m=Join-Path $env:TEMP '{replacement_marker}';try{{[IO.File]::WriteAllText($p+'replacement.result',[IO.File]::ReadAllText($m));Copy-Item -LiteralPath ($p+'{revocation_script}') -Destination (Join-Path $env:TEMP '{revocation_script}')}}finally{{Remove-Item -LiteralPath $m -ErrorAction SilentlyContinue}}")).await?;
    wait_marker(&manager, &id, replacement.path(), "replacement.result").await?;
    println!("RDP channel phase: replacement_and_cyrillic_filename_pass");
    permissions.directory_grant_id = None;
    manager
        .set_permissions(&id, permissions.clone())
        .map_err(|e| e.code())?;
    settle(&manager, &id, Duration::from_secs(3)).await?;
    command_remote_file(&manager, &id, &revocation_script).await?;
    // A fresh explicit grant retrieves only the earlier fixed result. A denied
    // read cannot be inferred merely from removing the client-side permission.
    let result_grant = manager
        .register_directory(folder.path())
        .map_err(|e| e.code())?;
    permissions.directory_grant_id = Some(result_grant.id);
    permissions.directory_writable = true;
    manager
        .set_permissions(&id, permissions)
        .map_err(|e| e.code())?;
    wait_folder(&manager, &id).await?;
    command_file(&manager,&id,folder.path(),&format!("$p='\\\\tsclient\\ConsoleCrypt\\';$m=Join-Path $env:TEMP '{revocation_marker}';try{{[IO.File]::WriteAllText($p+'revocation.result',[IO.File]::ReadAllText($m))}}finally{{Remove-Item -LiteralPath $m -ErrorAction SilentlyContinue;Remove-Item -LiteralPath (Join-Path $env:TEMP '{revocation_script}') -ErrorAction SilentlyContinue}}")).await?;
    wait_marker(&manager, &id, folder.path(), "revocation.result").await?;
    println!("RDP channel phase: remote_folder_revocation_pass");

    manager
        .set_permissions(&id, SessionPermissions::default())
        .map_err(|e| e.code())?;
    if manager.request_clipboard_text(&id) != Err(crate::RdpError::PermissionDenied) {
        return Err("revocation_failed");
    }
    manager.shutdown();
    println!("RDP channel acceptance: PASS");
    Ok(())
}
