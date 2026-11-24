use super::*;
use crate::{DockerConnector, DockerIo};
use async_trait::async_trait;
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn normalize_only_registry_repositories_and_bound_digests_to_exact_image() {
    let id = format!("sha256:{}", "1".repeat(64));
    let manifest = format!("sha256:{}", "2".repeat(64));
    assert_eq!(
        repository("python:3").as_deref(),
        Some("docker.io/library/python")
    );
    assert_eq!(
        repository("index.docker.io:443/team/api:3").as_deref(),
        Some("docker.io/team/api")
    );
    assert_eq!(
        repository("registry:5000/team/api:1").as_deref(),
        Some("registry:5000/team/api")
    );
    for invalid in [
        "localhost:5000/api:2",
        "localhost/api",
        "127.0.0.1/api",
        "user:password@host/foo",
        "https://host/foo",
        "a/../b",
        "a?x",
        "host:60000/team/api?secret",
        "docker.io/path#suffix",
    ] {
        assert!(repository(invalid).is_none(), "{invalid}");
    }
    assert!(sha256_id(&id));
    assert!(!sha256_id(&format!("{id}?arg=1")));
    let public = format!("alpine@{manifest}");
    let image = json!({"Id":id,"RepoTags":["local:local"],"RepoDigests":[public,public,format!("unrelated/base@{manifest}")],
        "Config":{"Env":["SECRET=fixture"]}});
    assert_eq!(
        candidates(&image, &id, "docker.io/alpine:3").unwrap(),
        [format!("docker.io/library/alpine@{manifest}")]
    );
    assert!(candidates(&image, &manifest, "alpine:3").is_err());
    assert!(candidates(&image, &id, "local:local").unwrap().is_empty());
    assert!(candidates(&image, &id, &format!("alpine@{id}"))
        .unwrap()
        .is_empty());
}

#[derive(Debug)]
struct Engine {
    id: String,
    manifest: String,
    status: u16,
    reported: String,
    calls: AtomicUsize,
}

#[async_trait]
impl DockerConnector for Engine {
    async fn connect(&self) -> Result<DockerIo, DockerError> {
        let (client, mut peer) = tokio::io::duplex(8192);
        let id = self.id.clone();
        let manifest = self.manifest.clone();
        let status = self.status;
        let reported = self.reported.clone();
        // This test server handles one request per connection in reply order.
        let n = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        tokio::spawn(async move {
            let mut stream = BufReader::new(&mut peer);
            let mut first = String::new();
            stream.read_line(&mut first).await.unwrap();
            let mut header = String::new();
            loop {
                header.clear();
                if stream.read_line(&mut header).await.unwrap() == 0 || header == "\r\n" {
                    break;
                }
            }
            let path = first.split_whitespace().nth(1).unwrap_or("");
            let (code, body) = if path == "/images/json?digests=true" {
                (
                    200,
                    json!([{"Id":id,"RepoTags":["demo:test"],"RepoDigests":[],"Size":65,"Created":99,"Labels":{"fixture":"private"}}]),
                )
            } else if path == "/images/demo:test/json" {
                (200, json!({"Id":reported}))
            } else if path == "/images/demo:test?force=false&noprune=true" {
                assert!(first.starts_with("DELETE "));
                (status, json!([]))
            } else if n == 1 {
                assert_eq!(path, format!("/images/{id}/json"));
                (
                    200,
                    json!({"Id":id, "RepoDigests":[format!("unrelated/base@{manifest}"),format!("local-stateless-api@{manifest}")]}),
                )
            } else {
                assert_eq!(
                    path,
                    format!("/distribution/docker.io/library/local-stateless-api@{manifest}/json")
                );
                (status, json!({"Descriptor":{"digest":reported}}))
            };
            let bytes = body.to_string();
            let response = format!(
                "HTTP/1.1 {code} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{bytes}",
                bytes.len()
            );
            stream
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .unwrap();
        });
        Ok(Box::new(client))
    }
}

#[tokio::test]
async fn local_containerd_digest_needs_a_matching_registry_descriptor_before_prefill() {
    let id = format!("sha256:{}", "1".repeat(64));
    let digest = format!("sha256:{}", "a".repeat(64));
    for (source, status, reported, expected, reads) in [
        ("local-stateless-api:local", 401, digest.as_str(), false, 2),
        ("local-stateless-api:local", 404, digest.as_str(), false, 2),
        ("local-stateless-api:local", 200, id.as_str(), false, 2),
        ("local-stateless-api:local", 200, digest.as_str(), true, 2),
        (
            "unlisted.example/team/api:1",
            200,
            digest.as_str(),
            false,
            1,
        ),
    ] {
        let engine = Arc::new(Engine {
            id: id.clone(),
            manifest: digest.clone(),
            status,
            reported: reported.into(),
            calls: AtomicUsize::default(),
        });
        let client = DockerClient::new(engine.clone());
        let resolved = client.image_repo_digests(&id, source).await.unwrap();
        assert_eq!(
            resolved,
            if expected {
                vec![format!("docker.io/library/local-stateless-api@{digest}")]
            } else {
                vec![]
            }
        );
        assert_eq!(engine.calls.load(Ordering::Relaxed), reads);
    }
}

#[tokio::test]
async fn image_inventory_and_removal_use_exact_references_without_force() {
    let id = format!("sha256:{}", "a".repeat(64));
    for (reported, status, outcome, calls) in [
        (id.clone(), 200, Some(true), 3),
        (id.clone(), 409, Some(false), 3),
        (format!("sha256:{}", "b".repeat(64)), 200, None, 2),
    ] {
        let engine = Arc::new(Engine {
            id: id.clone(),
            manifest: String::new(),
            status,
            reported,
            calls: AtomicUsize::new(0),
        });
        let client = DockerClient::new(engine.clone());
        let rows = client.images().await.unwrap();
        assert_eq!(rows[0]["size"], 65);
        assert!(rows[0].get("Labels").is_none());
        assert!(client
            .remove_image(&id, Some("demo:test?force=true"))
            .await
            .is_err());
        assert_eq!(
            client.remove_image(&id, Some("demo:test")).await.ok(),
            outcome
        );
        assert_eq!(engine.calls.load(Ordering::Relaxed), calls);
    }
}

#[tokio::test]
#[ignore = "requires local Docker Engine, local alpine:3.20 image and Docker Hub"]
async fn existing_alpine_manifest_is_verified_by_the_local_engine_without_a_pull() {
    let id =
        std::env::var("CC_DOCKER_PROBE_ALPINE_ID").expect("supply the ID of local alpine:3.20");
    let mut engines = crate::local::local_engines().into_iter();
    let endpoint = engines
        .next()
        .expect("a discovered local Docker endpoint")
        .endpoint;
    let docker = DockerClient::new(crate::local::connector(&endpoint).unwrap());
    let refs = docker.image_repo_digests(&id, "alpine:3.20").await.unwrap();
    assert_eq!(
        refs.len(),
        1,
        "Docker Engine must reach the Alpine registry"
    );
    assert!(refs[0].starts_with("docker.io/library/alpine@sha256:"));
}
