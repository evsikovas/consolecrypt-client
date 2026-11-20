//! Real signatures and owned EXT identity files. Metadata is never a job grant.
use crate::service_restart::*;
use crate::{ServiceIdentity, ServiceNode};
use cap_std::{ambient_authority, fs::Dir};
use cc_crypto_core::{DeviceSecretKeys, ServiceDelegationPolicy, ServiceTenantKind};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn own() -> tempfile::TempDir {
    let parent = PathBuf::from(
        std::env::var_os("CC_SVC_IDENTITY_TEST_ROOT").expect("owned actual EXT fixture"),
    );
    assert!(
        parent.is_absolute()
            && !fs::symlink_metadata(&parent)
                .unwrap()
                .file_type()
                .is_symlink()
    );
    let own = tempfile::tempdir_in(parent).unwrap();
    fs::set_permissions(own.path(), fs::Permissions::from_mode(0o700)).unwrap();
    own
}
fn create_runtime(path:&Path) {
    fs::create_dir(path).unwrap();fs::set_permissions(path,fs::Permissions::from_mode(0o700)).unwrap();
}
fn dir(path: &Path) -> Dir {
    Dir::open_ambient_dir(path, ambient_authority()).unwrap()
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn launch(root: &Path) -> HelperLaunch {
    HelperLaunch::Native {
        python: "/nonexistent-owned-python".into(),
        module_dir: "/nonexistent-owned-modules".into(),
        root: root.into(),
    }
}

// This creates signed METADATA for the fixed record codec/I/O tests. Empty
// capsules and credential-free routes CANNOT pass actual checked_enrollment;
// no execution/PreparedReview/Ready witness is synthesized here.
fn metadata(identity: &ServiceIdentity, root: &Path, issued: u64) -> EnrollmentRecord {
    let owner = DeviceSecretKeys::generate().unwrap();
    let public = owner.public_keys();
    let facts = identity.facts();
    let owner_id = Uuid::new_v4().to_string();
    let host = Uuid::new_v4();
    let service = Uuid::new_v4().to_string();
    let schedule = Uuid::new_v4().to_string();
    let tenant = ServicePolicyTenant {
        kind: ServiceTenantKind::LocalProfile,
        id: Uuid::new_v4().to_string(),
    };
    let origin:ServiceExecutionOrigin=serde_json::from_value(json!({"version":1,"service_id":service,"runner_revision":1,
        "runner_configuration_digest":"1".repeat(64),"signer_key_sha256":hex::encode(sha2::Sha256::digest(facts.public_keys.signing)),
        "recipient_key_sha256":hex::encode(sha2::Sha256::digest(facts.public_keys.encryption)),"environment_digest":"2".repeat(64),
        "helper_digest":"3".repeat(64),"known_hosts_digest":"4".repeat(64),"backend":"linuxNativeSsh","network":"serviceLocal"})).unwrap();
    let definition=json!({"version":1,"zone":"Etc/UTC","hour":12,"minute":0,"weekdays":[0,1,2,3,4,5,6],"graceSeconds":60,"gapPolicy":"skip","foldPolicy":"first","missedPolicy":"skip"});
    let definition_digest=hex::encode(sha2::Sha256::digest(serde_json::to_vec(&crate::project::canonical(definition.clone())).unwrap()));
    let policy:ServiceDelegationPolicy=serde_json::from_value(json!({"version":4,"id":Uuid::new_v4().to_string(),"owner_id":owner_id,
        "owner_epoch":1,"tenant":tenant,"service_id":service,"signer":{"kind":"ed25519","public_key":hex::encode(facts.public_keys.signing),"key_epoch":1,"certificate_epoch":1},
        "recipient":{"kind":"x25519","public_key":hex::encode(facts.public_keys.encryption),"key_epoch":1,"certificate_epoch":1},
        "project_id":Uuid::new_v4().to_string(),"project_revision":1,"template_digest":"5".repeat(64),"playbook":"playbook.yml",
        "schedule_id":schedule,"schedule_revision":1,"definition_digest":definition_digest,"inventory_digest":"7".repeat(64),
        "parameters_digest":"8".repeat(64),"environment_digest":"2".repeat(64),"hosts":[{"id":host.to_string(),"revision":1,"target_digest":"9".repeat(64)}],
        "credentials":[],"route_inputs":{"dependencies":[{"id":host.to_string(),"revision":1,"kind":"host"}],"key_trust":[{"id":Uuid::new_v4().to_string(),"revision":1,"kind":"known_host"}]},
        "modes":["check"],"quotas":{"max_jobs":2,"max_per_utc_day":2,"max_concurrent":1,"max_job_seconds":30,"max_forks":1},
        "policy_revision":1,"revocation_epoch":1,"issued_at":issued,"expires_at":issued+60,"desktop_lock_policy":"independentDelegatedService",
        "access_policy":{"mode":"boundedOffline","offline_not_after":issued+60},"execution_origin":origin})).unwrap();
    let signed = owner
        .sign_service_delegation(policy, &owner_id, &tenant, issued)
        .unwrap();
    let raw = signed.to_json().unwrap();
    let plan = ConnectionPlan {
        host_id: host.into(),
        name: "owned metadata".into(),
        target: cc_ssh_core::Endpoint::new("127.0.0.1", 22),
        username: "owned".into(),
        credential: None,
        host_key_policy: cc_models::host::HostKeyPolicy::Strict,
        route: vec![],
        proxy: None,
        forwards: vec![],
        backend: cc_models::host::SshBackend::Native,
        keepalive_secs: None,
        agent_forwarding: false,
        warnings: vec![],
    };
    let known:KnownHost=serde_json::from_value(json!({"id":Uuid::new_v4().to_string(),"host_pattern":"owned","key_type":"ssh-ed25519","public_key":"AAAAC3NzaC1lZDI1NTE5AAAAIGdnZ2dnZ2dnZ2dnZ2dnZ2dnZ2dnZ2dnZ2dnZ2dnZ2dn",
        "fingerprint_sha256":"SHA256:owned","source":"manual","added_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"})).unwrap();
    let pin = RuntimePin::open(&launch(root)).unwrap();
    EnrollmentRecord {
        version: 1,
        identity_id: facts.identity_id,
        key_epoch: 1,
        signing: hex::encode(facts.public_keys.signing),
        recipient: hex::encode(facts.public_keys.encryption),
        runtime: pin.identity,
        bootstrap: Bootstrap {
            origin,
            owner: OwnerPin {
                owner_id:owner_id.clone(),
                owner_epoch: 1,
                tenant:tenant.clone(),
                signing: hex::encode(public.signing),
                encryption: hex::encode(public.encryption),
                not_after: issued + 600,
            },
            schedule:json!({"id":schedule,"revision":1,"definition":definition}),
        },
        delegation: serde_json::from_slice(&raw).unwrap(),
        helper_ack:HelperEnrollmentAck {enrolled:true,delegation_digest:SignedServiceDelegation::verify_json(&raw,&owner_id,&public,&tenant,issued).unwrap().digest().unwrap(),revision_id:Uuid::new_v4()},
        targets: vec![cc_models::automation::ApprovedTarget {
            host_id: host,
            alias: "owned".into(),
            address: "127.0.0.1".into(),
            port: 22,
            username: "owned".into(),
            route_digest: "9".repeat(64),
            credential_refs: vec![],
        }],
        plans: vec![plan],
        known_hosts: vec![known],
        capsules: vec![],
    }
}

#[test]
fn held_create_only_metadata_record_reloads_and_never_overwrites() {
    let own = own();
    let root = own.path().join("runtime");
    create_runtime(&root);
    let id = Uuid::new_v4();
    let mut identity = ServiceIdentity::create(dir(own.path()), id).unwrap();
    let record = metadata(&identity, &root, now());
    identity.store_enrollment(&record).unwrap();
    let path = own
        .path()
        .join(format!("service-identity-{id}/enrollment.v1"));
    let bytes = fs::read(&path).unwrap();
    assert!(identity.store_enrollment(&record).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(identity);
    let restored = ServiceIdentity::open_existing(dir(own.path()), id).unwrap();
    assert!(restored.enrollment().unwrap().is_some());
    restored.recheck().unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
}
#[test]
fn duplicate_future_overbudget_and_foreign_record_deny_before_create() {
    let own = own();
    let root = own.path().join("runtime");
    create_runtime(&root);
    let mut identity = ServiceIdentity::create(dir(own.path()), Uuid::new_v4()).unwrap();
    let mut record = metadata(&identity, &root, now());
    let raw = record.encode().unwrap();
    let duplicate = format!(
        "{{\"version\":1,{}",
        std::str::from_utf8(&raw).unwrap().trim_start_matches('{')
    );
    assert!(EnrollmentRecord::parse(duplicate.as_bytes(), identity.facts()).is_err());
    let mut huge = raw.to_vec();
    huge.resize(MAX_RECORD + 1, b' ');
    assert!(EnrollmentRecord::parse(&huge, identity.facts()).is_err());
    record.version = 2;
    assert!(identity.store_enrollment(&record).is_err());
    record.version = 1;
    record.identity_id = Uuid::new_v4();
    assert!(identity.store_enrollment(&record).is_err());
    assert!(
        !own.path()
            .join(format!(
                "service-identity-{}/enrollment.v1",
                identity.facts().identity_id
            ))
            .exists()
    );
}
#[test]
fn identical_named_record_replacement_and_unknown_entries_are_not_adopted() {
    let own = own();
    let root = own.path().join("runtime");
    create_runtime(&root);
    let id = Uuid::new_v4();
    let mut identity = ServiceIdentity::create(dir(own.path()), id).unwrap();
    identity
        .store_enrollment(&metadata(&identity, &root, now()))
        .unwrap();
    let base = own.path().join(format!("service-identity-{id}"));
    let file = base.join("enrollment.v1");
    let bytes = fs::read(&file).unwrap();
    fs::rename(&file, own.path().join("original-record")).unwrap();
    fs::write(&file, bytes).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(identity.recheck().is_err());
    drop(identity);
    fs::write(base.join("future-format"), b"preserve").unwrap();
    assert!(ServiceIdentity::open_existing(dir(own.path()), id).is_err());
}
#[test]
fn runtime_inode_cannot_be_retargeted_even_with_same_private_layout() {
    let own = own();
    let root = own.path().join("runtime");
    create_runtime(&root);
    let pin = RuntimePin::open(&launch(&root)).unwrap();
    fs::rename(&root, own.path().join("original-runtime")).unwrap();
    create_runtime(&root);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(pin.check().is_err());
    let replacement = RuntimePin::open(&launch(&root)).unwrap();
    assert!(pin.identity != replacement.identity);
}
#[tokio::test]
async fn absent_enrollment_is_inert_and_expired_record_cannot_start_helper_or_create_store() {
    let own = own();
    let missing = own.path().join("missing-runtime");
    let id = Uuid::new_v4();
    let identity = ServiceIdentity::create(dir(own.path()), id).unwrap();
    let mut node = ServiceNode::with_identity(identity).unwrap();
    assert!(node.restore(&launch(&missing)).await.unwrap().is_none());
    assert!(!missing.exists());
    assert!(node.shutdown().await);
    let root = own.path().join("runtime");
    create_runtime(&root);
    let mut identity = ServiceIdentity::open_existing(dir(own.path()), id).unwrap();
    let expired = metadata(&identity, &root, now() - 90);
    identity.store_enrollment(&expired).unwrap();
    let mut node = ServiceNode::with_identity(identity).unwrap();
    assert!(node.restore(&launch(&root)).await.is_err());
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    assert!(node.shutdown().await);
}

#[test]
fn incomplete_fsync_return_preserves_file_and_denies_reload() {
    let own=own();let root=own.path().join("runtime");create_runtime(&root);let id=Uuid::new_v4();
    let mut identity=ServiceIdentity::create(dir(own.path()),id).unwrap();let record=metadata(&identity,&root,now());
    assert!(crate::service_identity::test_faulted_store(&mut identity,&record,1).is_err());
    let path=own.path().join(format!("service-identity-{id}/enrollment.v1"));let bytes=fs::read(&path).unwrap();assert!(!bytes.is_empty());
    assert!(identity.recheck().is_err());drop(identity);
    assert!(ServiceIdentity::open_existing(dir(own.path()),id).is_err());assert_eq!(fs::read(path).unwrap(),bytes);
}
#[test]
fn completed_record_with_lost_ack_is_only_original_recovery_data() {
    let own=own();let root=own.path().join("runtime");create_runtime(&root);let id=Uuid::new_v4();
    let mut identity=ServiceIdentity::create(dir(own.path()),id).unwrap();let record=metadata(&identity,&root,now());
    // Real body/file fsync + first directory sync, then an unconfirmed final
    // directory-sync return. No durable ACK or new execution grant is claimed.
    assert!(crate::service_identity::test_faulted_store(&mut identity,&record,2).is_err());
    let path=own.path().join(format!("service-identity-{id}/enrollment.v1"));let bytes=fs::read(&path).unwrap();drop(identity);
    let restored=ServiceIdentity::open_existing(dir(own.path()),id).unwrap();let observed=restored.enrollment().unwrap().unwrap();
    assert_eq!(observed.identity_id,record.identity_id);assert_eq!(observed.delegation,record.delegation);
    assert_eq!(observed.policy(now()).unwrap().body().expires_at,record.policy(now()).unwrap().body().expires_at);
    assert_eq!(fs::read(path).unwrap(),bytes);
    // This is DATA only: missing actual custody blocks before helper/material.
    // The ordinary real restore integration separately checks original header,
    // template, scheduler, JobStore and revocation, never a marker by itself.
    assert!(!root.join("service-version.json").exists());
}
