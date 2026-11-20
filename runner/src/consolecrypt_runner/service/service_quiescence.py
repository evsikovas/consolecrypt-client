"""Explicit installed-service raw cleanup; OS observation is never a grant.

Only the fixed service_control --retire-raw path calls this producer. Current
unit/cgroup/identity/store observations replace no historical worker receipt.
No supplied PID/path/proof boolean, service restart, Store recovery or signer.
"""
import fcntl, os, selectors, stat, time
from pathlib import Path
from consolecrypt_runner.errors import require
from consolecrypt_runner.sandbox import run_bounded

CGROUP = Path('/sys/fs/cgroup')
TEMPLATE_CGROUP = '/system.slice/system-consolecrypt\\x2dservice.slice'
UNIT = Path('/etc/systemd/system/consolecrypt-service@.service')
SYSTEMCTL = Path('/usr/bin/systemctl')
PROPERTIES = ('LoadState','ActiveState','SubState','MainPID','ControlGroup',
    'Restart','KillMode','Delegate','FragmentPath','DropInPaths')

def _need(ok, reason='Supported original systemd/cgroup-v2 service required'):
    require(ok, 'service_retention_quiescence', reason)

def _root_directory(path):
    fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
    try:
        for part in path.parts[1:]:
            child=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd)
            os.close(fd);fd=child;m=os.fstat(fd)
            _need(m.st_uid==0 and not m.st_mode&0o022)
        return fd
    except BaseException:
        os.close(fd);raise

