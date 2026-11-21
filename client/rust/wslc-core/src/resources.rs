//! `volume create`, `network create/remove/connect/disconnect` arguments.
use crate::{models::*, runtime::cli::validate_id, runtime::cli::validate_name};
use std::net::IpAddr;

fn invalid(message: &str) -> AppError {
    AppError::new("invalid_spec", "resources", message)
}

/// Built-in networks every session recreates; the UI never offers to remove them.
pub const BUILTIN_NETWORKS: &[&str] = &["bridge", "host", "none"];

fn token(value: &str, what: &str) -> AppResult<()> {
    let ok = !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    ok.then_some(())
        .ok_or_else(|| invalid(&format!("Некорректное значение: {what}")))
}

pub fn validate_cidr(value: &str) -> AppResult<()> {
    let bad = || invalid(&format!("«{value}» — не подсеть в формате CIDR"));
    let (ip, prefix) = value.split_once('/').ok_or_else(bad)?;
    let ip: IpAddr = ip.parse().map_err(|_| bad())?;
    let prefix: u8 = prefix.parse().map_err(|_| bad())?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    (prefix <= max).then_some(()).ok_or_else(bad)
}

fn options(pairs: &[EnvVar], flag: &str, args: &mut Vec<String>) -> AppResult<()> {
    for o in pairs {
        token(&o.key, "ключ параметра")?;
        if o.value.contains(['\0', '\n', '\r']) {
            return Err(invalid(
                "Значение параметра не может содержать перевод строки",
            ));
        }
        args.extend([flag.into(), format!("{}={}", o.key, o.value)]);
    }
    Ok(())
}

pub fn volume_create_args(spec: &VolumeSpec) -> AppResult<Vec<String>> {
    validate_name(&spec.name, "volume_create")?;
    let mut args: Vec<String> = vec!["volume".into(), "create".into()];
    if let Some(driver) = &spec.driver {
        token(driver, "драйвер")?;
        args.extend(["--driver".into(), driver.clone()]);
    }
    options(&spec.options, "--opt", &mut args)?;
    labels(&spec.labels, &mut args)?;
    args.push(spec.name.clone());
    Ok(args)
}

fn labels(pairs: &[EnvVar], args: &mut Vec<String>) -> AppResult<()> {
    for l in pairs {
        crate::runspec::validate_label_key(&l.key)?;
        if l.value.contains(['\0', '\n', '\r']) {
            return Err(invalid("Значение метки не может содержать перевод строки"));
        }
        args.extend(["--label".into(), format!("{}={}", l.key, l.value)]);
    }
    Ok(())
}

pub fn network_create_args(spec: &NetworkSpec) -> AppResult<Vec<String>> {
    validate_name(&spec.name, "network_create")?;
    if BUILTIN_NETWORKS.contains(&spec.name.as_str()) {
        return Err(invalid("Это имя занято встроенной сетью"));
    }
    let mut args: Vec<String> = vec!["network".into(), "create".into()];
    if let Some(driver) = &spec.driver {
        token(driver, "драйвер")?;
        args.extend(["--driver".into(), driver.clone()]);
    }
    if let Some(subnet) = &spec.subnet {
        validate_cidr(subnet)?;
        args.extend(["--subnet".into(), subnet.clone()]);
    }
    if let Some(range) = &spec.ip_range {
        validate_cidr(range)?;
        args.extend(["--ip-range".into(), range.clone()]);
    }
    if let Some(gateway) = &spec.gateway {
        gateway
            .parse::<IpAddr>()
            .map_err(|_| invalid("Шлюз должен быть IP-адресом"))?;
        args.extend(["--gateway".into(), gateway.clone()]);
    }
    if spec.internal {
        args.push("--internal".into());
    }
    labels(&spec.labels, &mut args)?;
    args.push(spec.name.clone());
    Ok(args)
}

pub fn network_remove_args(name: &str) -> AppResult<Vec<String>> {
    validate_name(name, "network_remove")?;
    if BUILTIN_NETWORKS.contains(&name) {
        return Err(invalid("Встроенную сеть удалить нельзя"));
    }
    Ok(vec!["network".into(), "remove".into(), name.into()])
}

