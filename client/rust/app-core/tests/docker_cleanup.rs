//! Opt-in acceptance test for selective removal against the local Docker Engine.
mod common;

use cc_app_core::{DockerConnectOutcome, DockerRemovalSelection};
use std::process::{Command, Output};

const IMAGE: &str = "alpine:3.20";

fn run(args: &[&str]) -> Output {
    Command::new("docker")
        .args(args)
        .output()
        .expect("docker CLI")
}

fn docker(args: &[&str]) -> String {
    let result = run(args);
    assert!(
        result.status.success(),
        "docker {}: {}",
        args[0],
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    project: String,
    volumes: Vec<String>,
    networks: Vec<String>,
    containers: Vec<String>,
}

impl Fixture {
    fn new() -> Self {
        let endpoint = std::env::var("DOCKER_HOST")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                docker(&[
                    "context",
                    "inspect",
                    "--format",
                    "{{.Endpoints.docker.Host}}",
                ])
            });
        assert!(
            endpoint.starts_with("unix://") || endpoint.starts_with("npipe://"),
            "local Docker required"
        );
        Self {
            project: format!("cc-cleanup-{}", uuid::Uuid::new_v4().simple()),
            volumes: vec![],
            networks: vec![],
            containers: vec![],
        }
    }

    fn volume(&mut self, suffix: &str) -> String {
        let name = format!("{}-{suffix}", self.project);
        let vol = docker(&[
            "volume",
            "create",
            "--label",
            &format!("com.docker.compose.project={}", self.project),
            "--label",
            &format!("com.docker.compose.volume={suffix}"),
            &name,
        ]);
        assert_eq!(vol, name);
        self.volumes.push(name.clone());
        name
    }

    fn network(&mut self, suffix: &str) -> (String, String) {
        let name = format!("{}-{suffix}", self.project);
        let id = docker(&[
            "network",
            "create",
            "--label",
            &format!("com.docker.compose.project={}", self.project),
            "--label",
            &format!("com.docker.compose.network={suffix}"),
            &name,
        ]);
        self.networks.push(id.clone());
        (name, id)
    }

    fn container(&mut self, suffix: &str, network: &str, volumes: &[&str]) -> (String, String) {
        let name = format!("{}-{suffix}", self.project);
        let project = format!("com.docker.compose.project={}", self.project);
        let service = format!("com.docker.compose.service={suffix}");
        let mut args = vec![
            "create",
            "--name",
            &name,
            "--network",
            network,
            "--label",
            &project,
            "--label",
            &service,
        ];
        let mounts: Vec<_> = volumes
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{v}:/data/{i}"))
            .collect();
        for mount in &mounts {
            args.extend(["-v", mount]);
        }
        args.extend([IMAGE, "sleep", "600"]);
        let id = docker(&args);
        self.containers.push(id.clone());
        (name, id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for id in self.containers.iter().rev() {
            let _ = run(&["container", "rm", "-f", id]);
        }
        for id in &self.networks {
            let _ = run(&["network", "rm", id]);
        }
        for name in &self.volumes {
            let check = run(&[
                "volume",
                "inspect",
                "--format",
                "{{index .Labels \"com.docker.compose.project\"}}",
                name,
            ]);
            if check.status.success()
                && String::from_utf8_lossy(&check.stdout).trim() == self.project
            {
                let _ = run(&["volume", "rm", name]);
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a local Docker Engine and alpine:3.20"]
async fn delete_only_selected_container_resources_and_preserve_stopped_neighbors() {
    let mut f = Fixture::new();
    let selected = f.volume("selected");
    let keep = f.volume("keep");
    let shared = f.volume("shared");
    let (private_net, private_id) = f.network("private");
    let (shared_net, shared_id) = f.network("shared");
    let (name, target) = f.container("target", &private_net, &[&selected, &keep, &shared]);
    let (sibling_name, sibling) = f.container("neighbor", &shared_net, &[&shared]);
    docker(&["start", &sibling]);
    docker(&["stop", "--time", "1", &sibling]);
    docker(&["start", &target]);
    docker(&["network", "connect", &shared_id, &target]);

    let dir = tempfile::tempdir().unwrap();
    let app = common::app(dir.path());
    app.create_local_profile(
        "Docker cleanup acceptance".into(),
        common::PASSPHRASE.into(),
    )
    .await
    .unwrap();
    let DockerConnectOutcome::Connected(s) = app.docker_connect("local".into()).await.unwrap()
    else {
        panic!("local Docker session required");
    };
    let plan = app
        .docker_removal_plan(s.id.clone(), target.clone())
        .await
        .unwrap();
    assert!(plan.running);
    assert_eq!(plan.container_id, target);
    let resource = |kind: &str, name: &str| {
        plan.resources
            .iter()
            .find(|r| r.kind == kind && r.name == name)
            .unwrap()
    };
    assert!(resource("volume", &selected).eligible);
    assert!(resource("volume", &keep).eligible);
    assert!(!resource("volume", &shared).eligible);
    assert!(resource("network", &private_net).eligible);
    assert!(!resource("network", &shared_net).eligible);
    let selection = DockerRemovalSelection {
        plan_id: plan.plan_id.clone(),
        confirm_name: name,
        stop_running: true,
        volumes: vec![resource("volume", &selected).id.clone()],
        networks: vec![resource("network", &private_net).id.clone()],
    };
    let result = app
        .docker_remove_with_cleanup(s.id.clone(), target.clone(), selection)
        .await
        .unwrap();
    assert!(
        result.container_removed && result.error.is_none(),
        "{result:?}"
    );
    assert!(!run(&["container", "inspect", &target]).status.success());
    assert!(!run(&["volume", "inspect", &selected]).status.success());
    assert!(!run(&["network", "inspect", &private_id]).status.success());
    assert!(run(&["volume", "inspect", &keep]).status.success());
    assert!(run(&["container", "inspect", &sibling]).status.success());
    assert!(run(&["network", "inspect", &shared_id]).status.success());

    let plan = app
        .docker_removal_plan(s.id.clone(), sibling.clone())
        .await
        .unwrap();
    assert!(!plan.running);
    let result = app
        .docker_remove_with_cleanup(
            s.id.clone(),
            sibling,
            DockerRemovalSelection {
                plan_id: plan.plan_id,
                confirm_name: sibling_name,
                stop_running: false,
                volumes: vec![],
                networks: vec![],
            },
        )
        .await
        .unwrap();
    assert!(
        result.container_removed && result.error.is_none(),
        "{result:?}"
    );
    assert!(run(&["volume", "inspect", &shared]).status.success());
    assert!(run(&["network", "inspect", &shared_id]).status.success());
    assert!(run(&["image", "inspect", IMAGE]).status.success());
    app.docker_disconnect(s.id).await.unwrap();
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a local Docker Engine, alpine:3.20 and the SSH integration testbed"]
async fn clean_up_a_stopped_container_on_a_saved_ssh_host() {
    use cc_models::host::HostKeyPolicy;
    use cc_ssh_core::testing::SshTestbed;
    use secrecy::ExposeSecret;

    let mut fixture = Fixture::new();
    let volume = fixture.volume("data");
    let (network, network_id) = fixture.network("isolated");
    let (name, target) = fixture.container("ssh-target", &network, &[&volume]);
    docker(&["start", &target]);
    docker(&["stop", "--time", "1", &target]);

    let host = tokio::task::spawn_blocking(SshTestbed::start_docker_host)
        .await
        .unwrap()
        .unwrap();
    host.exec_in(&host.bastion1, "addgroup tester docker")
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let app = common::app(dir.path());
    app.create_local_profile("SSH acceptance".into(), common::PASSPHRASE.into())
        .await
        .unwrap();
    let key = app
        .import_ssh_key(
            "Fixture".into(),
            Some(host.secrets.user.clone()),
            host.secrets.key.private_openssh.expose_secret().to_owned(),
            None,
            false,
            None,
        )
        .await
        .unwrap();
    let mut remote = common::host("Fixture", "127.0.0.1", Some(&key.id));
    remote.port = Some(host.bastion1_port);
    remote.host_key_policy = HostKeyPolicy::AcceptNew;
    let saved = app.save_host(remote).await.unwrap();
    let DockerConnectOutcome::Connected(session) = app.docker_connect(saved.id).await.unwrap()
    else {
        panic!("test SSH Docker session required");
    };
    let plan = app
        .docker_removal_plan(session.id.clone(), target.clone())
        .await
        .unwrap();
    assert!(!plan.running);
    assert!(plan
        .resources
        .iter()
        .any(|r| r.kind == "network" && r.id == network_id && r.eligible));
    let selection = DockerRemovalSelection {
        plan_id: plan.plan_id,
        confirm_name: name,
        volumes: vec![volume.clone()],
        networks: vec![network_id.clone()],
        stop_running: false,
    };
    let receipt = app
        .docker_remove_with_cleanup(session.id.clone(), target.clone(), selection.clone())
        .await
        .unwrap();
    assert!(
        receipt.container_removed && receipt.error.is_none(),
        "{receipt:?}"
    );
    assert!(app
        .docker_remove_with_cleanup(session.id.clone(), target.clone(), selection)
        .await
        .is_err());
    assert!(!run(&["container", "inspect", &target]).status.success());
    assert!(!run(&["volume", "inspect", &volume]).status.success());
    assert!(!run(&["network", "inspect", &network_id]).status.success());
    app.docker_disconnect(session.id).await.unwrap();
    app.shutdown().await.unwrap();
}
