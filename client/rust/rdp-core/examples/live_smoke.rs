//! Opt-in local acceptance. No credentials in arguments, source, Debug or output.
use cc_rdp_core::{ConnectConfig, Input, RdpManager, SessionStatus};
use secrecy::SecretString;
use serde::Deserialize;
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
    output_dir: PathBuf,
}
impl Drop for PrivateConfig {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[tokio::main]
async fn main() {
    if let Err(code) = run().await {
        eprintln!("RDP acceptance failed: {code}");
        std::process::exit(1);
    }
}

fn fingerprint(text: &str) -> Result<[u8; 32], &'static str> {
    let cleaned = text.replace(':', "");
    if cleaned.len() != 64 || !cleaned.is_ascii() {
        return Err("invalid_fingerprint");
    }
    let mut out = [0; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&cleaned[2 * i..2 * i + 2], 16)
            .map_err(|_| "invalid_fingerprint")?;
    }
    Ok(out)
}

async fn run() -> Result<(), &'static str> {
    let path =
        PathBuf::from(std::env::var_os("CC_RDP_TEST_CONFIG").ok_or("configuration_missing")?);
    let metadata = std::fs::metadata(&path).map_err(|_| "configuration_unavailable")?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 {
        return Err("configuration_invalid");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("configuration_permissions");
        }
    }
    let bytes = Zeroizing::new(std::fs::read(path).map_err(|_| "configuration_unavailable")?);
    let mut private: PrivateConfig =
        serde_json::from_slice(&bytes).map_err(|_| "configuration_invalid")?;
    drop(bytes);
    let manager = RdpManager::new();
    if std::env::var_os("CC_RDP_TEST_PROBE_ONLY").is_some() {
        let cert = manager
            .probe_certificate(&private.address, private.port)
            .await
            .map_err(|e| e.code())?;
        println!("Target certificate SHA-256: {}", cert.fingerprint);
        manager.shutdown();
        return Ok(());
    }
    let settings = ConnectConfig {
        address: private.address.clone(),
        port: private.port,
        username: private.username.clone(),
        domain: private.domain.clone(),
        width: private.width,
        height: private.height,
        accepted_certificate_sha256: fingerprint(&private.certificate_sha256)?,
    };
    let password = SecretString::from(std::mem::take(&mut private.password));
    let id = manager.connect(settings, password).map_err(|e| e.code())?;
    let start = Instant::now();
    let mut last_frame = None;
    while start.elapsed() < Duration::from_secs(180) && last_frame.is_none() {
        collect(&manager, &id, Duration::from_millis(100), &mut last_frame).await?;
    }
    if last_frame.is_none() {
        return Err("frame_timeout");
    }
    collect(&manager, &id, Duration::from_secs(3), &mut last_frame).await?;
    snapshot(
        &manager,
        &id,
        last_frame.as_ref().unwrap(),
        &private.output_dir,
        "before",
    )?;
    if std::env::var_os("CC_RDP_TEST_NOTEPAD").is_some() {
        manager
            .send_input(
                &id,
                vec![Input::Scancode {
                    code: 0x5b,
                    down: true,
                    extended: true,
                }],
            )
            .map_err(|e| e.code())?;
        tokio::time::sleep(Duration::from_millis(120)).await;
        manager
            .send_input(
                &id,
                vec![Input::Scancode {
                    code: 0x13,
                    down: true,
                    extended: false,
                }],
            )
            .map_err(|e| e.code())?;
        tokio::time::sleep(Duration::from_millis(120)).await;
        manager
            .send_input(&id, vec![Input::ReleaseAll])
            .map_err(|e| e.code())?;
        collect(&manager, &id, Duration::from_secs(3), &mut last_frame).await?;
        snapshot(
            &manager,
            &id,
            last_frame.as_ref().unwrap(),
            &private.output_dir,
            "after-win-r",
        )?;
        manager
            .send_input(&id, vec![Input::UnicodeText("notepad.exe".into())])
            .map_err(|e| e.code())?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        manager
            .send_input(
                &id,
                vec![Input::Scancode {
                    code: 0x1c,
                    down: true,
                    extended: false,
                }],
            )
            .map_err(|e| e.code())?;
        tokio::time::sleep(Duration::from_millis(120)).await;
        manager
            .send_input(&id, vec![Input::ReleaseAll])
            .map_err(|e| e.code())?;
        collect(&manager, &id, Duration::from_secs(4), &mut last_frame).await?;
        snapshot(
            &manager,
            &id,
            last_frame.as_ref().unwrap(),
            &private.output_dir,
            "after-open",
        )?;
        manager
            .send_input(&id, vec![Input::UnicodeText("Hello / Привет".into())])
            .map_err(|e| e.code())?;
        collect(&manager, &id, Duration::from_secs(3), &mut last_frame).await?;
        snapshot(
            &manager,
            &id,
            last_frame.as_ref().unwrap(),
            &private.output_dir,
            "after-text",
        )?;
    }
    if std::env::var_os("CC_RDP_TEST_RESIZE").is_some() {
        manager
            .send_input(
                &id,
                vec![Input::Resize {
                    width: 1920,
                    height: 1080,
                }],
            )
            .map_err(|e| e.code())?;
        collect(&manager, &id, Duration::from_secs(6), &mut last_frame).await?;
        snapshot(
            &manager,
            &id,
            last_frame.as_ref().ok_or("frame_timeout")?,
            &private.output_dir,
            "after-resize",
        )?;
        if last_frame
            .as_ref()
            .is_none_or(|frame| frame.width != 1920 || frame.height != 1080)
        {
            return Err("resize_not_applied");
        }
    }
    let mut frame = last_frame.ok_or("frame_timeout")?;
    save_frame(&frame, &private.output_dir, "frame")?;
    frame.rgba.zeroize();
    manager.shutdown();
    println!(
        "RDP acceptance: connected; decoded {}x{} RGBA frame",
        frame.width, frame.height
    );
    Ok(())
}

