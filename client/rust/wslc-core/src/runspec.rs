//! `wslc run` / `wslc create` arguments built from a validated [`RunSpec`].
//! Arguments are passed to the CLI one by one, so validation only has to stop values from
//! being read as options and keep paths and names in their documented shapes.
use crate::{
    desktop::{host_path, validate_container_path},
    models::*,
    runtime::cli::validate_name,
};
use serde_json::Value;
use std::net::IpAddr;

fn invalid(message: &str) -> AppError {
    AppError::new("invalid_spec", "run", message)
}

pub fn validate_image(image: &str) -> AppResult<()> {
    let ok = !image.is_empty()
        && image.len() <= 255
        && !image.starts_with('-')
        && image
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._/:@-".contains(c));
    ok.then_some(())
        .ok_or_else(|| invalid("Укажите образ, например nginx:latest"))
}

pub fn validate_container_name(name: &str) -> AppResult<()> {
    let mut chars = name.chars();
    let ok = name.len() <= 128
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    ok.then_some(())
        .ok_or_else(|| invalid("Имя: латиница, цифры и _ . -, начинается с буквы или цифры"))
}

fn validate_env_key(key: &str) -> AppResult<()> {
    let mut chars = key.chars();
    let ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some(())
        .ok_or_else(|| invalid(&format!("Некорректное имя переменной «{key}»")))
}

fn plain(value: &str, what: &str) -> AppResult<()> {
    if value.is_empty() || value.starts_with('-') || value.contains(['\0', '\n', '\r']) {
        return Err(invalid(&format!("Некорректное значение: {what}")));
    }
    Ok(())
}

fn single_line(value: &str, what: &str) -> AppResult<()> {
    if value.contains(['\0', '\n', '\r']) {
        return Err(invalid(&format!("Некорректное значение: {what}")));
    }
    Ok(())
}

/// Label keys such as `com.docker.compose.project`.
pub fn validate_label_key(key: &str) -> AppResult<()> {
    let mut chars = key.chars();
    let ok = key.len() <= 255
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c));
    ok.then_some(())
        .ok_or_else(|| invalid(&format!("Некорректный ключ метки «{key}»")))
}

/// Host names, domains and network aliases.
fn host_name(value: &str, what: &str) -> AppResult<()> {
    let mut chars = value.chars();
    let ok = value.len() <= 253
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    ok.then_some(())
        .ok_or_else(|| invalid(&format!("Некорректное значение: {what}")))
}

/// Go-style durations as `--health-*` options take them: `30s`, `1m30s`, `500ms`.
pub fn validate_duration(value: &str) -> AppResult<()> {
    let bad = || invalid(&format!("Некорректная длительность «{value}»"));
    let mut rest = value;
    if rest.is_empty() {
        return Err(bad());
    }
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .ok_or_else(bad)?;
        if digits == 0 || rest[..digits].parse::<f64>().is_err() {
            return Err(bad());
        }
        rest = &rest[digits..];
        let unit = ["ns", "us", "µs", "ms", "s", "m", "h"]
            .into_iter()
            .filter(|u| rest.starts_with(u))
            .max_by_key(|u| u.len())
            .ok_or_else(bad)?;
        rest = &rest[unit.len()..];
    }
    Ok(())
}

