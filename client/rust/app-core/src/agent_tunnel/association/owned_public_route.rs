//! Opt-in actual Personal public route + maintained Relay/Node/TLS workflow.
//! Test-only source; public intent/opaque APIs are never replaced by fixture ACKs.
use super::*;
use crate::{AccountMode, AppConfig};
use secrecy::ExposeSecret;
use cc_platform_core::{InMemorySecureStore,UnsupportedDeviceKeys};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{fs::OpenOptions, io::{Read, Write}, os::unix::fs::{MetadataExt, OpenOptionsExt}, path::{Path, PathBuf}};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;

type OwnedResult<T> = std::result::Result<T, ()>;
static FIRST_FAILURE_LINE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const DIAGNOSTIC_MAX_LINE: u32 = 2048;
fn record_failure_line(line: u32) {
    if (1..=DIAGNOSTIC_MAX_LINE).contains(&line) {
        let _ = FIRST_FAILURE_LINE.compare_exchange(0, line,
            std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct ServerReady { format:String, token:String, origin:String }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Endpoint { format:String, token:String, host:String, port:u16, user:String,
    subsystem:String, host_public_key:String }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct AssociationObserved {format:String,token:String,actor_revision:u64,checkpoint:u64,read_only_observation:bool}

struct Held { app:Option<AppCore>, target:Option<tokio::task::JoinHandle<OwnedResult<usize>>>,
    monitoring:Option<tokio::task::JoinHandle<OwnedResult<usize>>>, settings_peer:Option<AppCore> }
struct MonitoringTarget {
    port:u16, ca:String, connections:Arc<std::sync::atomic::AtomicUsize>,
    requests:Arc<std::sync::atomic::AtomicUsize>, stop:CancellationToken,
}
async fn monitoring_target(held:&mut Held,token:String)->OwnedResult<MonitoringTarget> {
    let cert=checked(rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]))?;
    let ca=cert.cert.pem();
    let tls=checked(checked(rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions())?.with_no_client_auth().with_single_cert(
            vec![cert.cert.der().clone()],rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into()))?;
    let acceptor=tokio_rustls::TlsAcceptor::from(Arc::new(tls));
    let listener=checked(tokio::net::TcpListener::bind("127.0.0.1:0").await)?;
    let port=checked(listener.local_addr())?.port();
    let connections=Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let requests=Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stop=CancellationToken::new();let end=stop.clone();let seen=connections.clone();let gets=requests.clone();
    held.monitoring=Some(tokio::spawn(async move {
        loop {
            let(socket,_)=tokio::select!{biased;_=end.cancelled()=>return Ok(gets.load(std::sync::atomic::Ordering::SeqCst)),
                result=listener.accept()=>checked(result)?};
            seen.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let mut stream=checked(checked(tokio::time::timeout(Duration::from_secs(5),acceptor.accept(socket)).await)?)?;
            let mut bytes=Vec::new();let mut chunk=[0;1024];
            while !bytes.windows(4).any(|v|v==b"\r\n\r\n") {
                let n=checked(checked(tokio::time::timeout(Duration::from_secs(5),stream.read(&mut chunk)).await)?)?;
                require(n>0&&bytes.len()+n<=16384)?;bytes.extend_from_slice(&chunk[..n]);
            }
            let headers=checked(String::from_utf8(bytes))?;let mut lines=headers.lines();
            require(lines.next()==Some("GET /owned-monitoring/api/v1/query?query=up HTTP/1.1"))?;
            let headers=headers.to_ascii_lowercase();
            require(headers.contains(&format!("\r\nhost: 127.0.0.1:{port}\r\n"))
                &&headers.contains(&format!("\r\nauthorization: bearer {token}\r\n"))
                &&!headers.contains("proxy-authorization:"))?;
            gets.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let body=json!({"status":"success","data":{"resultType":"vector","result":[
                {"metric":{"job":"owned-agent-route"},"value":[1,"1"]}]},
                "warnings":[format!("Untrusted echo: {token}")]}).to_string();
            checked(stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await)?;
            checked(stream.shutdown().await)?;
        }
    }));
    Ok(MonitoringTarget{port,ca,connections,requests,stop})
}
fn monitoring_request(source:&str,revision:u64,route:&str)->String {
    json!({"operationId":Uuid::new_v4(),"sourceId":source,"sourceRevision":revision,
        "selectedAgentRouteId":route,"action":{"kind":"query","expression":"up","time":null}}).to_string()
}
async fn monitoring_read_case(app:&AppCore,frame:&Value,association:&str,source:&str,
    target:&MonitoringTarget,token:&str)->OwnedResult<Value> {
    let intent=json!({"version":1,"profileId":frame["profileId"],"notificationScopeId":frame["notificationScopeId"],
        "associationId":association,"listenIp":"127.0.0.1","listenPort":0,
        "destinationIp":"127.0.0.1","destinationPort":target.port,"ttlSeconds":30,"idleSeconds":10,"resumeOriginal":true});
    let route=checked(app.clone().agent_tunnel_route_prepare_json(&intent.to_string()).await)?;
    let listening=value(checked(route.approve_once().await)?)?;
    require(listening["phase"]=="listening"&&listening["actorAdmission"]==false)?;
    let route_id=checked(listening["routeId"].as_str().ok_or(()))?;
    require(app.monitoring_read(monitoring_request(source,1,&Uuid::new_v4().to_string())).await.is_err()
        &&app.monitoring_read(monitoring_request(source,2,route_id)).await.is_err())?;
    require(target.connections.load(std::sync::atomic::Ordering::SeqCst)==0
        &&value(checked(route.details_json().await)?)?["actorAdmission"]==false)?;
    let result=checked(checked(tokio::time::timeout(Duration::from_secs(15),
        app.monitoring_read(monitoring_request(source,1,route_id))).await)?)?;
    require(result["sourceId"]==source&&result["sourceRevision"]==1&&result["result"]["status"]=="success"
        &&result.to_string().contains("[REDACTED]")&&!result.to_string().contains(token))?;
    require(app.monitoring_read(monitoring_request(source,1,route_id)).await.is_err())?;
    let terminal=value(checked(route.joined_json().await)?)?;
    let after=value(checked(route.details_json().await)?)?;
    require(terminal["phase"]=="unknown"&&terminal["cleanupState"]=="closedObserved"&&terminal["actorAdmission"]==true
        &&after["routeId"]==listening["routeId"]&&after["grantExpiresUtc"]==listening["grantExpiresUtc"])?;
    // A distinct never-consumed original demonstrates natural expiry, rather
    // than attributing the already-used route's denial solely to its TTL.
    let mut short=intent;short["ttlSeconds"]=json!(4);short["idleSeconds"]=json!(4);
    let expiring=checked(app.clone().agent_tunnel_route_prepare_json(&short.to_string()).await)?;
    let expiry_observer=expiring.original_join_observer();
    let pending=value(checked(expiring.approve_once().await)?)?;
    require(pending["phase"]=="listening"&&pending["actorAdmission"]==false&&pending["routeId"]!=listening["routeId"])?;
    let expires=checked(chrono::DateTime::parse_from_rfc3339(checked(pending["grantExpiresUtc"].as_str().ok_or(()))?))?.timestamp();
    checked(tokio::time::timeout(Duration::from_secs(6),async {
        while wall()?.seconds()<checked(u64::try_from(expires))? {tokio::time::sleep(Duration::from_millis(50)).await;}
        Ok::<_,()>(())
    }).await)??;
    require(app.monitoring_read(monitoring_request(source,1,checked(pending["routeId"].as_str().ok_or(()))?)).await.is_err())?;
    let expired=value(checked(expiry_observer.joined_json().await)?)?;
    let local_joined=expiry_observer.local_pump_joined();
    require(expired["phase"]=="unknown"&&expired["cleanupState"]=="unknown"&&local_joined&&expired["actorAdmission"]==false
        &&expired["remoteDrainObserved"]==false
        &&target.connections.load(std::sync::atomic::Ordering::SeqCst)==1
        &&target.requests.load(std::sync::atomic::Ordering::SeqCst)==1)?;
    let audit=value(checked(app.clone().agent_tunnel_access_audit_json(&frame.to_string()).await)?)?;
    let rows=audit_rows(&audit)?;require(rows.len()==17)?;
    for (group,events) in [(&rows[12..15],vec!["openRequested","opened","unknown"]),
        (&rows[15..17],vec!["openRequested","unknown"])] {
        for(row,event)in group.iter().zip(events) {
            require(row["record"]["event"]==event&&row["record"]["associationId"]==association
                &&row["record"]["grantId"]==group[0]["record"]["grantId"]
                &&row["record"]["grantDigest"]==group[0]["record"]["grantDigest"]
                &&row["record"]["expiresAt"]==group[0]["record"]["expiresAt"]
                &&row["record"]["destinationPort"]==target.port)?;
        }
    }
    Ok(json!({"approvedRouteId":route_id,"grantId":rows[12]["record"]["grantId"],
        "grantExpiresUtc":listening["grantExpiresUtc"],"httpGets":1,"tcpConnections":1,
        "sameOriginalUrlTlsAuth":true,"unknownRouteDenied":true,"staleSourceRevisionDenied":true,
        "duplicateUseDenied":true,"neverConsumedExpiredRouteDenied":true,"noDirectFallback":true,
        "queryCloseIssuedStopOrRevoke":false,"originalOwnerJoinedUnknown":true,"originalExpiryUnchanged":true,
        "unusedExpiredRouteId":pending["routeId"],"auditEvents":["openRequested","opened","unknown","openRequested","unknown"]}))
}
fn value(raw:String)->OwnedResult<Value>{checked(serde_json::from_str(&raw))}
async fn frame(app:&AppCore)->OwnedResult<Value> {
    let(owner,u)=checked(app.unlocked().await)?;
    Ok(json!({"version":1,"profileId":owner.profile_id.to_string(),
        "notificationScopeId":checked(u.notices.scope_id().ok_or(()))?}))
}
fn payload(name:&str,token:&str)->Vec<u8> {
    // JSON-looking application bytes MUST remain DATA, never a Stop command.
    format!("{{\"kind\":\"stop\",\"drained\":true}}\npublic-tun:{name}:{token}").into_bytes()
}
async fn echo_target(listener:tokio::net::TcpListener,token:String)->OwnedResult<usize> {
    let mut count=0;
    for name in ["stop","forcedClose","resume1","resume2","revoke"] {
        let(mut socket,_)=checked(checked(tokio::time::timeout(Duration::from_secs(40),listener.accept()).await)?)?;
        let expected=payload(name,&token);let mut actual=vec![0;expected.len()];
        checked(checked(tokio::time::timeout(Duration::from_secs(8),socket.read_exact(&mut actual)).await)?)?;
        require(actual==expected)?;
        checked(socket.write_all(&actual).await)?;
        // The canary never drives EOF. The actual Agent target owner must close.
        let mut next=[0;1];
        let end=checked(checked(tokio::time::timeout(Duration::from_secs(8),socket.read(&mut next)).await)?)?;
        require(end==0)?;count+=1;
    }
    Ok(count)
}
async fn connected(route:&super::public::AgentTunnelRouteOperation)->OwnedResult<Value> {
    checked(tokio::time::timeout(Duration::from_secs(5),async {
        let mut tick=tokio::time::interval(Duration::from_millis(20));
        loop {tick.tick().await;let view=value(checked(route.details_json().await)?)?;
            if view["phase"]=="connected" {require(view["actorAdmission"]==true)?;return Ok(view)}
            require(view["phase"]=="connecting"||view["phase"]=="listening")?;
        }
    }).await)?
}
async fn resume_pending(app:&AppCore,route:&super::public::AgentTunnelRouteOperation,
    original_frame:&Value,address:&str,expiry:&Value)->OwnedResult<Value> {
    checked(tokio::time::timeout(Duration::from_secs(20),async {
        let mut tick=tokio::time::interval(Duration::from_millis(20));
        loop {
            tick.tick().await;
            let view=value(checked(route.details_json().await)?)?;
            require(view["phase"]=="connected"||view["phase"]=="connecting"||view["phase"]=="listening")?;
            require(view["boundAddress"]==address && &view["grantExpiresUtc"]==expiry)?;
            let audit=value(checked(app.clone().agent_tunnel_access_audit_json(&original_frame.to_string()).await)?)?;
            let rows=audit_rows(&audit)?;require(rows.len()<=10)?;
            if rows.len()==10&&view["phase"]=="listening" {
                let first=&rows[6]["record"];let ended=&rows[8]["record"];let next=&rows[9]["record"];
                require(first["event"]=="openRequested"&&rows[7]["record"]["event"]=="opened"
                    &&ended["event"]=="unknown"&&next["event"]=="openRequested"
                    &&first["operationId"]==ended["operationId"]&&first["operationId"]!=next["operationId"])?;
                for field in ["associationId","grantId","grantDigest","policyDigest","hostId","hostRevision",
                    "destinationIp","destinationPort","expiresAt"] {
                    require(first[field]==ended[field]&&first[field]==next[field])?;
                }
                // Scheduling evidence only. Actual Node retirement, fresh nonce
                // and Actor resume must still precede the next SAME-TLS ACK.
                return Ok(next.clone())
            }
        }
    }).await)?
}
async fn saved_host(app:&AppCore,endpoint:Endpoint,credential:String)->OwnedResult<(ObjectId,i64)> {
    require(endpoint.host=="127.0.0.1"&&endpoint.port>0&&endpoint.user=="ccenroll"
        &&endpoint.subsystem=="consolecrypt-agent-enroll-v1")?;
    let parts:Vec<_>=endpoint.host_public_key.split_whitespace().collect();
    require(parts.len()==2&&parts[0]=="ssh-ed25519"&&parts.join(" ")==endpoint.host_public_key)?;
    checked(app.import_known_hosts(format!("[127.0.0.1]:{} {}\n",endpoint.port,endpoint.host_public_key)).await)?;
    let mut host=crate::HostDto::new("Owned actual public TUN endpoint","127.0.0.1");
    host.port=Some(endpoint.port);host.username=Some(endpoint.user);host.credential_id=Some(credential);
    host.host_key_policy=cc_ssh_core::HostKeyPolicy::Strict;
    let mut events=app.subscribe_events();
    let host=checked(app.save_host(host).await)?;let id=checked(crate::dto::parse_id("host",&host.id))?;
    let saved=id.to_string();
    checked(tokio::time::timeout(Duration::from_secs(5),async {
        loop {if let crate::AppEvent::ObjectsChanged {kind:cc_models::ObjectKind::Host,ids,origin:crate::ChangeOriginDto::Local}
            =checked(events.recv().await)? {if ids.iter().any(|v|v==&saved){break}}
        } Ok::<_,()>(())
    }).await)??;
    drop(events);
    let revision=checked(checked(app.unlocked().await)?.1.working().revision_of(id).ok_or(()))?;
    Ok((id,revision))
}
fn audit_rows(view:&Value)->OwnedResult<&Vec<Value>> {
    require(view["version"]==1&&view["boundedLocalObservations"]==true
        &&view["immutableLog"]==false&&view["remoteDeliveryGuarantee"]==false)?;
    checked(view["rows"].as_array().ok_or(()))
}
fn audit_history(view:&Value,association:&str,profile:&str,device:&str,user:&str)->OwnedResult<Vec<Value>> {
    let rows=audit_rows(view)?;require(rows.len()==20)?;
    let mut lineage=Vec::new();let mut operations=std::collections::BTreeSet::new();
    let mut grants=std::collections::BTreeSet::new();
    for(index,end)in ["stopRequested","cancelled","unknown","stopRequested","revoked"].iter().enumerate() {
        let start=[0,3,6,9,17][index];let group=&rows[start..start+3];
        let requested=&group[0]["record"];let opened=&group[1]["record"];let terminal=&group[2]["record"];
        require(requested["event"]=="openRequested"&&opened["event"]=="opened"&&terminal["event"]==*end)?;
        require(operations.insert(requested["operationId"].to_string()))?;
        if index==3 {
            let original=&rows[6]["record"];
            require(grants.contains(&requested["grantId"].to_string()))?;
            for field in ["associationId","grantId","grantDigest","policyDigest","hostId","hostRevision",
                "destinationIp","destinationPort","expiresAt"] {require(original[field]==requested[field])?;}
        } else {require(grants.insert(requested["grantId"].to_string()))?;}
        for row in group {
            require(row["profileId"]==profile&&row["deviceId"]==device&&row["personalUserId"]==user)?;
            let record=&row["record"];
            require(record["associationId"]==association&&record["operationId"]==requested["operationId"]
                &&record["grantId"]==requested["grantId"]&&record["grantDigest"]==requested["grantDigest"]
                &&record["policyDigest"]==requested["policyDigest"]&&record["hostId"]==requested["hostId"]
                &&record["hostRevision"]==requested["hostRevision"]&&record["destinationIp"]=="127.0.0.1"
                &&record["destinationPort"]==requested["destinationPort"]&&record["expiresAt"]==requested["expiresAt"])?;
        }
        require(requested["nativeControlFinished"]==false&&opened["nativeControlFinished"]==false
            &&terminal["nativeControlFinished"]==true&&terminal["countersExact"]==false)?;
        lineage.push(json!({"operationId":requested["operationId"],"grantId":requested["grantId"],
            "grantDigest":requested["grantDigest"],"policyDigest":requested["policyDigest"],
            "expiresAt":requested["expiresAt"],"terminal":terminal["event"]}));
    }
    Ok(lineage)
}
fn encrypted_envelope(path:&Path,private:&Path,uid:u32)->OwnedResult<Value> {
    require(path.starts_with(private)&&path.is_absolute())?;
    let before=checked(std::fs::symlink_metadata(path))?;
    require(before.is_file()&&!before.file_type().is_symlink()&&before.nlink()==1&&before.uid()==uid
        &&before.mode()&0o022==0&&before.len()>=16&&before.len()<=64*1024*1024)?;
    for ancestor in path.ancestors(){require(!checked(std::fs::symlink_metadata(ancestor))?.file_type().is_symlink())?;}
    let mut file=checked(OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK).open(path))?;
    let identity=|m:&std::fs::Metadata|(m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec());
    require(identity(&before)==identity(&checked(file.metadata())?))?;
    let mut header=[0;16];checked(file.read_exact(&mut header))?;
    require(&header!=b"SQLite format 3\0")?;
    use sha2::Digest;
    let mut digest=Sha256::new();digest.update(header);let mut count=16;
    let mut block=[0;65536];
    loop {let n=checked(file.read(&mut block))?;if n==0{break}count+=n;require(count<=64*1024*1024)?;digest.update(&block[..n]);}
    require(count as u64==before.len()&&identity(&before)==identity(&checked(file.metadata())?)
        &&identity(&before)==identity(&checked(std::fs::symlink_metadata(path))?))?;
    // Header/hash only are exported. This corroborates the real SQLCipher
    // open/commit/close/reopen path; it is not a standalone encryption proof.
    Ok(json!({"regularSingleLink":true,"bytes":count,"sha256":hex::encode(digest.finalize()),"plaintextSqliteHeader":false}))
}

