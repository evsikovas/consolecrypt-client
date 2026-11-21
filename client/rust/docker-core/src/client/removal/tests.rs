use super::*;
use crate::{DockerConnector, DockerIo};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn container() -> String {
    "a".repeat(64)
}
fn network() -> String {
    "d".repeat(64)
}

#[derive(Default, Debug)]
struct Engine {
    running: bool,
    auto: bool,
    removed: bool,
    deny_volume: bool,
    added_user: bool,
    new_volume_after_remove: bool,
    writes: Vec<String>,
}

impl Engine {
    fn response(&mut self, method: &str, path: &str) -> (u16, Value) {
        let id = container();
        let net = network();
        let volume = |name: &str, created: &str| {
            json!({
                "Name":name, "Driver":"local", "Scope":"local", "CreatedAt":created, "Labels":null,"Options":null
            })
        };
        let network = json!({"Name":"private-net","Id":net,"Driver":"bridge","Scope":"local","Labels":{},"Containers":{}});
        if method == "GET" {
            if path == format!("/containers/{id}/json") {
                if self.removed {
                    return (404, Value::Null);
                }
                return (
                    200,
                    json!({
                        "Id":id,"Name":"/example","Config":{"Labels":{}},"State":{"Running":self.running},
                        "HostConfig":{"AutoRemove":self.auto},"Mounts":[
                            {"Type":"bind","Source":"/private/host/path","Destination":"/config"},
                            {"Type":"volume","Name":"private-volume","Destination":"/data"},
                            {"Type":"volume","Name":"shared","Destination":"/backup"}],
                        "NetworkSettings":{"Networks":{"private-net":{"NetworkID":""},"bridge":{"NetworkID":"b".repeat(64)}}}
                    }),
                );
            }
            if path == "/containers/json?all=1" {
                let mut mounts = vec![json!({"Type":"volume","Name":"shared"})];
                if self.removed && self.added_user {
                    mounts.push(json!({"Type":"volume","Name":"private-volume"}));
                }
                let mut rows = vec![
                    json!({"Id":"e".repeat(64),"State":"exited","Mounts":mounts,"NetworkSettings":{"Networks":{}}}),
                ];
                if !self.removed {
                    rows.push(json!({"Id":id}));
                }
                return (200, Value::Array(rows));
            }
            if path == "/volumes" {
                return (
                    200,
                    json!({"Volumes":[volume("private-volume","original"),volume("shared","original")]}),
                );
            }
            if path == "/networks" {
                return (200, json!([network]));
            }
            if path == "/volumes/private-volume" {
                return (
                    200,
                    volume(
                        "private-volume",
                        if self.removed && self.new_volume_after_remove {
                            "replacement"
                        } else {
                            "original"
                        },
                    ),
                );
            }
            if path == format!("/networks/{net}") {
                return (200, network);
            }
        } else {
            self.writes.push(format!("{method} {path}"));
            if method == "POST" && path == format!("/containers/{id}/stop") {
                self.running = false;
                return (204, Value::Null);
            }
            if method == "DELETE" {
                if path == format!("/containers/{id}") && !self.running {
                    self.removed = true;
                    return (204, Value::Null);
                }
                if self.removed && path == "/volumes/private-volume" {
                    return (if self.deny_volume { 409 } else { 204 }, Value::Null);
                }
                if self.removed && path == format!("/networks/{net}") {
                    return (204, Value::Null);
                }
            }
        }
        (500, json!({"message":"unexpected test request"}))
    }
}

#[derive(Debug)]
struct TestSocket(Arc<Mutex<Engine>>);