fn extras_args(x: &RunExtras, args: &mut Vec<String>) -> AppResult<()> {
    for l in &x.labels {
        validate_label_key(&l.key)?;
        single_line(&l.value, "значение метки")?;
        args.extend(["--label".into(), format!("{}={}", l.key, l.value)]);
    }
    if let Some(h) = &x.hostname {
        host_name(h, "имя узла")?;
        args.extend(["--hostname".into(), h.clone()]);
    }
    if let Some(d) = &x.domainname {
        host_name(d, "домен")?;
        args.extend(["--domainname".into(), d.clone()]);
    }
    for a in &x.network_aliases {
        host_name(a, "сетевой псевдоним")?;
        args.extend(["--network-alias".into(), a.clone()]);
    }
    if let Some(ip) = &x.ip {
        if !ip.parse::<IpAddr>().is_ok_and(|ip| ip.is_ipv4()) {
            return Err(invalid("IP контейнера должен быть адресом IPv4"));
        }
        args.extend(["--ip".into(), ip.clone()]);
    }
    if x.no_healthcheck {
        args.push("--no-healthcheck".into());
    } else if let Some(h) = &x.health {
        plain(&h.cmd, "команда проверки здоровья")?;
        args.extend(["--health-cmd".into(), h.cmd.clone()]);
        for (flag, value) in [
            ("--health-interval", &h.interval),
            ("--health-timeout", &h.timeout),
            ("--health-start-period", &h.start_period),
        ] {
            if let Some(v) = value {
                validate_duration(v)?;
                args.extend([flag.into(), v.clone()]);
            }
        }
        if let Some(n) = h.retries {
            args.extend(["--health-retries".into(), n.to_string()]);
        }
    }
    for t in &x.tmpfs {
        let (path, options) = t.split_once(':').unwrap_or((t, ""));
        validate_container_path(path)?;
        if !options
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "=,.".contains(c))
        {
            return Err(invalid("Некорректные параметры tmpfs"));
        }
        args.extend(["--tmpfs".into(), t.clone()]);
    }
    for u in &x.ulimits {
        let (name, limits) = u
            .split_once('=')
            .ok_or_else(|| invalid("Ulimit: имя=мягкий[:жёсткий]"))?;
        let numbers = limits
            .split(':')
            .all(|n| n == "-1" || (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit())));
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase()) || !numbers {
            return Err(invalid("Ulimit: имя=мягкий[:жёсткий]"));
        }
        args.extend(["--ulimit".into(), u.clone()]);
    }
    if let Some(s) = &x.shm_size {
        number(s, "размер /dev/shm", "bkmg")?;
        args.extend(["--shm-size".into(), s.clone()]);
    }
    if let Some(sig) = &x.stop_signal {
        if sig.is_empty() || !sig.chars().all(|c| c.is_ascii_alphanumeric() || c == '+') {
            return Err(invalid("Некорректный сигнал остановки"));
        }
        args.extend(["--stop-signal".into(), sig.clone()]);
    }
    if let Some(t) = x.stop_timeout {
        args.extend(["--stop-timeout".into(), t.to_string()]);
    }
    for d in &x.dns {
        d.parse::<IpAddr>()
            .map_err(|_| invalid("DNS-сервер должен быть IP-адресом"))?;
        args.extend(["--dns".into(), d.clone()]);
    }
    for d in &x.dns_search {
        host_name(d, "домен поиска DNS")?;
        args.extend(["--dns-search".into(), d.clone()]);
    }
    for o in &x.dns_options {
        if o.is_empty()
            || o.starts_with('-')
            || !o
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ":._-".contains(c))
        {
            return Err(invalid("Некорректный параметр DNS"));
        }
        args.extend(["--dns-option".into(), o.clone()]);
    }
    if x.tty {
        args.push("--tty".into());
    }
    if x.interactive {
        args.push("--interactive".into());
    }
    if let Some(g) = &x.gpus {
        if g != "all" && !(!g.is_empty() && g.chars().all(|c| c.is_ascii_digit())) {
            return Err(invalid("GPU: all или число"));
        }
        args.extend(["--gpus".into(), g.clone()]);
    }
    Ok(())
}

fn number(value: &str, what: &str, suffixes: &str) -> AppResult<()> {
    let digits = value.trim_end_matches(|c: char| suffixes.contains(c.to_ascii_lowercase()));
    let ok = value.len() - digits.len() <= 1
        && !digits.is_empty()
        && digits
            .parse::<f64>()
            .is_ok_and(|n| n > 0.0 && n.is_finite());
    ok.then_some(())
        .ok_or_else(|| invalid(&format!("Некорректное значение: {what}")))
}

/// Validated arguments. With `env_file`, variables go through that file instead of `-e`,
/// so values never appear in the process list.
pub fn run_args(spec: &RunSpec, env_file: Option<&str>) -> AppResult<Vec<String>> {
    run_arguments(spec, env_file, true)
}

/// Windows path shape without touching the disk: drive-absolute or UNC, no quotes.
fn windows_path_shape(path: &str) -> AppResult<()> {
    let b = path.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    if !(drive || path.starts_with(r"\\")) || path.contains('"') {
        return Err(invalid("Некорректный путь к папке Windows"));
    }
    Ok(())
}

