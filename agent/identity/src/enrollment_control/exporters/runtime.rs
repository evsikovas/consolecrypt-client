//! Real nonroot child custody. No root setup, external service adoption or
//! success from a PID/port/JSON marker. Original Child and listener inode agree.
use super::*;
use std::net::{SocketAddr,TcpListener};
use std::process::{Child,Command,Stdio};
use std::os::unix::process::CommandExt;
use std::os::unix::fs::{DirBuilderExt,PermissionsExt};
use std::collections::BTreeMap;
pub(super) const ROOT:&str="/run/consolecrypt-agent/managed-exporter";
pub(super) fn original_remaining(deadline:Instant,now:Instant)->Result<Duration> {
    deadline.checked_duration_since(now).filter(|remaining|!remaining.is_zero()).ok_or(Error::Expired)
}

fn text(path:&Path,cap:u64)->Result<String> {
    let mut raw=Vec::new();let mut file=OpenOptions::new().read(true).custom_flags(NONBLOCK|CLOEXEC|NOFOLLOW)
        .open(path).map_err(|_|Error::Storage)?;
    Read::by_ref(&mut file).take(cap+1).read_to_end(&mut raw).map_err(|_|Error::Storage)?;
    if raw.len() as u64>cap{return Err(Error::Quota)}String::from_utf8(raw).map_err(|_|Error::Storage)
}
fn uid()->Result<u32> {
    text(Path::new("/proc/self/status"),16384)?.lines().find_map(|l|l.strip_prefix("Uid:"))
        .and_then(|v|v.split_whitespace().nth(1)).ok_or(Error::Storage)?.parse().map_err(|_|Error::Storage)
}
pub(super) fn environment()->Result<()> {
    let status=text(Path::new("/proc/self/status"),16384)?;
    let field=|name:&str|status.lines().find_map(|l|l.strip_prefix(name)).map(str::trim).ok_or(Error::Storage);
    if uid()?==0||field("NoNewPrivs:")?!="1"||u64::from_str_radix(field("CapEff:")?,16).map_err(|_|Error::Storage)?!=0
        ||u64::from_str_radix(field("CapPrm:")?,16).map_err(|_|Error::Storage)?!=0 {return Err(Error::ScopeDenied)}
    let groups:Vec<_>=field("Groups:")?.split_whitespace().collect();
    let gid=field("Gid:")?.split_whitespace().nth(1).ok_or(Error::Storage)?;
    if groups.iter().any(|g|*g!=gid){return Err(Error::ScopeDenied)}
    let cgroup=text(Path::new("/proc/self/cgroup"),4096)?;
    let rows:Vec<_>=cgroup.lines().collect();
    if rows.len()!=1{return Err(Error::CapabilityDenied)}
    let relative=rows[0].strip_prefix("0::/").ok_or(Error::CapabilityDenied)?;
    if relative.is_empty()||relative.split('/').any(|c|c.is_empty()||c=="."||c=="..") {return Err(Error::ScopeDenied)}
    let group=Path::new("/sys/fs/cgroup").join(relative);
    for path in [&group, &group.join("cgroup.procs")] {
        let m=fs::symlink_metadata(path).map_err(|_|Error::Storage)?;
        if m.file_type().is_symlink()||![0,uid()?].contains(&m.uid())||m.mode()&0o022!=0{return Err(Error::ScopeDenied)}
    }
    let number=|name:&str|->Result<u64>{text(&group.join(name),128)?.trim().parse().map_err(|_|Error::Quota)};
    if !(1..=256*1024*1024).contains(&number("memory.max")?)||number("memory.swap.max")?!=0
        ||!(2..=16).contains(&number("pids.max")?) {return Err(Error::Quota)}
    let cpu=text(&group.join("cpu.max"),128)?;let cpu:Vec<_>=cpu.split_whitespace().collect();
    if cpu.len()!=2{return Err(Error::Quota)}
    let quota:u64=cpu[0].parse().map_err(|_|Error::Quota)?;let period:u64=cpu[1].parse().map_err(|_|Error::Quota)?;
    if quota==0||period==0||quota.checked_mul(4).is_none_or(|n|n>period){return Err(Error::Quota)}
    let limits=text(Path::new("/proc/self/limits"),8192)?;
    let fd=limits.lines().find_map(|l|l.strip_prefix("Max open files"))
        .and_then(|v|v.split_whitespace().next()).ok_or(Error::Storage)?;
    if fd.parse::<u64>().map_err(|_|Error::Quota)?>128{return Err(Error::Quota)}
    private(Path::new(ROOT))?;Ok(())
}
fn private(path:&Path)->Result<()> {
    let owner=uid()?;
    for p in path.ancestors() {
        let m=fs::symlink_metadata(p).map_err(|_|Error::Storage)?;
        if !m.is_dir()||m.file_type().is_symlink()||![0,owner].contains(&m.uid())||m.mode()&0o022!=0 {
            return Err(Error::Storage)
        }
    }
    let m=fs::symlink_metadata(path).map_err(|_|Error::Storage)?;
    if m.uid()!=owner||m.mode()&0o7777!=0o700{return Err(Error::Storage)}Ok(())
}
fn identity(m:&fs::Metadata)->(u64,u64,u32,u32,u64) {(m.dev(),m.ino(),m.uid(),m.mode(),m.nlink())}
struct Baseline {path:PathBuf,id:(u64,u64),processes:BTreeMap<u32,u64>}
impl Baseline {
    fn process(pid:u32)->Result<u64> {
        let raw=text(&PathBuf::from(format!("/proc/{pid}/stat")),4096)?;
        raw.rsplit_once(") ").and_then(|(_,tail)|tail.split_whitespace().nth(19))
            .ok_or(Error::Storage)?.parse().map_err(|_|Error::Storage)
    }
    fn capture()->Result<Self> {
        environment()?;
        let raw=text(Path::new("/proc/self/cgroup"),4096)?;
        let rel=raw.trim().strip_prefix("0::/").ok_or(Error::Storage)?;
        let path=Path::new("/sys/fs/cgroup").join(rel);
        let m=fs::symlink_metadata(&path).map_err(|_|Error::Storage)?;
        let mut processes=BTreeMap::new();
        for raw in text(&path.join("cgroup.procs"),4096)?.lines() {
            if processes.len()>=16{return Err(Error::Quota)}
            let pid=raw.parse().map_err(|_|Error::Storage)?;
            if processes.insert(pid,Self::process(pid)?).is_some(){return Err(Error::Storage)}
        }
        if !processes.contains_key(&std::process::id()){return Err(Error::Storage)}
        Ok(Self{path,id:(m.dev(),m.ino()),processes})
    }
    fn no_new_processes(&self)->Result<()> {
        environment()?;
        let m=fs::symlink_metadata(&self.path).map_err(|_|Error::Storage)?;
        if (m.dev(),m.ino())!=self.id{return Err(Error::Storage)}
        let mut count=0;
        for raw in text(&self.path.join("cgroup.procs"),4096)?.lines() {
            count+=1;if count>16{return Err(Error::Quota)}
            let pid=raw.parse().map_err(|_|Error::Storage)?;
            if self.processes.get(&pid)!=Some(&Self::process(pid)?) {return Err(Error::UncertainCommit)}
        }
        Ok(())
    }
}
struct Namespace {directory:PathBuf,id:(u64,u64,u32,u32,u64),_lock:File,joined:bool}
impl Namespace {
    fn create()->Result<Self> {
        environment()?;
        let lock_path=Path::new(ROOT).join("owner.lock");
        let lock=OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .custom_flags(NOFOLLOW|NONBLOCK|CLOEXEC).mode(0o600).open(&lock_path).map_err(|_|Error::Storage)?;
        let held=lock.metadata().map_err(|_|Error::Storage)?;let named=fs::symlink_metadata(&lock_path).map_err(|_|Error::Storage)?;
        if !held.is_file()||held.uid()!=uid()?||held.mode()&0o7777!=0o600||held.nlink()!=1
            ||held.len()!=0||identity(&held)!=identity(&named) {return Err(Error::Storage)}
        File::try_lock(&lock).map_err(|_|Error::Storage)?;
        // A leftover/unknown namespace is a DENIAL, never a stale-parent join
        // witness or permission to adopt/delete a previous process's state.
        let mut entries=fs::read_dir(ROOT).map_err(|_|Error::Storage)?;
        if entries.next().ok_or(Error::Storage)?.map_err(|_|Error::Storage)?.path()!=lock_path||entries.next().is_some() {
            return Err(Error::UncertainCommit)
        }
        let mut nonce=[0u8;16];getrandom::fill(&mut nonce).map_err(|_|Error::Entropy)?;
        if nonce==[0;16]{return Err(Error::Entropy)}
        let directory=Path::new(ROOT).join(hex::encode(nonce));
        fs::DirBuilder::new().mode(0o700).create(&directory).map_err(|_|Error::Storage)?;
        let id=identity(&fs::symlink_metadata(&directory).map_err(|_|Error::Storage)?);
        Ok(Self{directory,id,_lock:lock,joined:false})
    }
    fn check(&self)->Result<()> {
        private(Path::new(ROOT))?;
        if identity(&fs::symlink_metadata(&self.directory).map_err(|_|Error::Storage)?)!=self.id {
            return Err(Error::Storage)
        }Ok(())
    }
    fn cleanup(&mut self)->Result<()> {
        self.check()?;
        if !self.joined||fs::read_dir(&self.directory).map_err(|_|Error::Storage)?.next().is_some() {
            return Err(Error::UncertainCommit)
        }
        fs::remove_dir(&self.directory).map_err(|_|Error::Storage)
    }
}
struct DeadlineGuard {cancel:Option<std::sync::mpsc::Sender<()>>,thread:Option<std::thread::JoinHandle<()>>}
impl DeadlineGuard {
    fn start(child:&Child,deadline:Instant)->Result<Self> {
        let pid=rustix::process::Pid::from_raw(child.id().try_into().map_err(|_|Error::Storage)?).ok_or(Error::Storage)?;
        // The original unreaped Child prevents PID reuse before pidfd_open.
        let fd=rustix::process::pidfd_open(pid,rustix::process::PidfdFlags::empty()).map_err(|_|Error::Storage)?;
        let(tx,rx)=std::sync::mpsc::channel();
        let thread=std::thread::Builder::new().name("cc-exporter-expiry".into()).spawn(move||{
            if rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_err() {
                let _=rustix::process::pidfd_send_signal(&fd,rustix::process::Signal::KILL);
            }
        }).map_err(|_|Error::Storage)?;
        Ok(Self{cancel:Some(tx),thread:Some(thread)})
    }
    fn joined(&mut self)->Result<()> {
        if let Some(cancel)=self.cancel.take(){let _=cancel.send(());}
        if let Some(thread)=self.thread.take(){thread.join().map_err(|_|Error::UncertainCommit)?}Ok(())
    }
}
pub(super) struct Owned {child:Child,namespace:Namespace,baseline:Baseline,deadline:Option<DeadlineGuard>,joined:bool,ready:bool,closed:bool,descendants_unknown:bool}
impl Owned {
    pub(super) fn spawn(catalog:&artifact::Catalog,expiry:Timestamp,deadline:Instant)->Result<Self> {
        original_remaining(deadline,Instant::now())?;
        catalog.check()?;let baseline=Baseline::capture()?;let mut namespace=Namespace::create()?;
        // Fixed loopback only. An existing listener is preserved, never adopted.
        let address:SocketAddr="127.0.0.1:19100".parse().map_err(|_|Error::InvalidWire)?;
        let probe=match TcpListener::bind(address) {Ok(v)=>v,Err(_)=>{namespace.joined=true;let _=namespace.cleanup();return Err(Error::Conflict)}};
        drop(probe);
        if let Err(error)=original_remaining(deadline,Instant::now()) {
            namespace.joined=true;let _=namespace.cleanup();return Err(error)
        }
        let child=Command::new("/proc/self/exe")
            .args(["worker-managed-exporter-exec".to_owned(),std::process::id().to_string(),
                Baseline::process(std::process::id())?.to_string(),expiry.seconds().to_string()])
            .current_dir(&namespace.directory).env_clear().env("LANG","C")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        let child=match child {Ok(v)=>v,Err(_)=>{namespace.joined=true;let _=namespace.cleanup();return Err(Error::Storage)}};
        let mut owned=Self{child,namespace,baseline,deadline:None,joined:false,ready:false,closed:false,descendants_unknown:false};
        // Install the original pidfd deadline even if spawn itself returned
        // late, then kill/join immediately rather than publish a running child.
        owned.deadline=Some(DeadlineGuard::start(&owned.child,deadline)?);
        original_remaining(deadline,Instant::now())?;Ok(owned)
    }
    pub(super) fn check(&mut self,catalog:&artifact::Catalog)->Result<bool> {
        self.namespace.check()?;environment()?;
        if self.child.try_wait().map_err(|_|Error::Storage)?.is_some(){self.joined=true;return Err(Error::WrongState)}
        let pid=self.child.id();
        if !self.ready&&catalog.same_worker(pid)? {return Ok(false)}
        catalog.check_child_executable(pid)?;
        // Fixed collectors never spawn child processes. A changed descendant
        // topology remains Unknown; a leader exit alone cannot attest drainage.
        if !text(&PathBuf::from(format!("/proc/{pid}/task/{pid}/children")),4096)?.trim().is_empty() {
            self.descendants_unknown=true;
            return Err(Error::UncertainCommit)
        }
        let sockets=text(Path::new("/proc/net/tcp"),1024*1024)?;
        let mut listening=None;
        for line in sockets.lines().skip(1) {
            let fields:Vec<_>=line.split_whitespace().collect();
            if fields.len()>9&&fields[1]=="0100007F:4A9C"&&fields[3]=="0A" {
                if listening.replace(fields[9].to_owned()).is_some(){return Err(Error::Conflict)}
            }
        }
        let mut owns=false;let mut count=0;
        for entry in fs::read_dir(format!("/proc/{pid}/fd")).map_err(|_|Error::Storage)? {
            count+=1;if count>128{return Err(Error::Quota)}
            let path=entry.map_err(|_|Error::Storage)?.path();
            let link=match fs::read_link(path) {
                Ok(value)=>value,
                Err(error) if error.kind()==std::io::ErrorKind::NotFound=>continue,
                Err(_)=>return Err(Error::Storage),
            };
            if let Some(inode)=&listening {
                if link.to_string_lossy()==format!("socket:[{inode}]"){owns=true}
            }
        }
        if listening.is_some()&&!owns{return Err(Error::Conflict)}
        self.ready=owns;Ok(owns)
    }
    pub(super) fn stop_join(&mut self)->Result<()> {
        if self.closed{return Ok(())}
        if !self.joined {
            let _=self.child.kill();let end=Instant::now()+Duration::from_secs(3);
            while Instant::now()<end {
                if self.child.try_wait().map_err(|_|Error::Storage)?.is_some(){self.joined=true;break}
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if !self.joined||self.descendants_unknown{return Err(Error::UncertainCommit)}
        // Child.wait is the positive original-child join. The immutable-group
        // baseline is a negative fence only: never adopt or kill any bystander.
        self.baseline.no_new_processes()?;
        if let Some(deadline)=&mut self.deadline {deadline.joined()?;}
        self.namespace.joined=true;self.namespace.cleanup()?;self.closed=true;Ok(())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {let _=self.stop_join();}
}

/// Private self-exec gate, not an execution RPC. Actual kernel parent must be
/// the same independently pinned worker executable in its fixed enrollment
/// role. That role reaches this fork only after real Actor admission.
pub(super) fn exec(args:&[String])->Result<()> {
    if args.len()!=4||args[0]!="worker-managed-exporter-exec" {return Err(Error::InvalidWire)}
    let parent:i32=args[1].parse().map_err(|_|Error::InvalidWire)?;
    let pid=rustix::process::Pid::from_raw(parent).ok_or(Error::InvalidWire)?;
    let parent_start:u64=args[2].parse().map_err(|_|Error::InvalidWire)?;
    let expiry=Timestamp::new(args[3].parse().map_err(|_|Error::InvalidWire)?)?;
    rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::KILL)).map_err(|_|Error::Storage)?;
    if rustix::process::getppid()!=Some(pid)||Baseline::process(parent as u32)?!=parent_start {
        return Err(Error::WrongContext)
    }
    environment()?;let catalog=artifact::Catalog::load()?;
    if !catalog.same_worker(parent as u32)? {return Err(Error::WrongContext)}
    let args=text(&PathBuf::from(format!("/proc/{parent}/cmdline")),4096)?.into_bytes();
    let parts:Vec<_>=args.split(|b|*b==0).collect();
    if parts.len()!=3||parts[1]!=b"worker-node-enroll-sqlite"||!parts[2].is_empty(){return Err(Error::WrongContext)}
    let now=Clock::observed()?;
    if expiry<=now||expiry.seconds()-now.seconds()>300{return Err(Error::Expired)}
    if rustix::process::getppid()!=Some(pid)||Baseline::process(parent as u32)?!=parent_start {
        return Err(Error::WrongContext)
    }
    catalog.check()?;
    let error=Command::new(catalog.execution_path())
        .args(["--web.listen-address=127.0.0.1:19100","--collector.disable-defaults","--collector.cpu","--collector.meminfo"])
        .env_clear().env("LANG","C").env("GOMAXPROCS","1").env("GOMEMLIMIT","128MiB").exec();
    let _=error;Err(Error::Storage)
}
