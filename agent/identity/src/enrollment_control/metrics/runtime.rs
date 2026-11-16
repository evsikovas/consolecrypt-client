//! Architecture observation before advertisement, never a signing authority.
use super::*;
use std::io::{Seek, SeekFrom};
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
    if cap>128*1024{return Err(Error::ScopeDenied)}
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
// Only bounded metadata uses held() above. A runtime ELF is kept on its
// original descriptor; neither its body nor a second copy is accumulated.
type BinaryIdentity=(u64,u64,u32,u32,u64,u64,i64,i64,i64,i64);
fn binary_identity(m:&fs::Metadata)->BinaryIdentity {
    (m.dev(),m.ino(),m.uid(),m.mode(),m.nlink(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())
}
struct BinaryScan {header:[u8;64],bytes:u64,hash:Hash}
fn scan_binary(reader:&mut impl Read,cap:u64)->Result<BinaryScan> {
    let mut hash=Sha256::new();let mut header=[0u8;64];let mut bytes=0u64;let mut chunk=[0u8;65536];
    loop {
        let n=reader.read(&mut chunk).map_err(|_|Error::Storage)?;
        if n==0 {break}
        if bytes<64 {
            let offset=bytes as usize;let count=n.min(64-offset);
            header[offset..offset+count].copy_from_slice(&chunk[..count]);
        }
        bytes=bytes.checked_add(n as u64).ok_or(Error::Storage)?;
        if bytes>cap {return Err(Error::Storage)}
        hash.update(&chunk[..n]);
    }
    Ok(BinaryScan {header,bytes,hash:Hash::from_bytes(hash.finalize().into())})
}
fn literal_equal(left:&mut impl Read,right:&mut impl Read,bytes:u64)->Result<bool> {
    let mut a=[0u8;65536];let mut b=[0u8;65536];let mut remaining=bytes;
    while remaining!=0 {
        let n=remaining.min(a.len()as u64)as usize;
        left.read_exact(&mut a[..n]).map_err(|_|Error::Storage)?;
        right.read_exact(&mut b[..n]).map_err(|_|Error::Storage)?;
        if a[..n]!=b[..n] {return Ok(false)}
        remaining-=n as u64;
    }
    // Both original descriptors must still end at the observed byte count.
    let mut tail=[0u8;1];
    if left.read(&mut tail).map_err(|_|Error::Storage)?!=0 {return Ok(false)}
    Ok(right.read(&mut tail).map_err(|_|Error::Storage)?==0)
}
fn checked_named_identity(file:&File,path:&Path,expected:BinaryIdentity)->Result<()> {
    if expected!=binary_identity(&file.metadata().map_err(|_|Error::Storage)?)
        ||expected!=binary_identity(&fs::symlink_metadata(path).map_err(|_|Error::Storage)?) {
        return Err(Error::Storage)
    }
    Ok(())
}
struct HeldBinary {path:PathBuf,file:File,identity:BinaryIdentity,scan:BinaryScan}
impl HeldBinary {
    fn capture(path:&Path,cap:u64)->Result<Self> {
        if extra_cap(path).is_none()&&path!=Path::new(VMAGENT){return Err(Error::ScopeDenied)}
        // Retain every existing root_digest parent/type/UID/mode/nlink/cap
        // observation before and after reading this same nofollow descriptor.
        let expected=root_digest(path)?;
        let named=fs::symlink_metadata(path).map_err(|_|Error::Storage)?;
        let mut file=OpenOptions::new().read(true).custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC)
            .open(path).map_err(|_|Error::Storage)?;
        let m=file.metadata().map_err(|_|Error::Storage)?;
        if !m.is_file()||m.uid()!=0||m.nlink()!=1||m.mode()&0o7022!=0||m.len()==0||m.len()>cap
            ||binary_identity(&m)!=binary_identity(&named){return Err(Error::Storage)}
        let scan=scan_binary(&mut file,cap)?;
        if scan.bytes!=m.len()||scan.hash!=expected
            ||binary_identity(&m)!=binary_identity(&file.metadata().map_err(|_|Error::Storage)?)
            ||binary_identity(&m)!=binary_identity(&fs::symlink_metadata(path).map_err(|_|Error::Storage)?)
            ||root_digest(path)?!=expected{return Err(Error::Storage)}
        Ok(Self {path:path.to_path_buf(),file,identity:binary_identity(&m),scan})
    }
    fn check(&self)->Result<()> {
        for parent in self.path.ancestors().skip(1) {
            let m=fs::symlink_metadata(parent).map_err(|_|Error::Storage)?;
            if !m.is_dir()||m.file_type().is_symlink()||m.uid()!=0||m.mode()&0o022!=0{return Err(Error::Storage)}
        }
        checked_named_identity(&self.file,&self.path,self.identity)
    }
    fn same_bytes(&mut self,other:&mut Self)->Result<bool> {
        self.check()?;other.check()?;
        if self.scan.bytes!=other.scan.bytes{return Ok(false)}
        self.file.seek(SeekFrom::Start(0)).map_err(|_|Error::Storage)?;
        other.file.seek(SeekFrom::Start(0)).map_err(|_|Error::Storage)?;
        let same=literal_equal(&mut self.file,&mut other.file,self.scan.bytes)?;
        self.check()?;other.check()?;
        Ok(same)
    }
}
fn elf(observed:&BinaryScan,platform:&str,hash:Hash,bytes:u64)->Result<()> {
    let machine=match platform{"linux-x86_64"=>62,"linux-aarch64"=>183,_=>return Err(Error::CapabilityDenied)};
    if observed.bytes!=bytes||observed.bytes<64||&observed.header[..7]!=b"\x7fELF\x02\x01\x01"
        ||u16::from_le_bytes([observed.header[18],observed.header[19]])!=machine||observed.hash!=hash{return Err(Error::WrongClaim)}
    Ok(())
}
pub(super) fn legacy()->Result<()> {
    if kernel()?!="linux-x86_64"{return Err(Error::CapabilityDenied)}
    let binary=HeldBinary::capture(Path::new(VMAGENT),23209584)?;
    elf(&binary.scan,"linux-x86_64",decode(json!(VMAGENT_SHA))?,23209584)
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
    let binary_path=release.join(name);let mut binary=HeldBinary::capture(&binary_path,cap)?;
    elf(&binary.scan,&runtime.platform,hash,runtime.binary_bytes)?;
    let mut stable=HeldBinary::capture(Path::new(stable_path),cap)?;
    elf(&stable.scan,&runtime.platform,hash,runtime.binary_bytes)?;
    if binary.scan.hash!=hash||stable.scan.hash!=hash||!binary.same_bytes(&mut stable)?||root_digest(&current_path)?!=current_hash
        ||root_digest(&record_path)?!=record_hash{return Err(Error::Storage)}
    Ok((hash,vec![(current_path,current_hash),(record_path,record_hash),(binary_path,hash)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn selected_namespace_has_no_caller_path_escape() {
        assert_eq!(extra_cap(Path::new(CODE).join("current.json").as_path()),Some(4096));
        assert_eq!(extra_cap(Path::new(CODE).join("releases/0.1.2/installed.json").as_path()),Some(128*1024));
        for path in [Path::new(disk::BINARY),Path::new(VMAGENT)] {
            assert!(matches!(held(path,128*1024+1),Err(Error::ScopeDenied)));
        }
        for path in ["releases/../installed.json","releases/0.1.2/other","releases/0.1.2/collector/other","../current.json"] {
            assert_eq!(extra_cap(Path::new(CODE).join(path).as_path()),None);
        }
    }
    #[test] fn observed_elf_cannot_borrow_architecture_or_length() {
        let mut raw=vec![0;64];raw[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");raw[18..20].copy_from_slice(&183u16.to_le_bytes());
        let observed=scan_binary(&mut std::io::Cursor::new(&raw),64).unwrap();
        assert!(elf(&observed,"linux-aarch64",digest(&raw),64).is_ok());
        assert!(elf(&observed,"linux-x86_64",digest(&raw),64).is_err());
        assert!(elf(&observed,"linux-aarch64",digest(&raw),65).is_err());
        assert!(elf(&observed,"linux-aarch64",Hash::from_bytes([1;32]),64).is_err());
    }
    struct ShortReads<'a> {raw:&'a[u8],chunk:usize}
    impl Read for ShortReads<'_> {
        fn read(&mut self,out:&mut[u8])->std::io::Result<usize> {
            let n=out.len().min(self.chunk).min(self.raw.len());out[..n].copy_from_slice(&self.raw[..n]);self.raw=&self.raw[n..];Ok(n)
        }
    }
    #[test] fn streaming_hash_and_header_cover_partial_reads_and_full_length() {
        let mut raw=vec![0x91;131073];raw[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");raw[18..20].copy_from_slice(&62u16.to_le_bytes());
        let observed=scan_binary(&mut ShortReads {raw:&raw,chunk:3},raw.len()as u64).unwrap();
        assert_eq!(observed.bytes,raw.len()as u64);assert_eq!(observed.hash,digest(&raw));
        assert_eq!(observed.header.as_slice(),&raw[..64]);assert!(elf(&observed,"linux-x86_64",digest(&raw),raw.len()as u64).is_ok());
        assert!(scan_binary(&mut std::io::Cursor::new(&raw),raw.len()as u64-1).is_err());
    }
    #[test] fn literal_comparison_checks_boundary_bytes_short_reads_and_both_eofs() {
        let raw=vec![0x43;131073];let mut changed=raw.clone();changed[65536]^=1;
        assert!(literal_equal(&mut ShortReads {raw:&raw,chunk:3},&mut ShortReads {raw:&raw,chunk:11},raw.len()as u64).unwrap());
        assert!(!literal_equal(&mut std::io::Cursor::new(&raw),&mut std::io::Cursor::new(&changed),raw.len()as u64).unwrap());
        let mut extra=raw.clone();extra.push(1);
        assert!(!literal_equal(&mut std::io::Cursor::new(&raw),&mut std::io::Cursor::new(&extra),raw.len()as u64).unwrap());
        assert!(literal_equal(&mut std::io::Cursor::new(&raw),&mut std::io::Cursor::new(&raw[..raw.len()-1]),raw.len()as u64).is_err());
    }
    #[test] fn named_replacement_cannot_borrow_the_original_held_file() {
        let mut random=[0u8;16];getrandom::fill(&mut random).unwrap();
        let root=std::env::temp_dir().join(format!("cc-runtime-held-{}",hex::encode(random)));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root_identity=binary_identity(&root.symlink_metadata().unwrap());
        let named=root.join("original");let away=root.join("held-away");
        let mut file=OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&named).unwrap();
        file.write_all(b"same bytes").unwrap();file.sync_all().unwrap();
        let identity=binary_identity(&file.metadata().unwrap());
        checked_named_identity(&file,&named,identity).unwrap();
        fs::rename(&named,&away).unwrap();
        let mut replacement=OpenOptions::new().write(true).create_new(true).mode(0o600).open(&named).unwrap();
        replacement.write_all(b"same bytes").unwrap();replacement.sync_all().unwrap();
        let replaced=binary_identity(&replacement.metadata().unwrap());
        assert_ne!(identity.1,replaced.1);
        assert!(checked_named_identity(&file,&named,identity).is_err());
        assert_eq!(fs::read(&named).unwrap(),b"same bytes");
        drop(file);drop(replacement);
        assert_eq!(binary_identity(&away.symlink_metadata().unwrap()).0,identity.0);
        assert_eq!(binary_identity(&away.symlink_metadata().unwrap()).1,identity.1);
        assert_eq!(binary_identity(&named.symlink_metadata().unwrap()),replaced);
        fs::remove_file(away).unwrap();fs::remove_file(named).unwrap();
        let final_root=root.symlink_metadata().unwrap();
        assert_eq!((final_root.dev(),final_root.ino()),(root_identity.0,root_identity.1));
        fs::remove_dir(root).unwrap();
    }

}
