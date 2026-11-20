//! Actual TLS Git -> WSL private helper -> Windows files -> helper upload.
use cc_automation_core::{HelperLaunch, ProjectRoot, UploadedRevision};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{self, BufRead},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    launch: HelperLaunch,
    url: String,
    ca_pem: String,
    commit: String,
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    if line.len() > 128 * 1024 {
        return Err("fixture exceeds limit".into());
    }
    let fixture: Fixture = serde_json::from_str(&line)?;
    let helper = fixture.launch.spawn().await?;
    let result=async {
        let mut captured=helper.peer.request("git_capture",json!({"url":fixture.url,"reference":"main","ca_pem":fixture.ca_pem}),Duration::from_secs(180)).await?;
        let source=captured.as_object_mut().ok_or("invalid source response")?.remove("source").ok_or("missing source metadata")?;
        let capture:UploadedRevision=serde_json::from_value(captured)?;
        let folder=tempfile::tempdir()?;
        let root=ProjectRoot::open(folder.path())?;
        let revision=root.download(&capture,&helper.peer,&CancellationToken::new()).await?;
        assert_eq!(revision.digest,capture.digest);
        let (provenance,stamp)=root.git_source()?.ok_or("missing Git provenance")?;
        assert_eq!(provenance.commit,fixture.commit);
        assert_eq!(provenance.url,fixture.url);
        assert_eq!(provenance.ca_sha256,Some(hex::encode(Sha256::digest(fixture.ca_pem.as_bytes()))));
        assert_eq!(source["metadata_sha256"],stamp);
        assert_eq!(std::fs::read(folder.path().join("binary.dat"))?,(0..1024).flat_map(|_|0..=255u8).collect::<Vec<_>>());
        let uploaded=root.upload(&revision,&helper.peer).await?;
        assert_eq!(uploaded.digest,capture.digest);
        for id in [capture.revision_id,uploaded.revision_id] {
            helper.peer.request("revision_release",json!({"revision_id":id}),Duration::from_secs(5)).await?;
        }
        Ok::<Value,Box<dyn std::error::Error>>(json!({"status":"PASS","tls_git":true,"native_windows_download":true,"independent_sha256":true,"portable_modes":true,"upload_roundtrip":true}))
    }.await;
    helper.close().await;
    println!("{}", result?);
    Ok(())
}
