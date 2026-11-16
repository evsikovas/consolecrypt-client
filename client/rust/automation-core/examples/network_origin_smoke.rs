use cc_automation_core::HelperLaunch;
use serde::Deserialize;
use serde_json::json;
use std::{
    io::{self, BufRead},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    launch: HelperLaunch,
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let fixture: Fixture = serde_json::from_str(&line)?;
    println!("phase: spawn-helper");
    let helper = fixture.launch.spawn().await?;
    let probe = helper
        .peer
        .request("probe", json!({}), Duration::from_secs(30))
        .await?;
    assert_eq!(probe["non_root"], true);
    println!("phase: helper-ready");
    assert_eq!(probe["wsl"], true);
    let mut stream = fixture
        .launch
        .wsl_origin_stream(
            "127.0.0.1",
            fixture.port,
            probe["environment_digest"].as_str().unwrap(),
        )
        .await?;
    let origin = hex::encode(stream.scope_digest());
    println!("phase: bridge-spawned");
    // More than both bridge buffer capacities; interleave to exercise backpressure.
    let payload: Vec<u8> = (0..64 * 1024).map(|index| (index % 256) as u8).collect();
    for _ in 0..17 {
        tokio::time::timeout(Duration::from_secs(10), stream.write_all(&payload)).await??;
        let mut echoed = vec![0u8; payload.len()];
        tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut echoed)).await??;
        assert_eq!(payload, echoed);
    }
    println!("phase: payload-verified");
    stream.shutdown().await?;
    let mut tail = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut tail)).await??;
    assert!(tail.is_empty());
    drop(stream);
    helper.close().await;
    println!(
        "{}",
        json!({"status":"PASS","origin":"explicit WSL user/distro localhost","bytes":17*64*1024,
        "scope_digest":origin,"listener_on_windows":false,"credentials_transferred":false})
    );
    Ok(())
}
