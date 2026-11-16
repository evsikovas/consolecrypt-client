//! One fixed maintained Prometheus disk profile. Same Actor/Lease/Permit as RAM.
//! Neither the catalog, filesystem binding nor layout marker admits an operation.
use super::*;

pub(super) const PROFILE:&str="maintained-prometheus-disk-512mib-2h-v1";
pub(super) const CATALOG:&str="/etc/consolecrypt-agent/collector-disk.json";
pub(super) const ROOT:&str="/run/consolecrypt-agent/disk";
pub(super) const STORE:&str="/var/lib/consolecrypt-agent/metrics-disk";
pub(super) const BINARY:&str="/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/prometheus-hardmode";
pub(super) const ADAPTER:&str="/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/disk_unit_adapter.py";
const ENTRY:&str="/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/disk_unit_entry.py";
const SPEC:&str="/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/disk_unit_spec.py";
const BINDING:&str="/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/disk_binding.py";

type Identity=(u64,u64,u32,u32,u32,u64,u64,i64,i64,i64,i64);
fn identity(m:&fs::Metadata)->Identity {(m.dev(),m.ino(),m.mode(),m.uid(),m.gid(),m.nlink(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())}
#[derive(Clone)]
struct Pin {path:PathBuf,file:std::sync::Arc<File>,identity:Identity}
impl Pin {
    fn capture(path:&str,expected:Hash)->Result<Self> {
        let cap=match path {BINARY=>256*1024*1024,PYTHON=>16*1024*1024,ADAPTER|ENTRY|SPEC|BINDING=>65536,_=>return Err(Error::ScopeDenied)};
        let path=PathBuf::from(path);Self::parents(&path)?;
        let before=fs::symlink_metadata(&path).map_err(|_|Error::Storage)?;
        let mode=if path==Path::new(BINARY)||path==Path::new(PYTHON){0o755}else{0o644};
        if !before.is_file()||before.file_type().is_symlink()||before.uid()!=0||before.nlink()!=1
            ||before.mode()&0o777!=mode||before.mode()&0o7000!=0||before.len()==0||before.len()>cap{return Err(Error::Storage)}
        let mut file=OpenOptions::new().read(true).custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC).open(&path).map_err(|_|Error::Storage)?;
        if identity(&before)!=identity(&file.metadata().map_err(|_|Error::Storage)?){return Err(Error::Storage)}
        let mut hash=Sha256::new();let mut count=0u64;let mut chunk=[0u8;65536];
        loop {let n=file.read(&mut chunk).map_err(|_|Error::Storage)?;if n==0{break}count+=n as u64;
            if count>cap{return Err(Error::Quota)}hash.update(&chunk[..n]);}
        if count!=before.len()||Hash::from_bytes(hash.finalize().into())!=expected
            ||identity(&before)!=identity(&file.metadata().map_err(|_|Error::Storage)?)
            ||identity(&before)!=identity(&fs::symlink_metadata(&path).map_err(|_|Error::Storage)?){return Err(Error::Storage)}
        Ok(Self {path,file:std::sync::Arc::new(file),identity:identity(&before)})
    }
    fn parents(path:&Path)->Result<()> {
        for parent in path.ancestors().skip(1) {let m=fs::symlink_metadata(parent).map_err(|_|Error::Storage)?;
            if !m.is_dir()||m.file_type().is_symlink()||m.uid()!=0||m.mode()&0o022!=0{return Err(Error::Storage)}}Ok(())
    }
    fn check(&self)->Result<()> {Self::parents(&self.path)?;
        if identity(&self.file.metadata().map_err(|_|Error::Storage)?)!=self.identity
            ||identity(&fs::symlink_metadata(&self.path).map_err(|_|Error::Storage)?)!=self.identity{return Err(Error::Storage)}Ok(())}
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Descriptor {format:String,collector_id:ResourceId,manifest:Value,prometheus_sha256:Hash,
    adapter_sha256:Hash,entry_sha256:Hash,spec_sha256:Hash,binding_sha256:Hash,interpreter_sha256:Hash}
#[derive(Clone)]
pub(super) struct Installed {pub(super) binary_hash:Hash,pins:Vec<Pin>}
impl Installed {
    pub(super) fn check(&self)->Result<()> {
        match fs::symlink_metadata(super::CATALOG) {Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err(Error::Conflict)}
        for p in &self.pins {p.check()?;}Ok(())
    }
}
pub(super) fn no_sessions(root:&str)->Result<()> {
    match fs::symlink_metadata(root) {Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(()),Err(_)=>return Err(Error::Storage),Ok(_)=>{}}
    private_directory(Path::new(root))?;
    for item in fs::read_dir(root).map_err(|_|Error::Storage)?.take(33) {
        let item=item.map_err(|_|Error::Storage)?;
        if item.file_name()!=".collector.lock"{return Err(Error::UncertainCommit)}
        let m=fs::symlink_metadata(item.path()).map_err(|_|Error::Storage)?;
        if !m.is_file()||m.file_type().is_symlink()||m.uid()!=service_uid()?||m.nlink()!=1||m.mode()&0o777!=0o600{return Err(Error::Storage)}
    }Ok(())
}
pub(super) fn load()->Result<Option<Catalog>> {
    match fs::symlink_metadata(CATALOG) {
        Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Err(_)=>return Err(Error::Storage),Ok(_)=>{},
    }
    // A fixed admin catalog selects exactly one profile. Never silently choose
    // disk when a volatile catalog is also installed or ambiguously linked.
    match fs::symlink_metadata(super::CATALOG) {Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err(Error::Conflict)}
    no_sessions(super::SESSION_ROOT)?;no_sessions(ROOT)?;
    tmpfs_root_at(ROOT)?;private_directory(Path::new(STORE))?;
    let raw=root_file(Path::new(CATALOG))?;let parsed:Value=serde_json::from_slice(&raw).map_err(|_|Error::InvalidWire)?;
    if canonical(&parsed)?!=raw{return Err(Error::InvalidWire)}
    let value:Descriptor=decode(parsed)?;
    let v2=value.format=="cc-agent-disk-collector/v2";
    if !v2&&value.format!="cc-agent-disk-collector/v1"{return Err(Error::CapabilityDenied)}
    if v2!=(value.manifest["schema_version"]==json!(2)){return Err(Error::CapabilityDenied)}
    validate(&value.manifest)?;
    let fixed=if v2 {
        let(actual,pins)=runtime::selected_disk(&value.manifest)?;
        if actual!=value.prometheus_sha256{return Err(Error::WrongClaim)}
        pins.into_iter().map(|(path,expected)| RootPin::capture(&path,expected)).collect::<Result<Vec<_>>>()?
    }else{
        if rustix::system::uname().machine().to_bytes()!=b"x86_64"{return Err(Error::CapabilityDenied)}
        Vec::new()
    };
    let hash=digest(&json_body(&value.manifest)?);check_collector_id(value.collector_id,hash)?;
    let values=[(BINARY,value.prometheus_sha256),(ADAPTER,value.adapter_sha256),(ENTRY,value.entry_sha256),
        (SPEC,value.spec_sha256),(BINDING,value.binding_sha256),(PYTHON,value.interpreter_sha256)];
    let mut pins=Vec::new();for(path,hash)in values {pins.push(Pin::capture(path,hash)?);}
    Ok(Some(Catalog {id:value.collector_id,manifest:value.manifest,hash,raw,fixed,
        disk:Some(Installed {binary_hash:value.prometheus_sha256,pins})}))
}
pub(super) fn validate(v:&Value)->Result<()> {
    if v["schema_version"]==json!(2) {
        let host=v["labels"]["cc_host_id"].as_str().ok_or(Error::WrongClaim)?;
        let runtime=cc_agent_node_wire::metrics_manifest_runtime(v).map_err(|_|Error::CapabilityDenied)?
            .ok_or(Error::CapabilityDenied)?;
        if runtime.kind!="prometheusHardmode"{return Err(Error::CapabilityDenied)}
        cc_agent_node_wire::validate_metrics_manifest(v,host).map_err(|_|Error::CapabilityDenied)?;
        return Ok(())
    }
    exact(v,&["schema_version","mode","targets","labels","scrape_interval_seconds","receiver","buffer","profile","bufferMode"])?;
    if v["profile"]!=PROFILE||v["bufferMode"]!="persistentNativeWal"
        ||v["buffer"]!=json!({"max_bytes":536870912u64,"max_age_seconds":7200})||json_body(v)?.len()>8192{return Err(Error::CapabilityDenied)}
    let mut common=v.clone();common.as_object_mut().ok_or(Error::InvalidWire)?.remove("profile");
    common.as_object_mut().ok_or(Error::InvalidWire)?.remove("bufferMode");
    common["buffer"]=json!({"max_bytes":33554432u64,"max_age_seconds":300});
    // Validation only. The actual resource/lease uses the FULL disk hash above.
    validate_manifest(&common)?;
    // The reused native profile bounds ALL active series at256, not a
    // vmagent per-target rolling estimator. Refuse a smaller requested bound
    // rather than silently ignore it or introduce another series registry.
    for target in v["targets"].as_array().ok_or(Error::InvalidWire)? {
        integer(&target["series_limit"],256,50000)?;
    }
    Ok(())
}
pub(super) fn scrape(catalog:&Catalog)->Result<Vec<u8>> {
    let interval=integer(&catalog.manifest["scrape_interval_seconds"],1,300)?;let mut configs=Vec::new();
    for t in catalog.manifest["targets"].as_array().ok_or(Error::InvalidWire)? {
        let(scheme,authority,path)=endpoint(&t["url"],true)?;
        configs.push(json!({"job_name":format!("cc_{}",name(&t["id"])?),"scheme":scheme,"metrics_path":path,
            "scrape_interval":format!("{interval}s"),"scrape_timeout":format!("{}s",10u64.min(interval)),
            "honor_labels":false,"honor_timestamps":true,"follow_redirects":false,
            "sample_limit":t["sample_limit"],"label_limit":t["label_limit"],
            "body_size_limit":format!("{}B",integer(&t["response_bytes"],256,8*1024*1024)?),
            "label_name_length_limit":128,"label_value_length_limit":256,
            "static_configs":[{"targets":[authority],"labels":catalog.manifest["labels"]}]}));
    }
    json_body(&json!({"global":{"scrape_interval":format!("{interval}s")},"scrape_configs":configs,
        "remote_write":[{"url":catalog.manifest["receiver"]["url"],"follow_redirects":false,
            "remote_timeout":"3s","send_exemplars":false,"send_native_histograms":false,
            "metadata_config":{"send":false},"queue_config":{"min_shards":1,"max_shards":1,
                "capacity":1000,"max_samples_per_send":256,"batch_send_deadline":"1s","min_backoff":"1s","max_backoff":"1s"}}]}))
}
pub(super) fn binding(pin:&OwnerPin,node:&PublicNodeIdentity,certificate:&Certificate,catalog:&Catalog)->Result<Value> {
    let context=serde_json::to_value(&pin.context).map_err(|_|Error::InvalidWire)?;
    let tenant=&context["tenant"];
    if !matches!(tenant["kind"].as_str(),Some("localProfile"|"personalAccount")){return Err(Error::UnsupportedTeam)}
    Ok(json!({"owner_kind":tenant["kind"],"owner_id":tenant["id"],"owner_deployment_id":context["deploymentId"],
        "owner_signing_key_sha256":pin.key.fingerprint(),"agent_id":node.agent_id,
        "node_key_fingerprint":node.key_fingerprint,"certificate_sha256":certificate.digest()?,
        "collector_id":catalog.id,"manifest_sha256":catalog.hash}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest()->Value {json!({"schema_version":1,"mode":"remote_write",
        "targets":[{"id":"app","url":"http://127.0.0.1:9100/metrics","kind":"application",
            "sample_limit":1000,"series_limit":256,"response_bytes":4096,"label_limit":32}],
        "labels":{"cc_host_id":"11111111-1111-4111-8111-111111111111","site":"local","environment":"test"},
        "scrape_interval_seconds":1,"receiver":{"url":"http://127.0.0.1:9090/api/v1/write"},
        "profile":PROFILE,"bufferMode":"persistentNativeWal","buffer":{"max_bytes":536870912u64,"max_age_seconds":7200}})}
    #[test]
    fn disk_v1_remains_valid_and_v2_runtime_changes_actual_resource() {
        let old=manifest();validate(&old).unwrap();let legacy=digest(&json_body(&old).unwrap());
        let mut next=old.clone();next["schema_version"]=json!(2);
        next["runtime"]=json!({"kind":"prometheusHardmode","upstreamTag":"3.16.0","platform":"linux-aarch64",
            "binarySha256":"a".repeat(64),"binaryBytes":64000});
        validate(&next).unwrap();assert_ne!(legacy,digest(&json_body(&next).unwrap()));
        let mut bad=next.clone();bad["runtime"]["kind"]=json!("vmagent");assert!(validate(&bad).is_err());
        let mut bad=next;bad["schema_version"]=json!(1);assert!(validate(&bad).is_err());
        validate(&old).unwrap();
    }
}
