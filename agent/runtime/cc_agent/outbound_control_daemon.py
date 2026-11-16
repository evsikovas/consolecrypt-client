"""Explicitly provisioned finite outbound SSH carrier polling; no enrollment.

Only fixed installed files are inputs. No URL/path/clock/token CLI flags, proxy,
redirect, DNS fallback, registration renewal or retry of uncertain HTTP exists.
"""
import base64
import grp
import hashlib
import http.client
import ipaddress
import json
import os
import pwd
from pathlib import Path
import re
import socket
import ssl
import stat
import threading
import time
import uuid
from urllib.parse import urlsplit

from .outbound_control import bridge_once
from .wss_carrier import RelayEndpoint, RelayOffer

_TRANSPORT = Path('/etc/consolecrypt-agent/tunnel-transport.json')
_CA = Path('/etc/consolecrypt-agent/tunnel-relay-ca.pem')
_BOOTSTRAP = Path('/var/lib/consolecrypt-agent/tunnel/control/bootstrap.json')
_NODE_CONFIG = Path('/etc/consolecrypt-agent/enrollment-control.json')
_NODE_PATHS = ('/var/lib/consolecrypt-agent/identity/node', '/var/lib/consolecrypt-agent/identity/sqlite',
               '/var/lib/consolecrypt-agent/checkpoints/enrollment')
_CAP = 16384
_REASONS = frozenset({'stopped','expired','clock_changed','metadata_changed','http_uncertain',
    'http_denied','offer_invalid','budget_exhausted','cleanup_unknown'})


class ControlUnavailable(OSError):
    def __init__(self, reason='metadata_changed'):
        self.reason = reason if reason in _REASONS else 'metadata_changed'
        super().__init__(self.reason)


def _need(value, reason='metadata_changed'):
    if not value:
        raise ControlUnavailable(reason)


def _json(raw, canonical=False):
    def pairs(items):
        result = {}
        for key, value in items:
            _need(key not in result)
            result[key] = value
        return result
    def number(_):
        raise ControlUnavailable()
    value = json.loads(raw, object_pairs_hook=pairs, parse_float=number, parse_constant=number)
    if canonical:
        _need(raw == (json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=True)+'\n').encode('ascii'))
    return value


def _uuid(value):
    _need(type(value) is str and str(uuid.UUID(value)) == value and uuid.UUID(value).int != 0)


def _hash(value):
    _need(type(value) is str and re.fullmatch('[0-9a-f]{64}', value))


def _credential(value):
    _need(type(value) is str and re.fullmatch('ccrc_[A-Za-z0-9_-]{43}', value))
    decoded = base64.urlsafe_b64decode(value[5:]+'=')
    _need(len(decoded)==32 and base64.urlsafe_b64encode(decoded).decode('ascii').rstrip('=')==value[5:])


def _identity(meta):
    return (meta.st_dev,meta.st_ino,meta.st_mode,meta.st_uid,meta.st_gid,meta.st_nlink,
            meta.st_size,meta.st_mtime_ns,meta.st_ctime_ns)


def _same_service(user,group,users,groups,uid,gid,supplementary):
    _need(uid>0 and gid>0 and user.pw_name==group.gr_name=='consolecrypt-agent'
        and user.pw_uid==uid and user.pw_gid==group.gr_gid==gid
        and user.pw_dir=='/var/lib/consolecrypt-agent'
        and user.pw_shell in {'/usr/sbin/nologin','/sbin/nologin'})
    _need(len(users)<=4096 and len(groups)<=4096
        and [u.pw_name for u in users if u.pw_uid==uid]==['consolecrypt-agent']
        and [g.gr_name for g in groups if g.gr_gid==gid]==['consolecrypt-agent']
        and set(supplementary)<={gid})


