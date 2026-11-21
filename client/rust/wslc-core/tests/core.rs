use cc_wslc_core::{
    logs::*,
    models::*,
    runtime::{
        cli::{Runtime, VersionSupport, list_args, validate_id, validate_name, version_support},
        parser, process,
    },
    services::{ContainerService, check_action},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn real_network_capture_preserves_full_ids_and_string_booleans() {
    let rows =
        parser::networks(include_str!("fixtures/wslc-3.0.1/captured/networks.ndjson")).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name.as_deref(), Some("bridge"));
    assert!(
        rows.iter()
            .all(|r| r.id.len() == 64 && r.ipv4 == Some(true) && r.ipv6 == Some(false))
    );
    assert_eq!(rows[2].driver.as_deref(), Some("null"));
    assert!(parser::networks(r#"{"ID":"a","IPv4":"unknown"}"#).is_err());
    assert!(parser::networks(r#"{"ID":"a","IPv4":true}"#).is_err());
    assert_eq!(parser::networks(r#"{"ID":"a"}"#).unwrap()[0].ipv4, None);
    assert_eq!(
        list_args("network"),
        ["network", "list", "--no-trunc", "--format", "json"]
    );
}

#[test]
fn real_empty_captures_match_the_version_specific_parsers() {
    let captures = [
        include_str!("fixtures/wslc-3.0.1/captured/container-list.json"),
        include_str!("fixtures/wslc-3.0.1/captured/image-list.json"),
        include_str!("fixtures/wslc-3.0.1/captured/volume-list.json"),
    ];
    for capture in captures {
        let value: serde_json::Value = serde_json::from_str(capture).unwrap();
        assert_eq!(value["exitCode"], 0);
        assert_eq!(value["stderr"], "");
        assert!(
            parser::list_documents(value["stdout"].as_str().unwrap(), "captured")
                .unwrap()
                .is_empty()
        );
    }
}
#[test]
fn version_gate_accepts_verified_and_newer_3x_releases_only() {
    let captured = include_str!("fixtures/wslc-3.0.3/captured/version.txt");
    assert_eq!(version_support(captured), VersionSupport::Verified);
    assert_eq!(
        version_support(
            "wslc 3.0.1.0
"
        ),
        VersionSupport::Verified
    );
    assert_eq!(version_support("wslc 3.0.4.0"), VersionSupport::Compatible);
    assert_eq!(version_support("wslc 3.2.0.0"), VersionSupport::Compatible);
    for bad in [
        "wslc 3.0.0.9",
        "wslc 2.9.9.9",
        "wslc 4.0.0.0",
        "wslc 3.0.3",
        "wslc 3.0.x.0",
        "3.0.3.0",
        "",
    ] {
        assert_eq!(version_support(bad), VersionSupport::Unsupported, "{bad}");
    }
}

#[test]
fn real_303_captures_parse_with_new_fields() {
    let rows = parser::containers(include_str!(
        "fixtures/wslc-3.0.3/captured/containers.ndjson"
    ))
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.id.len() == 64));
    assert!(rows.iter().all(|r| r.state == ContainerState::Exited));
    assert!(
        rows[0]
            .status_display
            .as_deref()
            .is_some_and(|s| s.starts_with("Exited"))
    );
    let images =
        parser::images(include_str!("fixtures/wslc-3.0.3/captured/images.ndjson")).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].tag.as_deref(), Some("1.30"));
    let networks =
        parser::networks(include_str!("fixtures/wslc-3.0.3/captured/networks.ndjson")).unwrap();
    assert_eq!(networks.len(), 3);
    let network = parser::inspect_document(
        include_str!("fixtures/wslc-3.0.3/captured/network-inspect-bridge.json"),
        "inspect_network",
    )
    .unwrap();
    assert_eq!(network["Name"], "bridge");

    let d = parser::inspect_container(
        include_str!("fixtures/wslc-3.0.3/captured/inspect-web.json"),
        false,
    )
    .unwrap();
    assert_eq!(d.name.as_deref(), Some("wslc-desk-web"));
    assert_eq!(d.state, ContainerState::Exited);
    assert_eq!(d.exit_code, Some(255));
    assert_eq!(d.command.as_deref(), Some("nginx -g daemon off;"));
    assert_eq!(d.ports.len(), 1);
    assert_eq!(d.ports[0].container_port, "80/tcp");
    assert_eq!(d.ports[0].host_port.as_deref(), Some("18080"));
    assert_eq!(d.networks.len(), 1);
    assert_eq!(d.networks[0].name, "bridge");
    assert!(d.finished_at.is_some());
    assert!(!d.inspect_json.contains("/usr/local/sbin"));
}

#[test]
fn informational_fields_never_fail_the_document() {
    let text = format!(
        r#"[{{"Id":"{ID}","State":{{"Status":"running","FinishedAt":7}},"Ports":"80/tcp","NetworkSettings":{{"Networks":[]}},"Config":{{"Cmd":"sh"}}}}]"#
    );
    let d = parser::inspect_container(&text, false).unwrap();
    assert!(d.ports.is_empty() && d.networks.is_empty());
    assert_eq!((d.command, d.finished_at), (None, None));
    assert!(
        parser::containers(r#"{"ID":"a","Status":5}"#).unwrap()[0]
            .status_display
            .is_none()
    );
}

fn details() -> ContainerDetails {
    parser::inspect_container(include_str!("fixtures/schema-derived/inspect.json"), false).unwrap()
}

#[test]
fn ndjson_preserves_unicode_unknown_and_missing_fields() {
    let rows =
        parser::containers(include_str!("fixtures/schema-derived/containers.ndjson")).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name.as_deref(), Some("веб-сервер"));
    assert_eq!(rows[1].ports_display, None);
    assert_eq!(rows[2].state, ContainerState::Unknown);
    assert_eq!(rows[2].raw_state.as_deref(), Some("future-state"));
    assert_eq!(
        ContainerState::from_raw(Some("stopped")),
        ContainerState::Unknown
    );
}

#[test]
fn empty_success_is_empty_but_invalid_response_fails() {
    assert!(parser::containers("\r\n ").unwrap().is_empty());
    for bad in [
        "[]",
        "null",
        "error",
        "{\"ID\":9}",
        "{\"ID\":\"a\",\"Ports\":[]}",
        "{\"Names\":\"x\"}",
    ] {
        assert!(parser::containers(bad).is_err(), "{bad}");
    }
    assert!(parser::inspect_container("[]", false).is_err());
}

#[test]
fn inspect_hides_environment_and_does_not_invent_auto_remove() {
    let d = details();
    assert_eq!(d.auto_remove, None);
    assert!(!d.inspect_json.contains("synthetic-test-secret"));
    assert_eq!(d.mounts.as_ref().unwrap()[1].read_write, Some(false));
    assert!(
        parser::inspect_container(include_str!("fixtures/schema-derived/inspect.json"), true)
            .unwrap()
            .inspect_json
            .contains("synthetic-test-secret")
    );
}

#[test]
fn inventory_does_not_invent_sizes_or_tags() {
    let images = parser::images(include_str!("fixtures/schema-derived/images.ndjson")).unwrap();
    assert_eq!(images[1].repository, None);
    assert_eq!(images[1].size_display, None);
    let volumes = parser::volumes(include_str!("fixtures/schema-derived/volumes.ndjson")).unwrap();
    assert_eq!(volumes[0].size_display, None);
}

#[test]
fn arguments_use_existing_container_id_and_no_session_override() {
    assert_eq!(
        list_args("container"),
        [
            "container",
            "list",
            "--all",
            "--no-trunc",
            "--format",
            "json"
        ]
    );
    for bad in ["name", "--help", "x; shutdown", "a", "../wslc.exe"] {
        assert!(validate_id(bad).is_err());
    }
    assert!(validate_id(ID).is_ok());
    for bad in ["", "-f", "a b", "bridge;x", "../net"] {
        assert!(validate_name(bad, "inspect").is_err(), "{bad}");
    }
    assert!(validate_name("my_net-1.0", "inspect").is_ok());
    assert!(process::resolve_cli(Some("wslc.exe")).is_err());
}

#[test]
fn operation_gates_cover_confirmation_and_auto_remove() {
    let mut d = details();
    assert!(check_action(&d, ContainerAction::Start, false).is_err());
    assert!(check_action(&d, ContainerAction::Stop, false).is_err());
    assert!(check_action(&d, ContainerAction::Stop, true).is_ok());
    d.auto_remove = Some(true);
    assert!(check_action(&d, ContainerAction::Restart, true).is_err());
    d.state = ContainerState::Unknown;
    assert!(check_action(&d, ContainerAction::Stop, true).is_err());
}

#[test]
fn output_decode_supports_utf16_and_rejects_damaged_utf8() {
    let bytes: Vec<_> = "Версия WSL: 3.0.1.0\r\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert!(
        process::decode(&bytes, "version")
            .unwrap()
            .contains("Версия")
    );
    assert!(process::decode(&[0xff], "list").is_err());
}

#[test]
fn log_limits_and_cyrillic_chunk_boundaries() {
    let mut buffer = LogBuffer::default();
    for _ in 0..10_100 {
        buffer.push("строка".into());
    }
    assert_eq!(buffer.records().len(), MAX_LINES);
    assert_eq!(buffer.evicted, 100);
    for _ in 0..200 {
        buffer.push("я".repeat(MAX_RECORD_BYTES));
    }
    assert!(buffer.bytes() <= MAX_BYTES);
    assert!(
        buffer
            .records()
            .iter()
            .all(|r| r.text.len() <= MAX_RECORD_BYTES)
    );
    assert!(buffer.records().back().unwrap().truncated);
    let mut decoder = LineDecoder::default();
    let mut lines = Vec::new();
    for b in "Привет мир\r\n".as_bytes() {
        decoder.feed(&[*b], |line| lines.push(line));
    }
    assert_eq!(lines[0].text, "Привет мир");
    decoder.feed(&vec![b'a'; MAX_RECORD_BYTES * 10], |line| lines.push(line));
    decoder.feed(b"\nnext\n", |line| lines.push(line));
    assert!(lines[1].truncated);
    assert_eq!(lines[2].text, "next");
}

struct FakeRuntime {
    inspections: AtomicUsize,
    mutations: AtomicUsize,
    missing: bool,
    timeout: bool,
    active: AtomicUsize,
    max_active: AtomicUsize,
}
impl FakeRuntime {
    fn new() -> Self {
        Self {
            inspections: AtomicUsize::new(0),
            mutations: AtomicUsize::new(0),
            missing: false,
            timeout: false,
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
        }
    }
}
impl Runtime for FakeRuntime {
    async fn list_containers(&self) -> AppResult<Vec<ContainerSummary>> {
        Ok(Vec::new())
    }
    async fn inspect_container(&self, _: &str, _: bool) -> AppResult<ContainerDetails> {
        self.inspections.fetch_add(1, Ordering::SeqCst);
        if self.missing {
            Err(AppError::new("not_found", "inspect", "Удалён"))
        } else {
            Ok(details())
        }
    }
    async fn mutate(&self, id: &str, _: ContainerAction, _: Duration) -> AppResult<()> {
        assert_eq!(id, ID);
        self.mutations.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        if self.timeout {
            Err(AppError::new("timeout", "stop", "Неопределённый результат"))
        } else {
            Ok(())
        }
    }
    async fn remove(&self, id: &str, _: bool) -> AppResult<()> {
        assert_eq!(id, ID);
        self.mutations.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn running_container_is_removed_only_with_force_and_verified_by_list() {
    let service = ContainerService::new(FakeRuntime::new());
    let error = service.remove(ID, false).await.unwrap_err();
    assert_eq!(error.code, "invalid_state");
    assert_eq!(service.runtime.mutations.load(Ordering::SeqCst), 0);
    service.remove(ID, true).await.unwrap();
    assert_eq!(service.runtime.mutations.load(Ordering::SeqCst), 1);
    assert!(service.remove("name", true).await.is_err());
}

#[test]
fn real_stats_capture_parses_percentages_and_usage() {
    let rows = parser::stats(include_str!("fixtures/wslc-3.0.3/captured/stats.ndjson")).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.id.len() == 64));
    assert_eq!(rows[0].memory_percent, Some(0.25));
    assert_eq!(rows[0].cpu_percent, Some(0.0));
    assert!(rows[0].memory_usage.as_deref().unwrap().ends_with("GiB"));
    assert_eq!(rows[0].pids, Some(25));
    let odd = parser::stats(r#"{"ID":"a","CPUPerc":"--","PIDs":"7"}"#).unwrap();
    assert_eq!((odd[0].cpu_percent, odd[0].pids), (None, Some(7)));
    assert!(parser::stats(r#"{"CPUPerc":"1%"}"#).is_err());
}

#[test]
fn file_listing_sorts_directories_first_and_marks_links() {
    let rows = parser::file_entries("f-\tindex.html\ndl\tbin\nd-\tetc\nf-\t.env\ngarbage\nxx\t\n");
    let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["bin", "etc", ".env", "index.html"]);
    assert!(rows[0].is_dir && rows[0].is_link);
    assert!(!rows[2].is_dir && !rows[2].is_link);
}

#[test]
fn shell_targets_are_rebuilt_from_validated_parts() {
    use cc_wslc_core::desktop::*;
    use std::path::Path;
    assert_eq!(
        simplify_path(Path::new(r"\\?\C:\Program Files\WSL\wslc.exe")),
        Path::new(r"C:\Program Files\WSL\wslc.exe")
    );
    assert_eq!(
        simplify_path(Path::new(r"\\?\UNC\server\share\x")),
        Path::new(r"\\server\share\x")
    );
    assert_eq!(
        port_url("localhost", 18081, false).unwrap(),
        "http://localhost:18081/"
    );
    assert_eq!(port_url("::1", 443, true).unwrap(), "https://[::1]:443/");
    assert_eq!(
        port_url("192.168.1.5", 80, false).unwrap(),
        "http://192.168.1.5:80/"
    );
    for (host, port) in [
        ("evil.com", 80),
        ("1.2.3.4/x", 80),
        ("localhost", 0),
        ("", 80),
    ] {
        assert!(port_url(host, port, false).is_err(), "{host}");
    }
    for bad in [
        "relative\\dir",
        "http://example.com",
        "C:relative",
        r"C:\no\such\wslc-desk-dir",
    ] {
        assert!(host_path(bad).is_err(), "{bad}");
    }
    let here = std::env::current_dir().unwrap();
    assert!(host_path(&here.to_string_lossy()).is_ok());
    let args = terminal_args(ID, Some("/data")).unwrap();
    assert_eq!(&args[..5], ["exec", "-it", "-w", "/data", ID]);
    assert!(terminal_args(ID, Some("data")).is_err());
    assert!(terminal_args("abc", None).is_err());
    assert!(validate_container_path("/a\0b").is_err());
}

#[tokio::test]
async fn missing_container_never_runs_mutation() {
    let mut runtime = FakeRuntime::new();
    runtime.missing = true;
    let service = ContainerService::new(runtime);
    assert!(
        service
            .operate(ID, ContainerAction::Stop, true)
            .await
            .is_err()
    );
    assert_eq!(service.runtime.mutations.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn timeout_is_not_retried_and_state_is_read_again() {
    let mut runtime = FakeRuntime::new();
    runtime.timeout = true;
    let service = ContainerService::new(runtime);
    let result = service
        .operate(ID, ContainerAction::Stop, true)
        .await
        .unwrap();
    assert!(result.uncertain);
    assert_eq!(service.runtime.mutations.load(Ordering::SeqCst), 1);
    assert_eq!(service.runtime.inspections.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn mutations_for_the_same_id_are_serialized() {
    let service = Arc::new(ContainerService::new(FakeRuntime::new()));
    let (a, b) = tokio::join!(
        service.operate(ID, ContainerAction::Restart, true),
        service.operate(ID, ContainerAction::Restart, true)
    );
    assert!(a.is_ok() && b.is_ok());
    assert_eq!(service.runtime.max_active.load(Ordering::SeqCst), 1);
}

fn spec() -> RunSpec {
    RunSpec {
        image: "nginx:1.30".into(),
        name: Some("web".into()),
        ports: vec![PortSpec {
            host_ip: Some("0.0.0.0".into()),
            host_port: 8080,
            container_port: 80,
            udp: false,
        }],
        mounts: vec![MountSpec {
            volume: true,
            source: "data".into(),
            target: "/data".into(),
            read_only: true,
        }],
        env: vec![EnvVar {
            key: "TOKEN".into(),
            value: "s3cr3t value".into(),
        }],
        command: vec!["sh".into(), "-c".into(), "echo $TOKEN".into()],
        entrypoint: None,
        workdir: Some("/srv".into()),
        user: None,
        network: Some("bridge".into()),
        cpus: Some("0.5".into()),
        memory: Some("512m".into()),
        auto_remove: false,
        start: true,
        extras: None,
    }
}

#[test]
fn run_spec_builds_arguments_and_keeps_secrets_out_of_argv() {
    use cc_wslc_core::runspec::*;
    let args = run_args(&spec(), Some(r"C:\Temp\x.env")).unwrap();
    assert_eq!(&args[..4], ["run", "--detach", "--name", "web"]);
    assert!(
        args.windows(2)
            .any(|w| w == ["--publish", "0.0.0.0:8080:80"])
    );
    assert!(args.windows(2).any(|w| w == ["--volume", "data:/data:ro"]));
    assert!(
        args.windows(2)
            .any(|w| w == ["--env-file", r"C:\Temp\x.env"])
    );
    assert!(!args.iter().any(|a| a.contains("s3cr3t")));
    assert_eq!(
        &args[args.len() - 4..],
        ["nginx:1.30", "sh", "-c", "echo $TOKEN"]
    );
    let preview = preview(&spec()).unwrap();
    assert!(preview.starts_with("wslc run --detach --name web"));
    assert!(preview.contains("--env 'TOKEN=s3cr3t value'"));
    assert!(preview.ends_with("nginx:1.30 sh -c 'echo $TOKEN'"));
    assert_eq!(quote("it's"), "'it''s'");
    let mut created = spec();
    created.start = false;
    created.env.clear();
    assert_eq!(run_args(&created, None).unwrap()[0], "create");
}

#[test]
fn run_spec_rejects_option_injection_and_bad_values() {
    use cc_wslc_core::runspec::*;
    let cases: &[fn(&mut RunSpec)] = &[
        |s| s.image = "--privileged".into(),
        |s| s.image = "bad image".into(),
        |s| s.name = Some("-x".into()),
        |s| s.ports[0].host_ip = Some("evil".into()),
        |s| s.ports[0].container_port = 0,
        |s| s.mounts[0].target = "data".into(),
        |s| s.mounts[0].source = "../x".into(),
        |s| {
            s.mounts[0].volume = false;
            s.mounts[0].source = r"C:\no\such\wslc-desk-dir".into();
        },
        |s| s.env[0].key = "1BAD".into(),
        |s| s.env[0].value = "a\nb".into(),
        |s| s.cpus = Some("-1".into()),
        |s| s.memory = Some("12xb".into()),
        |s| s.user = Some("--rm".into()),
        |s| s.command = vec![String::new()],
    ];
    for (i, change) in cases.iter().enumerate() {
        let mut s = spec();
        change(&mut s);
        assert!(run_args(&s, None).is_err(), "case {i}");
    }
    assert_eq!(created_id(&format!("pulling...\n{ID}\n")).unwrap(), ID);
    assert!(created_id("Error: no such image").is_err());
}

#[test]
fn existing_container_becomes_a_minimal_run_command() {
    use cc_wslc_core::runspec::*;
    let container: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/wslc-3.0.3/captured/inspect-web.json"
    ))
    .unwrap();
    let image: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/wslc-3.0.3/captured/image-inspect-nginx.json"
    ))
    .unwrap();
    let spec = spec_from_inspect(&container[0], Some(&image[0]), true);
    assert_eq!(spec.name.as_deref(), Some("wslc-desk-web"));
    assert!(spec.command.is_empty() && spec.env.is_empty());
    assert_eq!(spec.ports[0].host_ip, None);
    assert_eq!(
        copy_command(&spec).unwrap(),
        "wslc run --detach --name wslc-desk-web --publish 18080:80 \
         mcr.microsoft.com/azurelinux/base/nginx:1.30"
    );
    // Without the image document the command and environment are kept explicitly.
    let bare = spec_from_inspect(&container[0], None, true);
    assert_eq!(bare.command, ["nginx", "-g", "daemon off;"]);
    assert_eq!(bare.env.len(), 1);
    assert!(spec_from_inspect(&container[0], None, false).env.is_empty());

    let custom: serde_json::Value = serde_json::json!({
        "Name": "/api",
        "Image": "sha256:x",
        "Config": {
            "Image": "example/api:1",
            "Env": ["PATH=/bin", "TOKEN=a b"],
            "Entrypoint": ["/entry.sh", "--serve"],
            "Cmd": ["--port", "80"],
            "WorkingDir": "/srv",
            "User": "1000"
        },
        "HostConfig": {"NetworkMode": "backend", "Memory": 536870912, "NanoCpus": 1500000000},
        "Ports": {"53/udp": [{"HostIp": "0.0.0.0", "HostPort": "5353"}], "80/tcp": null},
        "Mounts": [
            {"Type": "bind", "Source": r"D:\gone\folder", "Destination": "/cfg", "ReadWrite": false},
            {"Type": "volume", "Name": "api-data", "Source": "", "Destination": "/data", "ReadWrite": true}
        ]
    });
    let image = serde_json::json!({"Config": {"Env": ["PATH=/bin"], "Entrypoint": null, "Cmd": ["sh"], "WorkingDir": "", "User": ""}});
    let spec = spec_from_inspect(&custom, Some(&image), true);
    let command = copy_command(&spec).unwrap();
    assert_eq!(
        command,
        concat!(
            "wslc run --detach --name api --publish 0.0.0.0:5353:53/udp ",
            r"--volume 'D:\gone\folder:/cfg:ro' --volume api-data:/data ",
            "--env 'TOKEN=a b' --entrypoint /entry.sh --workdir /srv --user 1000 ",
            "--network backend --cpus 1.5 --memory 512m example/api:1 --serve --port 80"
        )
    );
    // The wizard still refuses the missing folder when it is actually run.
    assert!(run_args(&spec, None).is_err());
}

#[test]
fn real_event_lines_parse_and_unknown_shapes_are_skipped() {
    use cc_wslc_core::events::parse_event;
    let die =
        parse_event(include_str!("fixtures/wslc-3.0.3/captured/event-die.json").trim()).unwrap();
    assert_eq!(
        (die.kind.as_str(), die.action.as_str()),
        ("container", "die")
    );
    assert_eq!(die.id.len(), 64);
    assert_eq!(die.name.as_deref(), Some("wslc-desk-evtest"));
    assert_eq!(die.exit_code, Some(0));
    let net = parse_event(
        r#"{"Action":"connect","Actor":{"Attributes":{"name":"bridge"},"ID":"n1"},"Type":"network","time":5}"#,
    )
    .unwrap();
    assert_eq!((net.kind.as_str(), net.time), ("network", 5));
    for bad in ["", "null", "{}", r#"{"Type":"container"}"#, "not json"] {
        assert!(parse_event(bad).is_none(), "{bad}");
    }
    assert!(
        parse_event(&format!(
            r#"{{"Type":"x","Action":"y","id":"{}"}}"#,
            "a".repeat(70_000)
        ))
        .is_none()
    );
}

#[test]
fn environment_marks_values_inherited_from_the_image() {
    use cc_wslc_core::runspec::env_entries;
    let container = serde_json::json!({"Config": {"Env": [
        "PATH=/usr/bin", "TOKEN=a=b", "EMPTY=", "NOVALUE"
    ]}});
    let image = serde_json::json!({"Config": {"Env": ["PATH=/usr/bin"]}});
    let env = env_entries(&container, Some(&image));
    let view: Vec<_> = env
        .iter()
        .map(|e| (e.key.as_str(), e.value.as_str(), e.from_image))
        .collect();
    assert_eq!(
        view,
        [
            ("PATH", "/usr/bin", true),
            ("TOKEN", "a=b", false),
            ("EMPTY", "", false),
            ("NOVALUE", "", false),
        ]
    );
    assert!(env_entries(&container, None).iter().all(|e| !e.from_image));
}

#[test]
fn log_options_build_history_arguments() {
    use cc_wslc_core::log_readers::log_args;
    let opts = |tail, since: Option<&str>, timestamps| LogOptions {
        tail,
        since: since.map(str::to_owned),
        timestamps,
    };
    assert_eq!(
        log_args(ID, &opts(None, None, false)).unwrap(),
        ["container", "logs", "--tail", "1000", "--follow", ID]
    );
    assert_eq!(
        log_args(ID, &opts(None, Some("1793260000"), true)).unwrap(),
        [
            "container",
            "logs",
            "--since",
            "1793260000",
            "--timestamps",
            "--follow",
            ID
        ]
    );
    assert_eq!(
        log_args(ID, &opts(Some(50), Some("2026-10-29T06:00:00Z"), false)).unwrap()[2..6],
        ["--since", "2026-10-29T06:00:00Z", "--tail", "50"]
    );
    for bad in ["", "--rm", "yesterday", "1h"] {
        assert!(
            log_args(ID, &opts(None, Some(bad), false)).is_err(),
            "{bad}"
        );
    }
    assert!(log_args(ID, &opts(Some(1_000_000), None, false)).is_err());
    assert!(log_args("name", &opts(None, None, false)).is_err());
}

#[test]
fn file_transfer_arguments_keep_paths_out_of_scripts() {
    use cc_wslc_core::files::*;
    assert_eq!(
        read_args(ID, "/etc/nginx/nginx.conf").unwrap(),
        ["exec", ID, "cat", "--", "/etc/nginx/nginx.conf"]
    );
    let write = write_args(ID, "/srv/a b.txt").unwrap();
    assert_eq!(write[..4], ["exec", "-i", ID, "sh"]);
    assert_eq!(write.last().unwrap(), "/srv/a b.txt");
    assert!(!write[5].contains("a b"));
    assert!(write_args(ID, "/srv/").is_err());
    assert!(read_args(ID, "relative").is_err());
    assert!(read_args("x", "/etc").is_err());
    for bad in ["", ".", "..", "a/b", "nul\0"] {
        assert!(validate_file_name(bad).is_err(), "{bad:?}");
    }
    assert_eq!(join("/", "x"), "/x");
    assert_eq!(join("/data", "x"), "/data/x");
    assert_eq!(as_text("привет".as_bytes().to_vec()).unwrap(), "привет");
    assert_eq!(as_text(vec![b'a', 0, b'b']).unwrap_err().code, "binary");
    assert_eq!(as_text(vec![0xff, 0xfe]).unwrap_err().code, "encoding");
}

#[test]
fn job_output_splits_lines_and_marks_progress_updates() {
    use cc_wslc_core::jobs::JobLineSplitter;
    let mut s = JobLineSplitter::default();
    let mut lines = Vec::new();
    s.feed(b"Pulling\r\nlayer 10%\rlayer 50%\rlay", |l| lines.push(l));
    s.feed(b"er 100%\nDone\n\n", |l| lines.push(l));
    s.feed(b"tail", |l| lines.push(l));
    s.finish(|l| lines.push(l));
    let view: Vec<_> = lines
        .iter()
        .map(|l| (l.text.as_str(), l.progress))
        .collect();
    assert_eq!(
        view,
        [
            ("Pulling", false),
            ("layer 10%", true),
            ("layer 50%", true),
            ("layer 100%", false),
            ("Done", false),
            ("tail", false),
        ]
    );
}

#[test]
fn pull_and_build_arguments_are_validated() {
    use cc_wslc_core::runspec::*;
    assert_eq!(pull_args("nginx:1.30").unwrap(), ["pull", "nginx:1.30"]);
    assert!(pull_args("--all-tags").is_err());
    let here = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let spec = BuildSpec {
        context: here.clone(),
        dockerfile: None,
        tags: vec!["demo/app:1".into()],
        build_args: vec![EnvVar {
            key: "VERSION".into(),
            value: "1 2".into(),
        }],
        target: Some("runtime".into()),
        no_cache: true,
        pull: false,
    };
    let args = build_args(&spec).unwrap();
    assert_eq!(&args[..3], ["build", "--progress", "plain"]);
    assert!(args.windows(2).any(|w| w == ["--tag", "demo/app:1"]));
    assert!(args.windows(2).any(|w| w == ["--build-arg", "VERSION=1 2"]));
    assert!(args.contains(&"--no-cache".to_owned()));
    assert_eq!(args.last().unwrap(), &here);
    let mut bad = spec.clone();
    bad.tags = vec!["-t".into()];
    assert!(build_args(&bad).is_err());
    let mut bad = spec.clone();
    bad.context = r"C:\no\such\wslc-desk-dir".into();
    assert!(build_args(&bad).is_err());
    let mut bad = spec;
    bad.dockerfile = Some(here);
    assert!(build_args(&bad).is_err(), "a folder is not a Dockerfile");
}

#[test]
fn registry_arguments_keep_the_password_out_of_argv() {
    use cc_wslc_core::runspec::*;
    assert_eq!(
        login_args(Some("ghcr.io"), "octo").unwrap(),
        ["login", "--username", "octo", "--password-stdin", "ghcr.io"]
    );
    assert_eq!(
        login_args(None, "me").unwrap(),
        ["login", "--username", "me", "--password-stdin"]
    );
    assert_eq!(
        logout_args(Some("localhost:5000")).unwrap(),
        ["logout", "localhost:5000"]
    );
    for bad in ["", "-p", "a b", "evil;rm"] {
        assert!(
            login_args(Some(bad), "u").is_err() || bad.is_empty(),
            "{bad}"
        );
    }
    assert!(login_args(Some("ghcr.io"), "--password").is_err());
    assert!(login_args(Some("ghcr.io"), "has space").is_err());
    assert_eq!(
        tag_args("nginx:1.30", "me/nginx:prod").unwrap(),
        ["tag", "nginx:1.30", "me/nginx:prod"]
    );
    assert!(tag_args("nginx", "-f").is_err());
    assert_eq!(
        push_args("ghcr.io/me/app:1").unwrap(),
        ["push", "ghcr.io/me/app:1"]
    );
    assert!(image_ref(&format!("sha256:{ID}")).is_ok());
    assert!(image_ref("repo/app:1").is_ok());
    assert!(image_ref("sha256:abc").is_err());
    assert!(image_ref("--force").is_err());
}

#[test]
fn archive_arguments_check_files_and_folders() {
    use cc_wslc_core::runspec::*;
    let here = std::env::current_dir().unwrap();
    let out = here
        .join("wslc-desk-out.tar")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        save_args(&["nginx:1.30".into()], &out).unwrap(),
        ["save", "--output", out.as_str(), "nginx:1.30"]
    );
    assert!(save_args(&[], &out).is_err());
    assert!(save_args(&["--all".into()], &out).is_err());
    assert!(save_args(&["a:1".into()], r"C:\no\such\dir\x.tar").is_err());
    assert!(
        save_args(&["a:1".into()], &here.to_string_lossy()).is_err(),
        "a folder"
    );
    assert!(save_args(&["a:1".into()], "relative.tar").is_err());
    assert_eq!(export_args(ID, &out).unwrap()[3], ID);
    assert!(export_args("web", &out).is_err());
    let cargo = here.join("Cargo.toml").to_string_lossy().into_owned();
    assert_eq!(
        load_args(&cargo).unwrap(),
        ["load", "--input", cargo.as_str()]
    );
    assert!(load_args(&here.to_string_lossy()).is_err());
    assert_eq!(import_args(&cargo, Some("me/fs:1")).unwrap()[2], "me/fs:1");
    assert!(import_args(&cargo, Some("-x")).is_err());
}

#[test]
fn volume_and_network_arguments_are_validated() {
    use cc_wslc_core::resources::*;
    let vol = VolumeSpec {
        name: "data".into(),
        driver: Some("vhd".into()),
        options: vec![EnvVar {
            key: "size".into(),
            value: "10G".into(),
        }],
        labels: vec![],
    };
    assert_eq!(
        volume_create_args(&vol).unwrap(),
        [
            "volume", "create", "--driver", "vhd", "--opt", "size=10G", "data"
        ]
    );
    let mut bad = vol.clone();
    bad.driver = Some("--rm".into());
    assert!(volume_create_args(&bad).is_err());
    let net = NetworkSpec {
        name: "backend".into(),
        driver: None,
        subnet: Some("10.231.0.0/24".into()),
        gateway: Some("10.231.0.1".into()),
        ip_range: Some("10.231.0.128/25".into()),
        internal: true,
        labels: vec![],
    };
    assert_eq!(
        network_create_args(&net).unwrap(),
        [
            "network",
            "create",
            "--subnet",
            "10.231.0.0/24",
            "--ip-range",
            "10.231.0.128/25",
            "--gateway",
            "10.231.0.1",
            "--internal",
            "backend"
        ]
    );
    for (subnet, ok) in [
        ("fd00::/64", true),
        ("10.0.0.0/33", false),
        ("10.0.0.0", false),
        ("x/8", false),
    ] {
        assert_eq!(validate_cidr(subnet).is_ok(), ok, "{subnet}");
    }
    let mut builtin = net.clone();
    builtin.name = "bridge".into();
    assert!(network_create_args(&builtin).is_err());
    assert!(network_remove_args("host").is_err());
    assert_eq!(
        network_remove_args("backend").unwrap(),
        ["network", "remove", "backend"]
    );
    assert_eq!(
        network_connect_args("backend", ID, &["api".into()], Some("10.231.0.10")).unwrap(),
        [
            "network",
            "connect",
            "--network-alias",
            "api",
            "--ip",
            "10.231.0.10",
            "backend",
            ID
        ]
    );
    assert!(network_connect_args("backend", ID, &[], Some("fd00::1")).is_err());
    assert!(network_connect_args("backend", "web", &[], None).is_err());
    assert_eq!(
        network_disconnect_args("backend", ID).unwrap()[1],
        "disconnect"
    );
}

#[test]
fn volume_backup_passes_names_as_arguments() {
    use cc_wslc_core::resources::*;
    let here = std::env::current_dir().unwrap();
    let out = here
        .join("data backup.tar.gz")
        .to_string_lossy()
        .into_owned();
    let args = volume_backup_args("app-data", &out).unwrap();
    assert_eq!(
        &args[..6],
        [
            "run",
            "--rm",
            "--volume",
            "app-data:/source:ro",
            "--volume",
            &format!("{}:/backup", here.display())
        ]
    );
    assert_eq!(args[6], BACKUP_IMAGE);
    assert_eq!(args.last().unwrap(), "data backup.tar.gz");
    assert!(
        !args[9].contains("data backup"),
        "the name is never part of the script"
    );
    assert!(volume_backup_args("../x", &out).is_err());
    assert!(volume_backup_args("app", r"C:\no\such\dir\a.tar.gz").is_err());
    let input = here.join("Cargo.toml").to_string_lossy().into_owned();
    let restore = volume_restore_args("new-data", &input, true).unwrap();
    assert_eq!(restore[3], "new-data:/target");
    assert!(restore[5].ends_with(":/backup:ro"));
    assert_eq!(&restore[restore.len() - 2..], ["Cargo.toml", "clear"]);
    assert_eq!(
        volume_restore_args("v", &input, false)
            .unwrap()
            .last()
            .unwrap(),
        "keep"
    );
    assert!(volume_restore_args("v", &here.to_string_lossy(), false).is_err());
}

#[test]
fn topology_reads_images_networks_volumes_and_folders() {
    use cc_wslc_core::topology::*;
    let web = include_str!("fixtures/wslc-3.0.3/captured/inspect-web.json");
    let one = parse(web).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].name.as_deref(), Some("wslc-desk-web"));
    assert_eq!(one[0].networks, ["bridge"]);
    assert!(one[0].image_id.as_deref().unwrap().starts_with("sha256:"));
    let many = r#"[{"Id":"a","Name":"/x","State":{"Status":"running"},"Image":"sha256:1",
        "Config":{"Image":"nginx"},"NetworkSettings":{"Networks":{"backend":{},"bridge":{}}},
        "Mounts":[{"Type":"volume","Name":"data"},{"Type":"bind","Source":"C:\\site"},{"Type":"tmpfs"}]},
        {"Name":"/no-id"}]"#;
    let links = parse(many).unwrap();
    assert_eq!(links.len(), 1, "entries without an ID are skipped");
    assert_eq!(links[0].state, ContainerState::Running);
    assert_eq!(links[0].volumes, ["data"]);
    assert_eq!(links[0].binds, [r"C:\site"]);
    assert_eq!(links[0].networks.len(), 2);
    assert!(parse("{}").is_err());
    assert!(parse("[1]").is_err());
    assert!(parse("[]").unwrap().is_empty());
}

