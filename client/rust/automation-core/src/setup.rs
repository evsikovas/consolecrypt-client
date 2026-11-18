//! Explicit setup of a device-local Linux/WSL Runner. Only fixed Native shell
//! preflight; no caller shell interpolation, system package changes, target
//! credentials or project code are involved.
use crate::{AutomationError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

include!(concat!(env!("OUT_DIR"), "/runner_package.rs"));
const SCRIPT: &str = include_str!("../../../../runner/setup/environment.py");
const PYTHON_PREFLIGHT: &str = include_str!("setup/python_preflight.sh");

pub(crate) fn builtin_setup_script() -> Result<String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use std::io::Write;
    // Compile each original module separately: both legitimately have their
    // own future import. Raw concatenation made every R9 setup action fail.
    let modules = [
        include_str!("../../../../runner/setup/environment_cleanup.py"),
        SCRIPT,
    ];
    let raw = serde_json::to_vec(&modules)
        .map_err(|_| AutomationError::Remote("setup_bootstrap_failed".into()))?;
    if raw.len() > 256 * 1024 {
        return Err(AutomationError::Remote("setup_command_too_large".into()));
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder
        .write_all(&raw)
        .map_err(|_| AutomationError::Remote("setup_bootstrap_failed".into()))?;
    let encoded = STANDARD.encode(
        encoder
            .finish()
            .map_err(|_| AutomationError::Remote("setup_bootstrap_failed".into()))?,
    );
    // Only trusted embedded public source is compressed here, never a caller
    // payload. stdin framing/liveness remains untouched for setup and cleanup.
    let script = format!(
        "import sys,json\ntry:\n import base64,zlib\n _cc_codes=[compile(s,'<consolecrypt-setup>','exec',dont_inherit=True) for s in json.loads(zlib.decompress(base64.b64decode('{encoded}')))]\nexcept Exception:\n print(json.dumps({{'ok':False,'error':'setup_bootstrap_failed'}}));sys.exit(70)\nfor _cc_code in _cc_codes:exec(_cc_code,globals())\n"
    );
    if script.len() > 15 * 1024 {
        return Err(AutomationError::Remote("setup_command_too_large".into()));
    }
    Ok(script)
}

fn command_budget(command: &std::process::Command) -> Result<()> {
    // Conservative Windows quoting bound: each UTF-16 code unit can need at
    // most one extra backslash, plus quotes/separator per argument and NUL.
    fn units(value: &std::ffi::OsStr) -> usize {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            value.encode_wide().count()
        }
        #[cfg(not(windows))]
        {
            value.to_string_lossy().encode_utf16().count()
        }
    }
    let units = std::iter::once(command.get_program())
        .chain(command.get_args())
        .try_fold(1usize, |total, value| {
            total.checked_add(units(value).checked_mul(2)?.checked_add(4)?)
        });
    if units.is_none_or(|units| units > 32767) {
        return Err(AutomationError::Remote("setup_command_too_large".into()));
    }
    Ok(())
}

