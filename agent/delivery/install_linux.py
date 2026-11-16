"""Explicit fixed-path Linux delivery around existing signed staging policy.

No download, keys, enrollment, profile mutation, vault access or data purge.
Selection metadata is never reported as the actual running runtime version.
"""
import hashlib,json,os,selectors,stat,subprocess,sys,time,zipfile,tempfile
from contextvars import ContextVar
from itertools import islice
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
import stage
from release_verifier import ReleaseVerifier
from prototype_package import SOURCES,RUNTIME_VERSION_FILE,runtime_declared_version
from package_wheel import PACKAGE as WSS_PACKAGE,SHA256 as WSS_SHA256
from notice_inventory import INVENTORY
import installed_runtime_policy as runtime_policy
import release_platforms
UNIT='consolecrypt-agent.service'

# Observation only: stable transaction boundaries, never caller data/authority.
_DIAGNOSTIC_STAGES=frozenset(('bootstrap','candidate_capture','signature_verify',
    'package_contract','unit_identity','stop_join','stage','collector_prerequisite',
    'runtime_policy','start','rollback','remove'))
_diagnostic_stage=ContextVar('agent_install_diagnostic_stage',default='bootstrap')

def _diagnostic(value):
    _diagnostic_stage.set(value)

def _closed_failure(error):
    value=_diagnostic_stage.get()
    stage_name=value if value in _DIAGNOSTIC_STAGES else 'bootstrap'
    if isinstance(error,subprocess.TimeoutExpired):kind='process_timeout'
    elif isinstance(error,FileNotFoundError):kind='missing_input'
    elif isinstance(error,PermissionError):kind='permission_denied'
    elif isinstance(error,(json.JSONDecodeError,zipfile.BadZipFile)):kind='invalid_public_metadata'
    elif isinstance(error,stage.StageError):kind='staging_denied'
    elif isinstance(error,(ValueError,TypeError,KeyError)):kind='policy_denied'
    elif isinstance(error,OSError):kind='io_unavailable'
    else:kind='unexpected'
    return{'stage':stage_name,'errorClass':kind}


def need(v):
    if not v:raise ValueError('Linux Agent delivery refused')

def read(path,limit,uid=0):
    path=stage._safe_path(path);need(path.is_absolute())
    if uid is not None:
        for p in(path,*path.parents):
            m=p.lstat();need(m.st_uid==uid and not m.st_mode&0o022)
    fd=os.open(path,os.O_RDONLY|getattr(os,'O_BINARY',0)|getattr(os,'O_NONBLOCK',0)|getattr(os,'O_NOFOLLOW',0)|getattr(os,'O_CLOEXEC',0))
    try:
        m=os.fstat(fd);need(stat.S_ISREG(m.st_mode)and m.st_nlink==1 and m.st_size<=limit)
        raw=os.read(fd,limit+1);n=os.fstat(fd);named=path.lstat()
        need(len(raw)==m.st_size and (m.st_dev,m.st_ino,m.st_size,m.st_mtime_ns,m.st_ctime_ns)==(n.st_dev,n.st_ino,n.st_size,n.st_mtime_ns,n.st_ctime_ns)and(named.st_dev,named.st_ino)==(m.st_dev,m.st_ino))
        return raw
    finally:os.close(fd)

class Paths:
    def __init__(self,root=Path('/')):
        self.root=Path(root);self.prefix=self.root/'opt/consolecrypt-agent-install'
        self.trust=self.root/'etc/consolecrypt-agent-release/trust.json'
        self.key=self.root/'etc/consolecrypt-agent-release/public-key.json'
        self.verifier=self.root/'usr/local/libexec/consolecrypt-agent-release/cc-agent-identity'
        self.artifact=self.root/'var/cache/consolecrypt-agent/candidate.zip'
        self.manifest=self.root/'var/cache/consolecrypt-agent/candidate-release.json'
        self.lock=self.root/'opt/consolecrypt-agent-install.lock'
        self.unit=self.root/'etc/systemd/system'/UNIT
        self.unit_template=self.root/'usr/local/libexec/consolecrypt-agent-install/consolecrypt-agent-installed.service'
        self.egress_config=self.root/'etc/consolecrypt-agent/egress.json'
        self.runtime_dropin=self.unit.parent/(UNIT+'.d')/runtime_policy.DROPIN
        self.runtime_record=self.prefix/'runtime-policy-owner.json'
        self.collector_provision=self.root/'usr/local/libexec/consolecrypt-agent-install/collector_provision.py'