// SAME real server, account and encrypted vault; no injected policy response or
// test-only admission choice. Run only after every original route has joined.
async fn settings_readback(app:&AppCore)->OwnedResult<()> {
    let mut events=app.subscribe_events();
    let host=cc_models::host::Host::new("Owned settings read barrier","192.0.2.29");
    let id=host.id.to_string();
    let(_,u)=checked(app.unlocked().await)?;
    checked(u.writer.put(cc_models::VaultObject::Host(host)).await)?;drop(u);
    checked(tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            if let crate::AppEvent::ObjectsChanged{kind:cc_models::ObjectKind::Host,
                origin:crate::ChangeOriginDto::Local,ids}=checked(events.recv().await)? {
                if ids.contains(&id){return Ok::<_,()>(())}
            }
        }
    }).await)??;
    checked(app.sync_now().await)?;Ok(())
}
async fn settings_pair(app:&AppCore,held:&mut Held,private:&Path,origin:&str,ca:&[u8],
    token:&str,password:&str,source:&str)->OwnedResult<Value> {
    require(crate::settings_preservation::RELEASED_SETTINGS_PRESERVATION_FLOOR==Some("1.2.0"))?;
    checked(app.sync_now().await)?;settings_readback(app).await?;
    let review=checked(app.monitoring_settings_prepare_consent_json(&frame(app).await?.to_string()).await)?;
    let details=value(checked(review.details_json().await)?)?;
    require(details["kind"]=="ownerConsent"&&details["canConfirm"]==true
        &&details["releasedMinimumClientVersion"]=="1.2.0"
        &&details["offeredMinimumClientVersion"]=="1.2.0"&&details["accountState"]=="off")?;
    let confirmed=value(checked(review.confirm().await)?)?;
    require(confirmed["action"]=="accountPolicyConfirmed"&&review.confirm().await.is_err()
        &&review.details_json().await.is_err())?;
    drop(review);
    const QUERY:&str="sum(owned_candidate_panel_marker)";
    const NAME:&str="owned-candidate-project-reference";
    let catalog=checked(app.monitoring_sources().await)?;
    let result=checked(app.monitoring_panel_create(checked(catalog.revision.ok_or(()))?,
        crate::monitoring_sources::MonitoringPanelDefinition{title:"Owned candidate panel".into(),
            source_id:source.into(),source_revision:1,query:QUERY.into(),
            unit_hint:crate::monitoring_sources::MonitoringUnitHint::Unknown}).await)?;
    let panels=checked(result.panels.ok_or(()))?;require(panels.len()==1)?;
    let panel=panels[0].id.clone();
    checked(app.sync_now().await)?;settings_readback(app).await?;
    let project=checked(app.automation_project_create(NAME.into(),None).await)?;
    checked(app.automation_project_save(project.id.clone(),"site.yml".into(),None,
        "---\n- hosts: all\n  tasks: []\n# owned file body must remain local\n".into()).await)?;
    let context=checked(app.automation_project_metadata_context(project.id.clone()).await)?;
    let mut request=context["frame"].clone();request["expected_reference_revision"]=json!(0);
    let metadata=checked(app.automation_project_metadata_publish(request.to_string()).await)?;
    let records=checked(metadata["catalog"]["records"].as_array().ok_or(()))?;
    require(records.len()==1&&records[0]["runnable"]==false&&records[0]["local_root_bound"]==false
        &&!metadata.to_string().contains("owned file body must remain local"))?;
    checked(app.sync_now().await)?;
    let vault=checked(app.unlocked().await)?.1.vault_id.to_string();
    let data=private.join("settings-peer");checked(std::fs::create_dir(&data))?;
    let mut cfg=checked(AppConfig::for_tests(data.to_string_lossy().into_owned())
        .with_server_public_ca_der(origin,vec![ca.to_vec()]))?;
    cfg.client_version="1.2.0".into();cfg.background_sync=false;cfg.auto_start_tunnels=false;
    let peer=checked(AppCore::new(cfg))?;held.settings_peer=Some(peer.clone());
    checked(peer.create_synced_profile("Owned candidate peer".into(),origin.into(),
        format!("public-tun-{token}@example.invalid"),format!("Owned public account {token}"),AccountMode::Login).await)?;
    checked(peer.join_vault_with_passphrase(vault,password.into()).await)?;
    checked(peer.sync_now().await)?;settings_readback(&peer).await?;
    require(checked(peer.monitoring_panel_definition(panel.clone(),1).await)?.query==QUERY
        &&checked(peer.automation_project_metadata_catalog().await)?["catalog"]==metadata["catalog"]
        &&checked(peer.automation_projects().await)?.projects.is_empty())?;
    let mut ordinary=checked(peer.get_vault_settings().await)?;
    ordinary.vault_name="Owned candidate settings roundtrip".into();
    checked(peer.save_vault_settings(ordinary).await)?;checked(peer.sync_now().await)?;
    checked(app.sync_now().await)?;settings_readback(app).await?;
    require(checked(app.monitoring_panel_definition(panel,1).await)?.query==QUERY
        &&checked(app.automation_project_metadata_catalog().await)?["catalog"]==metadata["catalog"]
        &&checked(app.automation_projects().await)?.projects.len()==1)?;
    checked(peer.shutdown().await)?;drop(held.settings_peer.take());
    Ok(json!({"candidateVersion":"1.2.0","originalOwnerConsent":true,"oneUseConsent":true,
        "realServerPolicy":true,"secondAppCorePanelRead":true,"secondAppCoreMetadataRead":true,
        "secondAppCoreRootless":true,"ordinarySettingsPreservedBoth":true,"projectFileBodySynced":false}))
}

