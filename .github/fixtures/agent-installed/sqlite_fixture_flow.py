"""Disposable fixture primitives; never a production authority or secret boundary."""
import http.client
import json
import os
from pathlib import Path
import stat
import subprocess
import time
import errno
import select

NODE_UID=10001


def phase_event(phase,action):
    """Bounded public timing edges, never proof/key/argv diagnostics."""
    if phase not in {'begin','complete','readiness_begin','readiness_complete','sign_begin','sign_complete','http_begin','http_complete'}:
        raise ValueError('unknown fixture phase')
    if action not in {'install','heartbeat','restart-check','assert-stopped','uninstall','sign-request','http200','http403'}:
        raise ValueError('unknown fixture action')
    value={'phase':phase,'action':action,'monotonic_seconds':time.monotonic(),
           'boot_uptime_seconds':float(Path('/proc/uptime').read_text().split()[0]),
           'clock_ticks_per_second':os.sysconf('SC_CLK_TCK')}
    print('CC_AGENT_PHASE_JSON='+json.dumps(value),flush=True)
    return value


class OwnedPidObserver:
    """Observe one already-owned PID without creating subprocesses per poll."""
    def __init__(self,pid):
        if type(pid) is not int or not 1<pid<2**31:raise ValueError('owned PID required')
        self.pid=pid;self.fd=None;self.poller=None;self.closed=False
        self.starttime,self.state=self._identity()
        if hasattr(os,'pidfd_open'):
            try:self.fd=os.pidfd_open(pid,0)
            except OSError as error:
                if error.errno not in {errno.ENOSYS,errno.EINVAL}:raise
        try:
            if self._identity()[0]!=self.starttime:raise ProcessLookupError('owned PID identity changed during observation setup')
            if self.fd is not None:
                self.poller=select.poll();self.poller.register(self.fd,select.POLLIN|select.POLLHUP|select.POLLERR)
        except BaseException:
            self.close();raise

    def _identity(self):
        text=Path('/proc',str(self.pid),'stat').read_text()
        # comm may contain spaces or parentheses; starttime is field22 after
        # the final ') ' delimiter, not whitespace token22 of the whole line.
        prefix,separator,rest=text.rpartition(') ')
        if not separator or not prefix.startswith(str(self.pid)+' ('):raise ProcessLookupError('owned PID stat shape differs')
        fields=rest.split()
        if len(fields)<20:raise ProcessLookupError('owned PID stat incomplete')
        return fields[19],fields[0]

    def alive(self):
        if self.closed:return False
        if self.poller is not None and self.poller.poll(0):return False
        try:start,state=self._identity()
        except (FileNotFoundError,ProcessLookupError):return False
        return start==self.starttime and state not in {'Z','X','x'}

    def close(self):
        self.closed=True
        if self.fd is not None:os.close(self.fd);self.fd=None

    def __enter__(self):return self
    def __exit__(self,*args):self.close()


def node_credentials():
    # Native cap-drop fixture exercises same-UID kernel calls. Only the later
    # RAM guest can exercise root -> node UID; it is not a host account change.
    if os.geteuid() not in {0,NODE_UID}:
        raise PermissionError('fixture credential context differs')
    if os.geteuid()==0:
        os.setgroups([])
    os.setgid(NODE_UID)
    os.setuid(NODE_UID)
    if os.geteuid()!=NODE_UID or os.getegid()!=NODE_UID:
        raise PermissionError('node credential setup failed')


def owner_via_fd(owner,expected_sha,args,value=None):
    import hashlib
    owner=Path(owner)
    for parent in [owner,*owner.parents]:
        if stat.S_ISLNK(parent.lstat().st_mode):raise ValueError('linked test owner tool refused')
    fd=os.open(owner,os.O_RDONLY|os.O_NOFOLLOW)
    try:
        info=os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_mode&0o6000:raise ValueError('invalid test owner mode')
        with os.fdopen(os.dup(fd),'rb') as stream:
            if hashlib.file_digest(stream,'sha256').hexdigest()!=expected_sha:raise ValueError('frozen test owner changed')
        # This grants the executable only to this child. Agent and worker are
        # launched separately with close_fds and never receive this descriptor.
        result=subprocess.run([str(owner),*map(str,args)],executable='/proc/self/fd/'+str(fd),pass_fds=(fd,),
            preexec_fn=node_credentials,input=value,capture_output=True,timeout=15,check=False)
        if result.returncode:raise RuntimeError('typed external test owner command failed: '+str(result.returncode))
        return json.loads(result.stdout)
    finally:os.close(fd)


def assert_no_owner_fd(pid,owner):
    expected=Path(owner).stat()
    for entry in Path('/proc',str(pid),'fd').iterdir():
        try:info=entry.stat()
        except FileNotFoundError:continue
        if (info.st_dev,info.st_ino)==(expected.st_dev,expected.st_ino):
            raise AssertionError('test owner descriptor leaked to Agent/worker')


def owned_workers(agent_pid,binary,expected_sha,owner,*,timeout=2):
    import hashlib
    binary=Path(binary)
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        children=set()
        for task in Path('/proc',str(agent_pid),'task').iterdir():
            try:children.update((task/'children').read_text().split())
            except (FileNotFoundError,ProcessLookupError):continue
        found=[]
        for value in children:
            child=Path('/proc')/value
            try:
                command=(child/'cmdline').read_bytes().split(b'\0')
                if len(command)<2 or command[:2]!=[str(binary).encode(),b'worker-sqlite'] or (child/'exe').resolve()!=binary:
                    continue
                with (child/'exe').open('rb') as stream:
                    if hashlib.file_digest(stream,'sha256').hexdigest()!=expected_sha:raise AssertionError('owned worker mapped image differs')
                assert_no_owner_fd(int(value),owner)
                found.append(int(value))
            except (FileNotFoundError,ProcessLookupError):continue
        if len(found)==1:return found
        if len(found)>1:raise AssertionError('multiple owned SQLite workers')
        time.sleep(.05)
    raise AssertionError('one owned SQLite worker was not observed under Agent threads')


def ready_without_proof(port,*,timeout=60,alive=None):
    deadline=time.monotonic()+timeout;attempts=0
    while time.monotonic()<deadline:
        if alive is not None and not alive():raise RuntimeError('owned Agent exited before readiness')
        client=http.client.HTTPConnection('127.0.0.1',port,timeout=min(2,max(.1,deadline-time.monotonic())))
        attempts+=1
        try:
            client.request('GET','/v1/heartbeat')
            response=client.getresponse();body=json.loads(response.read(4096))
            if response.status!=403 or body!={'error':'scope_denied'}:
                raise AssertionError('proof-free readiness must deny telemetry')
            return {'proof_free_status':403,'attempts':attempts,'lease_generated_during_poll':False}
        except (OSError,http.client.HTTPException):time.sleep(.1)
        finally:client.close()
    raise RuntimeError('proof-free readiness deadline expired')
