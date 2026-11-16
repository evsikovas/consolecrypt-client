//! Offline release-manifest-only signer. Never linked into the Agent launcher.
//! No node/owner statement selection, raw-message endpoint or key generation.
#![forbid(unsafe_code)]
use cc_agent_identity_policy::{
    PublicKey, ReleaseManifest, ReleaseSignerPin, SignatureBytes, Signed, Statement,
};
use ed25519_dalek::{Signer, SigningKey};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use zeroize::Zeroizing;

const SEED_ENV: &str = "CC_AGENT_RELEASE_ED25519_SEED_HEX";
type Result<T> = std::result::Result<T, &'static str>;

fn absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::CurDir | Component::ParentDir))
    {
        return Err("absolute_plain_path_required");
    }
    for parent in path.ancestors() {
        if fs::symlink_metadata(parent)
            .map_err(|_| "path_unavailable")?
            .file_type()
            .is_symlink()
        {
            return Err("path_link_refused");
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_input(path: &Path, maximum: usize, secret: bool) -> Result<Zeroizing<Vec<u8>>> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    absolute(path)?;
    // Linux-only fixed O_NOFOLLOW|O_NONBLOCK, including FIFO-before-fstat denial.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(0x20000 | 0x800)
        .open(path)
        .map_err(|_| "input_open_failed")?;
    let before = file.metadata().map_err(|_| "input_metadata_failed")?;
    if !before.is_file() || before.nlink() != 1 || before.len() > maximum as u64 {
        return Err("input_type_or_size_refused");
    }
    if secret {
        let uid = fs::metadata("/proc/self")
            .map_err(|_| "operator_uid_unavailable")?
            .uid();
        let parent = fs::metadata(path.parent().ok_or("seed_parent_required")?)
            .map_err(|_| "seed_parent_unavailable")?;
        if before.uid() != uid
            || before.mode() & 0o777 != 0o600
            || before.len() != 32
            || parent.uid() != uid
            || parent.mode() & 0o777 != 0o700
        {
            return Err("private_operator_seed_0600_parent_0700_required");
        }
    }
    let mut raw = Zeroizing::new(Vec::with_capacity(maximum.min(16384)));
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "input_read_failed")?;
    let after = file.metadata().map_err(|_| "input_metadata_failed")?;
    let named = fs::symlink_metadata(path).map_err(|_| "input_name_changed")?;
    let stamp = |m: &fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    if raw.len() != before.len() as usize
        || raw.len() > maximum
        || stamp(&before) != stamp(&after)
        || stamp(&before) != stamp(&named)
    {
        return Err("input_changed");
    }
    Ok(raw)
}
#[cfg(not(target_os = "linux"))]
fn read_input(_: &Path, _: usize, _: bool) -> Result<Zeroizing<Vec<u8>>> {
    Err("linux_release_tool_required")
}

fn env_seed(raw: Option<Zeroizing<String>>) -> Result<Zeroizing<[u8; 32]>> {
    let raw = raw.ok_or("dedicated_agent_release_secret_required")?;
    if raw.len() != 64
        || !raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("dedicated_agent_release_secret_invalid");
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    hex::decode_to_slice(raw.as_str(), seed.as_mut())
        .map_err(|_| "dedicated_agent_release_secret_invalid")?;
    Ok(seed)
}

fn sign_typed(
    raw: &[u8],
    seed: Zeroizing<[u8; 32]>,
    expected: &str,
    now: u64,
) -> Result<(Signed<ReleaseManifest>, ReleaseSignerPin)> {
    if raw.is_empty() || raw.len() > 16 * 1024 || seed.iter().all(|v| *v == 0) {
        return Err("release_input_invalid");
    }
    let body: ReleaseManifest =
        serde_json::from_slice(raw).map_err(|_| "release_manifest_schema_invalid")?;
    body.validate().map_err(|_| "release_manifest_invalid")?;
    if body.protocol != 1 || body.issued_at.seconds() > now || body.expires_at.seconds() <= now {
        return Err("release_manifest_not_live");
    }
    let key = SigningKey::from_bytes(&seed);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    public
        .validate()
        .map_err(|_| "release_public_key_invalid")?;
    // Explicit authenticated PUBLIC pin prevents silently selecting a different
    // CI key. It is never derived from an incoming release envelope.
    if expected.len() != 64 || hex::encode(public.as_bytes()) != expected {
        return Err("dedicated_release_key_pin_mismatch");
    }
    let signature = SignatureBytes::from_bytes(
        key.sign(
            &body
                .signing_bytes()
                .map_err(|_| "release_manifest_invalid")?,
        )
        .to_bytes(),
    );
    Ok((
        Signed { body, signature },
        ReleaseSignerPin {
            format: 1,
            public_key: public,
        },
    ))
}