async fn workflow(ipc:&Ipc,held:&mut Held)->OwnedResult<Value> {
    let server:ServerReady=ipc.wait("server-ready.json").await?;
    ipc.frame("cc-tun-server-ready/v1",&server.format,&server.token)?;
    let origin=checked(url::Url::parse(&server.origin))?;
    require(origin.scheme()=="https"&&origin.host_str()==Some("127.0.0.1")&&origin.port().is_some_and(|p|p>0)
        &&origin.username().is_empty()&&origin.password().is_none()&&origin.query().is_none()&&origin.fragment().is_none()
        &&server.origin==format!("https://127.0.0.1:{}",checked(origin.port().ok_or(()))?))?;
    let ca=ipc.read("relay-ca.der",8192)?.ok_or(())?;
    let private=PathBuf::from(checked(std::env::var("CC_TUN_NATIVE_PRIVATE"))?);
    require(private==Path::new("/native-private").join(format!("owned-{}",ipc.token)))?;
    let meta=checked(std::fs::symlink_metadata(&private))?;
    require(meta.is_dir()&&!meta.file_type().is_symlink()&&meta.uid()==ipc.uid&&meta.mode()&0o777==0o700)?;
    let data=private.join("app");checked(std::fs::create_dir(&data))?;
    let config=||->OwnedResult<AppConfig>{let mut c=checked(AppConfig::for_tests(data.to_string_lossy().into_owned())
        .with_server_public_ca_der(&server.origin,vec![ca.clone()]))?;c.background_sync=false;c.auto_start_tunnels=false;c.client_version="1.2.0".into();Ok(c)};
    // SAME opaque test SecureStore survives a real Core/DB close+reopen. No
    // private key is read/exported here; this makes no OS-keychain/process-restart claim.
    let secure=Arc::new(InMemorySecureStore::new());
    let app=checked(AppCore::with_platform(config()?,secure.clone(),Arc::new(UnsupportedDeviceKeys)))?;held.app=Some(app.clone());
    let vault_password=format!("Owned public tunnel vault {}",ipc.token);
    let joined=checked(app.create_synced_profile("Owned public TUN".into(),server.origin.clone(),
        format!("public-tun-{}@example.invalid",ipc.token),format!("Owned public account {}",ipc.token),AccountMode::Register).await)?;
    let _recovery=checked(app.create_vault(vault_password.clone()).await)?;
    let(identity_key,profile,device,user)={
        let owner=checked(app.session().await)?;let api=checked(owner.api())?;
        let authenticated=checked(api.capture_reader_auth().await)?;
        require(authenticated.user_id().to_string()==joined.user_id&&authenticated.device_id()==owner.device_id())?;
        (PublicKey::from_bytes(owner.identity().public_keys().signing),owner.profile_id.to_string(),
         owner.device_id().to_string(),joined.user_id.clone())
    };
    let original_frame=frame(&app).await?;
    let identity=checked(app.agent_node_client_identity_prepare_json(&original_frame.to_string()).await)?;
    require(value(checked(identity.confirm_creation().await)?)?["phase"]=="stored")?;
    require(value(checked(identity.joined_json().await)?)?["cleanupState"]=="closedObserved")?;drop(identity);
    let public={
        let scope=Arc::new(checked(OriginalScope::capture(&app,&CancellationToken::new(),Duration::from_secs(300)).await)?);
        let custody=checked(ManagedTlsCustody::capture(&app,scope).await)?;
        let(phase,public)=checked(custody.stored_metadata(&app).await)?;
        require(phase==Some(cc_storage_core::TunnelTlsPhase::Stored))?;checked(public.ok_or(()))?
    };
    require(public.certificate_der.len()<=2048&&public.certificate_sha256==hex::encode(Sha256::digest(&public.certificate_der)))?;
    let ssh=checked(cc_ssh_core::keys::generate_key(cc_ssh_core::keys::KeyGenAlgorithm::Ed25519,"owned public TUN",None))?;
    let ssh_public=ssh.public_openssh.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    let credential=checked(app.import_ssh_key("Owned public TUN".into(),Some("ccenroll".into()),
        ssh.private_openssh.expose_secret().to_owned(),None,false,None).await)?;drop(ssh);
    let now=wall()?;let agent=checked(AgentId::new(&Uuid::new_v4().to_string()))?;
    let pin=OwnerPin{context:cc_agent_node_wire::Context{tenant:Tenant::PersonalAccount{id:checked(PersonalAccountId::new(&user))?},
        deployment_id:checked(DeploymentId::new(&Uuid::new_v4().to_string()))?},key:identity_key,epoch:1,checkpoint:0,
        expires_at:checked(now.checked_add(300))?};
    ipc.publish("public-route-bootstrap.json",json!({"format":"cc-owned-public-tun-bootstrap/v1","token":ipc.token,
        "userId":user,"deviceId":device,"nativeProfileId":profile,"ownerPublicKey":identity_key,
        "sshClientPublicKey":ssh_public,"clientTlsPublic":{"identityId":public.identity_id,
            "certificateDerHex":hex::encode(&public.certificate_der),"certificateSha256":public.certificate_sha256},
        "agentId":agent,"genesisOwnerPin":pin}))?;
    let endpoint:Endpoint=ipc.wait("public-route-ready.json").await?;
    ipc.frame("cc-owned-public-tun-ready/v1",&endpoint.format,&endpoint.token)?;
    let(host,revision)=saved_host(&app,endpoint,credential.id).await?;
    let current_frame=frame(&app).await?;
    let request=json!({"version":1,"profileId":profile,"notificationScopeId":current_frame["notificationScopeId"],
        "hostId":host,"expectedHostRevision":revision.to_string()});
    let enrollment=checked(app.agent_node_association_register_json(&request.to_string()).await)?;
    let offer=value(checked(enrollment.probe().await)?)?;require(offer["phase"]=="offerReview"&&offer["node"]["agentId"]==json!(agent))?;
    let claim=value(checked(enrollment.approve_offer().await)?)?;require(claim["phase"]=="claimReview")?;
    let lifetime=120u32.min(checked(u32::try_from(checked(claim["maxCertificateLifetimeSeconds"].as_u64().ok_or(()))?))?);require(lifetime>0)?;
    let associated=value(checked(enrollment.approve_certificate(lifetime).await)?)?;
    require(associated["phase"]=="associated"&&associated["actorRevision"]=="3"&&associated["checkpoint"]=="2")?;
    require(value(checked(enrollment.joined_json().await)?)?["cleanupState"]=="closedObserved")?;drop(enrollment);
    let association=checked(associated["associationId"].as_str().ok_or(()))?.to_owned();
    ipc.publish("public-route-associated.json",json!({"format":"cc-owned-public-tun-associated/v1","token":ipc.token,
        "associationId":association,"agentId":agent,"actorRevision":3,"checkpoint":2}))?;
    // Scheduling-only barrier: Root independently observes actual R3/C2 before
    // first admit can advance the journal. Never an authorization/Node ACK.
    let observed:AssociationObserved=ipc.wait("root-associated-observed.json").await?;
    ipc.frame("cc-owned-public-tun-associated-observed/v1",&observed.format,&observed.token)?;
    require(observed.actor_revision==3&&observed.checkpoint==2&&observed.read_only_observation)?;
    // Save original source/auth/TLS BEFORE capturing any route's working-set
    // fence. The query overrides transport only; these inputs never mutate.
    let monitoring_token=format!("owned-monitoring-{}",ipc.token);
    let monitoring=monitoring_target(held,monitoring_token.clone()).await?;
    let credential=checked(app.add_password_credential("Owned Agent monitoring token".into(),None,monitoring_token.clone()).await)?;
    let configuration=cc_monitoring_core::SourceConfig{id:String::new(),name:"Owned Agent monitoring".into(),
        endpoint:format!("https://127.0.0.1:{}/owned-monitoring",monitoring.port),
        route:cc_monitoring_core::Route::SshHost{host_id:host.to_string()},
        tls:cc_monitoring_core::TlsConfig{custom_ca_pem:Some(monitoring.ca.clone()),..Default::default()},
        auth:cc_monitoring_core::AuthRef::BearerRef{credential_id:credential.id}};
    let catalog=checked(app.monitoring_source_create(0,configuration).await)?;
    require(catalog.sources.len()==1)?;
    let monitoring_source=catalog.sources[0].id.clone();let saved_configuration=catalog.sources[0].configuration.clone();
    let current_frame=frame(&app).await?;
    let listener=checked(tokio::net::TcpListener::bind("127.0.0.1:0").await)?;
    let destination=checked(listener.local_addr())?.port();
    held.target=Some(tokio::spawn(echo_target(listener,ipc.token.clone())));
    let mut route_results=Vec::new();
    let mut revoked_observation=Value::Null;
    for name in ["stop","forcedClose","resume","revoke"] {
        let monitoring_observation=if name=="revoke" {
            Some(monitoring_read_case(&app,&current_frame,&association,&monitoring_source,&monitoring,&monitoring_token).await?)
        }else{None};
        let opted=matches!(name,"resume"|"forcedClose");
        let intent=json!({"version":1,"profileId":profile,"notificationScopeId":current_frame["notificationScopeId"],
            "associationId":association,"listenIp":"127.0.0.1","listenPort":0,
            "destinationIp":"127.0.0.1","destinationPort":destination,
            "ttlSeconds":if name=="resume"{60}else{30},"idleSeconds":if name=="resume"{30}else{10},
            "resumeOriginal":opted});
        let mut route=checked(app.clone().agent_tunnel_route_prepare_json(&intent.to_string()).await)?;
        let review=value(checked(route.details_json().await)?)?;
        require(review["actorAdmission"]==false&&review["resumeOriginalSignedOptIn"].is_boolean())?;
        require(review["resumeOriginalSignedOptIn"]==json!(opted))?;
        if name=="resume" {require(review["resumeTransportImplemented"]==true)?;}
        let listening=value(checked(route.approve_once().await)?)?;
        require(listening["phase"]=="listening"&&listening["actorAdmission"]==false)?;
        if !opted {
            require(route.detach_auto_open_json().await.is_err())?;
            require(checked(app.clone().agent_tunnel_route_attach_saved_json(&intent.to_string()).await)?.is_none())?;
        }
        let address=checked(listening["boundAddress"].as_str().ok_or(()))?;
        let mut client=checked(tokio::net::TcpStream::connect(address).await)?;
        // PUBLIC connected state requires genuine same-TLS ACK+durable Opened.
        let _connected=connected(&route).await?;
        let actual=payload(if name=="resume"{"resume1"}else{name},&ipc.token);checked(client.write_all(&actual).await)?;
        let mut echo=vec![0;actual.len()];checked(checked(tokio::time::timeout(Duration::from_secs(5),client.read_exact(&mut echo)).await)?)?;
        require(actual==echo)?;
        let mut actual_echo_bytes=actual.len();let mut resume_lineage=None;
        let mut auto_open_observation=None;
        if opted {
            // Genuine already-admitted original route: detach is pending
            // custody, never Closed. Dropping its UI operation must not drop
            // the actual TLS/client/Node or silently create another grant.
            let before_attach=value(checked(app.clone().agent_tunnel_access_audit_json(&current_frame.to_string()).await)?)?;
            let detached=value(checked(route.detach_auto_open_json().await)?)?;
            require(detached["phase"]=="detached"&&detached["cleanupState"]=="pending"
                &&detached["originalOperationRetained"]==true&&detached["remoteDrainObserved"]==false
                &&detached["boundAddress"]==listening["boundAddress"]
                &&detached["grantExpiresUtc"]==listening["grantExpiresUtc"]
                &&detached["remainingMs"].as_u64().is_some_and(|ms|ms>0&&ms<=60000))?;
            let mut changed=intent.clone();changed["listenIp"]=json!("::1");
            require(checked(app.clone().agent_tunnel_route_attach_saved_json(&changed.to_string()).await)?.is_none())?;
            let attach_intent=intent.to_string();
            let(first_claim,second_claim)=tokio::join!(
                app.clone().agent_tunnel_route_attach_saved_json(&attach_intent),
                app.clone().agent_tunnel_route_attach_saved_json(&attach_intent));
            let attached=match(first_claim,second_claim) {
                (Ok(Some(attached)),Err(_))|(Err(_),Ok(Some(attached)))=>attached,
                _=>return Err(()),
            };
            // The previous detached operation still exists AFTER the new
            // exclusive claim. All its mutable methods must now be inert.
            require(route.stop().await.is_err()&&route.approve_once().await.is_err()
                &&route.detach_auto_open_json().await.is_err()&&route.details_json().await.is_err())?;
            route.cancel(); // A stale explicitly detached proxy cannot cancel the new owner.
            // A second explicit handoff remains the SAME grant. The oldest
            // proxy cannot withdraw this newer unclaimed Native-only ticket.
            let second_detach=value(checked(attached.detach_auto_open_json().await)?)?;
            require(second_detach["grantExpiresUtc"]==detached["grantExpiresUtc"]
                &&second_detach["boundAddress"]==detached["boundAddress"])?;
            route.cancel();
            let reclaimed=checked(checked(app.clone().agent_tunnel_route_attach_saved_json(&attach_intent).await)?.ok_or(()))?;
            attached.cancel();
            drop(route);drop(attached);route=reclaimed;
            let reattached=connected(&route).await?;
            require(reattached["boundAddress"]==listening["boundAddress"]
                &&reattached["grantExpiresUtc"]==listening["grantExpiresUtc"]
                &&reattached["limits"]==listening["limits"])?;
            let after_attach=value(checked(app.clone().agent_tunnel_access_audit_json(&current_frame.to_string()).await)?)?;
            require(audit_rows(&before_attach)?==audit_rows(&after_attach)?)?;
            auto_open_observation=Some(json!({"detachReceipt":detached,"reattachedSameGrant":true,
                "oldHandleDroppedAfterNewClaim":true,"exclusiveSecondAttachDenied":true,
                "oldTicketCannotWithdrawNewHandoff":true,
                "oldMutableMethodsDenied":true,"wrongListenerNotAttached":true,"noNewAuditOperation":true,
                "processRestartClaimed":false}));
        }
        if name=="resume" {
            // Real application disconnect drops Native's actual old TLS/WSS.
            // No marker, caller drain flag, second prepare/approval or new grant.
            drop(client);
            let pending=resume_pending(&app,&route,&current_frame,address,&listening["grantExpiresUtc"]).await?;
            client=checked(tokio::net::TcpStream::connect(address).await)?;
            let second=connected(&route).await?;
            require(second["boundAddress"]==listening["boundAddress"]
                &&second["grantExpiresUtc"]==listening["grantExpiresUtc"])?;
            let data=payload("resume2",&ipc.token);checked(client.write_all(&data).await)?;
            let mut echo=vec![0;data.len()];checked(checked(tokio::time::timeout(Duration::from_secs(5),client.read_exact(&mut echo)).await)?)?;
            require(data==echo)?;actual_echo_bytes+=data.len();
            let audit=value(checked(app.clone().agent_tunnel_access_audit_json(&current_frame.to_string()).await)?)?;
            let rows=audit_rows(&audit)?;
            require(rows.len()==11&&rows[9]["record"]==pending&&rows[10]["record"]["event"]=="opened"
                &&rows[10]["record"]["operationId"]==pending["operationId"])?;
            resume_lineage=Some(json!({"grantId":pending["grantId"],"grantDigest":pending["grantDigest"],
                "policyDigest":pending["policyDigest"],"expiresAt":pending["expiresAt"],
                "firstOperationId":rows[6]["record"]["operationId"],"secondOperationId":pending["operationId"]}));
            let _request_only=checked(route.stop().await)?;
        }
        else if name=="stop" {let _request_only=checked(route.stop().await)?;}
        else if name=="forcedClose" {
            // Keep the actual application connection open. Below, ONLY the
            // ordinary reattached operation's Drop causes original cancellation.
        }
        else {
            let revoke=json!({"version":1,"profileId":profile,"notificationScopeId":current_frame["notificationScopeId"],"associationId":association});
            let review=checked(app.agent_node_association_revoke_prepare_json(&revoke.to_string()).await)?;
            let observed=value(checked(review.confirm_once().await)?)?;
            require(observed["phase"]=="revoked"&&observed["remoteRevocationObserved"]==true
                &&observed["associationId"]==association)?;
            revoked_observation=observed;
            require(value(checked(review.joined_json().await)?)?["cleanupState"]=="closedObserved")?;drop(review);
        }
        let finished=if name=="forcedClose" {
            let observer=route.original_join_observer();
            drop(route); // No second detach: this ordinary new owner MUST cancel.
            let finished=value(checked(observer.joined_json().await)?)?;
            require(app.clone().agent_tunnel_route_attach_saved_json(&intent.to_string()).await.is_err())?;
            if let Some(observation)=auto_open_observation.as_mut() {
                observation["reattachedDropWithoutDetachCancelledAndJoined"]=json!(true);
            }
            drop(client);finished
        }else{
            let finished=value(checked(route.joined_json().await)?)?;
            if name=="resume" {
                // Actual explicit Stop plus join cannot be revived by the weak
                // index, even while the old public operation is still held here.
                require(app.clone().agent_tunnel_route_attach_saved_json(&intent.to_string()).await.is_err())?;
            }
            drop(route);finished
        };
        require(finished["cleanupState"]=="closedObserved"&&finished["actorAdmission"]==true
            &&finished["remoteDrainObserved"]==false&&finished["countersExact"]==false&&finished["nativeAccountingOnly"]==true)?;
        if name=="stop"||name=="resume" {require(finished["phase"]=="stopRequested")?;}else{require(finished["phase"]=="unknown")?;}
        let mut observed=json!({"case":name,"actualEchoBytes":actual_echo_bytes,"publicTerminal":finished,
            "observedResumeOriginalSignedOptIn":review["resumeOriginalSignedOptIn"],"resumeExercised":name=="resume"});
        if let Some(auto_open)=auto_open_observation {observed["liveGrantAutoOpen"]=auto_open;}
        if let Some(monitoring)=monitoring_observation {observed["monitoringRead"]=monitoring;}
        if let Some(lineage)=resume_lineage {
            observed["originalGrantLineage"]=lineage;
            observed["boundAddress"]=listening["boundAddress"].clone();
            observed["originalGrantExpiresUtc"]=listening["grantExpiresUtc"].clone();
            observed["sameOpaqueOperation"]=json!(true);
            observed["sameTlsAckAndOpenedEachGeneration"]=json!(true);
            observed["applicationDisconnectSubset"]=json!(true);
            observed["externalServerOutageClaim"]=json!(false);
            observed["dataReplayed"]=json!(false);
        }
        ipc.publish(&format!("public-route-case-{name}.json"),json!({"format":"cc-owned-public-tun-case/v1",
            "token":ipc.token,"observation":observed}))?;
        route_results.push(observed);
    }
    let catalog=checked(app.monitoring_sources().await)?;
    require(catalog.sources.len()==1&&catalog.sources[0].configuration==saved_configuration)?;
    monitoring.stop.cancel();
    let http=checked(held.monitoring.as_mut().ok_or(()))?;
    require(checked(checked(tokio::time::timeout(Duration::from_secs(2),http).await)?)??==1)?;
    drop(held.monitoring.take());
    let target=checked(held.target.as_mut().ok_or(()))?;
    require(checked(checked(tokio::time::timeout(Duration::from_secs(5),target).await)?)??==5)?;
    drop(held.target.take());
    let audit=value(checked(app.clone().agent_tunnel_access_audit_json(&current_frame.to_string()).await)?)?;
    ipc.publish("public-route-audit-observation.json",json!({"format":"cc-owned-public-tun-audit-observation/v1",
        "token":ipc.token,"boundedLocalObservationOnly":true,"rows":audit_rows(&audit)?}))?;
    // This intentionally stays strict: the known watcher/EOF race may produce
    // Unknown instead of Revoked. Such an actual result is a PRODUCT GAP, not
    // permission for the fixture to delay/relabel or rewrite audit history.
    let lineage=audit_history(&audit,&association,&profile,&device,&user)?;
    let settings_sync=settings_pair(&app,held,&private,&server.origin,&ca,&ipc.token,&vault_password,&monitoring_source).await?;
    let witness=checked(checked(app.session().await)?.storage())?;
    let database=checked(witness.path().ok_or(()))?.to_path_buf();
    checked(app.shutdown().await)?;drop(held.app.take());drop(app);
    require(matches!(witness.setting_get::<Value>("cc.native.agent.tunnel-access.v1").await,
        Err(cc_storage_core::StorageError::Closed)))?;drop(witness);
    let envelope=encrypted_envelope(&database,&private,ipc.uid)?;
    let reopened=checked(AppCore::with_platform(config()?,secure.clone(),Arc::new(UnsupportedDeviceKeys)))?;held.app=Some(reopened.clone());
    checked(reopened.open_profile(profile.clone()).await)?;
    checked(reopened.unlock_with_passphrase(vault_password).await)?;
    require(reopened.clone().agent_tunnel_access_audit_json(&current_frame.to_string()).await.is_err())?;
    let reopen_frame=frame(&reopened).await?;
    let persisted=value(checked(reopened.clone().agent_tunnel_access_audit_json(&reopen_frame.to_string()).await)?)?;
    require(audit_rows(&persisted)?==audit_rows(&audit)?&&persisted["profileId"]==profile
        &&persisted["notificationScopeId"]==reopen_frame["notificationScopeId"])?;
    let foreign_data=private.join("foreign");checked(std::fs::create_dir(&foreign_data))?;
    let foreign=checked(AppCore::new(AppConfig::for_tests(foreign_data.to_string_lossy().into_owned())))?;
    checked(foreign.create_local_profile("Owned foreign scope".into(),"Owned foreign vault password42".into()).await)?;
    require(foreign.clone().agent_tunnel_access_audit_json(&reopen_frame.to_string()).await.is_err())?;
    checked(foreign.shutdown().await)?;drop(foreign);
    Ok(json!({"format":"cc-owned-public-tun-native-checks/v1","token":ipc.token,"routeCases":route_results,
        "settingsSync":settings_sync,"auditLineage":lineage,"auditRowsReopenedExact":true,"publicRevocation":revoked_observation,"oldScopeDenied":true,"foreignScopeDenied":true,
        "originalStorageClosed":true,"ciphertextEnvelope":envelope,"remoteDrainObserved":false,
        "countersExact":false,"nativeAccountingOnly":true,"productGapIfRevokedAuditMissing":true}))
}