pub fn network_connect_args(
    network: &str,
    container: &str,
    aliases: &[String],
    ip: Option<&str>,
) -> AppResult<Vec<String>> {
    validate_name(network, "network_connect")?;
    validate_id(container)?;
    let mut args: Vec<String> = vec!["network".into(), "connect".into()];
    for alias in aliases {
        token(alias, "псевдоним")?;
        args.extend(["--network-alias".into(), alias.clone()]);
    }
    if let Some(ip) = ip {
        let parsed: IpAddr = ip
            .parse()
            .map_err(|_| invalid("IP контейнера должен быть адресом IPv4"))?;
        if !parsed.is_ipv4() {
            return Err(invalid("IP контейнера должен быть адресом IPv4"));
        }
        args.extend(["--ip".into(), ip.into()]);
    }
    args.extend([network.into(), container.into()]);
    Ok(args)
}

pub fn network_disconnect_args(network: &str, container: &str) -> AppResult<Vec<String>> {
    validate_name(network, "network_disconnect")?;
    validate_id(container)?;
    Ok(vec![
        "network".into(),
        "disconnect".into(),
        network.into(),
        container.into(),
    ])
}

/// Small helper image with tar and gzip; pulled on first use when missing.
pub const BACKUP_IMAGE: &str = "mcr.microsoft.com/azurelinux/busybox:1.36";

/// The archive name is `$1`; the script never contains user input.
const BACKUP_SCRIPT: &str = r#"set -e
command -v tar >/dev/null || { echo "В образе нет tar" >&2; exit 3; }
tar -czf "/backup/$1" -C /source .
echo "Архив записан: $1""#;

const RESTORE_SCRIPT: &str = r#"set -e
command -v tar >/dev/null || { echo "В образе нет tar" >&2; exit 3; }
if [ "$2" = clear ]; then rm -rf /target/* /target/.[!.]* /target/..?* 2>/dev/null || true; fi
tar -xzf "/backup/$1" -C /target
echo "Том восстановлен из $1""#;

fn split_file(path: &std::path::Path) -> AppResult<(String, String)> {
    let dir = path
        .parent()
        .ok_or_else(|| invalid("Некорректный путь к архиву"))?
        .to_string_lossy()
        .into_owned();
    let name = path
        .file_name()
        .ok_or_else(|| invalid("Некорректный путь к архиву"))?
        .to_string_lossy()
        .into_owned();
    if name.starts_with('-') || name.contains(['/', '\\', '\0', '\n']) {
        return Err(invalid("Некорректное имя архива"));
    }
    Ok((dir, name))
}

pub fn volume_backup_args(volume: &str, output: &str) -> AppResult<Vec<String>> {
    validate_name(volume, "volume_backup")?;
    let (dir, name) = split_file(&crate::desktop::output_path(output)?)?;
    Ok(vec![
        "run".into(),
        "--rm".into(),
        "--volume".into(),
        format!("{volume}:/source:ro"),
        "--volume".into(),
        format!("{dir}:/backup"),
        BACKUP_IMAGE.into(),
        "sh".into(),
        "-c".into(),
        BACKUP_SCRIPT.into(),
        "wslc-desk-backup".into(),
        name,
    ])
}

/// Restores into `volume` (created when missing); `clear` empties it first.
pub fn volume_restore_args(volume: &str, input: &str, clear: bool) -> AppResult<Vec<String>> {
    validate_name(volume, "volume_restore")?;
    let file = crate::desktop::host_path(input)?;
    if !file.is_file() {
        return Err(invalid("Выберите архив тома"));
    }
    let (dir, name) = split_file(&file)?;
    Ok(vec![
        "run".into(),
        "--rm".into(),
        "--volume".into(),
        format!("{volume}:/target"),
        "--volume".into(),
        format!("{dir}:/backup:ro"),
        BACKUP_IMAGE.into(),
        "sh".into(),
        "-c".into(),
        RESTORE_SCRIPT.into(),
        "wslc-desk-restore".into(),
        name,
        if clear { "clear" } else { "keep" }.into(),
    ])
}