def _root_file(path,maximum):
    parent=_root_directory(path.parent)
    try:
        fd=os.open(path.name,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=parent)
        try:
            m=os.fstat(fd)
            _need(stat.S_ISREG(m.st_mode) and m.st_uid==0 and m.st_nlink==1
                and not m.st_mode&0o022 and m.st_size<=maximum)
            raw=os.read(fd,maximum+1);after=os.fstat(fd);named=os.stat(path.name,dir_fd=parent,follow_symlinks=False)
            _need(len(raw)==m.st_size and (m.st_dev,m.st_ino,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
                ==(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns)
                and (m.st_dev,m.st_ino)==(named.st_dev,named.st_ino))
            return raw
        finally:os.close(fd)
    finally:os.close(parent)

def _show(unit,selected,deadline,*,properties=PROPERTIES):
    _need(time.monotonic()<deadline)
    _need(_root_file(UNIT,32768)==_root_file(selected/'consolecrypt-service@.service',32768),
        'Original immutable installed unit differs')
    _root_file(SYSTEMCTL,4*1024*1024) # Fixed root-owned executable, no caller command.
    result=run_bounded([str(SYSTEMCTL),'--no-pager','--no-ask-password','show',
        '--property='+','.join(properties),'--',unit],timeout=min(5,max(.01,deadline-time.monotonic())),
        output_limit=8192,grace=1,environment={'PATH':'/usr/bin:/bin','LANG':'C.UTF-8',
            'LC_ALL':'C.UTF-8','SYSTEMD_COLORS':'0'})
    _need(result.exit_code==0 and not(result.truncated or result.cancelled or result.timed_out))
    values={}
    for line in result.output.splitlines():
        key,sep,value=line.partition('=')
        _need(sep and key in properties and key not in values and len(value)<=1024)
        values[key]=value
    _need(set(values)==set(properties) and values['LoadState']=='loaded'
        and values['FragmentPath']==str(UNIT) and values['DropInPaths']==''
        and values['Restart']=='no' and values['KillMode']=='control-group' and values['Delegate']=='no')
    _need(values['MainPID'].isascii() and values['MainPID'].isdigit())
    return values

def _cgroup_parent():
    # Kernel mount metadata only; neither stderr nor an empty D-Bus property is
    # evidence of a supported hierarchy. No cgroup is created or modified.
    with open('/proc/self/mountinfo','rb') as f:raw=f.read(256*1024+1)
    _need(len(raw)<=256*1024)
    mounts=[]
    for line in raw.decode('utf-8','strict').splitlines():
        left,sep,right=line.partition(' - ')
        fields=left.split();fs=right.split()
        if sep and len(fields)>=6 and fields[4]==str(CGROUP):mounts.append((fields,fs))
    _need(len(mounts)==1 and mounts[0][0][3]=='/' and mounts[0][1][0]=='cgroup2')
    return _root_directory(CGROUP/TEMPLATE_CGROUP.lstrip('/'))

def _wait_original_exit(pidfd,deadline):
    with selectors.DefaultSelector() as selector:
        selector.register(pidfd,selectors.EVENT_READ)
        _need(bool(selector.select(max(0,deadline-time.monotonic()))),'Original daemon exit unobserved')


def require_inactive(identity,selected):
    """Read-only early refusal. Not a serializable quiescence/cleanup proof.
    The actual producer repeats this under original identity/store custody.
    """
    unit='consolecrypt-service@'+identity+'.service';parent=_cgroup_parent()
    try:
        values=_show(unit,selected,time.monotonic()+5)
        _need(values['ActiveState']=='inactive' and values['SubState']=='dead'
            and values['MainPID']=='0' and values['ControlGroup']=='',
            'Automatic raw retention requires positively inactive/dead unit')
        try:os.stat(unit,dir_fd=parent,follow_symlinks=False)
        except FileNotFoundError:return
        _need(False,'Automatic raw retention requires absent original cgroup')
    finally:os.close(parent)


class _ObservedQuiescence:
    """Finite live kernel/file custody, not a serializable approval token."""
    def __init__(self,scope,identity,runtime,keydir,selected,endpoint_type,stop,*,inactive_only=False):
        self.scope=scope;self.identity=identity;self.runtime=runtime;self.keydir=keydir;self.selected=selected
        self.deadline=time.monotonic()+225;self.fds=[];self.pidfd=None;self.cgroup=None;self.events=None
        self.unit='consolecrypt-service@'+identity+'.service';self.pid=0;self.closed=False;self.files=[];self.directories=[]
        self.parent=None;self.cg_stat=None
        try:
            self.parent=_cgroup_parent();self.fds.append(self.parent);self.parent_stat=os.fstat(self.parent)
            self.identity_original=keydir.public_identity()
            values=_show(self.unit,selected,self.deadline)
            if values['ActiveState']=='inactive' and values['SubState']=='dead' and values['MainPID']=='0':
                _need(values['ControlGroup']=='')
                self._absent();self.source='systemdInactiveCgroupAbsent'
            else:
                _need(not inactive_only,'Automatic raw retention cannot attach or stop a service')
                _need(values['ActiveState']=='active' and values['SubState']=='running')
                _need(values['ControlGroup']==TEMPLATE_CGROUP+'/'+self.unit)
                endpoint=endpoint_type(identity);stream=None
                try:
                    stream=endpoint.connect()
                    import socket,struct
                    pid,uid,_=struct.unpack('3i',stream.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))
                    _need(pid>0 and uid==os.getuid() and str(pid)==values['MainPID'])
                    with open('/proc/'+str(pid)+'/cgroup','rb') as f:membership=f.read(4097)
                    _need(membership==('0::'+TEMPLATE_CGROUP+'/'+self.unit+'\n').encode())
                    _need(hasattr(os,'pidfd_open'),'Linux pidfd unavailable')
                    self.pid=pid;self.pidfd=os.pidfd_open(pid,0);self.fds.append(self.pidfd)
                    self.cgroup=os.open(self.unit,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.parent)
                    self.fds.append(self.cgroup);self.cg_stat=os.fstat(self.cgroup)
                    _need(self.cg_stat.st_uid==0 and not self.cg_stat.st_mode&0o022)
                    self.events=os.open('cgroup.events',os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.cgroup)
                    self.fds.append(self.events)
                    # Explicit --retire-raw only, AFTER genuine ended policy and
                    # TTL preflight. Ordinary results/status never call stop.
                    stop(stream)
                finally:
                    if stream is not None:stream.close()
                    endpoint.close()
                _wait_original_exit(self.pidfd,self.deadline)
                while not self._empty():
                    _need(time.monotonic()<self.deadline,'Original service cgroup remains populated')
                    time.sleep(.01)
                self.source='systemdPidfdCgroupV2'
                while True:
                    settled=_show(self.unit,selected,self.deadline)
                    if settled['ActiveState']=='inactive' and settled['SubState']=='dead' and settled['MainPID']=='0' and settled['ControlGroup']=='':break
                    _need(settled['ActiveState']=='deactivating' and time.monotonic()<self.deadline,'Original unit did not reach inactive/dead')
                    time.sleep(.01)
            # Never infer worker retirement from just lock absence. Only NOW,
            # after actual unit/kernel quiescence, retain the ORIGINAL locks so
            # no restart/new helper can race raw deletion.
            key=os.open('identity.v1',os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=keydir.fd)
            self.fds.append(key);self._private_file(key,197);fcntl.flock(key,fcntl.LOCK_EX|fcntl.LOCK_NB)
            self.files.append((keydir.fd,'identity.v1',key,os.fstat(key)))
            state=os.open('state',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=runtime.fd);self.fds.append(state)
            from .service_retention import _identity
            _identity(state);self.directories.append((runtime.fd,'state',state,os.fstat(state)))
            for name in ('.owner','.lock'):
                fd=os.open(name,os.O_RDWR|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state)
                self.fds.append(fd);self._private_file(fd,0);fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)
                self.files.append((state,name,fd,os.fstat(fd)))
            self.check()
        except BaseException:self.close();raise
    def _private_file(self,fd,size):
        m=os.fstat(fd)
        _need(stat.S_ISREG(m.st_mode) and m.st_uid==os.getuid() and stat.S_IMODE(m.st_mode)==0o600
            and m.st_nlink==1 and m.st_size==size)
    def _absent(self):
        try:os.stat(self.unit,dir_fd=self.parent,follow_symlinks=False)
        except FileNotFoundError:return True
        _need(False,'Canonical service cgroup still exists')
    def _empty(self):
        try:current=os.stat(self.unit,dir_fd=self.parent,follow_symlinks=False)
        except FileNotFoundError:return self._absent()
        _need(self.cg_stat is not None and (current.st_dev,current.st_ino)==(self.cg_stat.st_dev,self.cg_stat.st_ino))
        raw=os.pread(self.events,4097,0);_need(len(raw)<=4096)
        values={}
        for line in raw.decode('ascii','strict').splitlines():
            key,_,value=line.partition(' ');_need(key in ('populated','frozen') and key not in values and value in ('0','1'));values[key]=value
        _need(set(values)=={'populated','frozen'})
        return values['populated']=='0'
    def check(self):
        _need(not self.closed and time.monotonic()<self.deadline)
        self.runtime.check();self.keydir.check();_need(self.keydir.public_identity()==self.identity_original)
        for parent,name,fd,original in self.files+self.directories:
            current=os.fstat(fd);named=os.stat(name,dir_fd=parent,follow_symlinks=False)
            _need((current.st_dev,current.st_ino,current.st_mode,current.st_size,current.st_uid,current.st_nlink)
                ==(original.st_dev,original.st_ino,original.st_mode,original.st_size,original.st_uid,original.st_nlink)
                and (current.st_dev,current.st_ino)==(named.st_dev,named.st_ino),
                'Original identity/store lock name changed')
        current=_cgroup_parent()
        try:_need((os.fstat(current).st_dev,os.fstat(current).st_ino)==(self.parent_stat.st_dev,self.parent_stat.st_ino))
        finally:os.close(current)
        if self.source=='systemdInactiveCgroupAbsent':self._absent()
        else:
            with selectors.DefaultSelector() as selector:
                selector.register(self.pidfd,selectors.EVENT_READ);_need(bool(selector.select(0)))
            _need(self._empty())
        values=_show(self.unit,self.selected,self.deadline)
        _need(values['ActiveState']=='inactive' and values['SubState']=='dead' and values['MainPID']=='0'
            and values['ControlGroup']=='','Original unit is not currently stopped')
    def metadata(self,policy_digest):
        self.check();identity=self.identity_original
        return {'version':1,'source':self.source,'identity':self.identity,'policy_digest':policy_digest,
            'runtime_device':self.runtime.original.st_dev,'runtime_inode':self.runtime.original.st_ino,
            'identity_device':identity[0],'identity_inode':identity[1],
            'cgroup_parent_device':self.parent_stat.st_dev,'cgroup_parent_inode':self.parent_stat.st_ino,
            'cgroup_device':self.cg_stat.st_dev if self.cg_stat is not None else 0,
            'cgroup_inode':self.cg_stat.st_ino if self.cg_stat is not None else 0,
            'daemon_pid':self.pid,'observed_at':int(time.time())}
    def close(self):
        self.closed=True
        error=None
        for fd in reversed(self.fds):
            try:os.close(fd)
            except OSError as failure:error=error or failure
        self.fds=[]
        if error is not None:raise error
    def __enter__(self):return self
    def __exit__(self,*_):self.close()

def retire_fixed(scope,identity,runtime,keydir,selected,endpoint_type,stop,expected_policy,*,inactive_only=False):
    from .service_retention import quiescent_preflight,retire_quiescent
    # No stop/lock/deletion until genuine ended policy + original supported
    # signed records + actual raw TTL has passed. This is explicit maintenance.
    captured=quiescent_preflight(scope,runtime.original,expected_policy)
    with _ObservedQuiescence(scope,identity,runtime,keydir,selected,endpoint_type,stop,inactive_only=inactive_only) as proof:
        return retire_quiescent(scope,runtime.original,proof,captured)
