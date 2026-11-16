//! Read the separately administrator-installed immutable binary/materials.
//! No fetch, extraction, key generation, installation, adoption or repair.
use super::*;
use sha2::{Digest,Sha256};
use std::os::fd::AsRawFd;
use std::collections::BTreeMap;

pub(super) const BINARY:&str="/opt/consolecrypt-agent-managed-exporter/node_exporter";
const INVENTORY:&str="/opt/consolecrypt-agent-managed-exporter/notices/inventory.json";
const CATALOG:&str="/etc/consolecrypt-agent/managed-node-exporter.json";
type Stamp=(u64,u64,u64,u32,u32,u64,i64,i64,i64,i64);
fn stamp(m:&fs::Metadata)->Stamp {
    (m.dev(),m.ino(),m.len(),m.uid(),m.mode(),m.nlink(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())
}
struct Pinned {path:PathBuf,file:File,stamp:Stamp,hash:Hash,raw:Vec<u8>}
impl Pinned {
    fn open(path:&Path,cap:u64,executable:bool,retain:bool)->Result<Self> {
        crate::durable::absolute(path)?;
        for parent in path.ancestors().skip(1) {
            let m=fs::symlink_metadata(parent).map_err(|_|Error::Storage)?;
            if !m.is_dir()||m.file_type().is_symlink()||m.uid()!=0||m.mode()&0o022!=0 {
                return Err(Error::Storage)
            }
        }
        let m=fs::symlink_metadata(path).map_err(|_|Error::Storage)?;
        if !m.is_file()||m.file_type().is_symlink()||m.uid()!=0||m.nlink()!=1||m.len()==0||m.len()>cap
            ||m.mode()&0o7777!=if executable{0o755}else{0o644} {return Err(Error::Storage)}
        let mut file=OpenOptions::new().read(true).custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC)
            .open(path).map_err(|_|Error::Storage)?;
        if stamp(&m)!=stamp(&file.metadata().map_err(|_|Error::Storage)?) {return Err(Error::Storage)}
        let mut hash=Sha256::new();let mut raw=Vec::new();let mut n=0;let mut block=[0u8;65536];
        loop {
            let got=file.read(&mut block).map_err(|_|Error::Storage)?;if got==0{break}
            n+=got as u64;if n>cap{return Err(Error::Quota)}hash.update(&block[..got]);
            if retain {raw.extend_from_slice(&block[..got]);}else if raw.len()<20 {
                let take=(20-raw.len()).min(got);raw.extend_from_slice(&block[..take]);
            }
        }
        let value=Self{path:path.to_owned(),file,stamp:stamp(&m),hash:Hash::from_bytes(hash.finalize().into()),raw};
        if n!=m.len(){return Err(Error::Storage)}value.check()?;Ok(value)
    }
    fn check(&self)->Result<()> {
        let held=self.file.metadata().map_err(|_|Error::Storage)?;
        let named=fs::symlink_metadata(&self.path).map_err(|_|Error::Storage)?;
        if stamp(&held)!=self.stamp||stamp(&named)!=self.stamp||named.file_type().is_symlink() {
            return Err(Error::Storage)
        }Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct FilePolicy {format:String,policy:ManagedExporterPolicy,worker_sha256:Hash}
// Same existing cc-agent-notice-inventory/v1 material schema. CI/the explicit
// installer must run its existing complete provenance/material validator;
// Node independently rechecks all published material bytes and binary binding.
#[derive(Deserialize)]
struct Inventory {format:String,components:Vec<Component>}
#[derive(Deserialize)]
struct Component {
    kind:String,name:Option<String>,version:Option<String>,platform:Option<String>,
    binary_sha256:Option<Hash>,binary_bytes:Option<u64>,archive_sha256:Option<Hash>,
    #[serde(default)] notices:Vec<Notice>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Notice {original_path:String,package_path:String,sha256:Hash}

pub(super) struct Catalog {pub policy:ManagedExporterPolicy,pins:Vec<Pinned>,binary:usize,worker:File,worker_stamp:Stamp}
impl Catalog {
    pub(super) fn load()->Result<Self> {
        let catalog=Pinned::open(Path::new(CATALOG),16384,false,true)?;
        let config:FilePolicy=serde_json::from_slice(&catalog.raw).map_err(|_|Error::InvalidWire)?;
        if config.format!="cc-agent-installed-managed-exporter/v1" {return Err(Error::InvalidWire)}
        config.policy.validate()?;
        let binary=Pinned::open(Path::new(BINARY),64*1024*1024,true,false)?;
        let expected=match config.policy.artifact.platform.as_str() {
            "linux-x86_64" if cfg!(target_arch="x86_64")=>62,
            "linux-aarch64" if cfg!(target_arch="aarch64")=>183,
            _=>return Err(Error::CapabilityDenied),
        };
        if binary.hash!=config.policy.artifact.binary_sha256||binary.stamp.2!=config.policy.artifact.binary_bytes
            ||binary.raw.len()!=20||binary.raw[..6]!=*b"\x7fELF\x02\x01"
            ||u16::from_le_bytes([binary.raw[18],binary.raw[19]])!=expected {return Err(Error::WrongClaim)}
        let inventory=Pinned::open(Path::new(INVENTORY),256*1024,false,true)?;
        if inventory.hash!=config.policy.artifact.notice_inventory_sha256 {return Err(Error::WrongClaim)}
        let materials:Inventory=serde_json::from_slice(&inventory.raw).map_err(|_|Error::InvalidWire)?;
        if materials.format!="cc-agent-notice-inventory/v1"||materials.components.is_empty()
            ||materials.components.len()>128 {return Err(Error::Quota)}
        let nodes:Vec<_>=materials.components.iter().filter(|c|c.kind=="bundled-collector-binary"&&c.name.as_deref()==Some("node_exporter")).collect();
        let platform=if expected==62{"linux-amd64"}else{"linux-arm64"};
        if nodes.len()!=1||nodes[0].version.as_deref()!=Some("v1.12.1")||nodes[0].platform.as_deref()!=Some(platform)
            ||nodes[0].binary_sha256!=Some(binary.hash)||nodes[0].binary_bytes!=Some(binary.stamp.2)
            ||nodes[0].archive_sha256!=Some(config.policy.artifact.archive_sha256)||nodes[0].notices.is_empty() {
            return Err(Error::WrongClaim)
        }
        let mut pins=vec![catalog,binary,inventory];let mut seen=BTreeMap::new();let mut total=0u64;
        for component in &materials.components {
            for notice in &component.notices {
                if notice.original_path.is_empty()||notice.original_path.len()>512 {return Err(Error::InvalidWire)}
                let Some(hex)=notice.package_path.strip_prefix("notices/text/").and_then(|s|s.strip_suffix(".txt")) else {
                    return Err(Error::InvalidWire)
                };
                if hex.len()!=64||!hex.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)) {
                    return Err(Error::InvalidWire)
                }
                if let Some(previous)=seen.get(&notice.package_path) {
                    if previous!=&notice.sha256{return Err(Error::WrongClaim)}continue
                }
                seen.insert(notice.package_path.clone(),notice.sha256);
                if seen.len()>128 {return Err(Error::Quota)}
                let p=Path::new("/opt/consolecrypt-agent-managed-exporter").join(&notice.package_path);
                let material=Pinned::open(&p,128*1024,false,true)?;
                total+=material.stamp.2;if total>16*1024*1024||material.hash!=notice.sha256 {
                    return Err(Error::WrongClaim)
                }
                pins.push(material);
            }
        }
        if seen.is_empty(){return Err(Error::WrongClaim)}
        let mut worker=File::open("/proc/self/exe").map_err(|_|Error::Storage)?;
        let worker_meta=worker.metadata().map_err(|_|Error::Storage)?;
        if !worker_meta.is_file()||worker_meta.uid()!=0||worker_meta.nlink()!=1||worker_meta.mode()&0o7777!=0o755
            ||worker_meta.len()>64*1024*1024{return Err(Error::Storage)}
        let mut hash=Sha256::new();let mut block=[0u8;65536];let mut count=0;
        loop {let n=worker.read(&mut block).map_err(|_|Error::Storage)?;if n==0{break}count+=n as u64;
            if count>64*1024*1024{return Err(Error::Quota)}hash.update(&block[..n]);}
        if Hash::from_bytes(hash.finalize().into())!=config.worker_sha256||count!=worker_meta.len()
            ||stamp(&worker_meta)!=stamp(&worker.metadata().map_err(|_|Error::Storage)?) {return Err(Error::WrongClaim)}
        // File capabilities may clear Linux parent-death signal at exec. Both
        // independently pinned executable files must have no capability xattr.
        for file in [&worker,&pins[1].file] {
            let mut capability=[0u8;64];
            if !matches!(rustix::fs::fgetxattr(file,c"security.capability",&mut capability[..]),Err(rustix::io::Errno::NODATA)) {
                return Err(Error::ScopeDenied)
            }
        }
        Ok(Self{policy:config.policy,pins,binary:1,worker,worker_stamp:stamp(&worker_meta)})
    }
    pub(super) fn check(&self)->Result<()> {
        for pin in &self.pins {pin.check()?}
        if stamp(&self.worker.metadata().map_err(|_|Error::Storage)?)!=self.worker_stamp{return Err(Error::Storage)}
        self.policy.validate()
    }
    pub(super) fn same_worker(&self,pid:u32)->Result<bool> {
        Ok(stamp(&fs::metadata(format!("/proc/{pid}/exe")).map_err(|_|Error::Storage)?)==self.worker_stamp)
    }
    pub(super) fn execution_path(&self)->String {format!("/proc/self/fd/{}",self.pins[self.binary].file.as_raw_fd())}
    pub(super) fn check_child_executable(&self,pid:u32)->Result<()> {
        self.check()?;
        let executable=fs::metadata(format!("/proc/{pid}/exe")).map_err(|_|Error::Storage)?;
        let original=&self.pins[self.binary];
        if stamp(&executable)!=original.stamp {return Err(Error::WrongClaim)}Ok(())
    }
    pub(super) fn descriptor(&self)->Result<Value> {
        self.check()?;Ok(json!({"policy":self.policy,"manifestSha256":self.policy.digest()?}))
    }
}
