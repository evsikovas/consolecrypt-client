//! Synthetic signing fixture for isolated package verification tests only.
use cc_agent_identity_policy::*;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
fn write(path: PathBuf, value: &impl serde::Serialize) {
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.len() == 12 && args[0] == "--out" && args[2] == "--artifact" && args[4] == "--now" && args[6] == "--version" && args[8] == "--display-version" && args[10] == "--platform", "--out DIR --artifact ABS --now EPOCH --version SEQUENCE --display-version SEMVER --platform linux-x86_64");
    let out = PathBuf::from(&args[1]);
    let artifact = PathBuf::from(&args[3]);
    assert!(out.is_absolute() && out.is_dir() && artifact.is_absolute());
    let seconds: u64 = args[5].parse().unwrap();
    let key = SigningKey::from_bytes(&[77; 32]);
    let bytes = std::fs::metadata(&artifact).unwrap().len();
    assert!(bytes > 0 && bytes <= 256 * 1024 * 1024);
    let mut file = std::fs::File::open(&artifact).unwrap().take(bytes + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 32768];
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    let body = ReleaseManifest {
        format: 1,
        version: args[7].parse().unwrap(),
        display_version: args[9].clone(),
        protocol: 1,
        capabilities: Capabilities {
            version: 1,
            scopes: Scopes(vec![Scope::TelemetryRead]),
        },
        platform: args[11].clone(),
        artifact_sha256: Hash::from_bytes(digest.finalize().into()),
        artifact_bytes: bytes,
        issued_at: Timestamp::new(seconds.saturating_sub(1)).unwrap(),
        expires_at: Timestamp::new(seconds + 86400).unwrap(),
    };
    let signature = SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes());
    let signed = Signed { body, signature };
    write(
        out.join("release-public-key.json"),
        &ReleaseSignerPin {
            format: 1,
            public_key: PublicKey::from_bytes(key.verifying_key().to_bytes()),
        },
    );
    write(out.join("release-manifest.json"), &signed);
    println!(
        "{}",
        serde_json::json!({"testOnly":true,"publicKey":out.join("release-public-key.json"),"manifest":out.join("release-manifest.json"),"artifact":artifact})
    );
}
