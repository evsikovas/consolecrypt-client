use crate::{logs::LogDecoder, ContainerAction, DockerError, DockerIo, SharedConnector};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    body::Incoming,
    header::{CONNECTION, CONTENT_TYPE, HOST, UPGRADE},
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fmt, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct EngineInfo {
    pub engine_version: String,
    pub api_version: String,
    pub os: String,
    pub arch: String,
    pub containers_running: u64,
    pub containers_total: u64,
    pub images: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortMapping {
    pub private_port: u16,
    pub public_port: Option<u16>,
    pub protocol: String,
    pub ip: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerSummary {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub created: i64,
    pub ports: Vec<PortMapping>,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    /// Network name → container IP.
    pub networks: BTreeMap<String, String>,
    pub health: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogLine {
    pub stream: &'static str,
    pub text: String,
    pub timestamp: Option<String>,
}

/// An interactive exec: the hijacked stream carries terminal bytes both ways.
pub struct ExecSession {
    pub exec_id: String,
    pub io: DockerIo,
}

impl fmt::Debug for ExecSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecSession")
            .field("exec_id", &self.exec_id)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct DockerClient {
    connector: SharedConnector,
}

/// Container ids and names only: no path or query characters.
fn checked(id: &str) -> Result<&str, DockerError> {
    let valid = !id.is_empty()
        && id.len() <= 128
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    if valid {
        Ok(id)
    } else {
        Err(DockerError::InvalidRequest)
    }
}

fn health(status: &str) -> Option<String> {
    if status.contains("(healthy)") {
        Some("healthy".into())
    } else if status.contains("(unhealthy)") {
        Some("unhealthy".into())
    } else if status.contains("health: starting") {
        Some("starting".into())
    } else {
        None
    }
}

fn mask_env(inspect: &mut Value) {
    if let Some(env) = inspect
        .pointer_mut("/Config/Env")
        .and_then(Value::as_array_mut)
    {
        for entry in env {
            if let Some(text) = entry.as_str() {
                if let Some((key, value)) = text.split_once('=') {
                    if crate::sensitive_key(key) && !value.is_empty() {
                        *entry = Value::String(format!("{key}=••••••"));
                    }
                }
            }
        }
    }
}

impl DockerClient {
    pub fn new(connector: SharedConnector) -> Self {
        Self { connector }
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        upgrade: bool,
    ) -> Result<Response<Incoming>, DockerError> {
        let io = self.connector.connect().await?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(io))
            .await
            .map_err(|_| DockerError::Connection)?;
        tokio::spawn(async move {
            let _ = connection.with_upgrades().await;
        });
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, "docker");
        if upgrade {
            request = request.header(CONNECTION, "Upgrade").header(UPGRADE, "tcp");
        }
        let body = match body {
            Some(value) => {
                request = request.header(CONTENT_TYPE, "application/json");
                Full::new(Bytes::from(value.to_string()))
            }
            None => Full::new(Bytes::new()),
        };
        let request = request
            .body(body)
            .map_err(|_| DockerError::InvalidRequest)?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| DockerError::Connection)?;
        let status = response.status();
        if status.is_success() || status == StatusCode::SWITCHING_PROTOCOLS {
            return Ok(response);
        }
        let bytes = collect(response).await.unwrap_or_default();
        let message = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| value["message"].as_str().map(str::to_owned))
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        Err(match status {
            StatusCode::NOT_FOUND => DockerError::NotFound,
            _ => DockerError::Api {
                status: status.as_u16(),
                message,
            },
        })
    }

    async fn json(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, DockerError> {
        let response = tokio::time::timeout(REQUEST_TIMEOUT, self.send(method, path, body, false))
            .await
            .map_err(|_| DockerError::Connection)??;
        let bytes = tokio::time::timeout(REQUEST_TIMEOUT, collect(response))
            .await
            .map_err(|_| DockerError::Connection)??;
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| DockerError::Protocol)
    }

    pub async fn info(&self) -> Result<EngineInfo, DockerError> {
        let version = self.json(Method::GET, "/version", None).await?;
        let info = self.json(Method::GET, "/info", None).await?;
        let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_owned();
        Ok(EngineInfo {
            engine_version: text(&version, "Version"),
            api_version: text(&version, "ApiVersion"),
            os: text(&version, "Os"),
            arch: text(&version, "Arch"),
            containers_running: info["ContainersRunning"].as_u64().unwrap_or(0),
            containers_total: info["Containers"].as_u64().unwrap_or(0),
            images: info["Images"].as_u64().unwrap_or(0),
        })
    }

    pub async fn containers(&self, all: bool) -> Result<Vec<ContainerSummary>, DockerError> {
        let path = if all {
            "/containers/json?all=1"
        } else {
            "/containers/json"
        };
        let list = self.json(Method::GET, path, None).await?;
        let items = list.as_array().ok_or(DockerError::Protocol)?;
        Ok(items
            .iter()
            .map(|item| {
                let label = |key: &str| item["Labels"][key].as_str().map(str::to_owned);
                let status = item["Status"].as_str().unwrap_or_default().to_owned();
                let mut ports: Vec<PortMapping> = item["Ports"]
                    .as_array()
                    .map(|ports| {
                        ports
                            .iter()
                            .filter_map(|port| {
                                Some(PortMapping {
                                    private_port: u16::try_from(port["PrivatePort"].as_u64()?)
                                        .ok()?,
                                    public_port: port["PublicPort"]
                                        .as_u64()
                                        .and_then(|p| u16::try_from(p).ok()),
                                    protocol: port["Type"].as_str().unwrap_or("tcp").to_owned(),
                                    ip: port["IP"].as_str().map(str::to_owned),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                // IPv4 and IPv6 bindings of one port read as one mapping.
                ports.sort_by_key(|port| (port.private_port, port.public_port, port.ip.clone()));
                ports.dedup_by(|a, b| {
                    a.private_port == b.private_port
                        && a.public_port == b.public_port
                        && a.protocol == b.protocol
                });
                let networks = item["NetworkSettings"]["Networks"]
                    .as_object()
                    .map(|networks| {
                        networks
                            .iter()
                            .filter_map(|(name, network)| {
                                let ip =
                                    network["IPAddress"].as_str().filter(|ip| !ip.is_empty())?;
                                Some((name.clone(), ip.to_owned()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                ContainerSummary {
                    id: item["Id"].as_str().unwrap_or_default().to_owned(),
                    name: item["Names"][0]
                        .as_str()
                        .unwrap_or_default()
                        .trim_start_matches('/')
                        .to_owned(),
                    image: item["Image"].as_str().unwrap_or_default().to_owned(),
                    state: item["State"].as_str().unwrap_or("unknown").to_owned(),
                    health: health(&status),
                    status,
                    created: item["Created"].as_i64().unwrap_or(0),
                    ports,
                    compose_project: label("com.docker.compose.project"),
                    compose_service: label("com.docker.compose.service"),
                    networks,
                }
            })
            .collect())
    }

    /// `docker inspect` with secret-looking environment values hidden.
    pub async fn inspect(&self, id: &str) -> Result<Value, DockerError> {
        let mut value = self
            .json(
                Method::GET,
                &format!("/containers/{}/json", checked(id)?),
                None,
            )
            .await?;
        mask_env(&mut value);
        Ok(value)
    }

    pub async fn action(&self, id: &str, action: ContainerAction) -> Result<(), DockerError> {
        let id = checked(id)?;
        let (method, path) = match action {
            ContainerAction::Start => (Method::POST, format!("/containers/{id}/start")),
            ContainerAction::Stop => (Method::POST, format!("/containers/{id}/stop")),
            ContainerAction::Restart => (Method::POST, format!("/containers/{id}/restart")),
            ContainerAction::Pause => (Method::POST, format!("/containers/{id}/pause")),
            ContainerAction::Unpause => (Method::POST, format!("/containers/{id}/unpause")),
            ContainerAction::Kill => (Method::POST, format!("/containers/{id}/kill")),
            ContainerAction::Remove => (Method::DELETE, format!("/containers/{id}?force=1")),
        };
        // Stop and restart wait for the container's own stop timeout.
        let response = tokio::time::timeout(
            Duration::from_secs(90),
            self.send(method, &path, None, false),
        )
        .await
        .map_err(|_| DockerError::Connection)??;
        let _ = collect(response).await;
        Ok(())
    }

    /// Sends log lines in batches until the stream ends, `follow` is false and
    /// the tail was sent, or `cancel` fires.
    pub async fn logs(
        &self,
        id: &str,
        tail: u32,
        follow: bool,
        timestamps: bool,
        sink: mpsc::Sender<Vec<LogLine>>,
        cancel: CancellationToken,
    ) -> Result<(), DockerError> {
        let id = checked(id)?;
        let tty = self.inspect(id).await?["Config"]["Tty"]
            .as_bool()
            .unwrap_or(false);
        let path = format!(
            "/containers/{id}/logs?stdout=1&stderr=1&tail={}&follow={}&timestamps={}",
            tail.clamp(1, 100_000),
            u8::from(follow),
            u8::from(timestamps)
        );
        let response = self.send(Method::GET, &path, None, false).await?;
        let mut body = response.into_body();
        let mut decoder = LogDecoder::new(!tty, timestamps);
        loop {
            let frame = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                frame = body.frame() => frame,
            };
            let mut lines = Vec::new();
            match frame {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref() {
                        decoder.push(data, &mut lines);
                    }
                }
                Some(Err(_)) => return Err(DockerError::Connection),
                None => {
                    decoder.finish(&mut lines);
                    if !lines.is_empty() {
                        let _ = sink.send(lines).await;
                    }
                    return Ok(());
                }
            }
            if !lines.is_empty() && sink.send(lines).await.is_err() {
                return Ok(());
            }
        }
    }

    /// Starts an interactive TTY exec and returns its hijacked stream.
    pub async fn exec(
        &self,
        id: &str,
        command: &[String],
        cols: u16,
        rows: u16,
    ) -> Result<ExecSession, DockerError> {
        let id = checked(id)?;
        if command.is_empty() || command.len() > 64 {
            return Err(DockerError::InvalidRequest);
        }
        let created = self
            .json(
                Method::POST,
                &format!("/containers/{id}/exec"),
                Some(json!({
                    "AttachStdin": true,
                    "AttachStdout": true,
                    "AttachStderr": true,
                    "Tty": true,
                    "Cmd": command,
                    "Env": ["TERM=xterm-256color"],
                    "ConsoleSize": [rows, cols],
                })),
            )
            .await?;
        let exec_id = created["Id"]
            .as_str()
            .ok_or(DockerError::Protocol)?
            .to_owned();
        checked(&exec_id)?;
        let response = tokio::time::timeout(
            REQUEST_TIMEOUT,
            self.send(
                Method::POST,
                &format!("/exec/{exec_id}/start"),
                Some(json!({"Detach": false, "Tty": true})),
                true,
            ),
        )
        .await
        .map_err(|_| DockerError::Connection)??;
        let upgraded = hyper::upgrade::on(response)
            .await
            .map_err(|_| DockerError::Protocol)?;
        Ok(ExecSession {
            exec_id,
            io: Box::new(TokioIo::new(upgraded)),
        })
    }

    pub async fn resize_exec(
        &self,
        exec_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), DockerError> {
        let path = format!(
            "/exec/{}/resize?h={}&w={}",
            checked(exec_id)?,
            rows.max(1),
            cols.max(1)
        );
        let response = self.send(Method::POST, &path, None, false).await?;
        let _ = collect(response).await;
        Ok(())
    }
}

async fn collect(response: Response<Incoming>) -> Result<Bytes, DockerError> {
    let mut body = response.into_body();
    let mut out = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| DockerError::Connection)?;
        if let Some(data) = frame.data_ref() {
            if out.len() + data.len() > MAX_BODY {
                return Err(DockerError::Protocol);
            }
            out.extend_from_slice(data);
        }
    }
    Ok(Bytes::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_cannot_reach_other_paths() {
        assert!(checked("3f2a9c1d0e").is_ok());
        assert!(checked("shop-web-1").is_ok());
        for bad in ["", "../images", "a/b", "a?b", "-x", "a b"] {
            assert!(checked(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn secret_environment_is_masked() {
        let mut inspect = json!({"Config": {"Env": ["POSTGRES_PASSWORD=hunter2", "PATH=/usr/bin", "API_TOKEN=", "AWS_SECRET_ACCESS_KEY=x"]}});
        mask_env(&mut inspect);
        assert_eq!(
            inspect["Config"]["Env"],
            json!([
                "POSTGRES_PASSWORD=••••••",
                "PATH=/usr/bin",
                "API_TOKEN=",
                "AWS_SECRET_ACCESS_KEY=••••••"
            ])
        );
    }

    #[test]
    fn health_comes_from_the_status_text() {
        assert_eq!(health("Up 3 hours (healthy)").as_deref(), Some("healthy"));
        assert_eq!(
            health("Up 2 seconds (health: starting)").as_deref(),
            Some("starting")
        );
        assert_eq!(health("Exited (0) 2 days ago"), None);
    }
}