def trusted_parent(path,uid=0):
    parent=stage._safe_path(path);need(parent.is_absolute())
    for component in(parent,*parent.parents):
        info=component.lstat();need(stat.S_ISDIR(info.st_mode)and not info.st_mode&0o022)
        if uid is not None:need(info.st_uid==uid)
    return parent

def unit_preflight(paths,uid=0):
    _diagnostic('unit_identity')
    expected=read(paths.unit_template,32768,uid)
    # Only one exact locally-owned deterministic policy; no foreign drop-ins.
    runtime_policy.Review(paths,uid).check()
    if paths.unit.exists()or paths.unit.is_symlink():need(read(paths.unit,32768,uid)==expected)
    return expected

def publish_unit(paths,uid=0):
    expected=unit_preflight(paths,uid)
    if paths.unit.exists():return False # Exact known bytes; never replace.
    # Atomic EXCLUSIVE publication: link a completed private same-parent file,
    # never os.replace() a unit that may have appeared since preflight.
    if uid is not None:trusted_parent(paths.unit.parent,uid)
    fd,name=tempfile.mkstemp(prefix='.cc-agent-unit-',dir=paths.unit.parent);temporary=Path(name)
    try:
        with os.fdopen(fd,'wb')as output:output.write(expected);output.flush();os.fsync(output.fileno())
        temporary.chmod(0o644)
        try:os.link(temporary,paths.unit,follow_symlinks=False)
        except FileExistsError:need(read(paths.unit,32768,uid)==expected)
    finally:temporary.unlink()
    need(read(paths.unit,32768,uid)==expected)
    if os.name!='nt':
        parent=os.open(paths.unit.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
        try:os.fsync(parent)
        finally:os.close(parent)
    return True

def _verifier(paths,uid=0,*,_snapshots):
    _diagnostic('bootstrap')
    # An absent production release bootstrap is a hard refusal, not a local
    # synthetic-key/unsigned fallback. No caller selects trust/PID/path/command.
    value=stage._json(read(paths.trust,4096,uid))
    need(type(value)is dict and set(value)=={'version','verifierSha256','publicKeySha256','platform'}and type(value['version'])is int and value['version']==1)
    selected=release_platforms.select(value['platform'],'release')
    release_platforms.require_native(selected)
    key_bytes=read(paths.key,4096,uid);key_sha=stage.digest(key_bytes);need(key_sha==value['publicKeySha256'])
    need(stage.digest(read(paths.verifier,64*1024*1024,uid))==value['verifierSha256'])
    _diagnostic('candidate_capture')
    manifest_bytes=read(paths.manifest,32768,uid);artifact_bytes=read(paths.artifact,stage.MAX_PACKAGE,uid)
    manifest_sha=stage.digest(manifest_bytes);artifact_sha=stage.digest(artifact_bytes)
    floor=0
    if(paths.prefix/stage.MARKER).exists():
        read(paths.prefix/stage.MARKER,1024,uid)
        anchor_path=paths.prefix/'var/lib/consolecrypt-agent/release-anchor.json'
        if anchor_path.exists():read(anchor_path,4096,uid)
        _,anchor=stage._anchor(stage._root(paths.prefix));floor=anchor['version']if anchor else 0
    snapshot=tempfile.TemporaryDirectory(prefix='.cc-agent-input-',dir=paths.artifact.parent)
    _snapshots.append(snapshot)
    captured=Path(snapshot.name)
    for name,raw in(('artifact.zip',artifact_bytes),('public-key.json',key_bytes),('release.json',manifest_bytes)):
        fd=os.open(captured/name,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,'wb')as output:output.write(raw)
    result=ReleaseVerifier(paths.verifier,value['verifierSha256'],captured/'public-key.json',captured/'release.json',platform=value['platform'],current_version=floor)
    result._snapshot=snapshot;result._artifact=captured/'artifact.zip'
    # Exact same known artifact may be cryptographically rechecked; a different
    # artifact cannot lower the durable release sequence floor.
    if floor and anchor['artifactSha256']==stage.digest(read(paths.artifact,stage.MAX_PACKAGE,uid)):result.current_version=floor-1
    _diagnostic('signature_verify')
    need(result(result._artifact)is True and result.verified_release is not None)
    need(stage.digest(read(paths.manifest,32768,uid))==manifest_sha and stage.digest(read(paths.key,4096,uid))==key_sha
         and stage.digest(read(paths.artifact,stage.MAX_PACKAGE,uid))==artifact_sha and result.verified_release['artifactSha256']==artifact_sha)
    # Consume the SAME existing explicit package/wheel contract; a signed but
    # incomplete staging toy must not be selected for the real Linux service.
    _diagnostic('package_contract')
    with zipfile.ZipFile(result._artifact)as archive:
        infos=archive.infolist();need(len(infos)<=stage.MAX_FILES+1 and sum(i.file_size for i in infos)<=stage.MAX_EXPANDED+stage.MAX_METADATA)
        need(archive.getinfo('package.json').file_size<=stage.MAX_METADATA)
        package=stage._json(archive.read('package.json'))
        need(package['version']==result.verified_release['displayVersion']
             and archive.getinfo(RUNTIME_VERSION_FILE).file_size<=65536
             and runtime_declared_version(archive.read(RUNTIME_VERSION_FILE))==result.verified_release['displayVersion'])
        collector_scripts={'collector/unit_spec.py','collector/volatile_unit_adapter.py','collector/volatile_unit_entry.py'}
        vmagent='collector/vmagent-prod'
        need(set(SOURCES)|collector_scripts|{'launch.py','systemd/consolecrypt-agent.service','bin/cc-agent-identity',vmagent,WSS_PACKAGE,INVENTORY}<=set(package['files']))
        need(package['files'][WSS_PACKAGE]==WSS_SHA256
             and all(package['file_modes'][name]=='0644'for name in collector_scripts)
             and package['file_modes'][vmagent]=='0755'
             and package['files'][vmagent]==selected['binarySha256']
             and 64<=archive.getinfo(vmagent).file_size<=release_platforms.MAX_COLLECTOR
             and (selected['observedBytes']is None or archive.getinfo(vmagent).file_size==selected['observedBytes']))
        digest=hashlib.sha256();count=0;vm_header=b''
        with archive.open(vmagent)as stream:
            while block:=stream.read(65536):
                if not vm_header:vm_header=block[:64]
                count+=len(block);need(count<=release_platforms.MAX_COLLECTOR);digest.update(block)
        need(count==archive.getinfo(vmagent).file_size and digest.hexdigest()==selected['binarySha256'])
        release_platforms.require_elf(vm_header,selected)
        with archive.open('bin/cc-agent-identity')as stream:header=stream.read(20)
        need(len(header)==20 and header[:6]==b'\x7fELF\x02\x01'and int.from_bytes(header[18:20],'little')==selected['elfMachine'])
    releases=paths.prefix/'opt/consolecrypt-agent/releases'
    if releases.exists():
        entries=list(islice(releases.iterdir(),9));need(len(entries)<=8 and all(stage.VERSION.fullmatch(p.name)and p.is_dir()and not p.is_symlink()for p in entries))
        need(len(entries)<8 or result.verified_release['displayVersion']in{p.name for p in entries})
    result._original_input=(artifact_sha,key_sha,manifest_sha)
    return result

def verifier(paths,uid=0):
    snapshots=[]
    try:return _verifier(paths,uid,_snapshots=snapshots)
    except BaseException:
        for snapshot in snapshots:snapshot.cleanup()
        raise

class CapturedRelease:
    """Only the already verified ORIGINAL candidate at stage's existing gate.
    No signer, new verification rule or recovery from changed candidate files.
    """
    def __init__(self,actual,paths,uid):
        need(type(actual)is ReleaseVerifier and actual.verified_release is not None)
        self.actual=actual;self.paths=paths;self.uid=uid
        self.binding=actual._original_input;self.original=dict(actual.verified_release)
        self.verified_release=dict(self.original)
    @property
    def current_version(self):return self.actual.current_version
    @current_version.setter
    def current_version(self,value):self.actual.current_version=value
    def unchanged(self,artifact):
        need(Path(artifact)==self.actual._artifact)
        return(stage.digest(read(self.paths.artifact,stage.MAX_PACKAGE,self.uid)),stage.digest(read(self.paths.key,4096,self.uid)),stage.digest(read(self.paths.manifest,32768,self.uid)))==self.binding
    def __call__(self,artifact):
        try:
            if not self.unchanged(artifact)or self.actual(artifact)is not True:return False
            return self.unchanged(artifact)and self.actual.verified_release==self.original
        except(OSError,ValueError,TypeError):return False

class Systemd:
    def __init__(self,paths=None,configuration=None):
        self.paths=Paths()if paths is None else paths
        self.configuration=runtime_policy.File(self.paths.egress_config,0,4096,optional=True)if configuration is None else configuration
    def run(self,*args,timeout=30):
        r=subprocess.run(['/usr/bin/systemctl','--no-pager','--no-ask-password',*args],stdin=subprocess.DEVNULL,capture_output=True,timeout=timeout)
        need(r.returncode==0 and len(r.stdout)+len(r.stderr)<=16384);return r.stdout
    def show(self):
        raw=self.run('show','--property=LoadState,ActiveState,SubState,MainPID,ControlGroup,Result,FragmentPath,DropInPaths','--',UNIT)
        value=dict(line.split('=',1)for line in raw.decode().splitlines())
        need(set(value)=={'LoadState','ActiveState','SubState','MainPID','ControlGroup','Result','FragmentPath','DropInPaths'}and value['MainPID'].isascii()and value['MainPID'].isdigit())
        return value
    def known(self,value):
        self.configuration.unchanged()
        policy=runtime_policy.Review(self.paths);policy.check()
        if value['LoadState']=='not-found':need(value['MainPID']=='0'and value['DropInPaths']=='');return
        need(value['LoadState']=='loaded'and value['FragmentPath']==str(self.paths.unit)
             and value['DropInPaths']==policy.expected_paths())
        need(read(self.paths.unit,32768)==read(self.paths.unit_template,32768))
    def stop_join(self):
        _diagnostic('stop_join')
        before=self.show();self.known(before);pid=int(before['MainPID']);fds=[];group=None
        parent=os.open('/sys/fs/cgroup/system.slice',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
        try:
            parent_info=os.fstat(parent);need(parent_info.st_uid==0 and not parent_info.st_mode&0o022)
            if pid:
                need(before['ControlGroup']=='/system.slice/'+UNIT)
                pidfd=os.pidfd_open(pid,0);fds.append(pidfd)
                group=os.open(UNIT,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=parent);fds.append(group)
                original=os.fstat(group);need(original.st_uid==0 and not original.st_mode&0o022)
                events=os.open('cgroup.events',os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=group);fds.append(events)
            if before['LoadState']=='loaded':self.run('stop','--',UNIT)
            if pid:
                with selectors.DefaultSelector()as selector:selector.register(pidfd,selectors.EVENT_READ);need(bool(selector.select(5)))
            after=self.show();self.known(after);need(after['MainPID']=='0'and after['ActiveState']in('inactive','failed'))
            try:
                current=os.stat(UNIT,dir_fd=parent,follow_symlinks=False)
                need(group is not None and(current.st_dev,current.st_ino)==(original.st_dev,original.st_ino))
                population=dict(line.split()for line in os.pread(events,4097,0).decode().splitlines());need(population.get('populated')=='0')
            except FileNotFoundError:pass
            return{'originalProcessExited':bool(pid),'localWorkersJoined':True,'unitState':after['ActiveState']}
        finally:
            for fd in reversed(fds):os.close(fd)
            os.close(parent)
    def start(self):
        _diagnostic('start')
        self.known(self.show());self.run('enable','--',UNIT);self.run('start','--',UNIT)
        value=self.show();self.known(value);need(value['LoadState']=='loaded'and value['ActiveState']=='active'and value['SubState']=='running'and int(value['MainPID'])>1)
        return{'mainPid':int(value['MainPID']),'active':True,'runtimeVersion':None,'confirmation':'systemd_process_only_not_runtime_version'}
    def disable(self):
        value=self.show();self.known(value)
        if value['LoadState']=='loaded':self.run('disable','--',UNIT)
    def reload(self):self.run('daemon-reload')


def provision_collector(paths,uid=0):
    _diagnostic('collector_prerequisite')
    # Explicit local administrative infrastructure, NOT admission authority.
    # No wire chooses command/path/UID/PID/env. Root helper independently owns
    # its exact units and validates actual prerequisites before returning facts.
    before=read(paths.collector_provision,131072,uid)
    result=subprocess.run(['/usr/bin/python3','-I','-B',str(paths.collector_provision),'--provision'],
        stdin=subprocess.DEVNULL,capture_output=True,timeout=65,env={'LANG':'C.UTF-8'})
    need(result.returncode==0 and len(result.stdout)<=4096 and result.stderr==b''
         and result.stdout.endswith(b'\n')and result.stdout.count(b'\n')==1
         and read(paths.collector_provision,131072,uid)==before)
    value=runtime_policy.strict(result.stdout)
    need(type(value)is dict and set(value)=={'format','profile','uid','gid','managerInvocationId',
         'managerPid','managerStartTicks','mountId','environmentSha256','controllers','namespaceProbeExit'}
         and value['format']=='cc-agent-collector-provision-result/v1'
         and value['profile'] in ('ubuntu24-systemd255-v1','ubuntu24-systemd255-disk-v1')
         and runtime_policy.canonical(value)+b'\n'==result.stdout)
    import re
    need(type(value['managerInvocationId'])is str and re.fullmatch('[0-9a-f]{32}',value['managerInvocationId'])
         and value['managerInvocationId']!='0'*32
         and type(value['environmentSha256'])is str and re.fullmatch('[0-9a-f]{64}',value['environmentSha256'])
         and value['controllers']==['cpu','memory','pids']
         and type(value['namespaceProbeExit'])is int and value['namespaceProbeExit']==0)
    for key in('managerPid','managerStartTicks','mountId'):
        need(type(value[key])is int and 0<value[key]<2**63)
    need(value['managerPid']>1)
    return runtime_policy.collector_profile({'format':('cc-agent-installed-disk-collector-profile/v1' if value['profile']=='ubuntu24-systemd255-disk-v1' else 'cc-agent-installed-collector-profile/v1'),
        'uid':value['uid'],'gid':value['gid']})


def selected(paths,uid=0):
    read(paths.prefix/stage.MARKER,1024,uid)
    code=paths.prefix/'opt/consolecrypt-agent';pointer=code/'current.json'
    if not pointer.exists():return None
    value=stage._json(read(pointer,4096,uid));need(type(value)is dict and set(value)=={'version','artifact_sha256','previous_version'}and stage.VERSION.fullmatch(value['version']))
    release=code/'releases'/value['version'];record=stage._json(read(release/'installed.json',stage.MAX_METADATA,uid))
    need(record['version']==value['version']and record['artifact_sha256']==value['artifact_sha256']and len(record['files'])<=stage.MAX_FILES+1)
    total=0
    for name,digest in record['files'].items():
        stage._relative(name)
        total+=(release/name).stat().st_size;need(total<=stage.MAX_EXPANDED+stage.MAX_METADATA)
        need(stage.digest(read(release/name,stage.MAX_EXPANDED,uid))==digest);stage._check_mode(release/name,record['file_modes'][name])
    return value

def _transaction(paths,operation,*,start=False,collector=False,supervisor=None,uid=0,_holders):
    need(operation in('install','update','rollback','remove','status')and type(start)is bool and type(collector)is bool)
    need(not collector or(start and operation in('install','update')))
    # Tests pass a private temp layout and uid=None, but still use the REAL
    # ReleaseVerifier/Rust Ed25519 policy. Production main has no such inputs.
    _diagnostic('stage')
    previous=selected(paths,uid)if(paths.prefix/stage.MARKER).exists()else None
    if operation=='status':return{'phase':'observed','selectedRelease':previous,'actualRuntimeVersion':None,'statePreserved':True,'issuesAuthority':False}
    if operation in('install','update'):
        need((operation=='install'and previous is None)or(operation=='update'and previous is not None))
        verified=verifier(paths,uid) # Mandatory BEFORE process stop and stage writes.
        _holders.append(verified)
        need(type(verified)is ReleaseVerifier and verified.verified_release is not None)
    else:need(previous is not None)
    _diagnostic('unit_identity')
    policy=runtime_policy.Review(paths,uid) # Config+old receipt/inode/bytes captured BEFORE stop.
    unit_preflight(paths,uid)
    supervisor=Systemd(paths,policy.configuration)if supervisor is None else supervisor
    policy.check()
    joined=supervisor.stop_join()
    _diagnostic('runtime_policy')
    policy.check() # Changed config/foreign old policy never enters destination writes.
    if operation in('install','update'):
        _diagnostic('signature_verify')
        captured=CapturedRelease(verified,paths,uid)
        if captured(verified._artifact)is not True:
            raise stage.StageError('original verified candidate changed before destination writes')
    if operation=='remove':
        _diagnostic('remove')
        supervisor.disable();result=stage.uninstall(paths.prefix)
        return{'phase':'removed'if not result['preserved_modified']else'partiallyRemoved','software':result,'localJoin':joined,'configPreserved':True,'identityHistoryAndReplayPreserved':True,'actualRuntimeVersion':None}
    _diagnostic('stage')
    if not(paths.prefix/stage.MARKER).exists():stage.initialize_root(paths.prefix)
    # Public software selection backup only; NEVER copies identity/DB/vault keys.
    if previous is not None:stage._write(paths.prefix/'install-backup.json',json.dumps({'version':1,'selection':previous},sort_keys=True).encode(),0o600)
    if operation=='rollback':
        _diagnostic('rollback')
        wanted=stage.rollback(paths.prefix,public_install=True)
    else:wanted=stage.stage_release(paths.prefix,verified._artifact,captured,public_install=True)
    profile=policy.profile
    if collector:profile=provision_collector(paths,uid) # Explicit mode only; verified bytes already selected.
    _diagnostic('runtime_policy')
    policy.check()
    publish_unit(paths,uid) # Under main's lock, AFTER signature/preflight+join.
    _diagnostic('runtime_policy')
    if operation in('install','update')or start:policy.apply(profile)
    supervisor.reload() # Also stage-only: selected unit must actually be loaded.
    # A stage failure is allowed to have advanced the replay floor. It escapes
    # as UNKNOWN, stays stopped, and is never blindly interpreted as rollback.
    observed=None;rollback_observed=None
    if start:
        try:observed=supervisor.start()
        except Exception:
            if operation!='update':raise
            supervisor.stop_join() # No rollback file replacement while new owners live.
            _diagnostic('rollback')
            restored=stage.rollback(paths.prefix,public_install=True)
            need(restored['version']==previous['version']and restored['artifact_sha256']==previous['artifact_sha256'])
            rollback_observed=supervisor.start()
            return{'phase':'rolledBackAfterStartFailure','selectedRelease':restored,'requestedRelease':wanted,'process':rollback_observed,'actualRuntimeVersion':None,'configAndIdentityPreserved':True,'localJoin':joined}
    return{'phase':'selected','selectedRelease':wanted,'process':observed,'actualRuntimeVersion':None,'configAndIdentityPreserved':True,'localJoin':joined,'issuesAuthority':False}

def transaction(paths,operation,*,start=False,collector=False,supervisor=None,uid=0):
    holders=[]
    try:return _transaction(paths,operation,start=start,collector=collector,supervisor=supervisor,uid=uid,_holders=holders)
    finally:
        for actual in holders:actual._snapshot.cleanup()

def main():
    _diagnostic('bootstrap')
    need(sys.platform=='linux'and os.getuid()==os.geteuid()==0 and len(sys.argv)==3 and sys.argv[2]in('stage-only','start','start-collector'))
    paths=Paths();operation=sys.argv[1]
    # One fixed root-owned lock, outside the staging prefix so initialization
    # still insists on an empty prefix. Not a permission/authority registry.
    trusted_parent(paths.lock.parent) # BEFORE create/open; no symlink/root-write race.
    import fcntl
    fd=os.open(paths.lock,os.O_RDWR|os.O_CREAT|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC,0o600)
    try:
        info=os.fstat(fd);need(info.st_uid==0 and stat.S_ISREG(info.st_mode)and info.st_nlink==1 and stat.S_IMODE(info.st_mode)==0o600)
        fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)
        result=transaction(paths,operation,start=sys.argv[2]in('start','start-collector'),collector=sys.argv[2]=='start-collector')
        need((paths.lock.lstat().st_dev,paths.lock.lstat().st_ino)==(info.st_dev,info.st_ino))
        print(json.dumps(result,sort_keys=True))
    finally:os.close(fd)
if __name__=='__main__':
    try:main()
    except Exception as error:
        print(json.dumps({'phase':'unknown','actualRuntimeVersion':None,'automaticRetryAllowed':False,'reason':'release_trust_or_transaction_unavailable','diagnostic':_closed_failure(error)}));sys.exit(65)
