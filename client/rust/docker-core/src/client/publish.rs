use super::{image::sha256_id, DockerClient, REQUEST_TIMEOUT};
use crate::DockerError;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use http_body_util::BodyExt;
use hyper::{header::HeaderValue, Method};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::{net::Ipv4Addr, sync::LazyLock, time::Duration};
use zeroize::Zeroizing;

static AUTHORITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
    r"^([a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*)(?::([0-9]{1,5}))?$"
).unwrap()
});
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[a-z0-9]+(?:(?:[._]|__|-+)[a-z0-9]+)*(?:/[a-z0-9]+(?:(?:[._]|__|-+)[a-z0-9]+)*)*$",
    )
    .unwrap()
});
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_][a-zA-Z0-9._-]{0,127}$").unwrap());

pub fn registry_authority(input: &str) -> Option<String> {
    let address = input.trim().to_ascii_lowercase();
    let address = address
        .strip_prefix("https://")
        .unwrap_or(&address)
        .trim_end_matches('/');
    let m = AUTHORITY.captures(address)?;
    let host = &m[1];
    let port = m.get(2).map(|p| p.as_str());
    if host.len() > 253
        || (!host.contains('.') && port.is_none())
        || host == "localhost"
        || host.ends_with(".localhost")
        || host
            .parse::<Ipv4Addr>()
            .is_ok_and(|a| a.is_loopback() || a.is_unspecified())
        || port.is_some_and(|s| s.parse::<u16>().unwrap_or(0) == 0)
    {
        return None;
    }
    if matches!(
        host,
        "docker.io" | "registry-1.docker.io" | "index.docker.io"
    ) && matches!(port, None | Some("443"))
    {
        return Some("docker.io".into());
    }
    Some(address.to_owned())
}

fn pushed_digest(line: &[u8]) -> Result<Option<String>, DockerError> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let event: Value = serde_json::from_slice(line).map_err(|_| DockerError::Protocol)?;
    if event.get("error").is_some() || event.get("errorDetail").is_some() {
        return Err(DockerError::Api {
            status: 400,
            message: String::new(),
        });
    }
    let digest = event["aux"]["Digest"].as_str().or_else(|| {
        let (_, after) = event["status"].as_str()?.split_once(": digest: ")?;
        after.split_whitespace().next()
    });
    match digest {
        Some(d) if sha256_id(d) => Ok(Some(d.to_owned())),
        Some(_) => Err(DockerError::Protocol),
        None => Ok(None),
    }
}

impl DockerClient {
    /// Tag by immutable source ID on this engine, then stream its push to completion.
    /// The registry comes from the saved credential, independent of the editable path.
    pub async fn publish_image(
        &self,
        image_id: &str,
        registry: &str,
        path: &str,
        tag: &str,
        username: &str,
        password: &str,
    ) -> Result<String, DockerError> {
        let registry = registry_authority(registry).ok_or(DockerError::InvalidRequest)?;
        let repo = format!("{registry}/{path}");
        if !sha256_id(image_id)
            || repo.len() > 255
            || !PATH.is_match(path)
            || !TAG.is_match(tag)
            || username.is_empty()
            || password.is_empty()
            || registry == "docker.io" && !path.contains('/')
        {
            return Err(DockerError::InvalidRequest);
        }
        #[derive(Serialize)]
        struct Auth<'a> {
            username: &'a str,
            password: &'a str,
            serveraddress: &'a str,
        }
        let json = Zeroizing::new(
            serde_json::to_vec(&Auth {
                username,
                password,
                serveraddress: if registry == "docker.io" {
                    "https://index.docker.io/v1/"
                } else {
                    &registry
                },
            })
            .map_err(|_| DockerError::InvalidRequest)?,
        );
        let encoded = Zeroizing::new(URL_SAFE.encode(&*json));
        let mut auth = HeaderValue::from_str(&encoded).map_err(|_| DockerError::InvalidRequest)?;
        auth.set_sensitive(true);