pub(crate) fn run_arguments(
    spec: &RunSpec,
    env_file: Option<&str>,
    check_paths: bool,
) -> AppResult<Vec<String>> {
    validate_image(&spec.image)?;
    let mut args: Vec<String> = if spec.start {
        vec!["run".into(), "--detach".into()]
    } else {
        vec!["create".into()]
    };
    if let Some(name) = &spec.name {
        validate_container_name(name)?;
        args.extend(["--name".into(), name.clone()]);
    }
    for p in &spec.ports {
        if p.container_port == 0 {
            return Err(invalid("Порт должен быть от 1 до 65535"));
        }
        let mut publish = String::new();
        if let Some(ip) = &p.host_ip {
            let ip: IpAddr = ip
                .parse()
                .map_err(|_| invalid("Некорректный IP для порта"))?;
            publish = match ip {
                IpAddr::V4(v4) => format!("{v4}:"),
                IpAddr::V6(v6) => format!("[{v6}]:"),
            };
        }
        // Host port 0: the runtime picks a free one, like `-p 80` in Docker.
        match (p.host_port, publish.is_empty()) {
            (0, true) => publish.push_str(&p.container_port.to_string()),
            (0, false) => publish.push_str(&format!(":{}", p.container_port)),
            (host, _) => publish.push_str(&format!("{host}:{}", p.container_port)),
        }
        if p.udp {
            publish.push_str("/udp");
        }
        args.extend(["--publish".into(), publish]);
    }
    for m in &spec.mounts {
        validate_container_path(&m.target)?;
        if m.target.contains(':') {
            return Err(invalid("Путь в контейнере не может содержать «:»"));
        }
        let source = if m.volume {
            validate_name(&m.source, "run")?;
            m.source.clone()
        } else {
            if check_paths {
                host_path(&m.source)?.to_string_lossy().into_owned()
            } else {
                windows_path_shape(&m.source)?;
                m.source.clone()
            }
        };
        let mut volume = format!("{source}:{}", m.target);
        if m.read_only {
            volume.push_str(":ro");
        }
        args.extend(["--volume".into(), volume]);
    }
    for e in &spec.env {
        validate_env_key(&e.key)?;
        if e.value.contains(['\0', '\n', '\r']) {
            return Err(invalid(
                "Значение переменной не может содержать перевод строки",
            ));
        }
    }
    if !spec.env.is_empty() {
        match env_file {
            Some(path) => args.extend(["--env-file".into(), path.into()]),
            None => {
                for e in &spec.env {
                    args.extend(["--env".into(), format!("{}={}", e.key, e.value)]);
                }
            }
        }
    }
    if let Some(entrypoint) = &spec.entrypoint {
        plain(entrypoint, "entrypoint")?;
        args.extend(["--entrypoint".into(), entrypoint.clone()]);
    }
    if let Some(dir) = &spec.workdir {
        validate_container_path(dir)?;
        args.extend(["--workdir".into(), dir.clone()]);
    }
    if let Some(user) = &spec.user {
        plain(user, "пользователь")?;
        if !user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
        {
            return Err(invalid("Некорректный пользователь"));
        }
        args.extend(["--user".into(), user.clone()]);
    }
    if let Some(network) = &spec.network {
        validate_name(network, "run")?;
        args.extend(["--network".into(), network.clone()]);
    }
    if let Some(cpus) = &spec.cpus {
        number(cpus, "CPU", "")?;
        args.extend(["--cpus".into(), cpus.clone()]);
    }
    if let Some(memory) = &spec.memory {
        number(memory, "память", "bkmg")?;
        args.extend(["--memory".into(), memory.clone()]);
    }
    if spec.auto_remove {
        args.push("--rm".into());
    }
    if let Some(extras) = &spec.extras {
        extras_args(extras, &mut args)?;
    }
    args.push(spec.image.clone());
    for (i, arg) in spec.command.iter().enumerate() {
        if arg.contains('\0') || (i == 0 && arg.is_empty()) {
            return Err(invalid("Некорректная команда"));
        }
        args.push(arg.clone());
    }
    Ok(args)
}

/// PowerShell-safe quoting for display and copying: single quotes are literal there.
pub fn quote(arg: &str) -> String {
    let safe = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:=@,+-%".contains(c));
    if safe {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', "''"))
    }
}

