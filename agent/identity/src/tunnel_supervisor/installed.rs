//! Same installed/current/independent-floor trust boundary as delivery/launch.py.
//! No caller command, search path, pin discovery/creation or mutable executable.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

// Closed OS interpreter selection from the checked selected package only.
// No caller path, interpreter discovery, PATH search or fallback is permitted.
fn system_python(path:&str)->Result<&'static str> {
    match path {
        "/usr/bin/python3.11"=>Ok("/usr/bin/python3.11"),
        "/usr/bin/python3.12"=>Ok("/usr/bin/python3.12"),
        _=>Err(Error::CapabilityDenied),
    }
}
const CHILD: &str = "integration/tunnel_supervised_child.py";
const WHEEL: &str = "python/site-packages/websockets-17.4-py3-none-any.whl";
const WHEEL_HASH: &str = "c6bd2c9781915592b209160d0f017d4c7c82ad53075e6b0c24d2a89299e7f9e9";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    version: String,
    artifact_sha256: String,
    previous_version: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Installed {
    version: String,
    artifact_sha256: String,
    files: BTreeMap<String, String>,
    file_modes: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimePin {
    format: String,
    python_executable: String,
    python_sha256: String,
}

#[derive(Clone)]
pub(super) struct Pin {
    path: PathBuf,
    identity: (u64, u64, u64, u32, u32, i64, i64, i64, i64),
    root_owned: bool,
}
fn identity(m: &fs::Metadata) -> (u64, u64, u64, u32, u32, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.uid(),
        m.mode(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
impl Pin {
    pub(super) fn metadata(path: &Path, uid: u32, private: bool) -> Result<Self> {
        if !path.is_absolute()
            || !path.as_os_str().as_encoded_bytes().is_ascii()
            || path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(Error::Storage);
        }
        for parent in path.ancestors().skip(1) {
            let m = fs::symlink_metadata(parent).map_err(|_| Error::Storage)?;
            if !m.is_dir()
                || m.file_type().is_symlink()
                || m.uid() != 0 && m.uid() != uid
                || m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0
            {
                return Err(Error::Storage);
            }
        }
        let m = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
        if !m.is_file()
            || m.file_type().is_symlink()
            || m.nlink() != 1
            || m.len() > 65536
            || m.mode() & 0o022 != 0
            || m.uid() != 0 && m.uid() != uid
            || private && (m.uid() != uid || m.mode() & 0o777 != 0o600)
        {
            return Err(Error::Storage);
        }
        Ok(Self {
            path: path.to_owned(),
            identity: identity(&m),
            root_owned: false,
        })
    }
    pub(super) fn check(&self) -> Result<()> {
        for parent in self.path.ancestors().skip(1) {
            let m = fs::symlink_metadata(parent).map_err(|_| Error::Storage)?;
            if !m.is_dir()
                || m.file_type().is_symlink()
                || self.root_owned && (m.uid() != 0 || m.mode() & 0o022 != 0)
            {
                return Err(Error::Storage);
            }
        }
        let m = fs::symlink_metadata(&self.path).map_err(|_| Error::Storage)?;
        if m.file_type().is_symlink() || identity(&m) != self.identity {
            return Err(Error::Storage);
        }
        Ok(())
    }
}
pub(super) fn read_file(path: &Path, cap: u64, root_owned: bool) -> Result<(Vec<u8>, Pin)> {
    if !path.is_absolute()
        || !path.as_os_str().as_encoded_bytes().is_ascii()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(Error::Storage);
    }
    for parent in path.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(|_| Error::Storage)?;
        if !m.is_dir()
            || m.file_type().is_symlink()
            || root_owned && (m.uid() != 0 || m.mode() & 0o022 != 0)
        {
            return Err(Error::Storage);
        }
    }
    let before = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.nlink() != 1
        || before.len() > cap
        || before.mode() & 0o7022 != 0
        || root_owned && before.uid() != 0
    {
        return Err(Error::Storage);
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x20000 | 0x800)
        .open(path)
        .map_err(|_| Error::Storage)?;
    if identity(&before) != identity(&file.metadata().map_err(|_| Error::Storage)?) {
        return Err(Error::Storage);
    }
    let mut raw = Vec::new();
    (&mut file)
        .take(cap + 1)
        .read_to_end(&mut raw)
        .map_err(|_| Error::Storage)?;
    let pin = Pin {
        path: path.to_owned(),
        identity: identity(&before),
        root_owned,
    };
    if raw.len() as u64 != before.len() {
        return Err(Error::Storage);
    }
    pin.check()?;
    Ok((raw, pin))
}
fn valid_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|v| !v.is_empty() && v.len() <= 6 && v.bytes().all(|b| b.is_ascii_digit()))
}
fn safe_relative(value: &str) -> bool {
    let p = Path::new(value);
    !value.is_empty()
        && value.is_ascii()
        && !p.is_absolute()
        && !value.contains('\\')
        && !value.contains(':')
        && p.components()
            .all(|v| matches!(v, std::path::Component::Normal(_)))
}
fn closed_tree(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
    entries: &mut usize,
) -> Result<()> {
    for item in fs::read_dir(directory).map_err(|_| Error::Storage)? {
        *entries += 1;
        if *entries > 512
            || directory
                .strip_prefix(root)
                .map_err(|_| Error::Storage)?
                .components()
                .count()
                > 16
        {
            return Err(Error::Quota);
        }
        let p = item.map_err(|_| Error::Storage)?.path();
        let m = fs::symlink_metadata(&p).map_err(|_| Error::Storage)?;
        if m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err(Error::Storage);
        }
        if m.is_dir() {
            if files.len() > 514 {
                return Err(Error::Quota);
            }
            closed_tree(root, &p, files, entries)?;
        } else if m.is_file() {
            files.insert(
                p.strip_prefix(root)
                    .map_err(|_| Error::Storage)?
                    .to_str()
                    .ok_or(Error::Storage)?
                    .to_owned(),
            );
        } else {
            return Err(Error::Storage);
        }
        if files.len() > 130 {
            return Err(Error::Quota);
        }
    }
    Ok(())
}
pub(super) struct InstalledChild {
    pub release: PathBuf,
    pub script: PathBuf,
    pub python: &'static str,
    pins: Vec<Pin>,
}
impl InstalledChild {
    pub fn capture() -> Result<Self> {
        let executable = std::env::current_exe().map_err(|_| Error::Storage)?;
        if executable.file_name() != Some(std::ffi::OsStr::new("cc-agent-identity"))
            || executable.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new("bin"))
        {
            return Err(Error::Storage);
        }
        let release = executable
            .parent()
            .and_then(Path::parent)
            .ok_or(Error::Storage)?
            .to_owned();
        let version = release
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or(Error::Storage)?;
        if !valid_version(version) {
            return Err(Error::InvalidWire);
        }
        let releases = release.parent().ok_or(Error::Storage)?;
        if releases.file_name() != Some(std::ffi::OsStr::new("releases")) {
            return Err(Error::Storage);
        }
        let code = releases.parent().ok_or(Error::Storage)?;
        let (current, p1) = read_file(&code.join("current.json"), 4096, true)?;
        let current: Current = serde_json::from_slice(&current).map_err(|_| Error::InvalidWire)?;
        let (record, p2) = read_file(&release.join("installed.json"), 128*1024, true)?;
        let record: Installed = serde_json::from_slice(&record).map_err(|_| Error::InvalidWire)?;
        if current.version != version
            || record.version != current.version
            || current.artifact_sha256 != record.artifact_sha256
            || record.files.len() > 513
            || record.file_modes.keys().ne(record.files.keys())
        {
            return Err(Error::Storage);
        }
        if let Some(previous) = &current.previous_version {
            if !valid_version(previous) {
                return Err(Error::InvalidWire);
            }
        }
        let prefix = code.parent().and_then(Path::parent).ok_or(Error::Storage)?;
        let (floor, p3) = read_file(
            &prefix.join("var/lib/consolecrypt-agent/release-anchor.json"),
            4096,
            true,
        )?;
        let floor: serde_json::Value =
            serde_json::from_slice(&floor).map_err(|_| Error::InvalidWire)?;
        if floor["protocol"].as_u64() != Some(1) || floor["version"].as_u64().is_none_or(|v| v < 1)
        {
            return Err(Error::Storage);
        }
        // As in the existing launcher, an explicit root-selected rollback never
        // recreates/lowers the independent floor; this function does no writes.
        let mut actual = BTreeSet::new();
        closed_tree(&release, &release, &mut actual, &mut 0)?;
        let mut declared = record.files.keys().cloned().collect::<BTreeSet<_>>();
        declared.insert("installed.json".into());
        if actual != declared {
            return Err(Error::Storage);
        }
        let mut pins = vec![p1, p2, p3];
        let mut total = 0u64;
        for (name, expected) in &record.files {
            if !safe_relative(name) {
                return Err(Error::Storage);
            }
            let (raw, pin) = read_file(&release.join(name), if name=="collector/prometheus-hardmode" {192*1024*1024}else{64*1024*1024}, true)?;
            total += raw.len() as u64;
            if total > 256 * 1024 * 1024 + 128*1024 || hex::encode(Sha256::digest(&raw)) != *expected {
                return Err(Error::InvalidSignature);
            }
            let mode = record.file_modes.get(name).ok_or(Error::Storage)?;
            let expected_mode = match mode.as_str() {
                "0644" => 0o644,
                "0755" => 0o755,
                _ => return Err(Error::Storage),
            };
            if pin.identity.4 & 0o777 != expected_mode {
                return Err(Error::Storage);
            }
            pins.push(pin);
        }
        for name in [
            CHILD,
            "runtime/cc_agent/tunnel_supervised.py",
            WHEEL,
            "python/supervisor-runtime.json",
            "bin/cc-agent-identity",
        ] {
            if !record.files.contains_key(name) {
                return Err(Error::NotFound);
            }
        }
        if record.files[WHEEL] != WHEEL_HASH {
            return Err(Error::InvalidSignature);
        }
        if identity(&fs::metadata("/proc/self/exe").map_err(|_| Error::Storage)?).0
            != fs::metadata(&executable).map_err(|_| Error::Storage)?.dev()
            || fs::metadata("/proc/self/exe")
                .map_err(|_| Error::Storage)?
                .ino()
                != fs::metadata(&executable).map_err(|_| Error::Storage)?.ino()
        {
            return Err(Error::Storage);
        }
        let (pin, _) = read_file(&release.join("python/supervisor-runtime.json"), 4096, true)?;
        let pin: RuntimePin = serde_json::from_slice(&pin).map_err(|_| Error::InvalidWire)?;
        if pin.format != "cc-tunnel-supervisor-runtime/v1" {
            return Err(Error::CapabilityDenied);
        }
        let selected_python=system_python(&pin.python_executable)?;
        let (python, held) = read_file(Path::new(selected_python), 128 * 1024 * 1024, true)?;
        if held.identity.4 & 0o777 != 0o755 {return Err(Error::Storage)}
        if hex::encode(Sha256::digest(&python)) != pin.python_sha256 {
            return Err(Error::InvalidSignature);
        }
        pins.push(held);
        let value = Self {
            script: release.join(CHILD),
            python: selected_python,
            release,
            pins,
        };
        value.check()?;
        Ok(value)
    }
    pub fn check(&self) -> Result<()> {
        for p in &self.pins {
            p.check()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod runtime_pin_tests {
    use super::*;
    #[test]
    fn system_python_selection_is_closed_and_has_no_fallback() {
        assert_eq!(system_python("/usr/bin/python3.11").unwrap(),"/usr/bin/python3.11");
        assert_eq!(system_python("/usr/bin/python3.12").unwrap(),"/usr/bin/python3.12");
        for wrong in ["python3","/usr/bin/python3","/usr/bin/python3.13","/tmp/python3.12",
            "/usr/bin/../bin/python3.12","/usr/bin/python3.12 -c pass",""] {
            assert!(system_python(wrong).is_err());
        }
    }
    #[test]
    fn runtime_pin_keeps_closed_schema_and_rejects_caller_extensions() {
        let raw=br#"{"format":"cc-tunnel-supervisor-runtime/v1","pythonExecutable":"/usr/bin/python3.12","pythonSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        let pin:RuntimePin=serde_json::from_slice(raw).unwrap();
        assert_eq!(system_python(&pin.python_executable).unwrap(),"/usr/bin/python3.12");
        for raw in [br#"{"format":"cc-tunnel-supervisor-runtime/v1","pythonExecutable":"/usr/bin/python3.12","pythonSha256":"a","fallback":true}"#.as_slice(),
            br#"{"format":"cc-tunnel-supervisor-runtime/v1","pythonExecutable":"/usr/bin/python3.11","pythonExecutable":"/usr/bin/python3.12","pythonSha256":"a"}"#.as_slice()] {
            assert!(serde_json::from_slice::<RuntimePin>(raw).is_err());
        }
    }
}