#[async_trait]
impl DockerConnector for TestSocket {
    async fn connect(&self) -> Result<DockerIo, DockerError> {
        let (reader, writer) = tokio::io::duplex(65536);
        let state = self.0.clone();
        tokio::spawn(async move {
            let mut io = BufReader::new(writer);
            let mut line = String::new();
            io.read_line(&mut line).await.unwrap();
            let parts: Vec<_> = line.split_whitespace().map(str::to_owned).collect();
            if parts.len() < 2 {
                return;
            }
            loop {
                line.clear();
                if io.read_line(&mut line).await.unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let (status, body) = state.lock().unwrap().response(&parts[0], &parts[1]);
            let body = if status == 204 {
                String::new()
            } else {
                body.to_string()
            };
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = io.get_mut().write_all(response.as_bytes()).await;
        });
        Ok(Box::new(reader))
    }
}

async fn fixture(
    engine: Engine,
) -> (
    Arc<Mutex<Engine>>,
    DockerClient,
    DockerRemovalPlan,
    DockerRemovalSelection,
) {
    let state = Arc::new(Mutex::new(engine));
    let docker = DockerClient::new(Arc::new(TestSocket(state.clone())));
    let mut plan = docker.removal_plan(&container()).await.unwrap();
    plan.plan_id = "receipt".into();
    let selection = DockerRemovalSelection {
        plan_id: plan.plan_id.clone(),
        volumes: vec!["private-volume".into()],
        networks: vec![network()],
        stop_running: false,
        confirm_name: "example".into(),
    };
    (state, docker, plan, selection)
}

#[tokio::test]
async fn preview_protects_bind_builtin_and_stopped_sharer_and_resolves_created_network() {
    let (state, _, plan, _) = fixture(Engine::default()).await;
    for (name, reason) in [
        ("/config", "bind"),
        ("bridge", "default"),
        ("shared", "shared"),
        ("private-volume", "eligible"),
        ("private-net", "eligible"),
    ] {
        let r = plan.resources.iter().find(|r| r.name == name).unwrap();
        assert_eq!(r.reason, reason);
        assert_eq!(r.eligible, reason == "eligible");
        if name == "private-net" {
            assert_eq!(r.id, network());
        }
    }
    assert!(!serde_json::to_string(&plan)
        .unwrap()
        .contains("/private/host/path"));
    assert!(state.lock().unwrap().writes.is_empty());
}

#[tokio::test]
async fn deletes_exactly_opted_in_handles_each_conflict_and_never_forces() {
    let (state, docker, plan, mut selection) = fixture(Engine {
        running: true,
        deny_volume: true,
        ..Default::default()
    })
    .await;
    selection.stop_running = true;
    let result = docker
        .remove_with_cleanup(&plan, &selection, &CancellationToken::new())
        .await
        .unwrap();
    assert!(result.container_stopped && result.container_removed);
    assert_eq!(result.error, None);
    assert_eq!(
        result
            .resources
            .iter()
            .map(|r| (r.kind, r.status, r.reason))
            .collect::<Vec<_>>(),
        [
            ("volume", "skipped", "in_use"),
            ("network", "removed", "removed")
        ]
    );
    assert_eq!(
        state.lock().unwrap().writes,
        [
            format!("POST /containers/{}/stop", container()),
            format!("DELETE /containers/{}", container()),
            "DELETE /volumes/private-volume".to_string(),
            format!("DELETE /networks/{}", network())
        ]
    );
}

#[tokio::test]
async fn default_selection_preserves_every_resource() {
    let (state, docker, plan, mut selection) = fixture(Engine::default()).await;
    selection.volumes.clear();
    selection.networks.clear();
    let result = docker
        .remove_with_cleanup(&plan, &selection, &CancellationToken::new())
        .await
        .unwrap();
    assert!(result.container_removed && !result.container_stopped && result.resources.is_empty());
    assert_eq!(
        state.lock().unwrap().writes,
        [format!("DELETE /containers/{}", container())]
    );
}

#[tokio::test]
async fn rejects_injected_selection_wrong_typed_name_or_wrong_receipt_without_side_effects() {
    let (state, docker, plan, selection) = fixture(Engine::default()).await;
    for change in ["unknown", "shared", "duplicate", "name", "receipt"] {
        let mut pick = selection.clone();
        match change {
            "name" => pick.confirm_name = "other".into(),
            "receipt" => pick.plan_id = "stale".into(),
            "duplicate" => pick.volumes.push("private-volume".into()),
            name => pick.volumes.push(name.into()),
        }
        assert!(matches!(
            docker
                .remove_with_cleanup(&plan, &pick, &CancellationToken::new())
                .await,
            Err(DockerError::InvalidRequest)
        ));
    }
    assert!(state.lock().unwrap().writes.is_empty());
}

#[tokio::test]
async fn no_stop_or_auto_remove_or_newly_running_requires_review() {
    for (preview_running, auto, stop, reason) in [
        (true, false, false, "running"),
        (true, true, true, "auto_remove"),
        (false, false, true, "running"),
    ] {
        let (state, docker, plan, mut selection) = fixture(Engine {
            running: preview_running,
            auto,
            ..Default::default()
        })
        .await;
        state.lock().unwrap().running = true;
        selection.stop_running = stop;
        let result = docker
            .remove_with_cleanup(&plan, &selection, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.error, Some(reason));
        assert!(!result.container_removed && state.lock().unwrap().writes.is_empty());
    }
}

#[tokio::test]
async fn volume_replaced_or_claimed_after_container_deletion_is_preserved() {
    for (new_volume_after_remove, added_user, reason) in
        [(true, false, "changed"), (false, true, "shared")]
    {
        let (state, docker, plan, mut selection) = fixture(Engine {
            new_volume_after_remove,
            added_user,
            ..Default::default()
        })
        .await;
        selection.networks.clear();
        let result = docker
            .remove_with_cleanup(&plan, &selection, &CancellationToken::new())
            .await
            .unwrap();
        assert!(result.container_removed);
        assert_eq!(
            (result.resources[0].status, result.resources[0].reason),
            ("skipped", reason)
        );
        assert_eq!(
            state.lock().unwrap().writes,
            [format!("DELETE /containers/{}", container())]
        );
    }
}

#[test]
fn protects_external_managed_volume_and_unknown_ownership() {
    let volume = json!({"Name":"data","Driver":"local","Scope":"local","CreatedAt":"observed","Options":{},"Labels":{}});
    for (labels, expected) in [
        (
            json!({"com.docker.compose.project":"app","com.docker.compose.volume":"db"}),
            "eligible",
        ),
        (Value::Null, "external"),
        (
            json!({"com.docker.compose.project":"other","com.docker.compose.volume":"db"}),
            "external",
        ),
        (json!({"com.docker.swarm.service.name":"db"}), "external"),
    ] {
        let mut object = volume.clone();
        object["Labels"] = labels;
        let mut r = reference("volume", "data", "data", "unknown");
        classify(&mut r, &object, Some("app"));
        assert_eq!(r.reason, expected);
    }
    for missing in ["Labels", "Options"] {
        let mut incomplete = volume.clone();
        incomplete.as_object_mut().unwrap().remove(missing);
        let mut r = reference("volume", "data", "data", "unknown");
        classify(&mut r, &incomplete, None);
        assert_eq!(r.reason, "unknown");
    }
    let mut object = volume.clone();
    object["Options"] = json!({"type":"none","o":"bind","device":"/external"});
    let mut r = reference("volume", "data", "data", "unknown");
    classify(&mut r, &object, None);
    assert!(!r.eligible);
    assert_eq!(
        usage(&json!([{"Id":"e".repeat(64)}]), &container(), &r),
        "unknown"
    );
}