class _Held:
    def __init__(self, path, uid, private=False):
        self.path,self.fd=path,None
        self.parents=[]
        for p in path.parents:
            m=p.lstat()
            _need(stat.S_ISDIR(m.st_mode) and not stat.S_ISLNK(m.st_mode)
                and m.st_uid in ({0,uid} if private else {0}) and not m.st_mode & 0o022)
            self.parents.append((p,(m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid)))
        if private:
            m=path.parent.lstat();_need(m.st_uid==uid and stat.S_IMODE(m.st_mode)==0o700)
        before=path.lstat()
        _need(stat.S_ISREG(before.st_mode) and not stat.S_ISLNK(before.st_mode)
            and before.st_uid==uid and before.st_nlink==1 and 0<before.st_size<=_CAP
            and (stat.S_IMODE(before.st_mode)==0o600 if private else not before.st_mode & 0o022))
        fd=os.open(path,os.O_RDONLY|os.O_CLOEXEC|os.O_NOFOLLOW|os.O_NONBLOCK)
        try:
            _need(_identity(os.fstat(fd))==_identity(before))
            raw=os.pread(fd,_CAP+1,0)
            _need(len(raw)==before.st_size and _identity(os.fstat(fd))==_identity(before)
                and _identity(path.lstat())==_identity(before))
            self.raw,self.captured,self.fd=raw,_identity(before),fd
        except BaseException:
            os.close(fd);raise

    def check(self):
        for p,captured in self.parents:
            m=p.lstat();_need((m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid)==captured)
        _need(self.fd is not None and _identity(os.fstat(self.fd))==self.captured
            and _identity(self.path.lstat())==self.captured
            and os.pread(self.fd,_CAP+1,0)==self.raw
            and _identity(os.fstat(self.fd))==self.captured and _identity(self.path.lstat())==self.captured)

    def close(self):
        if self.fd is not None:
            os.close(self.fd);self.fd=None


