//! SYNTHETIC OWNERS ONLY. Public known keys11/55 and token33 for private tests.
//! Never install/run this example as a production authority. Node keys are not
//! accepted as input: only public hello and actual node-signed claims are read.
#[cfg(not(all(target_os="linux", target_arch="aarch64")))]
compile_error!("ARM installed-acceptance fixture requires native Linux AArch64 target");
use cc_agent_identity_policy::*;
use ed25519_dalek::{Signer, SigningKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
fn owner() -> SigningKey {
    SigningKey::from_bytes(&[11; 32])
}
fn sign<T: Statement>(body: T) -> Signed<T> {
    let signature =
        SignatureBytes::from_bytes(owner().sign(&body.signing_bytes().unwrap()).to_bytes());
    Signed { body, signature }
}
fn sign_at_pin<T: Statement>(body: T, pin: &OwnerPin) -> Signed<T> {
    // Test-only pinned selection, never read/export an owner private file and
    // never silently use the old owner after a rotation.
    let key = [11, 55]
        .into_iter()
        .map(|n| SigningKey::from_bytes(&[n; 32]))
        .find(|key| PublicKey::from_bytes(key.verifying_key().to_bytes()) == pin.key)
        .expect("only explicit synthetic fixture owner pins are supported");
    Signed {
        signature: SignatureBytes::from_bytes(key.sign(&body.signing_bytes().unwrap()).to_bytes()),
        body,
    }
}
fn read<T: DeserializeOwned>(path: &Path) -> T {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .unwrap()
        .take(MAX_WIRE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= MAX_WIRE_BYTES);
    serde_json::from_slice(&bytes).unwrap()
}
fn write(path: &Path, value: &impl Serialize) {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}
fn argument(args: &[String], name: &str) -> String {
    let found: Vec<_> = args.windows(2).filter(|v| v[0] == name).collect();
    assert_eq!(found.len(), 1, "required exact test argument");
    found[0][1].clone()
}
struct Provider(Box<dyn CompareAndSwap>);
impl CompareAndSwap for Provider {
    fn load(&self) -> Result<State> {
        self.0.load()
    }
    fn compare_and_swap(&mut self, revision: u64, next: State) -> Result<()> {
        self.0.compare_and_swap(revision, next)
    }
}
fn provider(out: &Path, mode: &str, pin: OwnerPin, create: Option<Now>) -> Provider {
    let store: Box<dyn CompareAndSwap> = match mode {
        "file" => Box::new(if let Some(now) = create {
            FileStore::create(
                out.join("authority-store"),
                out.join("authority-anchor"),
                pin,
                now,
            )
            .unwrap()
        } else {
            FileStore::open(
                out.join("authority-store"),
                out.join("authority-anchor"),
                pin,
            )
            .unwrap()
        }),
        "sqlite" => {
            #[cfg(target_os = "linux")]
            {
                Box::new(if let Some(now) = create {
                    SqliteJournalStore::create(
                        out.join("authority-db"),
                        out.join("authority-anchor"),
                        pin,
                        now,
                        SqliteStorageBudget::default(),
                    )
                    .unwrap()
                } else {
                    SqliteJournalStore::open(
                        out.join("authority-db"),
                        out.join("authority-anchor"),
                        pin,
                        SqliteStorageBudget::default(),
                    )
                    .unwrap()
                })
            }
            #[cfg(not(target_os = "linux"))]
            {
                panic!("SQLite test provider is Linux-only")
            }
        }
        _ => panic!("unknown explicit test provider"),
    };
    Provider(store)
}
fn state(out: &Path, mode: &str) -> Actor<Provider> {
    let pin: OwnerPin = read(&out.join("owner-pin.json"));
    Actor::new(provider(out, mode, pin, None)).unwrap()
}
fn header(pin: &OwnerPin, checkpoint: u64) -> Header {
    Header {
        version: 1,
        context: pin.context.clone(),
        owner_epoch: pin.epoch,
        checkpoint,
    }
}

// Additional explicit purposes ONLY for the private installed-disk fixture.
// No caller policy/key/domain/signature bytes, credentials or BOOT stamp input.
fn metrics_arguments(args:&[String]) {
    let extra=if args[0]=="metrics-offer" {vec!["--node-public","--profile-id","--deployment-id"]}
        else if args[0]=="metrics-approve" {vec!["--claim"]} else {vec![]};
    assert!(matches!(args[0].as_str(),"metrics-offer"|"metrics-approve"|"metrics-lease"|
        "collector-service-grant"|"collector-service-revoke"|"collector-service-status"));
    assert_eq!((args.len()-1)%2,0);
    for pair in args[1..].chunks_exact(2) {
        assert!(["--out","--now","--provider"].contains(&pair[0].as_str())||extra.contains(&pair[0].as_str()));
        assert_eq!(args[1..].chunks_exact(2).filter(|p|p[0]==pair[0]).count(),1);
    }
    for key in extra {let _=argument(args,key);}
    let supplied:u64=argument(args,"--now").parse().unwrap();
    let observed=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert!(supplied.abs_diff(observed)<=2,"actual fixture wall clock only");
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct MetricsProbe {
    node:Value, owner_pin:OwnerPin, actor_revision:u64, certificate:Certificate,
    revoked:bool, advertised_scopes:Scopes, collector:Option<MetricsCatalog>,
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct MetricsCatalog {collector_id:ResourceId,manifest_sha256:Hash,manifest:Value}
fn verify_fixture<T:Statement>(signed:&Signed<T>,pin:&OwnerPin) {
    assert_eq!(pin.key,PublicKey::from_bytes(owner().verifying_key().to_bytes()));
    owner().verifying_key().verify_strict(&signed.body.signing_bytes().unwrap(),
        &ed25519_dalek::Signature::from_bytes(signed.signature.as_bytes())).unwrap();
}
fn metrics_probe(out:&Path,mode:&str,now:Now,command:&str)->(OwnerPin,Signed<Certificate>,MetricsProbe,MetricsCatalog) {
    // The guest saves ONLY the payload of its original normal-Node probe here.
    // This file alone grants nothing: actual Node signature/CAS remains required.
    let raw:Value=read(&out.join("metrics-probe.json"));
    assert_eq!(raw.as_object().unwrap().len(),7);assert!(raw.get("collector").is_some());
    let probe:MetricsProbe=serde_json::from_value(raw).unwrap();
    let authority=state(out,mode);let snapshot=authority.snapshot().unwrap();let pin=snapshot.owner().clone();
    assert_eq!(pin.key,PublicKey::from_bytes(owner().verifying_key().to_bytes()));
    assert_eq!(pin.epoch,1);assert_eq!(probe.owner_pin,pin);assert!(probe.actor_revision>0&&!probe.revoked);
    let cert:Signed<Certificate>=read(&out.join("certificate.json"));verify_fixture(&cert,&pin);
    assert_eq!(snapshot.certificate(cert.body.agent_id),Some(&cert.body));
    assert!(!snapshot.revoked(cert.body.agent_id));assert_eq!(probe.certificate,cert.body);
    assert_eq!(cert.body.scopes.0,vec![Scope::MetricsCollect]);
    assert_eq!(cert.body.capabilities.scopes,cert.body.scopes);
    assert_eq!(probe.advertised_scopes.0,vec![Scope::MetricsCollect]);
    assert!(now.0>=cert.body.not_before&&now.0<cert.body.expires_at&&now.0<pin.expires_at);
    assert_eq!(probe.node["agentId"],serde_json::to_value(cert.body.agent_id).unwrap());
    assert_eq!(probe.node["publicKey"],serde_json::to_value(cert.body.agent_key).unwrap());
    assert_eq!(probe.node["keyEpoch"],cert.body.key_epoch);
    assert_eq!(probe.node["keyFingerprint"],serde_json::to_value(cert.body.agent_key.fingerprint()).unwrap());
    let catalog=match &probe.collector {
        Some(catalog)=>catalog.clone(),
        None=>{
            assert!(matches!(command,"collector-service-status"|"collector-service-revoke"));
            // Actual status deliberately advertises null. The original public
            // catalog is data retained at approval, never a current observation.
            read(&out.join("collector-service-original-catalog.json"))
        }
    };
    let m=&catalog.manifest;
    assert_eq!(m["profile"],"maintained-prometheus-disk-512mib-2h-v1");
    assert_eq!(m["bufferMode"],"persistentNativeWal");
    assert_eq!(m["schema_version"],2);
    assert_eq!(m["buffer"]["max_bytes"],536870912u64);assert_eq!(m["buffer"]["max_age_seconds"],7200);
    // Validate the full production schema, including v2 ELF runtime tuple.
    let host=m["labels"]["cc_host_id"].as_str().unwrap();
    let digest=cc_agent_node_wire::validate_metrics_manifest(m,host).unwrap();
    assert_eq!(catalog.manifest_sha256.as_bytes(),digest.as_bytes());
    assert_eq!(catalog.collector_id,collector_service_resource(catalog.manifest_sha256).unwrap());
    if m["schema_version"]==2 {
        assert_eq!(m["runtime"]["kind"],"prometheusHardmode");
        assert_eq!(m["runtime"]["upstreamTag"],"3.16.0");
        assert_eq!(m["runtime"]["platform"],"linux-aarch64");
    }
    // Closed first subset: public loopback scrape/receiver, never credentials,
    // remote endpoints, arbitrary transports or any caller-supplied path.
    for endpoint in std::iter::once(&m["receiver"]["url"]).chain(m["targets"].as_array().unwrap().iter().map(|t|&t["url"])) {
        let url=endpoint.as_str().unwrap();
        assert!(url.starts_with("http://127.0.0.1:")&&!url.contains('@'));
    }
    (pin,cert,probe,catalog)
}
fn metrics_authorize(out:&Path,mode:&str,now:Now,command:&str)->Value {
    let(pin,certificate,probe,catalog)=metrics_probe(out,mode,now,command);
    let expires=now.0.checked_add(if command=="metrics-lease" {30}else{120}).unwrap()
        .min(certificate.body.expires_at).min(pin.expires_at);
    assert!(expires>now.0);
    if command=="metrics-lease" {
        let operation=Operation::MetricsCollect{collector:catalog.collector_id};
        let lease=sign_at_pin(Lease{header:header(&pin,pin.checkpoint),
            id:LeaseId::new("66666666-6666-4666-8666-666666666666").unwrap(),
            agent_id:certificate.body.agent_id,key_epoch:certificate.body.key_epoch,
            certificate_hash:certificate.body.digest().unwrap(),operation:operation.clone(),issued_at:now.0,expires_at:expires},&pin);
        let freshness=sign_at_pin(Freshness{header:header(&pin,pin.checkpoint),issued_at:now.0,expires_at:expires},&pin);
        let binding=NodeRequestBinding{certificate,lease,freshness,operation};
        write(&out.join("metrics-request.json"),&json!({"expectedRev":probe.actor_revision,
            "expectedPin":pin,"binding":binding,"collectorId":catalog.collector_id,"manifestSha256":catalog.manifest_sha256}));
    } else if command=="collector-service-grant" {
        assert!(probe.collector.is_some(),"grant requires original actual catalog probe");
        assert!(!out.join("collector-service-delegation.json").exists()
            &&!out.join("collector-service-original-catalog.json").exists(),"one original fixture service policy only");
        let policy=sign_at_pin(CollectorServiceDelegation{header:header(&pin,pin.checkpoint),
            service_id:ResourceId::new("88888888-8888-4888-8888-888888888888").unwrap(),
            policy_revision:1,revocation_epoch:1,agent_id:certificate.body.agent_id,
            key_epoch:certificate.body.key_epoch,recipient:certificate.body.agent_key,
            certificate_hash:certificate.body.digest().unwrap(),collector_id:catalog.collector_id,
            manifest_sha256:catalog.manifest_sha256,profile:CollectorServiceProfile::MaintainedPrometheusDisk512mib2hV1,
            quota:CollectorServiceQuota::fixed(),credential_targets:vec![],issued_at:now.0,expires_at:expires},&pin);
        policy.body.check_credential_targets(&catalog.manifest).unwrap();
        // BOOT stamp and the claim are generated only by the actual Node in
        // configureCollectorService, never derived from this fixture's input.
        write(&out.join("collector-service-original-catalog.json"),&catalog);
        write(&out.join("collector-service-delegation.json"),&policy);
        write(&out.join("collector-service-configure.json"),&json!({"expectedRev":probe.actor_revision,
            "expectedPin":pin,"delegation":policy}));
    } else {
        let policy:Signed<CollectorServiceDelegation>=read(&out.join("collector-service-delegation.json"));
        verify_fixture(&policy,&pin);
        assert_eq!(policy.body.agent_id,certificate.body.agent_id);
        assert_eq!(policy.body.certificate_hash,certificate.body.digest().unwrap());
        assert_eq!(policy.body.manifest_sha256,catalog.manifest_sha256);
        assert_eq!(policy.body.credential_targets.len(),0);
        if command=="collector-service-status" {
            write(&out.join("collector-service-observe.json"),&json!({"expectedRev":probe.actor_revision,
                "expectedPin":pin,"serviceId":policy.body.service_id,"delegationHash":policy.body.digest().unwrap()}));
            return json!({"testOnly":true,"command":command,"readOnly":true,"automaticRenewal":false});
        }
        let revoke=sign_at_pin(CollectorServiceRevocation{header:header(&pin,pin.checkpoint),
            service_id:policy.body.service_id,delegation_hash:policy.body.digest().unwrap(),
            policy_revision:1,revocation_epoch:2,issued_at:now.0},&pin);
        write(&out.join("collector-service-revoke.json"),&json!({"expectedRev":probe.actor_revision,
            "expectedPin":pin,"revocation":revoke}));
    }
    json!({"testOnly":true,"command":command,"freshMetricsOnly":true,"credentials":false,
        "interactiveTtlSeconds":30,"serviceTtlSeconds":120,"automaticRenewal":false})
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(!args.is_empty(), "test subcommand required");
    let out = PathBuf::from(argument(&args, "--out"));
    assert!(
        out.is_absolute() && out.is_dir(),
        "private existing absolute test output required"
    );
    let now = Now(Timestamp::new(argument(&args, "--now").parse().unwrap()).unwrap());
    let mode = args
        .windows(2)
        .find(|v| v[0] == "--provider")
        .map(|v| v[1].as_str())
        .unwrap_or("file");
    let metrics = args[0].starts_with("metrics-") || args[0].starts_with("collector-service-");
    if metrics { metrics_arguments(&args); }
    let summary = match args[0].as_str() {
        "offer" | "metrics-offer" => {
            let node: Value = read(&PathBuf::from(argument(&args, "--node-public")));
            assert_eq!(node["format"], 1);
            assert_eq!(node["keyEpoch"], 1);
            let agent_id: AgentId = serde_json::from_value(node["agentId"].clone()).unwrap();
            let key: PublicKey = serde_json::from_value(node["publicKey"].clone()).unwrap();
            let fingerprint: Hash = serde_json::from_value(node["keyFingerprint"].clone()).unwrap();
            assert_eq!(fingerprint, key.fingerprint());
            let context = Context {
                tenant: Tenant::LocalProfile {
                    id: LocalProfileId::new(&argument(&args, "--profile-id")).unwrap(),
                },
                deployment_id: DeploymentId::new(&argument(&args, "--deployment-id")).unwrap(),
            };
            let pin = OwnerPin {
                context,
                key: PublicKey::from_bytes(owner().verifying_key().to_bytes()),
                epoch: 1,
                checkpoint: 0,
                expires_at: now.0.checked_add(3600).unwrap(),
            };
            let mut authority = Actor::new(provider(&out, mode, pin.clone(), Some(now))).unwrap();
            let offer = sign(Offer {
                header: header(&pin, 1),
                invitation_id: InvitationId::new("33333333-3333-4333-8333-333333333333").unwrap(),
                agent_id,
                agent_key: key,
                token_hash: InvitationToken::from_bytes([33; 32]).unwrap().digest(),
                scopes: Scopes(vec![if metrics { Scope::MetricsCollect } else { Scope::TelemetryRead }]),
                issued_at: now.0,
                expires_at: now.0.checked_add(600).unwrap(),
            });
            authority.offer(offer.clone(), now).unwrap();
            write(&out.join("owner-pin.json"), &pin);
            write(&out.join("node-owner-pin.json"), &pin);
            write(&out.join("public-node.json"), &node);
            write(&out.join("offer.json"), &offer);
            write(&out.join("claim-input.json"), &json!({"offer":offer}));
            json!({"testOnly":true,"command":"offer","claimInput":out.join("claim-input.json"),"nodeOwnerPin":out.join("node-owner-pin.json")})
        }
        "approve" | "metrics-approve" => {
            let mut authority = state(&out, mode);
            if metrics {
                let offer: Signed<Offer> = read(&out.join("offer.json"));
                assert_eq!(offer.body.scopes.0, vec![Scope::MetricsCollect]);
                assert_eq!(authority.snapshot().unwrap().owner().key, PublicKey::from_bytes(owner().verifying_key().to_bytes()));
            }
            let claim: Signed<Claim> = read(&PathBuf::from(argument(&args, "--claim")));
            let claim_hash = authority
                .consume(
                    &InvitationToken::from_bytes([33; 32]).unwrap(),
                    claim.clone(),
                    now,
                )
                .unwrap();
            let pin = authority.snapshot().unwrap().owner().clone();
            let expires = now.0.checked_add(600).unwrap().min(pin.expires_at);
            let cert = sign(Certificate {
                header: header(&pin, pin.checkpoint + 1),
                agent_id: claim.body.agent_id,
                key_epoch: 1,
                agent_key: claim.body.agent_key,
                scopes: Scopes(vec![if metrics { Scope::MetricsCollect } else { Scope::TelemetryRead }]),
                capabilities: claim.body.capabilities.clone(),
                origin: CertificateOrigin::Enrollment {
                    invitation_id: claim.body.invitation_id,
                    claim_hash,
                    key_fingerprint: claim.body.agent_key.fingerprint(),
                },
                not_before: now.0,
                expires_at: expires,
            });
            authority.approve(cert.clone(), now).unwrap();
            let current = authority.snapshot().unwrap().owner().clone();
            write(&out.join("certificate.json"), &cert);
            write(&out.join("node-owner-pin.json"), &current);
            json!({"testOnly":true,"command":"approve","certificate":out.join("certificate.json"),"nodeOwnerPin":out.join("node-owner-pin.json")})
        }
        "propose-node-rotation" => {
            let authority = state(&out, mode);
            let snapshot = authority.snapshot().unwrap();
            let pin = snapshot.owner();
            let prepared: PreparedNodeKeyPublic =
                read(&PathBuf::from(argument(&args, "--prepared")));
            let previous: Signed<Certificate> = read(&out.join("certificate.json"));
            assert_eq!(prepared.format, 1);
            assert_eq!(prepared.from_key_epoch, 1);
            assert_eq!(prepared.proposed_key_epoch, 2);
            assert_eq!(prepared.agent_id, previous.body.agent_id);
            assert_eq!(
                prepared.old_key_fingerprint,
                previous.body.agent_key.fingerprint()
            );
            assert_eq!(previous.body.key_epoch, 1);
            assert!(!snapshot.revoked(prepared.agent_id));
            assert_eq!(
                snapshot
                    .certificate(prepared.agent_id)
                    .unwrap()
                    .digest()
                    .unwrap(),
                previous.body.digest().unwrap()
            );
            let expires = previous.body.expires_at.min(pin.expires_at);
            assert!(expires > now.0);
            let certificate = sign_at_pin(
                Certificate {
                    header: header(pin, pin.checkpoint + 1),
                    agent_id: prepared.agent_id,
                    key_epoch: 2,
                    agent_key: prepared.new_public_key,
                    scopes: previous.body.scopes.clone(),
                    capabilities: previous.body.capabilities.clone(),
                    origin: CertificateOrigin::Rotation {
                        previous_certificate: previous.body.digest().unwrap(),
                        previous_key_epoch: 1,
                    },
                    not_before: now.0,
                    expires_at: expires,
                },
                pin,
            );
            write(
                &out.join("rotation-input.json"),
                &PreparedRotationInput {
                    previous_certificate: previous,
                    certificate: certificate.clone(),
                },
            );
            write(&out.join("rotation-certificate.json"), &certificate);
            write(&out.join("node-owner-pin.json"), pin);
            json!({"testOnly":true,"command":"propose-node-rotation","input":out.join("rotation-input.json"),"rotationId":prepared.rotation_id,"activation":false})
        }
        "apply-node-rotation" => {
            let mut authority = state(&out, mode);
            let proofs: PreparedRotationProofs = read(&PathBuf::from(argument(&args, "--proofs")));
            authority
                .rotate(proofs.certificate.clone(), proofs.old, proofs.new, now)
                .unwrap();
            write(&out.join("certificate.json"), &proofs.certificate);
            write(
                &out.join("node-owner-pin.json"),
                authority.snapshot().unwrap().owner(),
            );
            json!({"testOnly":true,"command":"apply-node-rotation","revision":authority.snapshot().unwrap().revision(),"keyEpoch":2,"activation":false})
        }
        "lease" => {
            let authority = state(&out, mode);
            let snapshot = authority.snapshot().unwrap();
            let pin = snapshot.owner();
            let certificate: Signed<Certificate> = read(&out.join("certificate.json"));
            assert!(!snapshot.revoked(certificate.body.agent_id));
            assert_eq!(
                snapshot
                    .certificate(certificate.body.agent_id)
                    .unwrap()
                    .digest()
                    .unwrap(),
                certificate.body.digest().unwrap()
            );
            let expires = now
                .0
                .checked_add(300)
                .unwrap()
                .min(certificate.body.expires_at)
                .min(pin.expires_at);
            assert!(expires > now.0);
            let operation = Operation::TelemetryRead {
                source: ResourceId::new(&argument(&args, "--resource")).unwrap(),
            };
            let lease = sign_at_pin(
                Lease {
                    header: header(pin, pin.checkpoint),
                    id: LeaseId::new(&argument(&args, "--lease-id")).unwrap(),
                    agent_id: certificate.body.agent_id,
                    key_epoch: certificate.body.key_epoch,
                    certificate_hash: certificate.body.digest().unwrap(),
                    operation: operation.clone(),
                    issued_at: now.0,
                    expires_at: expires,
                },
                pin,
            );
            let freshness = sign_at_pin(
                Freshness {
                    header: header(pin, pin.checkpoint),
                    issued_at: now.0,
                    expires_at: expires,
                },
                pin,
            );
            let binding = NodeRequestBinding {
                certificate,
                lease,
                freshness,
                operation,
            };
            write(&out.join("request-input.json"), &binding);
            json!({"testOnly":true,"command":"lease","requestInput":out.join("request-input.json")})
        }
        "metrics-lease" | "collector-service-grant" | "collector-service-revoke" | "collector-service-status" => {
            metrics_authorize(&out, mode, now, args[0].as_str())
        }
        "rotate-owner" => {
            let mut authority = state(&out, mode);
            let pin = authority.snapshot().unwrap().owner().clone();
            assert_eq!(pin.epoch, 1, "one explicit synthetic rotation only");
            assert_eq!(
                pin.key,
                PublicKey::from_bytes(owner().verifying_key().to_bytes())
            );
            let replacement = SigningKey::from_bytes(&[55; 32]);
            let change = OwnerRotation {
                header: header(&pin, pin.checkpoint + 1),
                new_epoch: 2,
                new_key: PublicKey::from_bytes(replacement.verifying_key().to_bytes()),
                issued_at: now.0,
                expires_at: now.0.checked_add(300).unwrap().min(pin.expires_at),
            };
            let old = sign(change.clone());
            let new_body = NewOwnerProof(change);
            let new = Signed {
                signature: SignatureBytes::from_bytes(
                    replacement
                        .sign(&new_body.signing_bytes().unwrap())
                        .to_bytes(),
                ),
                body: new_body,
            };
            authority
                .rotate_owner(old.clone(), new.clone(), now)
                .unwrap();
            let current = authority.snapshot().unwrap().owner().clone();
            write(&out.join("node-owner-pin.json"), &current);
            write(
                &out.join("owner-rotation.json"),
                &json!({"old":old,"new":new}),
            );
            json!({"testOnly":true,"command":"rotate-owner","nodeOwnerPin":out.join("node-owner-pin.json"),"ownerEpoch":current.epoch,"checkpoint":current.checkpoint})
        }
        "revoke" => {
            let mut authority = state(&out, mode);
            let pin = authority.snapshot().unwrap().owner().clone();
            let cert: Signed<Certificate> = read(&out.join("certificate.json"));
            authority
                .revoke(
                    sign_at_pin(
                        Revocation {
                            header: header(&pin, pin.checkpoint + 1),
                            agent_id: cert.body.agent_id,
                            key_epoch: cert.body.key_epoch,
                            certificate_hash: cert.body.digest().unwrap(),
                            issued_at: now.0,
                        },
                        &pin,
                    ),
                    now,
                )
                .unwrap();
            write(
                &out.join("node-owner-pin.json"),
                authority.snapshot().unwrap().owner(),
            );
            json!({"testOnly":true,"command":"revoke"})
        }
        _ => panic!("unknown synthetic test command"),
    };
    println!("{}", summary);
}