#[test]
fn autostart_command_quotes_the_path_and_starts_hidden() {
    use cc_wslc_core::autostart::command;
    assert_eq!(
        command(std::path::Path::new(
            r"\\?\C:\Program Files\WSLc Desk\wslc_desk.exe"
        )),
        r#""C:\Program Files\WSLc Desk\wslc_desk.exe" --minimized"#
    );
}

#[cfg(windows)]
#[test]
fn autostart_entry_round_trips_in_a_scratch_key() {
    use cc_wslc_core::autostart::{enabled_in, set_in};
    use std::path::Path;
    // Never the real Run key: a scratch key under HKCU, removed at the end.
    let root = r"Software\WslcDeskTest";
    let key = r"Software\WslcDeskTest\Run";
    let exe = Path::new(r"C:\Program Files\WSLc Desk\wslc_desk.exe");
    let other = Path::new(r"D:\old\wslc_desk.exe");
    set_in(key, exe, false).expect("disabling an absent entry is fine");
    assert!(!enabled_in(key, exe));
    set_in(key, exe, true).unwrap();
    assert!(enabled_in(key, exe));
    assert!(
        !enabled_in(key, other),
        "another copy's entry does not count"
    );
    set_in(key, exe, false).unwrap();
    assert!(!enabled_in(key, exe));
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .delete_subkey_all(root)
        .unwrap();
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wslc-desk-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn settings_round_trip_and_leave_no_temporary_file() {
    use cc_wslc_core::{models::*, settings::*};
    let dir = scratch_dir("settings");
    let path = dir.join("WSLc Desk").join("settings.json");
    assert!(
        load_from(&path).templates.is_empty(),
        "missing file gives defaults"
    );
    let spec = RunSpec {
        image: "nginx:1.30".into(),
        name: None,
        ports: vec![PortSpec {
            host_ip: None,
            host_port: 8080,
            container_port: 80,
            udp: false,
        }],
        mounts: vec![],
        env: vec![EnvVar {
            key: "MODE".into(),
            value: String::new(),
        }],
        command: vec![],
        entrypoint: None,
        workdir: None,
        user: None,
        network: None,
        cpus: None,
        memory: None,
        auto_remove: false,
        start: true,
        extras: None,
    };
    let settings = DeskSettings {
        theme: Some("dark".into()),
        accent_color: Some("#4B89FF".into()),
        background_tint: Some("#20B7AE".into()),
        refresh_seconds: Some(10),
        close_to_tray: Some(true),
        templates: vec![ContainerTemplate {
            name: "web".into(),
            spec,
        }],
        ..Default::default()
    };
    save_to(&path, &settings).unwrap();
    assert!(!path.with_extension("json.tmp").exists());
    let back = load_from(&path);
    assert_eq!(back.theme.as_deref(), Some("dark"));
    assert_eq!(back.accent_color.as_deref(), Some("#4B89FF"));
    assert_eq!(back.background_tint.as_deref(), Some("#20B7AE"));
    assert_eq!(back.refresh_seconds, Some(10));
    assert_eq!(back.close_to_tray, Some(true));
    assert_eq!(back.notifications, None);
    assert_eq!(back.templates[0].spec.ports[0].host_port, 8080);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn damaged_settings_are_kept_aside_and_bad_templates_skipped() {
    use cc_wslc_core::settings::*;
    let dir = scratch_dir("settings-bad");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(
        &path,
        r#"{"theme":"light","future":1,"templates":[{"name":"broken"},
        {"name":"ok","spec":{"image":"busybox","name":null,"ports":[],"mounts":[],"env":[],
        "command":[],"entrypoint":null,"workdir":null,"user":null,"network":null,"cpus":null,
        "memory":null,"auto_remove":false,"start":true}}]}"#,
    )
    .unwrap();
    let loaded = load_from(&path);
    assert_eq!(
        loaded.theme.as_deref(),
        Some("light"),
        "unknown keys are ignored"
    );
    assert_eq!(loaded.templates.len(), 1);
    assert!(
        loaded.accent_color.is_none(),
        "old preferences keep the default palette"
    );
    assert!(loaded.background_tint.is_none());
    assert_eq!(loaded.templates[0].name, "ok");
    std::fs::write(&path, "{ not json").unwrap();
    assert!(load_from(&path).theme.is_none());
    assert!(!path.exists());
    assert!(
        path.with_extension("json.bad").exists(),
        "the damaged file is kept"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

const COMPOSE: &str = r#"
name: Shop
x-common: &common
  restart: unless-stopped
  environment:
    TZ: Europe/Moscow
services:
  web:
    <<: *common
    build:
      context: ./web
      args:
        VERSION: ${VERSION:-1.0}
    ports:
      - "8080:80"
      - "127.0.0.1:9000-9001:9000-9001/udp"
      - "3000"
    volumes:
      - ./site:/usr/share/nginx/html:ro
      - data:/data
      - /cache
    networks: [front, back]
    depends_on:
      db:
        condition: service_healthy
    command: nginx -g "daemon off;"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost"]
      interval: 10s
      retries: 3
    privileged: true
  db:
    image: postgres:16
    environment:
      - POSTGRES_PASSWORD=${DB_PASS:?нужен пароль}
      - FROM_SHELL
      - NOT_SET_ANYWHERE
    networks:
      back:
        aliases: [database]
        ipv4_address: 10.5.0.10
    secrets: [db_pass]
    mem_limit: 512mb
    stop_grace_period: 1m30s
    labels:
      tier: data
  debug:
    image: busybox
    profiles: [debug]
networks:
  front: {}
  back:
    ipam:
      config:
        - subnet: 10.5.0.0/24
volumes:
  data:
secrets:
  db_pass:
    file: ./db_pass.txt
"#;

fn compose_env() -> std::collections::HashMap<String, String> {
    [("DB_PASS", "s3cret"), ("FROM_SHELL", "shell")]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

#[test]
fn compose_file_becomes_ordered_wslc_steps() {
    use cc_wslc_core::compose::plan;
    use std::path::Path;
    let dir = Path::new(r"C:\no-such-wslc-desk\shop");
    let p = plan(COMPOSE, Some(dir), None, &compose_env()).unwrap();
    assert_eq!(p.project, "shop");
    let names: Vec<_> = p.services.iter().map(|s| s.service.as_str()).collect();
    assert_eq!(names, ["db", "web"], "dependencies first, profiles skipped");

    let web = &p.services[1];
    assert_eq!(web.container_name, "shop-web-1");
    assert_eq!(web.image, "shop-web");
    let build = web.build.as_ref().unwrap();
    assert_eq!(build.context, r"C:\no-such-wslc-desk\shop\web");
    assert_eq!(build.tags, ["shop-web"]);
    assert_eq!(build.build_args[0].value, "1.0", "default value of VERSION");
    let spec = &web.spec;
    assert!(!spec.start, "created, then started by the runner");
    assert_eq!(spec.env[0].key, "TZ", "merged from the x-common anchor");
    assert_eq!(spec.command, ["nginx", "-g", "daemon off;"]);
    let ports: Vec<_> = spec
        .ports
        .iter()
        .map(|p| {
            (
                p.host_ip.as_deref().unwrap(),
                p.host_port,
                p.container_port,
                p.udp,
            )
        })
        .collect();
    assert_eq!(
        ports,
        [
            ("0.0.0.0", 8080, 80, false),
            ("127.0.0.1", 9000, 9000, true),
            ("127.0.0.1", 9001, 9001, true),
            ("0.0.0.0", 0, 3000, false),
        ]
    );
    assert_eq!(spec.mounts[0].source, r"C:\no-such-wslc-desk\shop\site");
    assert!(spec.mounts[0].read_only && !spec.mounts[0].volume);
    assert_eq!(spec.mounts[1].source, "shop_data");
    assert!(
        spec.mounts[2].source.starts_with("shop_web_"),
        "anonymous volume gets a stable name"
    );
    assert_eq!(p.host_folders, [r"C:\no-such-wslc-desk\shop\site"]);
    assert_eq!(spec.network.as_deref(), Some("shop_front"));
    let extras = spec.extras.as_ref().unwrap();
    assert_eq!(extras.network_aliases, ["web"]);
    assert_eq!(web.extra_networks[0].network, "shop_back");
    assert_eq!(web.depends_on[0].service, "db");
    assert_eq!(web.depends_on[0].condition, "healthy");
    let health = extras.health.as_ref().unwrap();
    assert_eq!(health.cmd, "curl -f http://localhost");
    assert_eq!(health.interval.as_deref(), Some("10s"));
    assert_eq!(health.retries, Some(3));
    let label = |key: &str| {
        extras
            .labels
            .iter()
            .find(|l| l.key == key)
            .map(|l| l.value.as_str())
    };
    assert_eq!(label("com.docker.compose.project"), Some("shop"));
    assert_eq!(label("com.docker.compose.service"), Some("web"));
    assert_eq!(
        label("com.docker.compose.config-hash"),
        Some(web.config_hash.as_str())
    );

    let db = &p.services[0];
    let env: Vec<_> = db
        .spec
        .env
        .iter()
        .map(|e| (e.key.as_str(), e.value.as_str()))
        .collect();
    assert_eq!(
        env,
        [("POSTGRES_PASSWORD", "s3cret"), ("FROM_SHELL", "shell")]
    );
    let db_extras = db.spec.extras.as_ref().unwrap();
    assert_eq!(db.spec.network.as_deref(), Some("shop_back"));
    assert_eq!(db_extras.network_aliases, ["db", "database"]);
    assert_eq!(db_extras.ip.as_deref(), Some("10.5.0.10"));
    assert_eq!(db.spec.memory.as_deref(), Some("512m"));
    assert_eq!(db_extras.stop_timeout, Some(90));
    assert_eq!(db.spec.mounts[0].target, "/run/secrets/db_pass");
    assert!(db.spec.mounts[0].read_only);
    assert!(
        db_extras
            .labels
            .iter()
            .any(|l| l.key == "tier" && l.value == "data")
    );
    assert!(
        db.commands.iter().all(|c| !c.contains("s3cret")),
        "env values never appear in the preview"
    );
    assert!(db.commands[0].contains("--env-file"));

    let nets: Vec<_> = p.networks.iter().map(|n| n.spec.name.as_str()).collect();
    assert_eq!(nets, ["shop_front", "shop_back"]);
    assert_eq!(p.networks[1].spec.subnet.as_deref(), Some("10.5.0.0/24"));
    assert_eq!(p.volumes[0].spec.name, "shop_data");
    assert_eq!(p.volumes.len(), 2);
    let warnings = p.warnings.join("\n");
    for needle in [
        "privileged",
        "restart: unless-stopped",
        "debug",
        "анонимный",
        "0.0.0.0",
    ] {
        assert!(
            warnings.contains(needle),
            "warning about {needle}:\n{warnings}"
        );
    }

    let again = plan(COMPOSE, Some(dir), None, &compose_env()).unwrap();
    assert_eq!(
        again.services[1].config_hash, web.config_hash,
        "stable hash"
    );
    let changed = plan(
        &COMPOSE.replace("postgres:16", "postgres:17"),
        Some(dir),
        None,
        &compose_env(),
    )
    .unwrap();
    assert_ne!(changed.services[0].config_hash, db.config_hash);
    assert_eq!(changed.services[1].config_hash, web.config_hash);
}

#[test]
fn compose_errors_are_explained() {
    use cc_wslc_core::compose::plan;
    use std::collections::HashMap;
    use std::path::Path;
    let dir = Some(Path::new(r"C:\p\shop"));
    let none = HashMap::new();
    let e = plan(COMPOSE, dir, None, &none).unwrap_err();
    assert!(e.message.contains("нужен пароль"), "{}", e.message);
    let cycle = "services:\n  a:\n    image: x\n    depends_on: [b]\n  b:\n    image: y\n    depends_on: [a]\n";
    assert!(
        plan(cycle, dir, None, &none)
            .unwrap_err()
            .message
            .contains("Циклическая")
    );
    let undeclared = "services:\n  a:\n    image: x\n    volumes: [data:/data]\n";
    assert!(
        plan(undeclared, dir, None, &none)
            .unwrap_err()
            .message
            .contains("не объявлен")
    );
    let relative = "services:\n  a:\n    image: x\n    volumes: [./d:/d]\n";
    assert!(
        plan(relative, None, Some("p"), &none)
            .unwrap_err()
            .message
            .contains("папку проекта")
    );
    assert!(
        plan("services: [", dir, None, &none)
            .unwrap_err()
            .message
            .contains("YAML")
    );
    let injection = "services:\n  a:\n    image: \"--privileged\"\n";
    assert!(plan(injection, dir, None, &none).is_err());
}

#[test]
fn compose_interpolation_and_splitting_follow_compose() {
    use cc_wslc_core::compose::{interpolate, normalize_project, shell_split};
    use std::collections::{BTreeSet, HashMap};
    let env: HashMap<String, String> = [
        ("A".to_owned(), "1".to_owned()),
        ("E".to_owned(), String::new()),
    ]
    .into();
    let mut missing = BTreeSet::new();
    let mut i = |s: &str| interpolate(s, &env, &mut missing).unwrap();
    assert_eq!(i("$$HOME ${A} $A"), "$HOME 1 1");
    assert_eq!(i("${E:-d} ${E-d} ${X:-${A}}"), "d  1");
    assert_eq!(i("${A:+yes}${E:+no}"), "yes");
    assert_eq!(i("${NOPE}"), "");
    assert!(missing.contains("NOPE"));
    assert!(interpolate("${E:?empty}", &env, &mut missing).is_err());
    assert_eq!(
        shell_split(r#"sh -c 'echo "$1"' x\ y "a\"b""#).unwrap(),
        ["sh", "-c", r#"echo "$1""#, "x y", r#"a"b"#]
    );
    assert!(shell_split("echo 'open").is_err());
    assert_eq!(normalize_project("My App!"), "myapp");
}

#[test]
fn compose_containers_are_read_from_labels() {
    use cc_wslc_core::compose::containers;
    let docs: Vec<serde_json::Value> = serde_json::from_str(&format!(
        r#"[{{"Id":"{ID}","Name":"/shop-web-1","State":{{"Status":"running","Health":{{"Status":"healthy"}},"ExitCode":0}},
            "Config":{{"Labels":{{"com.docker.compose.project":"shop","com.docker.compose.service":"web",
            "com.docker.compose.config-hash":"abc"}}}}}},
           {{"Id":"x","Name":"/other","State":{{"Status":"exited"}},"Config":{{"Labels":null}}}}]"#
    ))
    .unwrap();
    let found = containers(&docs, "shop");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "shop-web-1");
    assert_eq!(found[0].service.as_deref(), Some("web"));
    assert_eq!(found[0].health.as_deref(), Some("healthy"));
    assert_eq!(found[0].config_hash.as_deref(), Some("abc"));
}

#[test]
fn run_extras_become_validated_options() {
    use cc_wslc_core::runspec::*;
    let mut s = spec();
    s.ports[0].host_port = 0;
    s.ports[0].host_ip = None;
    s.extras = Some(RunExtras {
        labels: vec![EnvVar {
            key: "com.example.tier".into(),
            value: "web".into(),
        }],
        network_aliases: vec!["web".into()],
        health: Some(HealthSpec {
            cmd: "curl -f http://localhost".into(),
            interval: Some("1m30s".into()),
            timeout: None,
            retries: Some(3),
            start_period: None,
        }),
        tmpfs: vec!["/run:size=64m".into()],
        ulimits: vec!["nofile=1024:2048".into()],
        stop_timeout: Some(90),
        ..Default::default()
    });
    let args = run_args(&s, None).unwrap();
    let joined = args.join(" ");
    for part in [
        "--publish 80",
        "--label com.example.tier=web",
        "--network-alias web",
        "--health-cmd curl -f http://localhost",
        "--health-interval 1m30s",
        "--health-retries 3",
        "--tmpfs /run:size=64m",
        "--ulimit nofile=1024:2048",
        "--stop-timeout 90",
    ] {
        assert!(joined.contains(part), "{part} in {joined}");
    }
    let image_at = args.iter().position(|a| a == &s.image).unwrap();
    assert!(args.iter().position(|a| a == "--stop-timeout").unwrap() < image_at);
    let bad: &[fn(&mut RunExtras)] = &[
        |x| {
            x.labels = vec![EnvVar {
                key: "-x".into(),
                value: String::new(),
            }]
        },
        |x| x.network_aliases = vec!["--rm".into()],
        |x| x.ip = Some("fd00::1".into()),
        |x| x.tmpfs = vec!["run".into()],
        |x| x.ulimits = vec!["nofile=lots".into()],
        |x| x.dns = vec!["--privileged".into()],
        |x| x.gpus = Some("--all".into()),
        |x| x.stop_signal = Some("-9".into()),
        |x| {
            x.health = Some(HealthSpec {
                cmd: "true".into(),
                interval: Some("soon".into()),
                timeout: None,
                retries: None,
                start_period: None,
            })
        },
    ];
    for (i, change) in bad.iter().enumerate() {
        let mut s = spec();
        let mut x = RunExtras::default();
        change(&mut x);
        s.extras = Some(x);
        assert!(run_args(&s, None).is_err(), "case {i}");
    }
}