impl AppCore {
    pub(crate) async fn __owned_public_tunnel_fixture() {
        assert_eq!(std::env::var("CC_PUBLIC_TUN_ROUTE_PROOF").as_deref(),Ok("1"));
        let ipc=Ipc::new().expect("fixed owned public TUN IPC");
        let mut held=Held{app:None,target:None,monitoring:None,settings_peer:None};
        let result=tokio::time::timeout(Duration::from_secs(300),Box::pin(workflow(&ipc,&mut held))).await;
        let timed_out=result.is_err();
        let mut cleanup=true;
        if let Some(target)=held.target.take(){target.abort();cleanup&=tokio::time::timeout(Duration::from_secs(2),target).await.is_ok();}
        if let Some(target)=held.monitoring.take(){target.abort();cleanup&=tokio::time::timeout(Duration::from_secs(2),target).await.is_ok();}
        if let Some(peer)=held.settings_peer.take(){cleanup&=tokio::time::timeout(Duration::from_secs(5),peer.shutdown()).await.is_ok_and(|r|r.is_ok());}
        if let Some(app)=held.app.take(){cleanup&=tokio::time::timeout(Duration::from_secs(5),app.shutdown()).await.is_ok_and(|r|r.is_ok());}
        let passed=matches!(&result,Ok(Ok(_)))&&cleanup;
        let checks=match result {Ok(Ok(value))=>value,_=>Value::Null};
        let code=if passed{Value::Null}else{diagnostic_code(FIRST_FAILURE_LINE.load(std::sync::atomic::Ordering::Relaxed),timed_out,cleanup)};
        ipc.publish("native-result.json",json!({"format":"cc-owned-public-tun-native-result/v1","token":ipc.token,
            "passed":passed,"cleanupObserved":cleanup,"checks":checks,"errorCode":code})).expect("bounded fixed result");
        assert!(passed,"actual public TUN route/assertion/cleanup failure");
    }
}
#[track_caller]
fn checked<T, E>(value: std::result::Result<T, E>) -> OwnedResult<T> {
    let line = std::panic::Location::caller().line();
    value.map_err(|_| { record_failure_line(line); () })
}
#[track_caller]
fn require(value: bool) -> OwnedResult<()> {
    if value { Ok(()) } else {
        record_failure_line(std::panic::Location::caller().line());
        Err(())
    }
}
fn diagnostic_code(line: u32, timed_out: bool, shutdown: bool) -> Value {
    if (1..=DIAGNOSTIC_MAX_LINE).contains(&line) { json!(format!("N:L{line}")) }
    else if timed_out { json!("N:timeout") }
    else if !shutdown { json!("N:shutdown") }
    else { json!("N:unclassified") }
}
fn wall() -> OwnedResult<Timestamp> {
    checked(Timestamp::new(checked(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH))?.as_secs()))
}
fn hex_der(value: &str) -> OwnedResult<Vec<u8>> {
    require(!value.is_empty() && value.len() <= 16384 && value.len() % 2 == 0)?;
    require(value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))?;
    checked(hex::decode(value))
}