class _Directory:
    def __init__(self,path,uid,gid):
        self.path,self.fd=path,None
        self.parents=[]
        for parent in (path,*path.parents):
            m=parent.lstat();_need(stat.S_ISDIR(m.st_mode) and not stat.S_ISLNK(m.st_mode)
                and m.st_uid in {0,uid} and not m.st_mode&0o022)
            self.parents.append((parent,(m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid)))
        before=path.lstat();_need(before.st_uid==uid and before.st_gid==gid and stat.S_IMODE(before.st_mode)==0o700)
        self.fd=os.open(path,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
        self.captured=(before.st_dev,before.st_ino,before.st_mode,before.st_uid,before.st_gid)
        try:self.check()
        except BaseException:self.close();raise
    def check(self):
        for path,captured in self.parents:
            m=path.lstat();_need((m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid)==captured)
        for m in (os.fstat(self.fd),self.path.lstat()):
            _need((m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid)==self.captured)
    def close(self):
        if self.fd is not None:os.close(self.fd);self.fd=None


class _Bootstrap:
    def __repr__(self): return '_Bootstrap([redacted])'

    @classmethod
    def capture(cls):
        uid=os.geteuid();_need(uid>0 and uid==os.getuid())
        _need(os.getgid()==os.getegid())
        _same_service(pwd.getpwnam('consolecrypt-agent'),grp.getgrnam('consolecrypt-agent'),
            pwd.getpwall(),grp.getgrall(),uid,os.getegid(),os.getgroups())
        result=cls();result.files=[]
        try:
            for path,owner,private in ((_TRANSPORT,0,False),(_CA,0,False),(_BOOTSTRAP,uid,True),(_NODE_CONFIG,0,False)):
                result.files.append(_Held(path,owner,private))
            transport,ca,bootstrap,node_config=result.files
            config=_json(transport.raw)
            _need(type(config) is dict and set(config)=={'format','serverName','relayOrigin','relayDialIp'}
                and config['format']=='cc-agent-installed-tunnel-transport/v1'
                and type(config['serverName']) is str and re.fullmatch('[a-z0-9.-]{1,253}',config['serverName']))
            # Maintained parser fixes the same trusted origin/IP profile as WSS.
            RelayEndpoint(config['relayOrigin'],'de787b9c-429b-471f-adb9-c2cb11339f75',config['relayDialIp']).uri()
            value=_json(bootstrap.raw,canonical=True)
            _need(type(value) is dict and set(value)=={'format','controlId','agentToken','expiresAt','node',
                'certificateDigest','ownerPin','actorRevision','transportConfigSha256','relayCaSha256'}
                and value['format']=='cc-agent-tunnel-control-bootstrap/v1')
            _uuid(value['controlId']);_credential(value['agentToken']);_hash(value['certificateDigest'])
            _need(value['transportConfigSha256']==hashlib.sha256(transport.raw).hexdigest()
                and value['relayCaSha256']==hashlib.sha256(ca.raw).hexdigest())
            node=value['node'];pin=value['ownerPin']
            _need(type(node) is dict and set(node)=={'format','agentId','keyEpoch','publicKey','keyFingerprint'}
                and type(node['format']) is int and type(node['keyEpoch']) is int
                and (node['format'],node['keyEpoch']) in {(1,1),(3,2)})
            _uuid(node['agentId']);_hash(node['publicKey']);_hash(node['keyFingerprint'])
            state=_json(node_config.raw)
            _need(type(state) is dict and set(state)=={'format','agentId','nodeDir','dbDir','anchorDir','ownerPin'}
                and state['format']=='cc-agent-enroll-config/v1' and state['agentId']==node['agentId']
                and tuple(state[n] for n in ('nodeDir','dbDir','anchorDir'))==_NODE_PATHS
                and state['ownerPin']=='/etc/consolecrypt-agent/enrollment-genesis-owner.json')
            for path in _NODE_PATHS:
                result.files.append(_Directory(Path(path),uid,os.getegid()))
            _need(type(node['keyEpoch']) is int and node['keyEpoch']>=1)
            _need(type(pin) is dict and set(pin)=={'context','key','epoch','checkpoint','expiresAt'})
            _hash(pin['key']);context=pin['context']
            _need(type(context) is dict and set(context)=={'tenant','deploymentId'})
            _uuid(context['deploymentId']);tenant=context['tenant']
            _need(type(tenant) is dict and set(tenant)=={'kind','id'} and tenant['kind'] in {'localProfile','personalAccount'})
            _uuid(tenant['id'])
            _need(type(pin['epoch']) is int and pin['epoch']>=1 and type(pin['checkpoint']) is int and pin['checkpoint']>=0
                and type(pin['expiresAt']) is int and type(value['actorRevision']) is int and value['actorRevision']>=0)
            wall,mono=time.time(),time.monotonic()
            _need(type(value['expiresAt']) is int and wall<value['expiresAt']<=min(pin['expiresAt'],wall+86400),'expired')
            ssl_context=ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            ssl_context.minimum_version=ssl.TLSVersion.TLSv1_2
            ssl_context.load_verify_locations(cadata=ca.raw.decode('ascii'))
            _need(ssl_context.verify_mode==ssl.CERT_REQUIRED and ssl_context.check_hostname
                and 1<=len(ssl_context.get_ca_certs())<=512)
            result.config,result.value,result.context=config,value,ssl_context
            result.wall,result.mono,result.end=wall,mono,mono+(value['expiresAt']-wall)
            result.check()
            return result
        except BaseException:
            result.close();raise

    def check(self):
        for held in self.files: held.check()

    def close(self):
        failed=False
        for held in self.files:
            try:held.close()
            except BaseException:failed=True
        _need(not failed,'cleanup_unknown')


class _Lifetime:
    def __init__(self, data, stop):
        self.data,self.stop=data,stop
        self.lock=threading.Lock();self.done=threading.Event();self.reason=None
        self.http_deadline=None;self.http_socket=None;self.last_wall=data.wall;self.last_mono=data.mono
        self.thread=threading.Thread(target=self._watch,name='cc-outbound-control-scope',daemon=False)

    def check(self):
        self.data.check()
        with self.lock:
            # Capture both clocks under the same lock as the retained pair.
            # Two concurrent checks cannot falsely report time going backwards.
            wall,mono=time.time(),time.monotonic()
            _need(wall>=self.last_wall and mono>=self.last_mono,'clock_changed')
            self.last_wall,self.last_mono=wall,mono
            _need(wall<self.data.value['expiresAt'] and mono<self.data.end,'expired')
            if self.reason: raise ControlUnavailable(self.reason)
            if self.http_deadline is not None: _need(mono<self.http_deadline,'http_uncertain')
        _need(not self.stop.is_set(),'stopped')

    def abort_http(self):
        with self.lock: owned=self.http_socket
        if owned is not None:
            try:owned.shutdown(socket.SHUT_RDWR)
            except OSError:pass
            try:owned.close()
            except OSError:pass

    def _watch(self):
        while not self.done.wait(.05):
            try:self.check()
            except BaseException as error:
                with self.lock:
                    if self.reason is None:self.reason=error.reason if type(error) is ControlUnavailable else 'metadata_changed'
                self.stop.set();self.abort_http();return

    def start(self): self.thread.start()

    def begin_http(self):
        self.check()
        with self.lock:self.http_deadline=min(time.monotonic()+5,self.data.end)

    def own_socket(self, owned):
        with self.lock:self.http_socket=owned

    def remaining(self):
        self.check()
        with self.lock: return max(.001,self.http_deadline-time.monotonic())

    def end_http(self):
        self.abort_http()
        with self.lock:self.http_socket=None;self.http_deadline=None

    def close(self):
        self.done.set();self.abort_http()
        if self.thread.ident is not None:self.thread.join(1)
        _need(not self.thread.is_alive(),'cleanup_unknown')


class _CappedFile:
    def __init__(self, actual): self.actual,self.left=actual,16384
    def _accept(self, raw):
        self.left-=len(raw);_need(self.left>=0,'http_uncertain');return raw
    def readline(self, maximum=-1):
        return self._accept(self.actual.readline(min(self.left+1,maximum) if maximum>=0 else self.left+1))
    def read(self, maximum=-1):
        return self._accept(self.actual.read(min(self.left+1,maximum) if maximum>=0 else self.left+1))
    def close(self): self.actual.close()
    def __getattr__(self,name): return getattr(self.actual,name)


class _Socket:
    def __init__(self, actual, lifetime): self.actual,self.lifetime=actual,lifetime
    def sendall(self, raw):
        self.actual.settimeout(self.lifetime.remaining());self.actual.sendall(raw);self.lifetime.check()
    def makefile(self,*args,**kwargs): return _CappedFile(self.actual.makefile(*args,**kwargs))
    def close(self): self.actual.close()


class _Connection(http.client.HTTPSConnection):
    def __init__(self,data,lifetime):
        self.data,self.lifetime=data,lifetime
        parsed=urlsplit(data.config['relayOrigin'])
        super().__init__(parsed.hostname,parsed.port or 443,timeout=5,context=data.context)
        self.set_debuglevel(0)

    def connect(self):
        ip=ipaddress.ip_address(self.data.config['relayDialIp'])
        raw=socket.socket(socket.AF_INET if ip.version==4 else socket.AF_INET6,socket.SOCK_STREAM)
        self.lifetime.own_socket(raw)
        try:
            raw.settimeout(self.lifetime.remaining());raw.connect((str(ip),self.port))
            # No handshake under a lock; watcher may abort our exact socket.
            tls=self.data.context.wrap_socket(raw,server_hostname=self.host,do_handshake_on_connect=False)
            self.lifetime.own_socket(tls)
            tls.settimeout(self.lifetime.remaining());tls.do_handshake()
            self.lifetime.check();self.sock=_Socket(tls,self.lifetime)
        except BaseException:
            raw.close();raise


def _next(data,lifetime):
    started=time.monotonic();lifetime.begin_http();connection=None;response=None
    try:
        connection=_Connection(data,lifetime);connection.connect()
        parsed=urlsplit(data.config['relayOrigin']);authority=parsed.netloc
        connection.request('POST','/v1/relay/control/'+data.value['controlId']+'/next',body=b'{}',headers={
            'Host':authority,'Authorization':'Bearer '+data.value['agentToken'],'Content-Type':'application/json',
            'Accept':'application/json','Connection':'close','x-cc-protocol-version':'1.13'})
        response=connection.getresponse();lifetime.check()
        _need(len(response.getheaders())<=32,'http_uncertain')
        if response.status==204:
            _need(response.read(1)==b'','http_uncertain');return None,started
        _need(response.status==200,'http_denied') # Never read redirect/error body or Location.
        _need(response.getheader('Content-Encoding') in (None,'identity'),'http_uncertain')
        body=response.read(1025);lifetime.check();_need(0<len(body)<=1024,'http_uncertain')
        offer=_json(body)
        _need(type(offer) is dict and set(offer)=={'slotId','capability','expiresInSeconds','idleSeconds','maxCiphertextBytes'},'offer_invalid')
        for name,low,high in (('expiresInSeconds',1,300),('idleSeconds',1,30),('maxCiphertextBytes',1,8*1024**2)):
            _need(type(offer[name]) is int and low<=offer[name]<=high,'offer_invalid')
        _uuid(offer['slotId'])
        return offer,started
    except ControlUnavailable:raise
    except Exception:raise ControlUnavailable('http_uncertain') from None
    finally:
        if response is not None:response.close()
        if connection is not None:connection.close()
        lifetime.end_http()


def run_daemon(stop):
    _need(type(stop) is threading.Event)
    data=lifetime=None;reason='stopped';offers=allocated=bridges=0;joined=True;bridge_cleanup=True
    try:
        data=_Bootstrap.capture();lifetime=_Lifetime(data,stop);lifetime.start()
        while True:
            lifetime.check()
            raw,started=_next(data,lifetime)
            if raw is not None:
                _need(offers<64 and allocated+raw['maxCiphertextBytes']<=512*1024**2,'budget_exhausted')
                # Anchor duration BEFORE HTTP, not after response: RTT never
                # silently extends the server's original slot remaining time.
                seconds=min(raw['expiresInSeconds'],int(data.end-started))
                _need(seconds>0 and time.monotonic()<started+seconds,'expired')
                offer=RelayOffer(RelayEndpoint(data.config['relayOrigin'],raw['slotId'],data.config['relayDialIp']),
                    raw['capability'],received_at=started,expires_in_seconds=seconds,
                    idle_seconds=raw['idleSeconds'],max_ciphertext_bytes=raw['maxCiphertextBytes'])
                raw=None;offers+=1;allocated+=offer.max_bytes
                bridge_cleanup=False # Until an actual returned closure observation exists.
                observed=bridge_once(offer,data.context,stop)
                if (observed.end=='cleanup_unknown' or observed.ssh_closed is False
                        or observed.carrier_closed is False):
                    bridge_cleanup=False
                    raise ControlUnavailable('cleanup_unknown')
                bridge_cleanup=True
                if observed.ssh_closed is True and observed.carrier_closed is True:
                    bridges+=1 # Actual endpoint retirement, not merely a return.
                # Clean transport retirement is not an admission/drain/retry
                # permit. Only a separately dispatched fresh offer may follow.
            lifetime.check()
            if stop.wait(2):lifetime.check() # AFTER completed response/bridge; never before-start cadence.
    except ControlUnavailable as error:reason=error.reason
    except Exception:reason='metadata_changed'
    finally:
        if lifetime is not None:
            try:lifetime.close()
            except BaseException:joined=False;reason='cleanup_unknown'
        # Never close/reuse FDs while an unjoined watcher may still read them.
        if data is not None and joined:
            try:data.close()
            except BaseException:joined=False;reason='cleanup_unknown'
    return dict(event='outboundControlEnded',reason=reason,offersConsumed=offers,bridgesRetired=bridges,
        allocatedCiphertextBytes=allocated,cleanupObserved=joined and bridge_cleanup and reason!='cleanup_unknown',actorAdmission=False,resumeAuthority=False,
        remoteDrainObserved=False)