#[cfg(target_os = "linux")]
fn outputs(
    directory: &Path,
    signed: &Signed<ReleaseManifest>,
    pin: &ReleaseSignerPin,
) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    absolute(directory.parent().ok_or("output_parent_required")?)?;
    let parent =
        fs::metadata(directory.parent().unwrap()).map_err(|_| "output_parent_unavailable")?;
    let uid = fs::metadata("/proc/self")
        .map_err(|_| "operator_uid_unavailable")?
        .uid();
    if parent.uid() != uid || parent.mode() & 0o022 != 0 {
        return Err("operator_owned_output_parent_required");
    }
    // Exclusive new directory; incomplete public output is never overwritten.
    fs::DirBuilder::new()
        .mode(0o700)
        .create(directory)
        .map_err(|_| "new_output_directory_required")?;
    for (name, raw) in [
        ("release-manifest.json", serde_json::to_vec(signed)),
        ("public-key.json", serde_json::to_vec(pin)),
    ] {
        let mut raw = raw.map_err(|_| "public_serialization_failed")?;
        raw.push(b'\n');
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join(name))
            .map_err(|_| "public_output_failed")?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| "public_output_failed")?;
    }
    File::open(directory)
        .and_then(|f| f.sync_all())
        .map_err(|_| "output_directory_sync_failed")?;
    File::open(directory.parent().unwrap())
        .and_then(|f| f.sync_all())
        .map_err(|_| "output_parent_sync_failed")
}
#[cfg(not(target_os = "linux"))]
fn outputs(_: &Path, _: &Signed<ReleaseManifest>, _: &ReleaseSignerPin) -> Result<()> {
    Err("linux_release_tool_required")
}

fn run(args: &[String]) -> Result<()> {
    if args.len() != 9
        || args[1] != "--manifest"
        || args[3] != "--output-dir"
        || args[5] != "--expected-public-key"
        || !matches!(args[7].as_str(), "--seed-file" | "--seed-env")
    {
        return Err("fixed_release_signer_arguments_required");
    }
    let raw = read_input(&PathBuf::from(&args[2]), 16 * 1024, false)?;
    let seed = if args[7] == "--seed-file" {
        let bytes = read_input(&PathBuf::from(&args[8]), 32, true)?;
        let mut seed = Zeroizing::new([0u8; 32]);
        seed.copy_from_slice(&bytes);
        seed
    } else {
        if args[8] != SEED_ENV {
            return Err("fixed_dedicated_seed_environment_required");
        }
        let raw = std::env::var(SEED_ENV).ok().map(Zeroizing::new);
        std::env::remove_var(SEED_ENV);
        env_seed(raw)?
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "clock_unavailable")?
        .as_secs();
    let (signed, pin) = sign_typed(&raw, seed, &args[6], now)?;
    outputs(&PathBuf::from(&args[4]), &signed, &pin)?;
    println!(
        "{}",
        serde_json::json!({"format":"cc-agent-release-signed/v1","publicKeyFingerprint":hex::encode(pin.public_key.fingerprint().as_bytes()),"signatureDomain":ReleaseManifest::DOMAIN})
    );
    Ok(())
}

