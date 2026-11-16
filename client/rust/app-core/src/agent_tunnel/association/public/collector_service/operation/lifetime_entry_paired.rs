//! UNRUN genuine public protocol acceptance. No synthetic Ready/Peer/ACK.
//! Ignored until an independently pinned actual owned fixture is supplied.
use super::*;
use cc_platform_core::{InMemorySecureStore, UnsupportedDeviceKeys};
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use std::{fs::File, io::{Read, Write}, path::{Path, PathBuf},
    sync::{OnceLock, Weak, atomic::{AtomicUsize, Ordering}}};

const CASES: [&str;9] = ["positiveLifetime7", "legacySingle5", "foreignPeer", "foreignScope",
    "refSwap", "badSignedReceipt", "lostSignedReceipt", "lateSqlCas", "lockBeforePublish"];
const PASSWORD: &str = "owned lifetime protocol profile passphrase42";
const LOCAL_ROWS: &str = "cc.native.agent.node-associations.v1";
const FORMAT: &str = "cc-owned-lifetime-protocol/v1";

#[derive(Clone,Copy,PartialEq,Eq)]
pub(in crate::agent_tunnel::association::public::collector_service) enum Point { BeforeLifetimeAdmit, AfterVerifiedReceipt, BeforePublish }
struct Hook {
    core: Weak<crate::app::Inner>, scope: Arc<OriginalScope>, point: Option<Point>,
    gate: Option<(oneshot::Sender<Context>,oneshot::Receiver<()>)>, publications: Arc<AtomicUsize>,
}
fn hook() -> &'static Mutex<Option<Hook>> {
    static HOOK: OnceLock<Mutex<Option<Hook>>> = OnceLock::new();
    HOOK.get_or_init(||Mutex::new(None))
}
pub(in crate::agent_tunnel::association::public::collector_service)
async fn checkpoint(context:&Context,point:Point)->AppResult<()> {
    let gate={let mut h=hook().lock().map_err(|_|unavailable())?;
        h.as_mut().filter(|h|Weak::ptr_eq(&h.core,&context.core)&&Arc::ptr_eq(&h.scope,&context.scope)
            &&h.point==Some(point)).and_then(|h|h.gate.take())};
    if let Some((observed,release))=gate {
        observed.send(context.clone()).map_err(|_|unavailable())?;
        release.await.map_err(|_|unavailable())?;
    }
    Ok(())
}
pub(in crate::agent_tunnel::association::public::collector_service)
fn published(context:&Context)->AppResult<()> {
    let h=hook().lock().map_err(|_|unavailable())?;
    if let Some(h)=h.as_ref().filter(|h|Weak::ptr_eq(&h.core,&context.core)&&Arc::ptr_eq(&h.scope,&context.scope)) {
        // Called only AFTER actual Context.publish and its final current fence.
        h.publications.fetch_add(1,Ordering::SeqCst);
    }
    Ok(())
}
struct Armed { observed:oneshot::Receiver<Context>, release:Option<oneshot::Sender<()>>, publications:Arc<AtomicUsize> }
fn arm(review:&AgentCollectorServiceReview,point:Option<Point>)->Armed {
    let(observed,receiver)=oneshot::channel();let(release,hold)=oneshot::channel();
    let publications=Arc::new(AtomicUsize::new(0));
    let mut slot=hook().lock().unwrap();assert!(slot.is_none());
    *slot=Some(Hook {core:review.shared.context.core.clone(),scope:review.shared.context.scope.clone(),point,
        gate:Some((observed,hold)),publications:publications.clone()});
    Armed {observed:receiver,release:Some(release),publications}
}
impl Drop for Armed {fn drop(&mut self){hook().lock().unwrap().take();}}