        self.json(
            Method::POST,
            &format!("/images/{image_id}/tag?repo={repo}&tag={tag}"),
            None,
        )
        .await?;
        let push = format!("/images/{repo}/push?tag={tag}");
        let response = tokio::time::timeout(
            REQUEST_TIMEOUT,
            self.send_with_registry_auth(Method::POST, &push, None, false, Some(auth)),
        )
        .await
        .map_err(|_| DockerError::Connection)??;
        let mut body = response.into_body();
        let mut pending = Vec::new();
        let mut digest = None;
        while let Some(frame) = tokio::time::timeout(Duration::from_secs(300), body.frame())
            .await
            .map_err(|_| DockerError::Connection)?
        {
            let frame = frame.map_err(|_| DockerError::Connection)?;
            let Some(bytes) = frame.data_ref() else {
                continue;
            };
            for part in bytes.split_inclusive(|b| *b == b'\n') {
                if pending.len() + part.len() > 65536 {
                    return Err(DockerError::Protocol);
                }
                pending.extend_from_slice(part);
                if part.ends_with(b"\n") {
                    if let Some(d) = pushed_digest(&pending)? {
                        digest = Some(d);
                    }
                    pending.clear();
                }
            }
        }
        if let Some(d) = pushed_digest(&pending)? {
            digest = Some(d);
        }
        let digest = digest.ok_or(DockerError::Protocol)?;
        // If another publisher moved the local tag during the push, ask the user to retry.
        let tagged = self
            .json(Method::GET, &format!("/images/{repo}:{tag}/json"), None)
            .await?;
        if tagged["Id"].as_str() != Some(image_id) {
            return Err(DockerError::Protocol);
        }
        Ok(format!("{repo}@{digest}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authority_and_result_are_bound_to_the_registry() {
        assert_eq!(
            registry_authority("https://INDEX.DOCKER.IO:443/").as_deref(),
            Some("docker.io")
        );
        for invalid in [
            "127.0.0.1:5000",
            "hub",
            "repo.example.org/path",
            "user@repo.example.com",
            "http://evil.test",
            "repo.test:65536",
        ] {
            assert!(registry_authority(invalid).is_none());
        }
        assert!(!PATH.is_match("../foreign:tag"));
        assert!(!TAG.is_match("a?tag=b"));
        assert!(pushed_digest(br#"{"error":"unauthorized"}"#).is_err());
        let digest = format!("sha256:{}", "a".repeat(64));
        for input in [
            serde_json::json!({"aux":{"Digest":digest}}),
            serde_json::json!({"status":format!("v1: digest: {digest} size: 12")} ),
        ] {
            assert_eq!(
                pushed_digest(input.to_string().as_bytes()).unwrap(),
                Some(digest.clone())
            );
        }
    }

    #[derive(Debug)]
    struct RegistryEngine {
        push_fails: bool,
        retagged: bool,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl crate::DockerConnector for RegistryEngine {
        async fn connect(&self) -> Result<crate::DockerIo, DockerError> {
            use std::sync::atomic::Ordering;
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let (client, peer) = tokio::io::duplex(8192);
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            let (push_fails, retagged) = (self.push_fails, self.retagged);
            tokio::spawn(async move {
                let id = format!("sha256:{}", "1".repeat(64));
                let digest = format!("sha256:{}", "a".repeat(64));
                let mut stream = BufReader::new(peer);
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                let request = line.trim().to_owned();
                let mut auth = None;
                loop {
                    line.clear();
                    if stream.read_line(&mut line).await.unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':') {
                        if key.eq_ignore_ascii_case("x-registry-auth") {
                            auth = Some(value.trim().to_string());
                        }
                    }
                }
                let (code, body) = match index {
                    0 => {
                        assert_eq!(request, format!("POST /images/{id}/tag?repo=registry.example/team/app&tag=v1 HTTP/1.1"));
                        assert!(auth.is_none());
                        (201, String::new())
                    }
                    1 => {
                        assert_eq!(
                            request,
                            "POST /images/registry.example/team/app/push?tag=v1 HTTP/1.1"
                        );
                        let data: Value =
                            serde_json::from_slice(&URL_SAFE.decode(auth.unwrap()).unwrap())
                                .unwrap();
                        assert_eq!(
                            data,
                            serde_json::json!({"username":"builder","password":"fixture","serveraddress":"registry.example"})
                        );
                        let mut events = format!(
                            "{{\"status\":\"Pushing\"}}\n{{\"aux\":{{\"Digest\":\"{digest}\"}}}}\n"
                        );
                        if push_fails {
                            events.push_str("{\"errorDetail\":{\"message\":\"denied\"}}\n");
                        }
                        (200, events)
                    }
                    2 => {
                        assert_eq!(
                            request,
                            "GET /images/registry.example/team/app:v1/json HTTP/1.1"
                        );
                        assert!(auth.is_none());
                        (
                            200,
                            serde_json::json!({"Id":if retagged {&digest} else {&id}}).to_string(),
                        )
                    }
                    _ => panic!("unexpected request"),
                };
                let headers = format!(
                    "HTTP/1.1 {code} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .get_mut()
                    .write_all(headers.as_bytes())
                    .await
                    .unwrap();
                for part in body.as_bytes().chunks(17) {
                    stream.get_mut().write_all(part).await.unwrap();
                    tokio::task::yield_now().await;
                }
            });
            Ok(Box::new(client))
        }
    }

    #[tokio::test]
    async fn authenticates_only_the_push_and_returns_only_a_confirmed_digest() {
        use std::sync::{atomic::Ordering, Arc};
        for (fail, retagged, calls) in [(false, false, 3), (true, false, 2), (false, true, 3)] {
            let engine = Arc::new(RegistryEngine {
                push_fails: fail,
                retagged,
                calls: Default::default(),
            });
            let result = DockerClient::new(engine.clone())
                .publish_image(
                    &format!("sha256:{}", "1".repeat(64)),
                    "registry.example",
                    "team/app",
                    "v1",
                    "builder",
                    "fixture",
                )
                .await;
            if fail || retagged {
                assert!(result.is_err());
            } else {
                assert_eq!(
                    result.unwrap(),
                    format!("registry.example/team/app@sha256:{}", "a".repeat(64))
                );
            }
            assert_eq!(engine.calls.load(Ordering::SeqCst), calls);
        }
    }
}
