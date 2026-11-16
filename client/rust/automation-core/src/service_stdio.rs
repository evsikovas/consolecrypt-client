//! Private process entry over the existing bounded helper frames. No listener.
use crate::{
    AutomationError, EnvironmentProbe, HelperLaunch, MAX_FRAME_BYTES, Result, ServiceCapsuleInput,
    ServiceNode,
};
use cc_crypto_core::{DevicePublicKeys, ServiceExecutionOrigin, ServicePolicyTenant};
use cc_models::{automation::JobMode, known_host::KnownHost};
use cc_ssh_core::ConnectionPlan;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::{BufRead, Read, Write},
    sync::Arc,
    time::Duration,
};
use tokio::io::AsyncReadExt;
use uuid::Uuid;
fn denied() -> AutomationError {
    AutomationError::Invalid("service control unavailable")
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u8,
    request_id: Uuid,
    operation: String,
    body: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitialUnlockObservation {
    locked: bool,
    generation: u64,
}
// Compatibility observation from the ordinary Native connection handshake.
// This cannot configure an executor, release material, or change an enrolled
// service policy. After bootstrap ALL Desktop vault-state requests stay denied.
fn observe_initial_unlock(body: Value) -> Result<Value> {
    let value: InitialUnlockObservation = serde_json::from_value(body).map_err(|_| denied())?;
    if value.locked || value.generation == 0 {
        return Err(denied());
    }
    Ok(json!({"observed":true,"service_local":true}))
}
use crate::service_restart::Bootstrap;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    template: Value,
    plans: Vec<ConnectionPlan>,
    known_hosts: Vec<KnownHost>,
    capsules: Vec<ServiceCapsuleInput>,
}
async fn probe(launch: &HelperLaunch) -> Result<Value> {
    let mut command = launch.base_command()?;
    command.arg("--probe");
    let mut child = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let mut output = child.stdout.take().ok_or_else(denied)?;
    let mut raw = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(60), async {
        (&mut output)
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_to_end(&mut raw)
            .await?;
        let status = child.wait().await?;
        if !status.success() || raw.len() > MAX_FRAME_BYTES {
            return Err(denied());
        }
        let value = crate::json::parse(&raw)?;
        EnvironmentProbe::parse(value.clone())?;
        Ok(value)
    })
    .await;
    match read {
        Ok(value) => value,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(AutomationError::Timeout)
        }
    }
}

/// Standalone service process owns ONE runtime. Never call this from AppCore or
/// FRB. The embedding process manager must supply the original private pipe and
/// owned root; OS-service installation/key persistence are separate deployment.
pub fn service_node_stdio(launch: HelperLaunch) -> Result<()> {
    if !cfg!(target_os = "linux") || !matches!(launch, HelperLaunch::Native { .. }) {
        return Err(denied());
    }
    service_node_stdio_owned(launch, ServiceNode::new()?)
}
/// Explicit local process-manager entry. No path/default provisioning fallback.
pub fn service_node_stdio_with_identity(
    launch: HelperLaunch,
    identity: crate::ServiceIdentity,
) -> Result<()> {
    if !cfg!(target_os = "linux") || !matches!(launch, HelperLaunch::Native { .. }) {
        return Err(denied());
    }
    service_node_stdio_owned(launch, ServiceNode::with_identity(identity)?)
}
async fn activate_background(
    owner: Arc<tokio::sync::Mutex<ServiceNode>>,
    cleanup: Arc<tokio::sync::Mutex<()>>,
    stop: tokio_util::sync::CancellationToken,
    mode: JobMode,
    finished: tokio_util::sync::CancellationToken,
) {
    struct Finish(tokio_util::sync::CancellationToken);
    impl Drop for Finish {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
    let _finished = Finish(finished);
    loop {
        let result = owner.lock().await.activate_due(mode).await;
        if result.is_err() {
            break;
        }
        tokio::select! {biased; _=stop.cancelled()=>break, _=tokio::time::sleep(Duration::from_secs(20))=>{}}
    }
    let _status_ack = cleanup.lock().await;
    owner.lock().await.shutdown().await;
}
mod dispatch;
#[cfg(target_os = "linux")]
mod unix;

fn parse_request(raw: &[u8], seen: &mut HashSet<Uuid>) -> Result<Request> {
    if raw.len() > MAX_FRAME_BYTES || raw.last() != Some(&b'\n') {
        return Err(denied());
    }
    let request: Request =
        serde_json::from_value(crate::json::parse(raw)?).map_err(|_| denied())?;
    if request.version != 1
        || request.request_id.is_nil()
        || !request.body.is_object()
        || request.operation.len() > 64
        || seen.len() >= 10000
        || !seen.insert(request.request_id)
    {
        return Err(denied());
    }
    Ok(request)
}
fn reply(request: &Request, result: Result<Value>) -> Result<Vec<u8>> {
    let value = match result {
        Ok(value) => {
            json!({"version":1,"type":"response","request_id":request.request_id,"result":value,"error":null})
        }
        Err(_) => {
            json!({"version":1,"type":"response","request_id":request.request_id,"result":null,
  "error":{"code":"service_control","message":"Service control unavailable"}})
        }
    };
    let mut bytes = serde_json::to_vec(&value).map_err(|_| denied())?;
    if bytes.len() >= MAX_FRAME_BYTES {
        return Err(denied());
    }
    bytes.push(b'\n');
    Ok(bytes)
}
fn service_node_stdio_owned(launch: HelperLaunch, node: ServiceNode) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let dispatch = runtime.block_on(dispatch::Dispatcher::new(launch, node))?;
    let mut reader = std::io::BufReader::new(std::io::stdin().lock());
    let mut output = std::io::stdout().lock();
    let mut seen = HashSet::new();
    let mut protocol_ok = true;
    loop {
        let mut raw = Vec::new();
        let count =
            Read::take(&mut reader, (MAX_FRAME_BYTES + 1) as u64).read_until(b'\n', &mut raw)?;
        if count == 0 {
            break;
        }
        let request = match parse_request(&raw, &mut seen) {
            Ok(v) => v,
            Err(_) => {
                protocol_ok = false;
                break;
            }
        };
        let bytes = match reply(&request, runtime.block_on(dispatch.dispatch(&request))) {
            Ok(v) => v,
            Err(_) => {
                protocol_ok = false;
                break;
            }
        };
        if output
            .write_all(&bytes)
            .and_then(|_| output.flush())
            .is_err()
        {
            protocol_ok = false;
            break;
        }
        if request.operation == "service_close" {
            break;
        }
    }
    runtime.block_on(dispatch.finish(!protocol_ok))?;
    if protocol_ok { Ok(()) } else { Err(denied()) }
}
/// Fixed inherited Unix listener; same Node/dispatcher, never an alternate grant.
pub fn service_node_unix_with_identity(
    launch: HelperLaunch,
    identity: crate::ServiceIdentity,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        unix::run(launch, identity)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (launch, identity);
        Err(denied())
    }
}

