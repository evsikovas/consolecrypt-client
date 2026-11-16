//! Architecture observation before advertisement, never a signing authority.
use super::*;
const CODE:&str="/opt/consolecrypt-agent-install/opt/consolecrypt-agent";
fn kernel()->Result<String> {
    // Existing pinned rustix implements the actual kernel observation safely;
    // this crate keeps forbid(unsafe_code), with no child or caller selector.
    let value=rustix::system::uname();
    if value.sysname().to_bytes()!=b"Linux"{return Err(Error::CapabilityDenied)}
    match value.machine().to_str().map_err(|_|Error::InvalidWire)? {"x86_64"=>Ok("linux-x86_64".into()),
        "aarch64"=>Ok("linux-aarch64".into()),_=>Err(Error::CapabilityDenied)}
}
fn version(text:&str)->bool {
    let parts=text.split('.').collect::<Vec<_>>();parts.len()==3&&parts.iter().all(|v|
        !v.is_empty()&&v.len()<=6&&v.bytes().all(|c|c.is_ascii_digit()))
}
pub(super) fn extra_cap(path:&Path)->Option<u64> {
    if path==Path::new(disk::BINARY){return Some(192*1024*1024)}
    if path==Path::new(CODE).join("current.json").as_path(){return Some(4096)}
    let relative=path.strip_prefix(Path::new(CODE).join("releases")).ok()?;
    let parts=relative.components().map(|c|c.as_os_str().to_str()).collect::<Option<Vec<_>>>()?;
    if parts.is_empty()||!version(parts[0]){return None}
    match parts[1..] { ["installed.json"]=>Some(128*1024),["collector","vmagent-prod"]=>Some(23209584),
        ["collector","prometheus-hardmode"]=>Some(192*1024*1024),_=>None }
}
fn held(path:&Path,cap:u64)->Result<(Vec<u8>,Hash)> {
    // The original fixed-path digest routine checks parents/type/UID/mode/
    // nlink/nofollow/nonblock/FD+named identity. Recheck its full SHA afterwards.
    if extra_cap(path).is_none()&&path!=Path::new(VMAGENT){return Err(Error::ScopeDenied)}
    let before=root_digest(path)?;
    let mut file=OpenOptions::new().read(true).custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC)
        .open(path).map_err(|_|Error::Storage)?;
    let m=file.metadata().map_err(|_|Error::Storage)?;
    if !m.is_file()||m.uid()!=0||m.nlink()!=1||m.mode()&0o7022!=0||m.len()==0||m.len()>cap{return Err(Error::Storage)}
    let identity=|m:&fs::Metadata|(m.dev(),m.ino(),m.uid(),m.mode(),m.nlink(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec());
    let mut raw=Vec::new();Read::by_ref(&mut file).take(cap+1).read_to_end(&mut raw).map_err(|_|Error::Storage)?;
    if raw.len()as u64!=m.len()||identity(&m)!=identity(&file.metadata().map_err(|_|Error::Storage)?)
        ||identity(&m)!=identity(&fs::symlink_metadata(path).map_err(|_|Error::Storage)?)
        ||digest(&raw)!=before||root_digest(path)?!=before{return Err(Error::Storage)}
    Ok((raw,before))
}
fn elf(raw:&[u8],platform:&str,hash:Hash,bytes:u64)->Result<()> {
    let machine=match platform{"linux-x86_64"=>62,"linux-aarch64"=>183,_=>return Err(Error::CapabilityDenied)};
    if raw.len()as u64!=bytes||raw.len()<64||&raw[..7]!=b"\x7fELF\x02\x01\x01"
        ||u16::from_le_bytes([raw[18],raw[19]])!=machine||digest(raw)!=hash{return Err(Error::WrongClaim)}
    Ok(())
}
pub(super) fn legacy()->Result<()> {
    if kernel()?!="linux-x86_64"{return Err(Error::CapabilityDenied)}
    let(raw,_)=held(Path::new(VMAGENT),23209584)?;
    elf(&raw,"linux-x86_64",decode(json!(VMAGENT_SHA))?,23209584)
}
pub(super) fn selected(manifest:&Value)->Result<(Hash,Vec<(PathBuf,Hash)>)> {
    selected_kind(manifest,false)
}
pub(super) fn selected_disk(manifest:&Value)->Result<(Hash,Vec<(PathBuf,Hash)>)> {
    selected_kind(manifest,true)
}
fn selected_kind(manifest:&Value,disk:bool)->Result<(Hash,Vec<(PathBuf,Hash)>)> {
    let runtime=cc_agent_node_wire::metrics_manifest_runtime(manifest).map_err(|_|Error::CapabilityDenied)?
        .ok_or(Error::CapabilityDenied)?;
    if runtime.kind!=if disk{"prometheusHardmode"}else{"vmagent"}{return Err(Error::CapabilityDenied)}
    let name=if disk{"collector/prometheus-hardmode"}else{"collector/vmagent-prod"};
    let stable_path=if disk{disk::BINARY}else{VMAGENT};
    let cap=if disk{192*1024*1024}else{23209584};
    if kernel()?!=runtime.platform{return Err(Error::CapabilityDenied)}
    let current_path=Path::new(CODE).join("current.json");let(current_raw,current_hash)=held(&current_path,4096)?;
    let current:Value=serde_json::from_slice(&current_raw).map_err(|_|Error::InvalidWire)?;
    let selected=current["version"].as_str().filter(|v|version(v)).ok_or(Error::InvalidWire)?;
    let release=Path::new(CODE).join("releases").join(selected);
    let record_path=release.join("installed.json");let(record_raw,record_hash)=held(&record_path,128*1024)?;
    let record:Value=serde_json::from_slice(&record_raw).map_err(|_|Error::InvalidWire)?;
    let verified=&record["verified_release"];
    exact(verified,&["version","displayVersion","protocol","platform","artifactSha256","artifactBytes"])?;
    let artifact=record["artifact_sha256"].as_str().ok_or(Error::InvalidWire)?;
    if record["format"]!="cc-agent-installed-release/v2"||record["version"]!=current["version"]
        ||record["artifact_sha256"]!=current["artifact_sha256"]||verified["displayVersion"]!=current["version"]
        ||verified["artifactSha256"]!=current["artifact_sha256"]||verified["platform"]!=runtime.platform
        ||artifact.len()!=64||!artifact.bytes().all(|c|c.is_ascii_digit()||(b'a'..=b'f').contains(&c))
        ||verified["version"].as_u64().is_none_or(|v|v==0)||verified["protocol"]!=json!(1)
        ||verified["artifactBytes"].as_u64().is_none_or(|n|n==0||n>96*1024*1024)
        ||record["files"][name]!=runtime.binary_sha256
        ||record["file_modes"][name]!="0755"{return Err(Error::WrongClaim)}
    let hash:Hash=decode(json!(runtime.binary_sha256))?;
    let binary_path=release.join(name);let(raw,binary_hash)=held(&binary_path,cap)?;
    elf(&raw,&runtime.platform,hash,runtime.binary_bytes)?;
    let(stable,stable_hash)=held(Path::new(stable_path),cap)?;
    elf(&stable,&runtime.platform,hash,runtime.binary_bytes)?;
    if binary_hash!=hash||stable_hash!=hash||stable!=raw||root_digest(&current_path)?!=current_hash
        ||root_digest(&record_path)?!=record_hash{return Err(Error::Storage)}
    Ok((hash,vec![(current_path,current_hash),(record_path,record_hash),(binary_path,hash)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn selected_namespace_has_no_caller_path_escape() {
        assert_eq!(extra_cap(Path::new(CODE).join("current.json").as_path()),Some(4096));
        assert_eq!(extra_cap(Path::new(CODE).join("releases/0.1.2/installed.json").as_path()),Some(32768));
        for path in ["releases/../installed.json","releases/0.1.2/other","releases/0.1.2/collector/other","../current.json"] {
            assert_eq!(extra_cap(Path::new(CODE).join(path).as_path()),None);
        }
    }
    #[test] fn observed_elf_cannot_borrow_architecture_or_length() {
        let mut raw=vec![0;64];raw[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");raw[18..20].copy_from_slice(&183u16.to_le_bytes());
        assert!(elf(&raw,"linux-aarch64",digest(&raw),64).is_ok());
        assert!(elf(&raw,"linux-x86_64",digest(&raw),64).is_err());
        assert!(elf(&raw,"linux-aarch64",digest(&raw),65).is_err());
        assert!(elf(&raw,"linux-aarch64",Hash::from_bytes([1;32]),64).is_err());
    }
}