struct Ipc {root:PathBuf,token:Uuid,case:&'static str}
impl Ipc {
    fn path(&self,name:&str)->PathBuf {assert!(!name.contains('/')&&!name.contains('\\'));self.root.join(format!("{}-{name}.json",self.case))}
    fn publish(&self,name:&str,body:Value) {
        let bytes=serde_json::to_vec(&json!({"format":FORMAT,"token":self.token,"case":self.case,"body":body})).unwrap();
        assert!(bytes.len()<=32768);
        let mut file=tempfile::Builder::new().prefix(".native-lifetime-").tempfile_in(&self.root).unwrap();
        file.write_all(&bytes).unwrap();file.as_file().sync_all().unwrap();
        file.persist_noclobber(self.path(name)).unwrap();
    }
    async fn wait<T:DeserializeOwned>(&self,name:&str)->T {
        loop {let path=self.path(name);
            if let Ok(meta)=std::fs::symlink_metadata(&path) {
                assert!(meta.is_file()&&!meta.file_type().is_symlink()&&meta.len()<=32768);
                let mut raw=Vec::new();Read::by_ref(&mut File::open(path).unwrap()).take(32769).read_to_end(&mut raw).unwrap();
                assert!(raw.len()<=32768);
                let v:Value=serde_json::from_slice(&raw).unwrap();
                assert_eq!(v["format"],FORMAT);assert_eq!(v["token"],json!(self.token));assert_eq!(v["case"],self.case);
                assert_eq!(v.as_object().unwrap().len(),4);
                return serde_json::from_value(v["body"].clone()).unwrap()
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    async fn barrier(&self,name:&str,phase:&str) {
        let b:Barrier=self.wait(name).await;assert_eq!(b.phase,phase);
        // Scheduling only. This cannot establish peer trust or sign/grant data.
    }
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Ready {host:String,port:u16,user:String,subsystem:String,host_public_key:String}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Barrier {phase:String}
fn value(raw:String)->Value {serde_json::from_str(&raw).unwrap()}
fn value_hash(value:&Value)->String {hex::encode(sha2::Sha256::digest(serde_json::to_vec(value).unwrap()))}
async fn frame(app:&AppCore)->Value {
    let(s,u)=app.unlocked().await.unwrap();
    json!({"version":1,"profileId":s.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap()})
}
async fn changed(events:&mut tokio::sync::broadcast::Receiver<crate::AppEvent>,kind:cc_models::ObjectKind,id:&str) {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {if let crate::AppEvent::ObjectsChanged {kind:k,ids,origin:crate::ChangeOriginDto::Local}=events.recv().await.unwrap() {
            if k==kind&&ids.iter().any(|v|v==id){break}
        }}
    }).await.expect("actual FIFO writer/readback completed before Context capture");
}
async fn password_ref(app:&AppCore,name:&str,target:Value)->Value {
    let mut events=app.subscribe_events();
    let c=app.add_password_credential(name.into(),Some(format!("owned-{name}")),format!("synthetic-{name}-password42")).await.unwrap();
    changed(&mut events,cc_models::ObjectKind::Credential,&c.id).await;
    let id=crate::dto::parse_id("credential",&c.id).unwrap();
    let revision=app.unlocked().await.unwrap().1.working().revision_of(id).unwrap();
    json!({"target":target,"credentialId":c.id,"expectedCredentialRevision":revision.to_string()})
}
async fn row(storage:&cc_storage_core::Storage,association:&str)->Value {
    let rows=storage.setting_get::<Value>(LOCAL_ROWS).await.unwrap().unwrap();
    rows["records"].as_array().unwrap().iter().find(|r|r["public"]["association_id"]==association).unwrap().clone()
}
async fn retired(review:&AgentCollectorServiceReview)->Value {
    review.cancel();let joined=value(review.joined_json().await.unwrap());
    assert_eq!(joined["cleanupState"],"closedObserved");joined
}
async fn prepare(app:&AppCore,request:&Value,legacy:bool)->AgentCollectorServiceReview {
    if legacy {app.clone().agent_collector_service_prepare_json(&request.to_string()).await.unwrap()}
    else {app.clone().agent_collector_service_lifetime_prepare_json(&request.to_string()).await.unwrap()}
}

#[cfg(test)]
#[tokio::test(flavor="multi_thread",worker_threads=4)]
#[ignore="requires pinned genuine public-entry182 Native + feature-only Node and real owned SSH/Actor/SQLite/catalog fixture"]
async fn actual_public_lifetime_nine_cases_original_context_and_signed_set_receipt() {
    run_selected(false).await;
}
#[cfg(test)]
#[tokio::test(flavor="multi_thread",worker_threads=4)]
#[ignore="requires same owned paired fixture with actual default-feature Node, two original saved refs and private registered receiver read"]
async fn actual_public_normal_lifetime_receiver_read_original_owner() {
    run_selected(true).await;
}
async fn run_selected(normal:bool) {
    assert_eq!(std::env::var("CC_LIFETIME_PAIR_PROOF").unwrap(),"1");
    tokio::time::timeout(Duration::from_secs(300),run(normal)).await.expect("same single 300s deadline; no retry");
}
#[cfg(all(feature="test-harness",any(target_os="linux",target_os="windows")))]
impl AppCore {
    pub(crate) async fn __owned_normal_lifetime_receiver_fixture() { run_selected(true).await; }
}
async fn run(normal:bool) {
    let raw=std::env::var("CC_LIFETIME_PAIR_TOKEN").unwrap();let token=Uuid::parse_str(&raw).unwrap();
    assert!(!token.is_nil()&&token.to_string()==raw);
    let root=PathBuf::from(std::env::var_os("CC_LIFETIME_PAIR_ROOT").unwrap());
    // Deliberately separate runtime workspace from the compiler source mount.
    // Root's /work/client/rust/app-core freeze is not the Windows checkout.
    // Missing/mismatched build binding fails before any profile or SSH work.
    const WINDOWS_CONTROL:&str=r"D:\Vibecode\ConsoleCrypt\Tasks\node-enroll\control";
    const LINUX_CONTROL:&str="/work/Tasks/node-enroll/control";
    let compiled=option_env!("CC_LIFETIME_PAIR_CONTROL_PREFIX").expect("actual as-built control prefix required");
    assert!((cfg!(target_os="windows")&&compiled==WINDOWS_CONTROL)
        ||(cfg!(target_os="linux")&&compiled==LINUX_CONTROL));
    let allowed=Path::new(compiled).canonicalize().unwrap();
    assert_eq!(root.canonicalize().unwrap().parent(),Some(allowed.as_path()));
    assert_eq!(root.file_name().unwrap().to_str().unwrap(),format!("owned-{token}"));
    for p in root.ancestors(){assert!(!std::fs::symlink_metadata(p).unwrap().file_type().is_symlink());}
    let cases:&[&'static str]=if normal {&["positiveLifetime7"]}else{&CASES};
    for &case in cases {run_case(Ipc {root:root.clone(),token,case},normal).await;}
    let ipc=Ipc {root,token,case:"all"};ipc.publish("native-complete",json!({"cases":cases,
        "protocolComponentOnly":!normal,"installedServiceAcceptance":false,"shutdownObserved":true}));
}
async fn run_case(ipc:Ipc,normal:bool) {
    let profile=tempfile::Builder::new().prefix("native-lifetime-").tempdir_in(&ipc.root).unwrap();
    let mut config=crate::AppConfig::for_tests(profile.path().to_string_lossy().into_owned());
    config.background_sync=false;config.auto_start_tunnels=false;
    let app=AppCore::with_platform(config,Arc::new(InMemorySecureStore::new()),Arc::new(UnsupportedDeviceKeys)).unwrap();
    app.create_local_profile(format!("Owned lifetime {}",ipc.case),PASSWORD.into()).await.unwrap();
    // All are real saved Password refs, before original prepare scopes exist.
    let scrape=password_ref(&app,"exporter",json!({"kind":"scrape","targetId":"exporter"})).await;
    let receiver=password_ref(&app,"receiver",json!({"kind":"receiver"})).await;
    let alternate=password_ref(&app,"alternate",json!({"kind":"receiver"})).await;
    let ssh=cc_ssh_core::keys::generate_key(cc_ssh_core::keys::KeyGenAlgorithm::Ed25519,"owned lifetime Native",None).unwrap();
    let public_ssh=ssh.public_openssh.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    let mut events=app.subscribe_events();
    let ssh_credential=app.import_ssh_key("Owned lifetime SSH".into(),Some("ccenroll".into()),ssh.private_openssh.expose_secret().to_owned(),None,false,None).await.unwrap();
    drop(ssh);changed(&mut events,cc_models::ObjectKind::Credential,&ssh_credential.id).await;drop(events);
    let f=frame(&app).await;
    let identity=app.agent_node_client_identity_prepare_json(&f.to_string()).await.unwrap();
    assert_eq!(value(identity.confirm_creation().await.unwrap())["phase"],"stored");
    assert_eq!(value(identity.joined_json().await.unwrap())["cleanupState"],"closedObserved");drop(identity);
    let bootstrap_scope=Arc::new(OriginalScope::capture(&app,&CancellationToken::new(),Duration::from_secs(300)).await.unwrap());
    let tls=crate::agent_tunnel::association::ManagedTlsCustody::capture(&app,bootstrap_scope.clone()).await.unwrap()
        .load(&app).await.unwrap().public_identity().unwrap().clone();
    let owner_key=bootstrap_scope.with_loaded(&app,|id|Ok(PublicKey::from_bytes(id.public_keys().signing))).await.unwrap();
    let now=bootstrap_scope.now().unwrap();let local=Uuid::new_v4();
    assert_ne!(local.to_string(),f["profileId"].as_str().unwrap());
    let agent=cc_agent_node_wire::AgentId::new(&Uuid::new_v4().to_string()).unwrap();
    let pin=cc_agent_node_wire::OwnerPin {context:cc_agent_node_wire::Context {
        tenant:cc_agent_node_wire::Tenant::LocalProfile{id:cc_agent_node_wire::LocalProfileId::new(&local.to_string()).unwrap()},
        deployment_id:cc_agent_node_wire::DeploymentId::new(&Uuid::new_v4().to_string()).unwrap()},
        key:owner_key,epoch:1,checkpoint:0,expires_at:now.0.checked_add(600).unwrap()};
    ipc.publish("bootstrap",json!({"agentId":agent,"genesisOwnerPin":pin,"sshClientPublicKey":public_ssh,"clientTlsPublic":tls,
        "requiredScrapeTargetId":"exporter","requireTwoBasicAuthRefs":true}));drop(bootstrap_scope);
    let endpoint:Ready=ipc.wait("ssh-ready").await;
    assert!(endpoint.host=="127.0.0.1"&&endpoint.port>0&&endpoint.user=="ccenroll"&&endpoint.subsystem=="consolecrypt-agent-enroll-v1");
    let parts:Vec<_>=endpoint.host_public_key.split_whitespace().collect();
    assert!(parts.len()==2&&parts[0]=="ssh-ed25519"&&parts.join(" ")==endpoint.host_public_key);
    app.import_known_hosts(format!("[127.0.0.1]:{} {}\n",endpoint.port,endpoint.host_public_key)).await.unwrap();
    let mut h=crate::HostDto::new("Owned lifetime pinned Node","127.0.0.1");h.port=Some(endpoint.port);
    h.username=Some(endpoint.user);h.credential_id=Some(ssh_credential.id);h.host_key_policy=cc_ssh_core::HostKeyPolicy::Strict;
    let mut events=app.subscribe_events();let h=app.save_host(h).await.unwrap();
    changed(&mut events,cc_models::ObjectKind::Host,&h.id).await;drop(events);
    let host=crate::dto::parse_id("host",&h.id).unwrap();let revision=app.unlocked().await.unwrap().1.working().revision_of(host).unwrap();
    ipc.publish("saved-host",json!({"hostId":host}));
    ipc.barrier("catalog-pinned","catalogPinned").await;
    let mut registration=frame(&app).await;registration["hostId"]=json!(host);registration["expectedHostRevision"]=json!(revision.to_string());
    let enrollment=app.clone().agent_metrics_node_association_register_json(&registration.to_string()).await.unwrap();
    let offer=value(enrollment.probe().await.unwrap());assert_eq!(offer["node"]["agentId"],json!(agent));assert_eq!(offer["phase"],"offerReview");
    let claim=value(enrollment.approve_offer().await.unwrap());assert_eq!(claim["phase"],"claimReview");
    let lifetime=120u32.min(claim["maxCertificateLifetimeSeconds"].as_u64().unwrap().try_into().unwrap());assert!(lifetime>0);
    let associated=value(enrollment.approve_certificate(lifetime).await.unwrap());assert_eq!(associated["phase"],"associated");
    assert_eq!(value(enrollment.joined_json().await.unwrap())["cleanupState"],"closedObserved");drop(enrollment);
    let association=associated["associationId"].as_str().unwrap().to_owned();
    let mut catalog_request=frame(&app).await;catalog_request["associationId"]=json!(association);
    let catalog=value(app.clone().agent_collector_service_catalog_json(&catalog_request.to_string()).await.unwrap());
    assert_eq!(catalog["cleanupState"],"closedObserved");
    let mut request=catalog_request;request["collectorId"]=catalog["collectorId"].clone();
    request["manifestSha256"]=catalog["manifestSha256"].clone();request["credentials"]=json!([scrape,receiver]);
    ipc.publish("arm-service",json!({"associationId":association,"fault":ipc.case,"originalProbeComplete":true}));
    ipc.barrier("service-armed","serviceFixtureArmed").await;
    let legacy=ipc.case=="legacySingle5";
    if legacy {
        // A real old public review cannot be upgraded even by private tamper.
        let old=prepare(&app,&request,true).await;
        old.shared.state.lock().unwrap().ready.as_mut().unwrap().credential_protocol=CredentialProtocol::ServiceLifetime;
        assert!(old.confirm_configuration().await.is_err());retired(&old).await;drop(old);
    }
    let review=prepare(&app,&request,legacy).await;
    let details=value(review.details_json().await.unwrap());
    if !legacy {assert_eq!(details["kind"],"finiteCollectorServiceLifetimeCredentials");
        assert_eq!(details["credentialPurpose"],"metrics-collector-service-credential-authorization-v1");}
    let storage=app.session().await.unwrap().storage().unwrap();
    let before=row(&storage,&association).await;assert!(before["collector_service"].is_null());
    let point=if normal {Some(Point::BeforeLifetimeAdmit)}else{match ipc.case {"lateSqlCas"=>Some(Point::AfterVerifiedReceipt),"lockBeforePublish"=>Some(Point::BeforePublish),_=>None}};
    let mut armed=arm(&review,point);
    let mut secondary=None;let mut displaced=None;
    if matches!(ipc.case,"foreignPeer"|"foreignScope"|"refSwap") {
        let mut other_request=request.clone();
        if ipc.case=="refSwap" {other_request["credentials"][1]=alternate;}
        let other=prepare(&app,&other_request,false).await;
        assert!(!Arc::ptr_eq(&review.shared.context.scope,&other.shared.context.scope));
        {let mut a=review.shared.state.lock().unwrap();let mut b=other.shared.state.lock().unwrap();
            let a=a.ready.as_mut().unwrap();let b=b.ready.as_mut().unwrap();
            match ipc.case {
                "foreignPeer"=>{displaced=a.peer.take();a.peer=b.peer.take();},
                "foreignScope"=>a.review_identity.scope=b.review_identity.scope.clone(),
                "refSwap"=>a.review_identity.credentials=b.review_identity.credentials.clone(),_=>unreachable!(),
            }
        }
        secondary=Some(other);
    }
    let actual_context=review.shared.context.clone();
    let original_owner={let s=review.shared.state.lock().unwrap();s.ready.as_ref().unwrap().ledger.audit_owner().unwrap()};
    let mut sql_before_hook=Value::Null;
    let result=if let Some(point)=point {
        let confirm=review.confirm_configuration();tokio::pin!(confirm);
        let actual=tokio::select!{v=&mut armed.observed=>v.unwrap(), r=&mut confirm=>panic!("actual fault point was never reached: {}",r.is_ok())};
        assert!(Arc::ptr_eq(&actual.scope,&actual_context.scope));actual.check().await.unwrap();
        sql_before_hook=row(&storage,&association).await;
        assert!(sql_before_hook["collector_service"]["credential_set"].is_object());
        if point==Point::BeforeLifetimeAdmit {
            assert!(normal&&ipc.case=="positiveLifetime7");
            let set=&sql_before_hook["collector_service"]["credential_set"];
            assert!(set["signed_admission_receipt_json"].is_null());
            let authorization:Signed<cc_agent_node_wire::CollectorServiceCredentialAuthorization>=
                serde_json::from_slice(&serde_json::from_value::<Vec<u8>>(set["authorization_json"].clone()).unwrap()).unwrap();
            cc_agent_node_wire::verify_statement(&authorization,review.shared.facts.pin.key).unwrap();
            let positions=authorization.body.credentials.iter().enumerate().filter(|(_,r)|
                matches!(&r.target,cc_agent_node_wire::CollectorCredentialTarget::Receiver {})).collect::<Vec<_>>();
            assert_eq!(positions.len(),1);let(slot,reference)=positions[0];
            assert_eq!(reference.username,"owned-receiver");
            let digest=authorization.body.digest().unwrap();
            assert_eq!(hex::encode(digest.as_bytes()),set["authorization_sha256"]);
            ipc.publish("normal-read-plan",json!({"session":hex::encode(authorization.body.recipient.body.original_start_nonce.as_bytes()),
                "leaf":format!("credential-{}.password",hex::encode(authorization.body.capsule_delegation_digest(slot).unwrap().as_bytes())),
                "authorizationDigest":digest,"manifestSha256":request["manifestSha256"],"receiverUrl":"http://127.0.0.1:19109/api/v1/write"}));
            // Scheduling-only observer arm. It is never an ACK, Permit or
            // replacement for the following original signed Node admission.
            ipc.barrier("normal-read-armed","normalReadObserverArmed").await;
            actual.check().await.unwrap();
        } else if point==Point::AfterVerifiedReceipt {
            assert!(sql_before_hook["collector_service"]["credential_set"]["signed_admission_receipt_json"].is_null());
            let id=Uuid::parse_str(&association).unwrap();
            storage.write(move |tx|{let s=tx.node_association_snapshot(&original_owner)?;tx.node_association_block(&s,id)}).await.unwrap();
            actual.check().await.unwrap(); // genuine SQL conflict, original Context remains live.
        } else {
            assert!(sql_before_hook["collector_service"]["credential_set"]["signed_admission_receipt_json"].is_array());
            app.lock().await.unwrap();assert!(actual.check().await.is_err());
        }
        let _=armed.release.take().unwrap().send(());confirm.await
    } else {review.confirm_configuration().await};
    let success=matches!(ipc.case,"positiveLifetime7"|"legacySingle5");
    // Opaque confirm may return an Ok JSON view whose phase is Unknown after
    // a post-dispatch failure. API return success is not admission evidence.
    let returned=result.as_ref().ok().map(|raw|value(raw.clone()));
    let configuration_recorded=returned.as_ref().is_some_and(|v|v["phase"]=="configurationRecorded");
    assert_eq!(configuration_recorded,success);
    if secondary_case(ipc.case) {assert!(result.is_err());}
    else if !success {if let Some(view)=&returned {assert_eq!(view["phase"],"unknown");}}
    assert_eq!(armed.publications.load(Ordering::SeqCst),usize::from(ipc.case=="positiveLifetime7"));
    let after=row(&storage,&association).await;
    if matches!(ipc.case,"foreignPeer"|"foreignScope"|"refSwap") {assert_eq!(before,after);}
    else if matches!(ipc.case,"badSignedReceipt"|"lostSignedReceipt"|"lateSqlCas") {
        assert!(after["collector_service"]["credential_set"]["signed_admission_receipt_json"].is_null());
    } else if !legacy {assert!(after["collector_service"]["credential_set"]["signed_admission_receipt_json"].is_array());}
    drop(displaced);let cleanup=retired(&review).await;
    if let Some(other)=secondary {retired(&other).await;drop(other);}
    ipc.publish("native-case",json!({"succeeded":configuration_recorded,"apiReturnedView":result.is_ok(),"publicationCount":armed.publications.load(Ordering::SeqCst),
        "localBeforeSha256":value_hash(&before),"localAfterSha256":value_hash(&after),
        "sqlAtActualHookSha256":value_hash(&sql_before_hook),"callerCleanup":cleanup}));
    // This releases real original Node owner stdin. It is NOT an ACK or a join proof.
    ipc.publish("stop-owner",json!({"requestOnly":true}));
    let evidence:Value=ipc.wait("fixture-result").await;
    if normal {
        assert_eq!(evidence["protocolComponentOnly"],false);
        assert_eq!(evidence["ordinaryNodeUsed"],true);assert_eq!(evidence["protocolFeatureAbsent"],true);
        assert_eq!(evidence["originalWorkerTreeRetired"],true);
        let sessions=evidence["serviceSessions"].as_array().unwrap();assert_eq!(sessions.len(),1);
        assert_eq!(sessions[0]["methods"],json!(["probeCollectorServiceLifetime","configureCollectorService","collectorServiceRecipient",
            "stageCollectorServiceLifetimeFrame","stageCollectorServiceLifetimeFrame","stageCollectorServiceLifetimeFrame","admitCollectorServiceLifetimeSet"]));
        let set=&after["collector_service"]["credential_set"];
        let authorization:Signed<cc_agent_node_wire::CollectorServiceCredentialAuthorization>=
            serde_json::from_slice(&serde_json::from_value::<Vec<u8>>(set["authorization_json"].clone()).unwrap()).unwrap();
        let signed:Signed<cc_agent_node_wire::CollectorServiceCredentialSetReceipt>=serde_json::from_value(evidence["genuineSignedReceipt"].clone()).unwrap();
        let hashes:Vec<Hash>=serde_json::from_value(set["ordered_capsule_sha256"].clone()).unwrap();
        let now=cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()).unwrap());
        cc_agent_node_wire::verify_collector_service_set_receipt(&review.shared.facts.pin,&review.shared.facts.certificate,
            &authorization,&signed,&hashes,set["minimum_actor_revision"].as_u64().unwrap(),now).unwrap();
        assert_eq!(signed.body.original_start_nonce,authorization.body.recipient.body.original_start_nonce);
        let read=&evidence["actualScopedSecretRead"];
        assert_eq!(read["authorizationDigest"],json!(authorization.body.digest().unwrap()));
        assert_eq!(read["session"],hex::encode(signed.body.original_start_nonce.as_bytes()));
        for field in ["receiverPasswordMatched","privateProjectionMatched","namespaceHeldStable","leafHeldStable","ordinaryOwnerExecutableMatched"] {
            assert_eq!(read[field],true);
        }
        assert_eq!(evidence["originalOwner"]["ownerExit"],0);
        assert_eq!(evidence["originalOwner"]["ownerWaited"],true);
        assert_eq!(evidence["originalOwner"]["readHandlesClosed"],true);
        assert_eq!(evidence["fault"],"none");
        drop(armed);drop(review);app.shutdown().await.unwrap();
        ipc.publish("native-retired",json!({"shutdownObserved":true,"caseAccepted":true,"protocolComponentOnly":false,
            "installedServiceAcceptance":false,"actualScopedSecretRead":true}));
        return;
    }
    assert_eq!(evidence["protocolComponentOnly"],true);assert_eq!(evidence["collectorSpawned"],false);
    assert_eq!(evidence["originalWorkerTreeRetired"],true);
    let sessions=evidence["serviceSessions"].as_array().unwrap();
    let main=if ipc.case=="legacySingle5" {1}else{0};
    let expected:Vec<&str>=if matches!(ipc.case,"foreignPeer"|"foreignScope"|"refSwap") {
        vec!["probeCollectorServiceLifetime"]
    } else if legacy {vec!["probeCollectorService","configureCollectorService","collectorServiceRecipient","deliverCollectorServiceCredential","deliverCollectorServiceCredential"]}
    else {vec!["probeCollectorServiceLifetime","configureCollectorService","collectorServiceRecipient",
        "stageCollectorServiceLifetimeFrame","stageCollectorServiceLifetimeFrame","stageCollectorServiceLifetimeFrame","admitCollectorServiceLifetimeSet"]};
    assert_eq!(sessions[main]["methods"],json!(expected));
    assert_eq!(sessions.len(),if secondary_case(ipc.case)||legacy {2}else{1});
    if secondary_case(ipc.case)||legacy {assert_eq!(sessions[if legacy{0}else{1}]["methods"],json!([if legacy{"probeCollectorService"}else{"probeCollectorServiceLifetime"}]));}
    let node=&evidence["originalOwner"];
    assert_eq!(node["format"],"cc-owned-lifetime-protocol-owner/v1");assert_eq!(node["token"],json!(ipc.token));
    if secondary_case(ipc.case) {assert_eq!(node["startsReserved"],0);assert_eq!(node["noOriginalFilesCreated"],true);
        assert_eq!(node["actorRevision"],associated["actorRevision"].as_str().unwrap().parse::<u64>().unwrap());}
    else {assert_eq!(node["startsReserved"],1);assert_eq!(node["originalFilesRetired"],true);
        assert_eq!(node["endCommitted"],true);assert_eq!(node["endReason"],"NoChildCreated");
        if legacy {assert!(node["admission"].is_null());}else{
            assert_eq!(node["admission"]["authorizationDigest"],after["collector_service"]["credential_set"]["authorization_sha256"]);
            assert_eq!(node["admission"]["orderedCapsuleHashes"],after["collector_service"]["credential_set"]["ordered_capsule_sha256"]);
            let signed:Signed<cc_agent_node_wire::CollectorServiceCredentialSetReceipt>=serde_json::from_value(evidence["genuineSignedReceipt"].clone()).unwrap();
            cc_agent_node_wire::verify_statement(&signed,review.shared.facts.node.public_key).unwrap();
            assert_eq!(json!(signed.body.authorization_digest),node["admission"]["authorizationDigest"]);
            assert_eq!(json!(signed.body.admitted_revision),node["admission"]["admittedRevision"]);
            assert_eq!(signed.body.actor_revision,signed.body.admitted_revision);
            assert_eq!(node["actorRevision"],signed.body.admitted_revision.checked_add(1).unwrap());
            let set=&after["collector_service"]["credential_set"];
            let authorization:Signed<cc_agent_node_wire::CollectorServiceCredentialAuthorization>=
                serde_json::from_slice(&serde_json::from_value::<Vec<u8>>(set["authorization_json"].clone()).unwrap()).unwrap();
            let hashes:Vec<Hash>=serde_json::from_value(node["admission"]["orderedCapsuleHashes"].clone()).unwrap();
            let now=cc_agent_node_wire::Now(cc_agent_node_wire::Timestamp::new(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()).unwrap());
            cc_agent_node_wire::verify_collector_service_set_receipt(&review.shared.facts.pin,&review.shared.facts.certificate,
                &authorization,&signed,&hashes,set["minimum_actor_revision"].as_u64().unwrap(),now).unwrap();
        }
    }
    if ipc.case=="badSignedReceipt" {assert_eq!(evidence["fault"],"flippedActualReceiptSignature");}
    else if ipc.case=="lostSignedReceipt" {assert_eq!(evidence["fault"],"droppedActualReceipt");}
    else {assert_eq!(evidence["fault"],"none");}
    drop(armed);drop(review);app.shutdown().await.unwrap();
    ipc.publish("native-retired",json!({"shutdownObserved":true,"caseAccepted":true,"protocolComponentOnly":true,
        "installedServiceAcceptance":false}));
}
fn secondary_case(case:&str)->bool {matches!(case,"foreignPeer"|"foreignScope"|"refSwap")}