#[cfg(test)]
mod initial_unlock_tests {
    use super::*;
    #[test]
    fn ordinary_initial_unlock_is_observation_only_and_strict() {
        let observed = observe_initial_unlock(json!({"locked":false,"generation":1})).unwrap();
        assert_eq!(observed, json!({"observed":true,"service_local":true}));
        for body in [
            json!({"locked":true,"generation":1}),
            json!({"locked":false,"generation":0}),
            json!({"locked":false}),
            json!({"locked":false,"generation":1.5}),
            json!({"locked":false,"generation":1,"approved":true}),
            json!({"locked":"false","generation":1}),
            json!({"locked":false,"generation":-1}),
        ] {
            assert!(observe_initial_unlock(body).is_err());
        }
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    fn raw(id: Uuid) -> Vec<u8> {
        let mut v =
            serde_json::to_vec(&json!({"version":1,"request_id":id,"operation":"probe","body":{}}))
                .unwrap();
        v.push(b'\n');
        v
    }
    #[test]
    fn duplicate_transport_id_denied_per_connection_without_minting_authority() {
        let data = raw(Uuid::new_v4());
        let mut first = HashSet::new();
        assert!(parse_request(&data, &mut first).is_ok());
        assert!(parse_request(&data, &mut first).is_err());
        let mut second = HashSet::new();
        assert!(parse_request(&data, &mut second).is_ok());
        // Parsing a new transport connection does not call Node, signer or JobStore.
    }
    #[test]
    fn exact_existing_connection_bound_retires_without_global_reset() {
        let mut seen = HashSet::new();
        for _ in 0..10000 {
            assert!(parse_request(&raw(Uuid::new_v4()), &mut seen).is_ok());
        }
        assert!(parse_request(&raw(Uuid::new_v4()), &mut seen).is_err());
        assert_eq!(seen.len(), 10000);
    }
    #[test]
    fn strict_frame_and_fixed_error_shape_survive_shared_dispatch_extraction() {
        let mut seen = HashSet::new();
        let id = Uuid::new_v4();
        let data = raw(id);
        let request = parse_request(&data, &mut seen).unwrap();
        let wire = reply(&request, Err(denied())).unwrap();
        let value: Value = serde_json::from_slice(&wire).unwrap();
        assert_eq!(value["request_id"], id.to_string());
        assert_eq!(
            value["error"],
            json!({"code":"service_control","message":"Service control unavailable"})
        );
        for data in [
            b"{\"version\":1,\"version\":1}\n".as_slice(),
            b"{}",
            b"{}\n",
            b"[]\n",
        ] {
            assert!(parse_request(data, &mut HashSet::new()).is_err());
        }
    }
}
