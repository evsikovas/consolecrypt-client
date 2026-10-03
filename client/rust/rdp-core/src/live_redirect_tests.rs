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
            frame.rgba.zeroize();
        }
        match poll.status {
            SessionStatus::Failed(e) => return Err(e.code()),
            SessionStatus::Disconnected => return Err("disconnected"),
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
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
async fn prepare_run(manager: &RdpManager, id: &str) -> Result<(), &'static str> {
    manager
        .send_input(
            id,
            vec![
                Input::ReleaseAll,
                Input::Scancode {
                    code: 0x01,
                    down: true,
                    extended: false,
                },
                Input::Scancode {
                    code: 0x01,
                    down: false,
                    extended: false,
                },
            ],
        )
        .map_err(|e| e.code())?;
    settle(manager, id, Duration::from_millis(250)).await?;
    // Clear both a previous Run error modal and its underlying Run dialog,
    // then return to a neutral desktop before opening the next short command.
    manager
        .send_input(
            id,
            vec![
                Input::Scancode {
                    code: 0x01,
                    down: true,
                    extended: false,
                },
                Input::Scancode {
                    code: 0x01,
                    down: false,
                    extended: false,
                },
            ],
        )
        .map_err(|e| e.code())?;
    settle(manager, id, Duration::from_millis(250)).await?;
    manager
        .send_input(
            id,
            vec![
                Input::Scancode {
                    code: 0x5b,
                    down: true,
                    extended: true,
                },
                Input::Scancode {
                    code: 0x20,
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
    settle(manager, id, Duration::from_millis(300)).await?;
    manager
        .send_input(
            id,
            vec![Input::Scancode {
                code: 0x5b,
                down: true,
                extended: true,
            }],
        )
        .map_err(|e| e.code())?;
    settle(manager, id, Duration::from_millis(150)).await?;
    manager
        .send_input(
            id,
            vec![Input::Scancode {
                code: 0x13,
                down: true,
                extended: false,
            }],
        )
        .map_err(|e| e.code())?;
    settle(manager, id, Duration::from_millis(150)).await?;
    manager
        .send_input(id, vec![Input::ReleaseAll])
        .map_err(|e| e.code())?;
    settle(manager, id, Duration::from_millis(700)).await?;
    // Clear any prior Run dialog content without reading the remote system clipboard.
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
                    code: 0x1e,
                    down: true,
                    extended: false,
                },
            ],
        )
        .map_err(|e| e.code())?;
    settle(manager, id, Duration::from_millis(100)).await?;
    manager
        .send_input(id, vec![Input::ReleaseAll])
        .map_err(|e| e.code())?;
    settle(manager, id, Duration::from_millis(250)).await?;
    Ok(())
}
async fn command_file(
    manager: &RdpManager,
    id: &str,
    folder: &std::path::Path,
    script: &str,
) -> Result<(), &'static str> {
    let name = format!("cc-{}.ps1", uuid::Uuid::new_v4().simple());
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(folder.join(&name), bytes).map_err(|_| "folder_fixture")?;
    prepare_run(manager, id).await?;
    let text = format!(
        r"powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File \\tsclient\ConsoleCrypt\{name}"
    );
    type_ascii_scancodes(manager, id, &text).await?;
    snapshot("file-run-before-enter")?;
    enter(manager, id)?;
    settle(manager, id, Duration::from_secs(5)).await?;
    snapshot("file-command-after-enter")
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
    if let Err(code) = run(false).await {
        panic!("RDP channel acceptance failed: {code}");
    }
}
#[tokio::test]
#[ignore = "requires owner-authorized Windows config and synthetic selected folder only"]
async fn live_selected_folder_real_windows_operations() {
    if let Err(code) = run(true).await {
        panic!("RDP folder acceptance failed: {code}");
    }
}
async fn run(files_only: bool) -> Result<(), &'static str> {
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
    let binary_hash = format!("{:X}", Sha256::digest(&binary));
    let grant = manager
        .register_directory(folder.path())
        .map_err(|e| e.code())?;
    let mut permissions = SessionPermissions {
        clipboard_enabled: true,
        directory_grant_id: None,
        directory_writable: false,
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
    permissions.directory_grant_id = Some(grant.id);
    manager
        .set_permissions(&id, permissions.clone())
        .map_err(|e| e.code())?;
    wait_folder(&manager, &id).await?;

    command_file(&manager,&id,folder.path(),&format!("$p='\\\\tsclient\\ConsoleCrypt\\sentinel.txt';$ok=[IO.File]::ReadAllText($p) -ceq '{sentinel}';try{{[IO.File]::WriteAllText($p,'forbidden');$ok=$false}}catch{{}};if($ok){{Set-Clipboard 'CC-RDP-READONLY-PASS'}}else{{Set-Clipboard 'CC-RDP-READONLY-FAIL'}}")).await?;
    if receive(&manager, &id).await?.trim_end() != "CC-RDP-READONLY-PASS" {
        return Err("readonly_remote_mismatch");
    }
    if std::fs::read_to_string(folder.path().join("sentinel.txt")).map_err(|_| "folder_fixture")?
        != sentinel
    {
        return Err("readonly_local_changed");
    }
    println!("RDP channel phase: readonly_read_and_write_denial_pass");
    permissions.directory_writable = true;
    manager
        .set_permissions(&id, permissions.clone())
        .map_err(|e| e.code())?;
    settle(&manager, &id, Duration::from_secs(4)).await?;
    let written = format!("CC-RDP-WRITE-{} / Привет", uuid::Uuid::new_v4());
    command_file(&manager,&id,folder.path(),&format!("$p='\\\\tsclient\\ConsoleCrypt\\roundtrip.txt';[IO.File]::WriteAllText($p,'{written}');if([IO.File]::ReadAllText($p) -ceq '{written}'){{Set-Clipboard 'CC-RDP-WRITE-PASS'}}else{{Set-Clipboard 'CC-RDP-WRITE-FAIL'}}")).await?;
    if receive(&manager, &id).await?.trim_end() != "CC-RDP-WRITE-PASS" {
        return Err("writable_remote_mismatch");
    }
    if std::fs::read_to_string(folder.path().join("roundtrip.txt"))
        .map_err(|_| "writable_local_missing")?
        != written
    {
        return Err("writable_local_mismatch");
    }
    println!("RDP channel phase: writable_roundtrip_pass");
    let remote_temp = format!("cc-rdp-{}.bin", uuid::Uuid::new_v4());
    let script = format!(
        r#"$ErrorActionPreference='Stop';$root='\\tsclient\ConsoleCrypt\';$t=Join-Path $env:TEMP '{remote_temp}';try{{
        $names=@(Get-ChildItem -LiteralPath $root|Select-Object -ExpandProperty Name);
        if(-not($names -contains 'binary.bin') -or -not($names -contains 'папка')){{throw 'enum'}};
        if((Get-ChildItem -LiteralPath ($root+'папка')|Select-Object -ExpandProperty Name) -cne 'пример.txt'){{throw 'nested'}};
        Copy-Item -LiteralPath ($root+'binary.bin') -Destination $t;
        if((Get-FileHash -LiteralPath $t -Algorithm SHA256).Hash -cne '{binary_hash}'){{throw 'read'}};
        Copy-Item -LiteralPath $t -Destination ($root+'returned.bin');
        if((Get-FileHash -LiteralPath ($root+'returned.bin') -Algorithm SHA256).Hash -cne '{binary_hash}'){{throw 'write'}};
        [IO.File]::WriteAllText($root+'returned.bin','old');Copy-Item -LiteralPath $t -Destination ($root+'returned.bin') -Force;
        Rename-Item -LiteralPath ($root+'returned.bin') -NewName 'переименован.bin';
        if((Get-FileHash -LiteralPath ($root+'переименован.bin') -Algorithm SHA256).Hash -cne '{binary_hash}'){{throw 'overwrite'}};
        Copy-Item -LiteralPath ($root+'переименован.bin') -Destination ($root+'delete.bin');
        Remove-Item -LiteralPath ($root+'delete.bin');
        if(Test-Path -LiteralPath ($root+'delete.bin')){{throw 'delete'}};
        [IO.File]::WriteAllText($root+'operations.result','PASS');
    }}catch{{[IO.File]::WriteAllText($root+'operations.result','FAIL')}}finally{{Remove-Item -LiteralPath $t -ErrorAction SilentlyContinue}}"#
    );
    command_file(&manager, &id, folder.path(), &script).await?;
    let until = Instant::now() + Duration::from_secs(30);
    while !folder.path().join("operations.result").exists() && Instant::now() < until {
        settle(&manager, &id, Duration::from_millis(200)).await?;
    }
    if std::fs::read_to_string(folder.path().join("operations.result"))
        .map_err(|_| "operations_result_missing")?
        != "PASS"
    {
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
    command(&manager,&id,r"if(Test-Path -LiteralPath '\\tsclient\ConsoleCrypt\binary.bin'){Set-Clipboard 'CC-RDP-RESIZE-PASS'}else{Set-Clipboard 'CC-RDP-RESIZE-FAIL'}").await?;
    if receive(&manager, &id).await?.trim_end() != "CC-RDP-RESIZE-PASS" {
        return Err("resize_sidechannel_failed");
    }
    println!("RDP channel phase: resize_preserves_drive_and_clipboard_pass");
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
    command_file(&manager,&id,replacement.path(),&format!("$p='\\\\tsclient\\ConsoleCrypt\\';$ok=([IO.File]::ReadAllText($p+'пример.txt') -ceq '{sentinel}') -and -not [IO.File]::Exists($p+'roundtrip.txt');if($ok){{Set-Clipboard 'CC-RDP-REPLACE-PASS'}}else{{Set-Clipboard 'CC-RDP-REPLACE-FAIL'}}")).await?;
    if receive(&manager, &id).await?.trim_end() != "CC-RDP-REPLACE-PASS" {
        return Err("folder_replacement_mismatch");
    }
    println!("RDP channel phase: replacement_and_cyrillic_filename_pass");
    permissions.directory_grant_id = None;
    manager
        .set_permissions(&id, permissions)
        .map_err(|e| e.code())?;
    settle(&manager, &id, Duration::from_secs(3)).await?;
    command(&manager,&id,"$ok=$false;try{[IO.File]::ReadAllText('\\\\tsclient\\ConsoleCrypt\\пример.txt')|Out-Null}catch{$ok=$true};if($ok){Set-Clipboard 'CC-RDP-REVOKE-PASS'}else{Set-Clipboard 'CC-RDP-REVOKE-FAIL'}").await?;
    if receive(&manager, &id).await?.trim_end() != "CC-RDP-REVOKE-PASS" {
        return Err("folder_revocation_mismatch");
    }
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