fn sorted_public(value:Value)->Value {
    match value {
        Value::Object(values)=>{
            let mut pairs:Vec<_>=values.into_iter().collect();pairs.sort_by(|a,b|a.0.cmp(&b.0));
            let mut out=serde_json::Map::new();
            for(key,value)in pairs {out.insert(key,sorted_public(value));} Value::Object(out)
        },
        Value::Array(values)=>Value::Array(values.into_iter().map(sorted_public).collect()),
        other=>other,
    }
}

struct Ipc { root: PathBuf, token: String, uid: u32 }
impl Ipc {
    fn new() -> OwnedResult<Self> {
        let root = PathBuf::from(checked(std::env::var("CC_TUN_COMPOSITION_ROOT"))?);
        let token = checked(std::env::var("CC_TUN_COMPOSITION_TOKEN"))?;
        let parsed = checked(uuid::Uuid::parse_str(&token))?;
        require(!parsed.is_nil() && parsed.to_string() == token)?;
        require(root.is_absolute() && root == Path::new("/tmp").join(format!("cc-tun-composition-{token}")))?;
        let uid = checked(std::fs::metadata("/proc/self"))?.uid();
        let value = Self { root, token, uid };
        value.check()?;
        Ok(value)
    }
    fn check(&self) -> OwnedResult<()> {
        let m = checked(std::fs::symlink_metadata(&self.root))?;
        require(m.is_dir() && !m.file_type().is_symlink() && m.uid() == self.uid && m.mode() & 0o777 == 0o700)
    }
    fn read(&self, name: &str, limit: u64) -> OwnedResult<Option<Vec<u8>>> {
        self.check()?;
        let path = self.root.join(name);
        let before = match std::fs::symlink_metadata(&path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(()),
        };
        require(before.is_file() && !before.file_type().is_symlink() && before.nlink() == 1
            && before.uid() == self.uid && before.mode() & 0o777 == 0o600 && before.len() > 0 && before.len() <= limit)?;
        let mut file = checked(OpenOptions::new().read(true).custom_flags(0x20000 | 0x800).open(&path))?;
        let identity = |m: &std::fs::Metadata| (m.dev(), m.ino(), m.len(), m.uid(), m.mode(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec());
        require(identity(&before) == identity(&checked(file.metadata())?))?;
        let mut bytes = Vec::new();
        checked(Read::by_ref(&mut file).take(limit + 1).read_to_end(&mut bytes))?;
        require(bytes.len() as u64 == before.len() && identity(&before) == identity(&checked(file.metadata())?)
            && identity(&before) == identity(&checked(std::fs::symlink_metadata(path))?))?;
        Ok(Some(bytes))
    }
    async fn wait<T: DeserializeOwned>(&self, name: &str) -> OwnedResult<T> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(raw) = self.read(name, 32768)? {
                require(raw.last() == Some(&b'\n') && raw.iter().filter(|&&b| b == b'\n').count() == 1)?;
                return checked(serde_json::from_slice(&raw));
            }
            require(tokio::time::Instant::now() < deadline)?;
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    fn publish(&self, name: &str, value: Value) -> OwnedResult<()> {
        self.check()?;
        let value=sorted_public(value);
        let mut raw = Zeroizing::new(checked(serde_json::to_vec(&value))?);
        require(raw.is_ascii())?;
        raw.push(b'\n');
        require(raw.len() <= 32768)?;
        let path = self.root.join(name);
        require(std::fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound))?;
        let temp = self.root.join(format!(".{name}.native-prepared"));
        let mut file = checked(OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp))?;
        checked(file.write_all(&raw))?;
        checked(file.sync_all())?;
        drop(file);
        // Exactly one trusted producer owns each fixed leaf under this root.
        checked(std::fs::rename(temp, path))?;
        checked(checked(std::fs::File::open(&self.root))?.sync_all())?;
        self.check()
    }
    fn frame(&self, format: &str, actual_format: &str, token: &str) -> OwnedResult<()> {
        require(actual_format == format && token == self.token)
    }
}

