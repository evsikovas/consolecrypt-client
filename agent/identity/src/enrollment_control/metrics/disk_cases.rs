// Real signed Node/Actor causals. No installed-unit or delivery claim.
fn disk_manifest() -> Value {
    let mut value=manifest();value["profile"]=json!(disk::PROFILE);
    value["bufferMode"]=json!("persistentNativeWal");
    value["buffer"]=json!({"max_bytes":536870912u64,"max_age_seconds":7200});value
}
#[test]
fn disk_full_manifest_address_cannot_reuse_ram_or_changed_policy_binding() {
    let ram=manifest();let disk=disk_manifest();disk::validate(&disk).unwrap();
    let full=digest(&json_body(&disk).unwrap());let id=collector_id(full).unwrap();
    assert_ne!(full,digest(&json_body(&ram).unwrap()));
    assert_eq!(check_collector_id(id,digest(&json_body(&ram).unwrap())),Err(Error::WrongClaim));
    for(field,replacement)in [("profile",json!("different-profile")),
        ("buffer",json!({"max_bytes":536870913u64,"max_age_seconds":7200})),
        ("buffer",json!({"max_bytes":536870912u64,"max_age_seconds":7201}))] {
        let mut changed=disk.clone();changed[field]=replacement;
        assert!(disk::validate(&changed).is_err());
        assert_eq!(check_collector_id(id,digest(&json_body(&changed).unwrap())),Err(Error::WrongClaim));
    }
    // Reusing common endpoint validation must never substitute its RAM hash.
    check_collector_id(id,full).unwrap();assert!(validate_manifest(&disk).is_err());
}
#[test]
fn disk_real_actor_permit_expires_with_original_session_not_data_residence() {
    let mut f=Fixture::new();f.id=collector_id(digest(&json_body(&disk_manifest()).unwrap())).unwrap();
    let b=f.binding();let pin=f.actor.snapshot().unwrap().owner().clone();
    let proof=create_metrics_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,f.id,b.clone(),Now(stamp(f.now))).unwrap();
    let permit=f.actor.admit(b.lease.clone(),proof,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap();
    // Replay must be tested at a still-valid time, before advancing this Actor's
    // monotonic observation floor to the lease/data-residence boundaries.
    let replay=create_metrics_node_request(&f.directory.0.join("node"),f.node.agent_id,&pin,f.id,b.clone(),Now(stamp(f.now))).unwrap();
    assert_eq!(f.actor.admit(b.lease,replay,Some(&b.freshness),&b.operation,Now(stamp(f.now))).unwrap_err(),Error::Replay);
    f.actor.check_permit(&permit,Now(stamp(f.now+29))).unwrap();
    assert_eq!(f.actor.check_permit(&permit,Now(stamp(f.now+30))),Err(Error::Expired));
    assert_eq!(f.actor.check_permit(&permit,Now(stamp(f.now+7200))),Err(Error::Expired));
    // A failed expiry check still advances the observation floor; rolling the
    // original caller time back cannot bypass that independent guard.
    assert_eq!(f.actor.check_permit(&permit,Now(stamp(f.now))),Err(Error::InvalidTime));
}
#[test]
fn disk_scrape_uses_native_agent_configuration_without_vmagent_disable_flag() {
    let value=disk_manifest();let hash=digest(&json_body(&value).unwrap());
    let catalog=Catalog{id:collector_id(hash).unwrap(),manifest:value,hash,raw:vec![],fixed:vec![],disk:None};
    let raw=disk::scrape(&catalog).unwrap();let parsed:Value=serde_json::from_slice(&raw).unwrap();
    assert_eq!(parsed["remote_write"][0]["follow_redirects"],json!(false));
    assert_eq!(parsed["remote_write"][0]["queue_config"]["max_shards"],json!(1));
    assert_eq!(parsed["scrape_configs"][0]["static_configs"][0]["targets"][0],json!("127.0.0.1:9100"));
    assert!(!String::from_utf8(raw).unwrap().contains("disableOnDiskQueue"));
}
#[test]
fn disk_refuses_series_bound_smaller_than_reused_native_global_profile() {
    let mut value=disk_manifest();value["targets"][0]["series_limit"]=json!(255);
    assert!(disk::validate(&value).is_err());
    value["targets"][0]["series_limit"]=json!(256);disk::validate(&value).unwrap();
    let mut ram=manifest();ram["targets"][0]["series_limit"]=json!(1);validate_manifest(&ram).unwrap();
}
