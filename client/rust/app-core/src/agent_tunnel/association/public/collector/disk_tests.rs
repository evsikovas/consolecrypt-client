fn fixed_manifest()->Value {json!({"schema_version":1,"mode":"remote_write",
    "targets":[{"id":"app","url":"http://127.0.0.1:9100/metrics","kind":"application","sample_limit":1000,
        "series_limit":1000,"response_bytes":4096,"label_limit":32}],
    "labels":{"cc_host_id":"11111111-1111-4111-8111-111111111111","site":"local","environment":"test"},
    "scrape_interval_seconds":1,"receiver":{"url":"http://127.0.0.1:9090/api/v1/write"},
    "buffer":{"max_bytes":33554432,"max_age_seconds":300}})}
fn native_disk_manifest()->Value {let mut m=fixed_manifest();m["profile"]=json!("maintained-prometheus-disk-512mib-2h-v1");
    m["bufferMode"]=json!("persistentNativeWal");m["buffer"]=json!({"max_bytes":536870912u64,"max_age_seconds":7200});m}
#[test]
fn disk_native_uses_full_manifest_hash_and_rejects_policy_substitution() {
    let host:ObjectId="11111111-1111-4111-8111-111111111111".parse().unwrap();
    let ram=fixed_manifest();let disk=native_disk_manifest();
    let original=protocol::metrics_manifest_digest(&disk,host).unwrap();
    assert_ne!(original,protocol::metrics_manifest_digest(&ram,host).unwrap());
    let mut bytes=protocol::canonical(&disk).unwrap();assert_eq!(bytes.pop(),Some(b'\n'));
    use sha2::Digest as _;
    assert_eq!(original,Hash::from_bytes(sha2::Sha256::digest(bytes.as_slice()).into()));
    for(field,value)in [("profile",json!("volatile-collection-session-v1")),
        ("buffer",json!({"max_bytes":536870913u64,"max_age_seconds":7200})),
        ("buffer",json!({"max_bytes":536870912u64,"max_age_seconds":7201}))] {
        let mut changed=disk.clone();changed[field]=value;
        assert!(protocol::metrics_manifest_digest(&changed,host).is_err());
    }
    assert!(protocol::metrics_manifest_digest(&disk,ObjectId::new()).is_err());
}
#[test]
fn disk_view_is_closed_distinct_and_ram_projection_is_byte_unchanged() {
    let original=json!({"profile":"volatile-collection-session-v1","crashReplay":false,
        "actorAdmission":false,"maxMemoryBytes":536870912u64,"maxTemporaryStateBytes":33554432u64});
    let mut ram=original.clone();manifest::Manifest(fixed_manifest()).project_profile(&mut ram).unwrap();assert_eq!(ram,original);
    let mut disk=original;manifest::Manifest(native_disk_manifest()).project_profile(&mut disk).unwrap();
    assert_eq!(disk["bufferMode"],json!("persistentNativeWal"));assert!(disk.get("crashReplay").is_none());
    assert_eq!(disk["diskBudget"].as_object().unwrap().len(),7);
    assert_eq!(disk["diskBudget"]["maxResidenceSeconds"],json!(7200));
    assert_eq!(disk["automaticRecovery"],json!(false));assert_eq!(disk["requiresFreshGrantForReplay"],json!(true));
    assert_eq!(disk["actorAdmission"],json!(false));
}
#[test]
fn disk_native_refuses_unhonorable_lower_series_bound_without_changing_ram() {
    let host:ObjectId="11111111-1111-4111-8111-111111111111".parse().unwrap();
    let mut disk=native_disk_manifest();disk["targets"][0]["series_limit"]=json!(255);
    assert!(protocol::metrics_manifest_digest(&disk,host).is_err());
    disk["targets"][0]["series_limit"]=json!(256);protocol::metrics_manifest_digest(&disk,host).unwrap();
    let mut ram=fixed_manifest();ram["targets"][0]["series_limit"]=json!(1);
    protocol::metrics_manifest_digest(&ram,host).unwrap();
}

#[test]
fn runtime_v2_native_keeps_full_hash_and_rejects_v1_or_cross_runtime_echo() {
    let host:ObjectId="11111111-1111-4111-8111-111111111111".parse().unwrap();
    let old=fixed_manifest();let old_digest=protocol::metrics_manifest_digest(&old,host).unwrap();
    let mut arm=old;arm["schema_version"]=json!(2);arm["runtime"]=json!({"kind":"vmagent","upstreamTag":"v1.156.0",
        "platform":"linux-aarch64","binarySha256":"564346d8acd0979b62c2671ad586d63b243a925307bfa54d0d5f94bd50dda4f5","binaryBytes":21299768});
    let digest=protocol::metrics_manifest_digest(&arm,host).unwrap();assert_ne!(digest,old_digest);
    let descriptor=protocol::MetricsCollector{collector_id:metrics_collector_id(digest).unwrap(),manifest_sha256:digest,manifest:arm.clone()};
    descriptor.check(host).unwrap();
    let forged=protocol::MetricsCollector{collector_id:metrics_collector_id(old_digest).unwrap(),manifest_sha256:old_digest,manifest:arm};
    assert!(forged.check(host).is_err());
    let mut disk=native_disk_manifest();disk["schema_version"]=json!(2);disk["runtime"]=descriptor.manifest["runtime"].clone();
    assert!(protocol::metrics_manifest_digest(&disk,host).is_err());
}
#[test]
fn disk_v2_catalog_uses_full_platform_digest_without_v1_resource_alias() {
    let host:ObjectId="11111111-1111-4111-8111-111111111111".parse().unwrap();
    let old=native_disk_manifest();let old_hash=protocol::metrics_manifest_digest(&old,host).unwrap();
    let mut arm=old.clone();arm["schema_version"]=json!(2);
    arm["runtime"]=json!({"kind":"prometheusHardmode","upstreamTag":"3.16.0","platform":"linux-aarch64",
        "binarySha256":"a".repeat(64),"binaryBytes":64000});
    let hash=protocol::metrics_manifest_digest(&arm,host).unwrap();assert_ne!(hash,old_hash);
    assert_ne!(metrics_collector_id(hash).unwrap(),metrics_collector_id(old_hash).unwrap());
    let stale=protocol::MetricsCollector{collector_id:metrics_collector_id(old_hash).unwrap(),manifest_sha256:old_hash,manifest:arm.clone()};
    assert!(stale.check(host).is_err());
    let mut amd=arm.clone();amd["runtime"]["platform"]=json!("linux-x86_64");
    assert_ne!(hash,protocol::metrics_manifest_digest(&amd,host).unwrap());
    let mut bytes=protocol::canonical(&arm).unwrap();bytes.pop();use sha2::Digest as _;
    assert_eq!(hash,Hash::from_bytes(sha2::Sha256::digest(bytes.as_slice()).into()));
    assert_eq!(old_hash,protocol::metrics_manifest_digest(&old,host).unwrap());
}