/// Only the built-in, displayed package preparation plan can become a shell
/// program. No project/model/user-provided arbitrary command is accepted.
pub fn prerequisite_script(reviewed: &str) -> Result<String> {
    let packages = reviewed
        .strip_prefix("sudo apt-get update && sudo apt-get install --no-install-recommends ")
        .ok_or(AutomationError::Invalid("invalid prerequisite command"))?;
    let values: Vec<_> = packages.split(' ').collect();
    if values.is_empty()
        || values.len() > 4
        || values
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != values.len()
        || values.iter().any(|package| {
            !["python3-venv", "bubblewrap", "git", "openssh-client"].contains(package)
        })
    {
        return Err(AutomationError::Invalid("invalid prerequisite packages"));
    }
    Ok(format!(
        "/usr/bin/sudo /usr/bin/apt-get update && /usr/bin/sudo /usr/bin/apt-get install --no-install-recommends {packages}; cc_setup_status=$?; printf '\\nRunner prerequisites: exit %s\\n' \"$cc_setup_status\"; exec /bin/sh -i"
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SetupTarget {
    Native,
    Wsl {
        distribution: String,
        user: Option<String>,
    },
    Remote {
        host_id: String,
    },
}

impl SetupTarget {
    pub fn command(&self) -> Result<std::process::Command> {
        let mut command = match self {
            Self::Native if cfg!(any(target_os = "linux", target_os = "macos")) => {
                std::process::Command::new("/bin/sh")
            }
            Self::Wsl { distribution, user } if cfg!(target_os = "windows") => {
                if distribution.is_empty()
                    || distribution.len() > 128
                    || distribution.starts_with('-')
                    || distribution.chars().any(char::is_control)
                    || user.as_ref().is_some_and(|v| {
                        v.is_empty()
                            || v.len() > 64
                            || v.starts_with('-')
                            || !v
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
                    })
                {
                    return Err(AutomationError::Invalid("invalid WSL setup target"));
                }
                let system = std::path::PathBuf::from(
                    std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
                );
                let mut command = std::process::Command::new(system.join("System32/wsl.exe"));
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                command.args(["--distribution", distribution]);
                if let Some(user) = user {
                    command.args(["--user", user]);
                }
                command.args(["--cd", "/", "--exec", "/bin/sh"]);
                command
            }
            _ => {
                return Err(AutomationError::Invalid(
                    "local setup unavailable on this platform",
                ));
            }
        };
        command.args([
            "-c",
            PYTHON_PREFLIGHT,
            "consolecrypt-setup-preflight",
            "-I",
            "-c",
            &builtin_setup_script()?,
        ]);
        command_budget(&command)?;
        Ok(command)
    }
}

pub async fn setup_environment(target: &SetupTarget, install: bool) -> Result<Value> {
    let mut command = tokio::process::Command::from(target.command()?);
    command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or(AutomationError::Closed)?;
    let mut stdout = child.stdout.take().ok_or(AutomationError::Closed)?;
    let raw = setup_payload(install, matches!(target, SetupTarget::Wsl { .. }))?;
    let future = async {
        stdin.write_all(&raw).await?;
        stdin.flush().await?;
        let mut output = vec![];
        (&mut stdout).take(65537).read_to_end(&mut output).await?;
        if output.len() > 65536 {
            return Err(AutomationError::Protocol("setup response too large"));
        }
        let status = child.wait().await?;
        drop(stdin);
        if !status.success() && output.is_empty() {
            return Err(AutomationError::Remote("setup_unavailable".into()));
        }
        let mut result = setup_result(&output, status.success(), install)?;
        if !install
            && matches!(target, SetupTarget::Wsl { .. })
            && result.get("wsl2") != Some(&Value::Bool(true))
        {
            result["install_supported"] = json!(false);
            result["missing"]
                .as_array_mut()
                .ok_or(AutomationError::Protocol("missing setup requirements"))?
                .push(json!("wsl2"));
        }
        Ok(result)
    };
    tokio::time::timeout(Duration::from_secs(if install { 850 } else { 30 }), future)
        .await
        .map_err(|_| AutomationError::Timeout)?
}

#[cfg(test)]
pub(crate) fn embedded_helper_digest()->String {
    use sha2::{Digest,Sha256};
    let mut digest=Sha256::new();
    // SETUP_FILES is sorted by build.rs exactly like probe's relative paths.
    for (name,text) in SETUP_FILES {
        if let Some(relative)=name.strip_prefix("src/consolecrypt_runner/").filter(|p|p.ends_with(".py")) {
            digest.update(relative.as_bytes());digest.update(b"\0");
            digest.update(text.as_bytes());digest.update(b"\0");
        }
    }
    hex::encode(digest.finalize())
}

fn setup_payload(install: bool, require_wsl2: bool) -> Result<Vec<u8>> {
    use sha2::{Digest, Sha256};
    let files = SETUP_FILES.iter().copied().collect::<BTreeMap<_, _>>();
    // Opaque package identity for managed-update UX, not a credential or grant.
    let fingerprint = hex::encode(Sha256::digest(
        serde_json::to_vec(&files)
            .map_err(|_| AutomationError::Protocol("invalid setup package"))?,
    ));
    let payload = if install {
        json!({"action":"install", "require_wsl2":require_wsl2,
               "files":files,"package_fingerprint":fingerprint})
    } else {
        json!({"action":"scan","expected_package_fingerprint":fingerprint})
    };
    let mut raw = serde_json::to_vec(&payload)
        .map_err(|_| AutomationError::Protocol("invalid setup package"))?;
    if raw.len() >= 4 * 1024 * 1024 {
        return Err(AutomationError::Protocol("setup package too large"));
    }
    raw.push(b'\n');
    Ok(raw)
}

fn setup_result(output: &[u8], success: bool, install: bool) -> Result<Value> {
    if output.len() > 65536 {
        return Err(AutomationError::Protocol("setup response too large"));
    }
    if !success && output.is_empty() {
        return Err(AutomationError::Remote("setup_unavailable".into()));
    }
    let value = crate::json::parse(output)?;
    if !success || value.get("ok") != Some(&Value::Bool(true)) {
        let code = value
            .get("error")
            .and_then(Value::as_str)
            .filter(|v| v.len() < 64 && v.bytes().all(|c| c.is_ascii_lowercase() || c == b'_'))
            .unwrap_or("setup_failed");
        return Err(AutomationError::Remote(code.into()));
    }
    let mut result = value
        .get("result")
        .cloned()
        .filter(Value::is_object)
        .ok_or(AutomationError::Protocol("missing setup result"))?;
    if !install {
        let manifest: Value =
            serde_json::from_str(include_str!("../../../../runner/environment.json"))
                .map_err(|_| AutomationError::Protocol("invalid built-in environment manifest"))?;
        result["required_packages"] = manifest["packages"].clone();
    }
    Ok(result)
}

/// The caller owns a dedicated, host-key-verified SSH route and closes it on
/// completion/cancellation. Only this built-in script crosses the exec channel.
pub async fn setup_remote_environment(
    session: &cc_ssh_core::SshSession,
    install: bool,
) -> Result<Value> {
    let command = format!(
        "for cc_python in /usr/bin/python3.12 /usr/bin/python3.14 /usr/bin/python3; do if [ -x \"$cc_python\" ]; then exec \"$cc_python\" -I -c '{}'; fi; done; exit 69",
        builtin_setup_script()?.replace('\'', "'\"'\"'")
    );
    let input = setup_payload(install, false)?;
    let output = tokio::time::timeout(
        Duration::from_secs(if install { 850 } else { 30 }),
        session.exec_with_private_input(&command, &input, 65536),
    )
    .await
    .map_err(|_| AutomationError::Timeout)?
    .map_err(|_| AutomationError::Remote("setup_ssh_failed".into()))?;
    setup_result(
        &output.stdout,
        output.exit_status == Some(0) && output.exit_signal.is_none(),
        install,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_keeps_two_original_modules_and_fits_windows_command_budget() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        use std::io::Read;
        let script = builtin_setup_script().unwrap();
        assert!(script.len() <= 15 * 1024);
        let encoded = script
            .split("base64.b64decode('")
            .nth(1)
            .unwrap()
            .split("')")
            .next()
            .unwrap();
        let compressed = STANDARD.decode(encoded).unwrap();
        let mut raw = vec![];
        flate2::read::ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let modules: Vec<String> = serde_json::from_slice(&raw).unwrap();
        assert_eq!(modules.len(), 2);
        assert_eq!(
            modules[0],
            include_str!("../../../../runner/setup/environment_cleanup.py")
        );
        assert_eq!(modules[1], SCRIPT);
        assert!(script.contains("dont_inherit=True"));
        let mut oversized = std::process::Command::new("owned-fixture.exe");
        oversized.arg("x".repeat(32767));
        assert!(
            matches!(command_budget(&oversized),Err(AutomationError::Remote(code)) if code=="setup_command_too_large")
        );
        let error = setup_result(
            br#"{"ok":false,"error":"setup_bootstrap_failed"}"#,
            false,
            false,
        )
        .unwrap_err();
        assert!(matches!(error,AutomationError::Remote(code) if code=="setup_bootstrap_failed"));
    }
    #[test]
    fn prerequisite_terminal_accepts_only_the_fixed_reviewed_package_set() {
        let prefix = "sudo apt-get update && sudo apt-get install --no-install-recommends ";
        assert!(
            prerequisite_script(&format!("{prefix}python3-venv bubblewrap"))
                .unwrap()
                .contains("/usr/bin/sudo /usr/bin/apt-get")
        );
        for suffix in [
            "",
            "git git",
            "git; echo bad",
            "git\n",
            "$(bad)",
            "--allow-unauthenticated",
            "curl",
            "git  bubblewrap",
        ] {
            assert!(
                prerequisite_script(&format!("{prefix}{suffix}")).is_err(),
                "{suffix}"
            );
        }
    }
    #[test]
    fn builtin_package_contains_exact_locks_and_no_local_state() {
        assert!(SETUP_FILES.len() > 30 && SETUP_FILES.len() < 128);
        assert!(
            SETUP_FILES
                .iter()
                .any(|(path, _)| *path == "requirements-linux-x86_64-py312.lock")
        );
        assert!(
            SETUP_FILES
                .iter()
                .all(|(path, _)| path.starts_with("src/consolecrypt_runner/")
                    || path.starts_with("requirements-"))
        );
        assert!(
            SETUP_FILES
                .iter()
                .map(|(_, content)| content.len())
                .sum::<usize>()
                < 4 * 1024 * 1024
        );
    }
    #[test]
    fn changed_bootstrap_package_is_outdated_even_without_version_bump() {
        use sha2::{Digest, Sha256};
        let mut legacy = SETUP_FILES
            .iter()
            .map(|(path, content)| (*path, (*content).to_owned()))
            .collect::<BTreeMap<_, _>>();
        let probe = legacy.get_mut("src/consolecrypt_runner/probe.py").unwrap();
        assert!(probe.contains("\"trusted_worker_bootstrap_v1\": True,"));
        *probe = probe.replace("\"trusted_worker_bootstrap_v1\": True,", "");
        let legacy_fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&legacy).unwrap()));
        let current: Value = serde_json::from_slice(&setup_payload(false, false).unwrap()).unwrap();
        assert_ne!(current["expected_package_fingerprint"], legacy_fingerprint);
    }
    #[test]
    fn scan_and_install_bind_the_same_embedded_package_for_update_detection() {
        let scan: Value = serde_json::from_slice(&setup_payload(false, false).unwrap()).unwrap();
        let install: Value = serde_json::from_slice(&setup_payload(true, false).unwrap()).unwrap();
        assert!(scan.get("files").is_none());
        assert_eq!(
            scan["expected_package_fingerprint"],
            install["package_fingerprint"]
        );
        assert_eq!(
            scan["expected_package_fingerprint"].as_str().unwrap().len(),
            64
        );
        assert!(install["files"].is_object());
    }
    #[test]
    fn missing_python_is_a_typed_setup_failure_without_remote_text() {
        let error = setup_result(
            br#"{"ok":false,"error":"setup_python_missing"}"#,
            false,
            false,
        )
        .unwrap_err();
        assert!(matches!(error, AutomationError::Remote(code) if code == "setup_python_missing"));
    }

    #[test]
    fn local_setup_uses_only_fixed_preflight_and_exact_original_python_arguments() {
        let target = if cfg!(windows) {
            SetupTarget::Wsl {
                distribution: "Owned distro; $(not-a-command)".into(),
                user: Some("alice".into()),
            }
        } else if cfg!(any(target_os = "linux", target_os = "macos")) {
            SetupTarget::Native
        } else {
            return;
        };
        let command = target.command().unwrap();
        let args: Vec<_> = command.get_args().map(|v| v.to_str().unwrap()).collect();
        let shell = args.iter().position(|v| *v == "-c").unwrap();
        assert_eq!(args[shell + 1], PYTHON_PREFLIGHT);
        assert_eq!(
            args[shell + 2..shell + 5],
            ["consolecrypt-setup-preflight", "-I", "-c"]
        );
        assert_eq!(args[shell + 5], builtin_setup_script().unwrap());
        assert_eq!(shell + 6, args.len());
        assert!(!PYTHON_PREFLIGHT.contains("Owned distro"));
        assert!(!PYTHON_PREFLIGHT.contains("alice"));
        assert!(PYTHON_PREFLIGHT.contains("exec /usr/bin/python3 \"$@\""));
    }

    #[test]
    fn wsl_target_is_explicit_argv_outside_the_fixed_native_preflight() {
        if !cfg!(windows) {
            return;
        }
        let target = SetupTarget::Wsl {
            distribution: "Ubuntu 24.04; $(bad)".into(),
            user: Some("alice".into()),
        };
        let command = target.command().unwrap();
        let args: Vec<_> = command.get_args().map(|v| v.to_string_lossy()).collect();
        assert_eq!(args[1], "Ubuntu 24.04; $(bad)");
        assert!(!args.iter().any(|v| v == "sh" || v == "bash"));
        assert!(args.windows(2).any(|v| v == ["--exec", "/bin/sh"]));
        assert_eq!(args.iter().filter(|v| **v == PYTHON_PREFLIGHT).count(), 1);
        for user in ["-root", "a b", "a;bad", "a\n"] {
            assert!(
                SetupTarget::Wsl {
                    distribution: "Ubuntu".into(),
                    user: Some(user.into())
                }
                .command()
                .is_err()
            );
        }
    }
}