async fn collect(
    manager: &RdpManager,
    id: &str,
    duration: Duration,
    last: &mut Option<cc_rdp_core::Frame>,
) -> Result<(), &'static str> {
    let start = Instant::now();
    while start.elapsed() < duration {
        let poll = manager.poll(id).map_err(|e| e.code())?;
        match poll.status {
            SessionStatus::Failed(e) => {
                if let Ok(diagnostics) = manager.diagnostics(id) {
                    eprintln!(
                        "RDP transport stage: {}; reactivation {} started / {} completed; decoder kind={} site={} subkind={}",
                        diagnostics.last_stage,
                        diagnostics.reactivations_started,
                        diagnostics.reactivations_completed,
                        diagnostics.decode_kind,
                        diagnostics.decode_site,
                        diagnostics.decode_subkind
                    );
                }
                return Err(e.code());
            }
            SessionStatus::Disconnected => return Err("disconnected"),
            _ => {}
        }
        if let Some(frame) = poll.frame {
            if let Some(old) = last {
                old.rgba.zeroize();
            }
            *last = Some(frame);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}
fn save_frame(
    frame: &cc_rdp_core::Frame,
    dir: &std::path::Path,
    label: &str,
) -> Result<(), &'static str> {
    std::fs::create_dir_all(dir).map_err(|_| "output_unavailable")?;
    std::fs::write(dir.join(format!("{label}.rgba")), &frame.rgba)
        .map_err(|_| "output_unavailable")?;
    std::fs::write(
        dir.join(format!("{label}.json")),
        format!(
            "{{\"width\":{},\"height\":{},\"sequence\":{},\"format\":\"RGBA8\"}}\n",
            frame.width, frame.height, frame.sequence
        ),
    )
    .map_err(|_| "output_unavailable")?;
    Ok(())
}
fn snapshot(
    manager: &RdpManager,
    id: &str,
    frame: &cc_rdp_core::Frame,
    dir: &std::path::Path,
    label: &str,
) -> Result<(), &'static str> {
    save_frame(frame, dir, label)?;
    let diagnostics = manager.diagnostics(id).map_err(|e| e.code())?;
    println!(
        "RDP phase {label}: frame {}; sent {} input batches / {} events / {} PDUs",
        frame.sequence,
        diagnostics.input_batches,
        diagnostics.input_events,
        diagnostics.input_pdus_written
    );
    Ok(())
}