fn main() {
    if let Err(code) = run(&std::env::args().collect::<Vec<_>>()) {
        eprintln!("{code}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_agent_identity_policy::{verify_release, Now, Timestamp};
    use sha2::{Digest, Sha256};
    fn body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"format":1,"version":2,"displayVersion":"0.1.0","protocol":1,
            "capabilities":{"version":1,"scopes":["telemetry.read","metrics.collect","exporters.manage","tunnel.open","jobs.execute","agent.update"]},
            "platform":"linux-x86_64","artifactSha256":hex::encode(Sha256::digest(b"owned public artifact")),"artifactBytes":21,"issuedAt":990,"expiresAt":1100})).unwrap()
    }
    fn expected() -> String {
        hex::encode(SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes())
    }
    #[test]
    fn actual_release_signature_verifies_and_other_public_key_fails() {
        let (signed, pin) =
            sign_typed(&body(), Zeroizing::new([7; 32]), &expected(), 1000).unwrap();
        use ed25519_dalek::Verifier;
        assert!(SigningKey::from_bytes(&[7; 32])
            .verifying_key()
            .verify_strict(
                &signed.body.signing_bytes().unwrap(),
                &ed25519_dalek::Signature::from_bytes(signed.signature.as_bytes())
            )
            .is_ok());
        assert!(SigningKey::from_bytes(&[8; 32])
            .verifying_key()
            .verify(
                &signed.body.signing_bytes().unwrap(),
                &ed25519_dalek::Signature::from_bytes(signed.signature.as_bytes())
            )
            .is_err());
        assert_eq!(
            pin.public_key.as_bytes(),
            SigningKey::from_bytes(&[7; 32]).verifying_key().as_bytes()
        );
    }
    #[test]
    fn generic_node_signed_envelope_and_unknown_field_cannot_be_signed() {
        for raw in [
            br#"{"body":{},"signature":"00"}"#.to_vec(),
            br#"{"operation":"node.request"}"#.to_vec(),
        ] {
            assert!(sign_typed(&raw, Zeroizing::new([7; 32]), &expected(), 1000).is_err());
        }
        let mut v: serde_json::Value = serde_json::from_slice(&body()).unwrap();
        v["rawMessage"] = "payload".into();
        assert!(sign_typed(
            &serde_json::to_vec(&v).unwrap(),
            Zeroizing::new([7; 32]),
            &expected(),
            1000
        )
        .is_err());
    }
    #[test]
    fn exact_expected_dedicated_pin_and_live_window_are_mandatory() {
        assert!(sign_typed(&body(), Zeroizing::new([7; 32]), &"0".repeat(64), 1000).is_err());
        assert!(sign_typed(&body(), Zeroizing::new([7; 32]), &expected(), 989).is_err());
        assert!(sign_typed(&body(), Zeroizing::new([7; 32]), &expected(), 1100).is_err());
    }
    #[test]
    fn missing_or_other_seed_environment_is_never_a_fallback() {
        assert!(env_seed(None).is_err());
        assert!(env_seed(Some(Zeroizing::new("RuStore key material".into()))).is_err());
        assert!(env_seed(Some(Zeroizing::new("07".repeat(32)))).is_ok());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn private_file_permissions_links_and_existing_output_are_fail_closed() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!(
            "cc-release-sign-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&base).unwrap();
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
        let seed = base.join("synthetic-seed");
        fs::write(&seed, [7; 32]).unwrap();
        fs::set_permissions(&seed, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_input(&seed, 32, true).is_ok());
        fs::set_permissions(&seed, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_input(&seed, 32, true).is_err());
        fs::set_permissions(&seed, fs::Permissions::from_mode(0o600)).unwrap();
        let link = base.join("linked");
        std::os::unix::fs::symlink(&seed, &link).unwrap();
        assert!(read_input(&link, 32, true).is_err());
        let artifact = base.join("artifact");
        fs::write(&artifact, b"owned public artifact").unwrap();
        let (signed, pin) =
            sign_typed(&body(), Zeroizing::new([7; 32]), &expected(), 1000).unwrap();
        verify_release(
            signed.clone(),
            &pin,
            &artifact,
            "linux-x86_64",
            1,
            1,
            Now(Timestamp::new(1000).unwrap()),
        )
        .unwrap();
        let out = base.join("public");
        outputs(&out, &signed, &pin).unwrap();
        assert!(outputs(&out, &signed, &pin).is_err());
        assert!(
            base.starts_with(std::env::temp_dir())
                && base
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("cc-release-sign-test-")
        );
        fs::remove_dir_all(base).unwrap();
    }
}