pub fn command_line(args: &[String]) -> String {
    std::iter::once("wslc".to_owned())
        .chain(args.iter().map(|a| quote(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn preview(spec: &RunSpec) -> AppResult<String> {
    Ok(command_line(&run_args(spec, None)?))
}

/// `run -d` and `create` print the new container ID as the last line.
pub fn push_args(image: &str) -> AppResult<Vec<String>> {
    validate_image(image)?;
    Ok(vec!["push".into(), image.into()])
}

pub fn tag_args(source: &str, target: &str) -> AppResult<Vec<String>> {
    validate_image(source)?;
    validate_image(target)?;
    Ok(vec!["tag".into(), source.into(), target.into()])
}

/// Registry host with an optional port, e.g. `ghcr.io` or `localhost:5000`.
pub fn validate_server(server: &str) -> AppResult<()> {
    let ok = !server.is_empty()
        && server.len() <= 253
        && !server.starts_with(['-', '.', ':'])
        && server
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c));
    ok.then_some(())
        .ok_or_else(|| invalid("Некорректный адрес реестра"))
}

/// The password always travels through stdin (`--password-stdin`), never argv.
pub fn login_args(server: Option<&str>, username: &str) -> AppResult<Vec<String>> {
    if username.is_empty()
        || username.starts_with('-')
        || username
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(invalid("Некорректное имя пользователя"));
    }
    let mut args: Vec<String> = vec![
        "login".into(),
        "--username".into(),
        username.into(),
        "--password-stdin".into(),
    ];
    if let Some(server) = server {
        validate_server(server)?;
        args.push(server.into());
    }
    Ok(args)
}

pub fn logout_args(server: Option<&str>) -> AppResult<Vec<String>> {
    let mut args: Vec<String> = vec!["logout".into()];
    if let Some(server) = server {
        validate_server(server)?;
        args.push(server.into());
    }
    Ok(args)
}

/// Removal target: a full image ID or a `name:tag` reference (removes only that tag).
pub fn image_ref(reference: &str) -> AppResult<()> {
    match reference.strip_prefix("sha256:") {
        Some(hex) => crate::runtime::cli::validate_id(hex),
        None => validate_image(reference),
    }
}

pub fn save_args(images: &[String], output: &str) -> AppResult<Vec<String>> {
    if images.is_empty() {
        return Err(invalid("Выберите образ"));
    }
    let out = crate::desktop::output_path(output)?;
    let mut args: Vec<String> = vec![
        "save".into(),
        "--output".into(),
        out.to_string_lossy().into(),
    ];
    for image in images {
        image_ref(image)?;
        args.push(image.clone());
    }
    Ok(args)
}

pub fn load_args(input: &str) -> AppResult<Vec<String>> {
    let file = host_path(input)?;
    if !file.is_file() {
        return Err(invalid("Выберите tar-архив"));
    }
    Ok(vec![
        "load".into(),
        "--input".into(),
        file.to_string_lossy().into(),
    ])
}

pub fn export_args(container: &str, output: &str) -> AppResult<Vec<String>> {
    crate::runtime::cli::validate_id(container)?;
    let out = crate::desktop::output_path(output)?;
    Ok(vec![
        "export".into(),
        "--output".into(),
        out.to_string_lossy().into(),
        container.into(),
    ])
}

pub fn import_args(input: &str, image: Option<&str>) -> AppResult<Vec<String>> {
    let file = host_path(input)?;
    if !file.is_file() {
        return Err(invalid("Выберите tar-архив"));
    }
    let mut args: Vec<String> = vec!["import".into(), file.to_string_lossy().into()];
    if let Some(image) = image {
        validate_image(image)?;
        args.push(image.into());
    }
    Ok(args)
}

pub fn pull_args(image: &str) -> AppResult<Vec<String>> {
    validate_image(image)?;
    Ok(vec!["pull".into(), image.into()])
}

/// `wslc build` arguments; plain progress gives one line per step.
pub fn build_args(spec: &BuildSpec) -> AppResult<Vec<String>> {
    let context = host_path(&spec.context)?;
    if !context.is_dir() {
        return Err(invalid("Контекст сборки должен быть папкой"));
    }
    let mut args: Vec<String> = vec!["build".into(), "--progress".into(), "plain".into()];
    if let Some(file) = &spec.dockerfile {
        let file = host_path(file)?;
        if !file.is_file() {
            return Err(invalid("Dockerfile не найден"));
        }
        args.extend(["--file".into(), file.to_string_lossy().into_owned()]);
    }
    for tag in &spec.tags {
        validate_image(tag)?;
        args.extend(["--tag".into(), tag.clone()]);
    }
    for a in &spec.build_args {
        validate_env_key(&a.key)?;
        if a.value.contains(['\0', '\n', '\r']) {
            return Err(invalid(
                "Значение аргумента сборки не может содержать перевод строки",
            ));
        }
        args.extend(["--build-arg".into(), format!("{}={}", a.key, a.value)]);
    }
    if let Some(target) = &spec.target {
        validate_name(target, "build")?;
        args.extend(["--target".into(), target.clone()]);
    }
    if spec.no_cache {
        args.push("--no-cache".into());
    }
    if spec.pull {
        args.push("--pull".into());
    }
    args.push(context.to_string_lossy().into_owned());
    Ok(args)
}

/// Command for an existing container: folders that no longer exist are kept as text.
pub fn copy_command(spec: &RunSpec) -> AppResult<String> {
    Ok(command_line(&run_arguments(spec, None, false)?))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

fn memory_text(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    match bytes {
        b if b % (1024 * MIB) == 0 => format!("{}g", b / (1024 * MIB)),
        b if b % MIB == 0 => format!("{}m", b / MIB),
        b if b % 1024 == 0 => format!("{}k", b / 1024),
        b => b.to_string(),
    }
}

pub fn env_entries(container: &Value, image: Option<&Value>) -> Vec<EnvEntry> {
    let inherited = image
        .map(|i| strings(&i["Config"]["Env"]))
        .unwrap_or_default();
    strings(&container["Config"]["Env"])
        .into_iter()
        .filter_map(|e| {
            let (key, value) = e.split_once('=').unwrap_or((&e, ""));
            (!key.is_empty()).then(|| EnvEntry {
                key: key.to_owned(),
                value: value.to_owned(),
                from_image: inherited.contains(&e),
            })
        })
        .collect()
}

/// Rebuilds the wizard spec from container Inspect. Settings equal to the image defaults
/// (environment, command, entrypoint, working directory, user) are left out.
pub fn spec_from_inspect(container: &Value, image: Option<&Value>, include_env: bool) -> RunSpec {
    let config = &container["Config"];
    let host = &container["HostConfig"];
    let defaults = image.map(|i| &i["Config"]).unwrap_or(&Value::Null);
    let image_env = strings(&defaults["Env"]);
    let env = if include_env {
        strings(&config["Env"])
            .into_iter()
            .filter(|e| !image_env.contains(e))
            .filter_map(|e| {
                let (key, value) = e.split_once('=')?;
                Some(EnvVar {
                    key: key.to_owned(),
                    value: value.to_owned(),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut ports = Vec::new();
    let map = container
        .get("Ports")
        .and_then(Value::as_object)
        .or_else(|| container["NetworkSettings"].get("Ports")?.as_object());
    for (key, bindings) in map.into_iter().flatten() {
        let (port, proto) = key.split_once('/').unwrap_or((key, "tcp"));
        let Ok(container_port) = port.parse::<u16>() else {
            continue;
        };
        for b in bindings.as_array().into_iter().flatten() {
            let Some(host_port) = b["HostPort"].as_str().and_then(|p| p.parse().ok()) else {
                continue;
            };
            let ip = b["HostIp"].as_str().unwrap_or_default();
            ports.push(PortSpec {
                host_ip: (!matches!(ip, "" | "127.0.0.1")).then(|| ip.to_owned()),
                host_port,
                container_port,
                udp: proto == "udp",
            });
        }
    }
    let mounts = container["Mounts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let volume = m["Type"].as_str() == Some("volume");
            let source = if volume {
                text(&m["Name"]).or_else(|| text(&m["Source"]))?
            } else {
                text(&m["Source"])?
            };
            Some(MountSpec {
                volume,
                source,
                target: text(&m["Destination"])?,
                read_only: m["ReadWrite"].as_bool() == Some(false),
            })
        })
        .collect();
    let mut entrypoint = strings(&config["Entrypoint"]);
    let mut command = strings(&config["Cmd"]);
    if entrypoint == strings(&defaults["Entrypoint"]) {
        entrypoint.clear();
        if command == strings(&defaults["Cmd"]) {
            command.clear();
        }
    } else if entrypoint.len() > 1 {
        // `--entrypoint` takes one executable; its fixed arguments lead the command.
        let rest = entrypoint.split_off(1);
        command = rest.into_iter().chain(command).collect();
    }
    let differs = |key: &str, default: &str| {
        text(&config[key]).filter(|v| {
            let base = defaults[key].as_str().unwrap_or_default();
            v != base && !(base.is_empty() && v == default)
        })
    };
    let cpus = host["NanoCpus"].as_u64().filter(|n| *n > 0).map(|n| {
        let s = format!("{:.3}", n as f64 / 1e9);
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    });
    RunSpec {
        image: text(&config["Image"])
            .or_else(|| text(&container["Image"]))
            .unwrap_or_default(),
        name: text(&container["Name"]).map(|n| n.trim_start_matches('/').to_owned()),
        ports,
        mounts,
        env,
        command,
        entrypoint: entrypoint.into_iter().next(),
        workdir: differs("WorkingDir", "/"),
        user: differs("User", ""),
        network: text(&host["NetworkMode"]).filter(|n| !matches!(n.as_str(), "bridge" | "default")),
        cpus,
        memory: host["Memory"].as_u64().filter(|m| *m > 0).map(memory_text),
        auto_remove: host["AutoRemove"].as_bool() == Some(true),
        start: true,
        extras: None,
    }
}

pub fn created_id(output: &str) -> AppResult<String> {
    let id = output
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or_default();
    crate::runtime::cli::validate_id(id)?;
    Ok(id.to_owned())
}
