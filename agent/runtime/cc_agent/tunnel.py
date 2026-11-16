"""Explicit bounded Agent tunnel; stdlib TLS, real Rust Permit, no public route.

The first carrier is owned direct TLS/TCP. A maintained WSS adapter is separate;
this module does not implement WebSocket, Relay enrollment or lease issuance.
"""
import hashlib
import ipaddress
import json
import math
import os
from pathlib import Path
import re
import select
import socket
import ssl
import stat
import threading
import time
import uuid

from integration.sqlite_worker_gate import WorkerChannel
from integration.reader_files import read_reader_file
from .tunnel_audit import TunnelAuditLog

POLICY_DOMAIN = b'consolecrypt.agent.tunnel.route-policy.v1\0'
CHUNK = 16384
_CODES = frozenset({'InvalidWire','InvalidKey','InvalidSignature','InvalidTime','Expired','WrongContext',
    'WrongEpoch','WrongCheckpoint','UnsupportedTeam','Replay','NotFound','WrongState','InvalidToken',
    'WrongClaim','ScopeDenied','CapabilityDenied','MissingFreshness','Revoked','Quota','Conflict',
    'Storage','Entropy','KeyReuse','UncertainCommit'})


def _os_now(): return time.time()


class TunnelUnavailable(Exception):
    def __str__(self): return 'tunnel_unavailable'


def _strict(raw, cap=16384):
    if type(raw) is not bytes or not 0 < len(raw) <= cap:
        raise TunnelUnavailable()
    def pairs(rows):
        result = {}
        for key, value in rows:
            if key in result: raise TunnelUnavailable()
            result[key] = value
        return result
    def no_number(value): raise TunnelUnavailable()
    try:
        return json.loads(raw, object_pairs_hook=pairs, parse_float=no_number, parse_constant=no_number)
    except (ValueError, UnicodeError, RecursionError):
        raise TunnelUnavailable() from None


def _canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode()


def _hash_file(path):
    return read_reader_file(path,128*1024**2,digest=True)


def _validate_policy(value):
    def exact(obj,keys):
        if type(obj) is not dict or set(obj)!=set(keys):raise TunnelUnavailable()
    def identifier(text):
        try:
            parsed=uuid.UUID(text)
            if type(text) is not str or parsed.int==0 or str(parsed)!=text:raise TunnelUnavailable()
        except (ValueError,TypeError,AttributeError):raise TunnelUnavailable() from None
    exact(value,('format','context','agentId','agentCertificateHash','keyEpoch','routeId','clientSubject',
        'clientDeviceId','clientTlsCertificateSha256','agentTlsCertificateSha256','destination','clientBind','limits'))
    if value['format']!='cc-agent-tunnel-route-policy/v1':raise TunnelUnavailable()
    for key in ('agentId','routeId','clientDeviceId'):identifier(value[key])
    for key in ('agentCertificateHash','clientTlsCertificateSha256','agentTlsCertificateSha256'):
        if type(value[key]) is not str or not re.fullmatch('[0-9a-f]{64}',value[key]) or value[key]=='0'*64:
            raise TunnelUnavailable()
    if type(value['keyEpoch']) is not int or not 1<=value['keyEpoch']<=2**64-1:raise TunnelUnavailable()
    exact(value['context'],('tenant','deploymentId'));identifier(value['context']['deploymentId'])
    for subject in (value['context']['tenant'],value['clientSubject']):
        exact(subject,('kind','id'))
        if subject['kind'] not in ('localProfile','personalAccount'):raise TunnelUnavailable()
        identifier(subject['id'])
    exact(value['destination'],('protocol','ip','port'));exact(value['clientBind'],('ip','port'))
    if value['destination']['protocol']!='tcp':raise TunnelUnavailable()
    for destination,minimum in ((value['destination'],1),(value['clientBind'],0)):
        if (destination['ip'] not in ('127.0.0.1','::1') or type(destination['port']) is not int
                or not minimum<=destination['port']<=65535):raise TunnelUnavailable()
    maxima={'maxTtlSeconds':300,'idleTimeoutSeconds':30,'maxChannels':4,'maxFrameBytes':16384,
        'maxPendingFramesPerChannel':4,'maxPendingFramesTotal':16,'maxQueuedBytesPerChannel':65536,
        'maxQueuedBytesTotal':262144,'maxBytesPerChannel':8388608,'maxBytesTotal':33554432}
    exact(value['limits'],maxima)
    limits=value['limits']
    if any(type(limits[k]) is not int or not 1<=limits[k]<=maximum for k,maximum in maxima.items()):
        raise TunnelUnavailable()
    if (limits['idleTimeoutSeconds']>limits['maxTtlSeconds'] or limits['maxFrameBytes']>limits['maxQueuedBytesPerChannel']
            or limits['maxQueuedBytesPerChannel']>limits['maxQueuedBytesTotal']
            or limits['maxPendingFramesPerChannel']>limits['maxPendingFramesTotal']
            or limits['maxBytesPerChannel']>limits['maxBytesTotal']):raise TunnelUnavailable()


class Policy:
    """Captured public DATA. Only the Rust worker validates owner authority."""
    __slots__ = ('_raw', '_path', '_identity', '_hash', '_value')
    def __repr__(self): return 'TunnelPolicy([redacted])'
    def __init__(self, path):
        self._path = Path(path)
        before = self._path.lstat()
        if self._path.is_symlink() or not self._path.is_file() or not 0 < before.st_size <= 16384:
            raise TunnelUnavailable()
        self._raw = read_reader_file(self._path,16384)
        self._identity = (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns)
        self._value = _strict(self._raw)
        _validate_policy(self._value)
        self._hash = hashlib.sha256(POLICY_DOMAIN + _canonical(self._value)).hexdigest()
        self.check()
    def check(self):
        info = self._path.lstat()
        before=(info.st_dev,info.st_ino,info.st_size,info.st_mtime_ns,info.st_ctime_ns)
        if (self._path.is_symlink() or not self._path.is_file()
                or before != self._identity or read_reader_file(self._path,16384) != self._raw):
            raise TunnelUnavailable()
        after=self._path.lstat()
        if (after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns)!=self._identity:
            raise TunnelUnavailable()
    @property
    def digest(self): return self._hash
    def value(self): return _strict(self._raw)


class TunnelCustody:
    """Only private IPC. Never sends a private handle over the Relay."""
    def __repr__(self): return 'TunnelCustody([redacted])'
    def __init__(self, *, binary, binary_sha256, database, anchor, owner_pin, node, policy):
        if _hash_file(binary) != binary_sha256 or not re.fullmatch('[0-9a-f]{64}', binary_sha256):
            raise TunnelUnavailable()
        self._policy = policy
        self._pid=os.getpid()
        self._protected=tuple(Path(x).resolve(strict=True) for x in (database,anchor,owner_pin,node,policy._path))
        self._channel = WorkerChannel([str(binary), 'worker-tunnel-sqlite', '--db-dir', str(database),
            '--anchor-dir', str(anchor), '--owner-pin', str(owner_pin), '--node-dir', str(node),
            '--policy', str(policy._path)], timeout=1.0)
    def begin(self, binding, actual_tls_certificate_sha256):
        if os.getpid()!=self._pid:raise TunnelUnavailable()
        started = time.monotonic()
        process, frame = self._channel.exchange_bound({'action':'begin', 'binding':binding,
            'clientTlsCertificateSha256':actual_tls_certificate_sha256})
        if frame.get('ok') is False:
            if set(frame) != {'ok','code'} or frame['code'] not in _CODES:
                self._channel.close()
            # Known per-request denials must never destroy another live handle.
            raise TunnelUnavailable()
        metadata = frame.get('metadata', {})
        handle = metadata.get('handle')
        guard = None
        if (type(handle) is str and re.fullmatch('[0-9a-f]{64}', handle) and handle != '0'*64):
            guard = _Permit(self._channel, process, handle, started,
                self._policy.value()['limits']['maxTtlSeconds'])
        try:
            if frame.get('ok') is not True or guard is None or set(metadata) != {
                    'handle','agentId','route','policyHash','expiresAt','proof'}:
                raise TunnelUnavailable()
            policy = self._policy.value()
            if (metadata['agentId'] != policy['agentId'] or metadata['route'] != policy['routeId']
                    or metadata['policyHash'] != self._policy.digest
                    or type(metadata['expiresAt']) is not int or metadata['expiresAt'] <= _os_now()):
                raise TunnelUnavailable()
            self._policy.check()
            guard.expires = metadata['expiresAt']
            guard.check()
            return guard, {k:v for k,v in metadata.items() if k != 'handle'}
        except BaseException:
            if guard is not None: guard.close()
            else: self._channel.close()
            raise
    def close(self):
        if os.getpid()!=self._pid:raise TunnelUnavailable()
        self._channel.close()


class _Permit:
    def __repr__(self): return 'TunnelPermit([redacted])'
    def __init__(self, channel, process, handle, started, ttl):
        self._channel, self._process, self._handle = channel, process, handle
        self._deadline = started + ttl
        self.expires = 0
        self._pid, self._lock = os.getpid(), threading.RLock()
        self._last = None
    def _owner(self):
        if os.getpid() != self._pid:
            raise TunnelUnavailable()
    def local(self):
        self._owner()
        with self._lock:
            now = _os_now()
            if (self._handle is None or type(now) not in (int,float) or not math.isfinite(now) or now < 0 or now >= self.expires
                    or time.monotonic() >= self._deadline or self._last is not None and now < self._last):
                raise TunnelUnavailable()
            self._last = now
    def check(self):
        self._owner()
        with self._lock:
            self.local()
            process, frame = self._channel.exchange_bound({'action':'check','handle':self._handle},
                expected_process=self._process, timeout=min(1.0, max(.001, self._deadline-time.monotonic())))
            if process is not self._process or frame != {'ok':True,'metadata':{'phase':'live'}}:
                raise TunnelUnavailable()
            self.local()
    def close(self):
        self._owner()
        with self._lock:
            handle, self._handle = self._handle, None
            if handle is None: return
            process, frame = self._channel.exchange_bound({'action':'close','handle':handle},
                expected_process=self._process, timeout=.5)
            if process is not self._process or frame != {'ok':True,'metadata':{'phase':'closed'}}:
                raise TunnelUnavailable()


class RouteSession:
    """One real route admission and immutable binding shared by <=4 channels.

    No automatic fresh lease, process restart, reset, retry or re-sign. It is
    retained by the explicit Agent controller, never reconstructed from JSON.
    """
    def __repr__(self): return 'TunnelRouteSession([redacted])'
    def __init__(self, policy, custody, stop, *, audit):
        if (type(audit) is not TunnelAuditLog or type(custody) is not TunnelCustody
                or custody._policy is not policy or audit._policy is not policy):raise TunnelUnavailable()
        self.policy, self._custody, self.stop = policy, custody, stop
        self._audit=audit
        directory=audit._path.parent.resolve(strict=True)
        if any(directory==p or directory in p.parents or p in directory.parents for p in custody._protected):
            raise TunnelUnavailable()
        self.budget=RouteBudget(policy)
        self._lock=threading.RLock()
        self._attempted=False
        self._guard=self._binding=self._metadata=None
        self._pid=os.getpid()
        self._closed_audited=False
    def _owner(self):
        if os.getpid()!=self._pid:raise TunnelUnavailable()
    def __enter__(self):
        self._owner();return self
    def __exit__(self,*unused):self.close()
    def attach(self,binding,actual_tls_peer):
        self._owner()
        with self._lock:
            if self.stop.is_set() or actual_tls_peer != self.policy.value()['clientTlsCertificateSha256']:
                raise TunnelUnavailable()
            captured=_canonical(binding)
            if not self._attempted:
                self._attempted=True  # Any unknown first begin stays terminal.
                self._binding=captured
                self._guard,metadata=self._custody.begin(binding,actual_tls_peer)
                self._metadata=_canonical(metadata)
                try:audited=self._audit.record('admitted')
                except Exception:audited=False
                if audited is not True:
                    self.stop.set()
                    guard,self._guard=self._guard,None
                    guard.close()
                    raise TunnelUnavailable()
            if self._guard is None or captured != self._binding:
                raise TunnelUnavailable()
            self.check()
            return _strict(self._metadata)
    def check(self):
        self._owner()
        with self._lock:
            if self.stop.is_set() or self._guard is None:raise TunnelUnavailable()
            try:
                self.policy.check();self._guard.check()
            except Exception:
                self.stop.set()
                raise
    @property
    def deadline(self):
        self._owner()
        with self._lock:
            if self._guard is None:raise TunnelUnavailable()
            return self._guard._deadline
    def close(self):
        self._owner()
        with self._lock:
            unavailable=self.stop.is_set()
            self.stop.set()
            guard,self._guard=self._guard,None
            failed=False
            if guard is not None:
                try:guard.close()
                except Exception:failed=True
            if not self._closed_audited:
                self._closed_audited=True
                if self._audit.record('operation_unavailable' if unavailable or failed else 'closed') is not True:
                    failed=True
            if failed:raise TunnelUnavailable()


def connect_relay_once(*, numeric_address, port, server_name, context, certificate_sha256):
    """Actual TLS peer observation, no proxy/redirect/DNS/fallback/reconnect.

    Relay registration is MainRoot-owned; this first carrier is for owned TCP
    acceptance. Connecting alone supplies NO Tunnel permission.
    """
    address = ipaddress.ip_address(numeric_address)
    if str(address) != numeric_address or type(port) is not int or not 1 <= port <= 65535:
        raise TunnelUnavailable()
    if (context.verify_mode != ssl.CERT_REQUIRED or not context.check_hostname
            or context.minimum_version != ssl.TLSVersion.TLSv1_3):
        raise TunnelUnavailable()
    raw = socket.socket(socket.AF_INET6 if address.version == 6 else socket.AF_INET, socket.SOCK_STREAM)
    wrapped = None
    try:
        raw.settimeout(5)
        raw.connect((numeric_address, port))
        wrapped = context.wrap_socket(raw, server_hostname=server_name)
        if (wrapped.version() != 'TLSv1.3'
                or hashlib.sha256(wrapped.getpeercert(binary_form=True)).hexdigest() != certificate_sha256):
            raise TunnelUnavailable()
        wrapped.settimeout(.2)
        return wrapped
    except BaseException:
        (wrapped or raw).close()
        raise


def _file_identity(info):
    return (info.st_dev, info.st_ino, info.st_size, info.st_uid, info.st_mode,
            info.st_mtime_ns, info.st_ctime_ns)


class ServerTLS:
    """Actual scoped TLS credential loader, NEVER the OS Node private key."""
    def __repr__(self): return 'TunnelServerTLS([redacted])'
    def __init__(self, *, certificate, private_key, client_ca, policy, custody):
        if type(custody) is not TunnelCustody or custody._policy is not policy:raise TunnelUnavailable()
        self._supervised_scope = None
        self._load_scoped_files(certificate, private_key, client_ca, policy, custody._protected)

    @classmethod
    def _for_owned_supervised_child(cls, *, certificate, private_key, client_ca, policy, owned):
        # TLS credentials/pipe lifetime only. This does NOT construct a Permit.
        from .tunnel_supervised import OwnedPipe
        if type(owned) is not OwnedPipe or owned.policy is not policy:
            raise TunnelUnavailable()
        owned.check()
        value = cls.__new__(cls)
        value._supervised_scope = owned
        value._load_scoped_files(certificate, private_key, client_ca, policy, ())
        return value

    def _load_scoped_files(self, certificate, private_key, client_ca, policy, protected):
        self._pins = []
        self._policy = policy
        opened = []
        try:
            for name, private in [(certificate,False),(private_key,True),(client_ca,False)]:
                path = Path(name)
                if not path.is_absolute() or '..' in path.parts:
                    raise TunnelUnavailable()
                resolved=path.resolve(strict=True)
                if any(resolved==p or p in resolved.parents or resolved in p.parents for p in protected):
                    raise TunnelUnavailable()
                for part in [path,*path.parents]:
                    if part.is_symlink(): raise TunnelUnavailable()
                before = path.lstat()
                if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or not 0 < before.st_size <= 65536
                        or before.st_mode & 0o022 or before.st_uid not in {0,os.getuid()}
                        or private and (before.st_uid != os.getuid() or stat.S_IMODE(before.st_mode) != 0o600)):
                    raise TunnelUnavailable()
                fd = os.open(path, os.O_RDONLY|os.O_CLOEXEC|os.O_NOFOLLOW|os.O_NONBLOCK)
                opened.append(fd)
                if _file_identity(os.fstat(fd)) != _file_identity(before): raise TunnelUnavailable()
                self._pins.append((path,_file_identity(before)))
            public = os.read(opened[0],65537)
            if len(public) != self._pins[0][1][2] or public.count(b'-----BEGIN CERTIFICATE-----') != 1:
                raise TunnelUnavailable()
            der = ssl.PEM_cert_to_DER_cert(public.decode('ascii'))
            if hashlib.sha256(der).hexdigest() != policy.value()['agentTlsCertificateSha256']:
                raise TunnelUnavailable()
            for fd in opened: os.lseek(fd,0,os.SEEK_SET)
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.minimum_version = context.maximum_version = ssl.TLSVersion.TLSv1_3
            context.verify_mode = ssl.CERT_REQUIRED
            context.options |= ssl.OP_NO_TICKET
            context.num_tickets = 0
            # Linux proc-fd paths retain already verified REGULAR inodes even
            # if an operator changes the pathname while OpenSSL parses them.
            context.load_cert_chain('/proc/self/fd/'+str(opened[0]), '/proc/self/fd/'+str(opened[1]), password=b'')
            context.load_verify_locations(cafile='/proc/self/fd/'+str(opened[2]))
            self._context = context
            self.check()
        finally:
            for fd in opened: os.close(fd)
    def check(self):
        self._policy.check()
        if self._supervised_scope is not None:
            self._supervised_scope.check()
        for path, captured in self._pins:
            if path.is_symlink() or _file_identity(path.lstat()) != captured:
                raise TunnelUnavailable()


class InnerTLS:
    def __init__(self, carrier, context, policy, stop):
        if type(context) is not ServerTLS:
            raise TunnelUnavailable()
        context.check()
        self.carrier, self.policy, self.stop, self.context = carrier, policy, stop, context
        self.incoming, self.outgoing = ssl.MemoryBIO(), ssl.MemoryBIO()
        self.ssl = context._context.wrap_bio(self.incoming, self.outgoing, server_side=True)
        self.guard = None
        self._handshake_done = False
        self._handshake_received = 0
    def checkpoint(self):
        if self.stop.is_set(): raise TunnelUnavailable()
        if self.ssl.pending()>16384:raise TunnelUnavailable()
        self.policy.check()
        self.context.check()
        if self.guard is not None: self.guard.check()
    def flush(self, deadline):
        while self.outgoing.pending:
            if time.monotonic() >= deadline or self.outgoing.pending > 65536: raise TunnelUnavailable()
            self.checkpoint()
            raw = self.outgoing.read(min(CHUNK, self.outgoing.pending))
            sent = 0
            while sent < len(raw):
                self.checkpoint()
                if time.monotonic() >= deadline: raise TunnelUnavailable()
                try: count = self.carrier.send(memoryview(raw)[sent:])
                except socket.timeout: raise TunnelUnavailable() from None
                if count <= 0: raise TunnelUnavailable()
                sent += count
    def feed(self, deadline):
        self.flush(deadline)
        if time.monotonic() >= deadline: raise TunnelUnavailable()
        self.checkpoint()
        try: raw = self.carrier.recv(CHUNK)
        except socket.timeout: return False
        if not raw or self.incoming.pending + len(raw) > 65536: raise TunnelUnavailable()
        if not self._handshake_done:
            self._handshake_received += len(raw)
            if self._handshake_received > 65536: raise TunnelUnavailable()
        self.incoming.write(raw)
        return True
    def handshake(self):
        deadline = time.monotonic() + 5
        while True:
            try: self.ssl.do_handshake(); break
            except ssl.SSLWantReadError: self.feed(deadline)
            except ssl.SSLWantWriteError: self.flush(deadline)
        self.flush(deadline)
        if self.ssl.version() != 'TLSv1.3': raise TunnelUnavailable()
        self._handshake_done = True
        actual = hashlib.sha256(self.ssl.getpeercert(binary_form=True)).hexdigest()
        if actual != self.policy.value()['clientTlsCertificateSha256']: raise TunnelUnavailable()
        return actual
    def read_open(self):
        deadline = time.monotonic() + 5
        raw = bytearray()
        while b'\n' not in raw:
            try: block = self.ssl.read(min(CHUNK, 16385-len(raw)))
            except ssl.SSLWantReadError: self.feed(deadline); continue
            except ssl.SSLWantWriteError: self.flush(deadline); continue
            if not block: raise TunnelUnavailable()
            raw.extend(block)
            if len(raw) > 16384: raise TunnelUnavailable()
        if not raw.endswith(b'\n') or raw.count(b'\n') != 1: raise TunnelUnavailable()
        value = _strict(bytes(raw))
        if set(value) != {'format','binding'} or value['format'] != 'cc-agent-tunnel-open/v1':
            raise TunnelUnavailable()
        return value['binding']
    def write(self, raw, deadline):
        offset = 0
        view = memoryview(raw)
        while offset < len(raw):
            self.checkpoint()
            try: offset += self.ssl.write(view[offset:offset+CHUNK])
            except ssl.SSLWantReadError: self.feed(deadline)
            except ssl.SSLWantWriteError: self.flush(deadline)
            self.flush(deadline)


class RouteBudget:
    """One explicit runtime generation, never a persistent authorization."""
    def __init__(self, policy):
        self.policy, self.lock = policy, threading.Lock()
        self.started = None
        self.active = self.charged = 0
        self.queued = self.frames = 0
    def reserve(self):
        with self.lock:
            limits = self.policy.value()['limits']
            now = time.monotonic()
            if self.started is None: self.started = now
            if now >= self.started + limits['maxTtlSeconds'] or self.active >= limits['maxChannels']:
                raise TunnelUnavailable()
            self.active += 1
    def charge(self, count):
        with self.lock:
            limits = self.policy.value()['limits']
            if (self.started is None or time.monotonic() >= self.started + limits['maxTtlSeconds']
                    or count <= 0 or self.charged + count > limits['maxBytesTotal']):
                raise TunnelUnavailable()
            self.charged += count
    def frame(self, requested):
        with self.lock:
            limits = self.policy.value()['limits']
            count = min(requested,limits['maxFrameBytes'],limits['maxQueuedBytesPerChannel'],
                        limits['maxQueuedBytesTotal']-self.queued)
            if count <= 0 or self.frames >= limits['maxPendingFramesTotal']:
                raise TunnelUnavailable()
            self.queued += count
            self.frames += 1
            return _FrameReservation(self,count)
    def release(self):
        with self.lock:
            if self.active <= 0: raise TunnelUnavailable()
            self.active -= 1


class _FrameReservation:
    def __init__(self,budget,capacity):
        self.budget,self.capacity,self.closed=budget,capacity,False
    def close(self):
        if self.closed:return
        with self.budget.lock:
            self.budget.queued-=self.capacity
            self.budget.frames-=1
            self.closed=True


def serve_once(*, carrier, inner_context, session):
    """A real acquired stream/Permit is required; no bool authorizer/provider."""
    guard = target = inner = frame = raw = None
    reserved=False
    result = {'outcome':'unknown','charged_bytes':0,'target_written_bytes':0,'client_written_bytes':0,
              'cleanup_observed':False, 'production_approval':False}
    try:
        session._owner()
        policy,budget,stop=session.policy,session.budget,session.stop
        policy.check()
        budget.reserve();reserved=True
        inner = InnerTLS(carrier, inner_context, policy, stop)
        audit_directory=session._audit._path.parent.resolve(strict=True)
        if any(audit_directory==path or audit_directory in path.parents or path in audit_directory.parents
                for path,_ in inner_context._pins):raise TunnelUnavailable()
        peer = inner.handshake()
        binding = inner.read_open()
        metadata = session.attach(binding,peer)
        inner.guard = session
        inner.checkpoint()
        description = policy.value()
        dest = description['destination']
        if dest['protocol'] != 'tcp' or dest['ip'] not in ('127.0.0.1','::1'):
            raise TunnelUnavailable()
        target = socket.socket(socket.AF_INET6 if dest['ip'] == '::1' else socket.AF_INET, socket.SOCK_STREAM)
        target.settimeout(1)
        target.connect((dest['ip'], dest['port']))
        if target.getpeername()[:2] != (dest['ip'], dest['port']): raise TunnelUnavailable()
        inner.checkpoint()
        ack = {'format':'cc-agent-tunnel-open-ack/v1', **metadata}
        encoded_ack=_canonical(ack)+b'\n'
        if len(encoded_ack)>4096:raise TunnelUnavailable()
        inner.write(encoded_ack, time.monotonic()+2)
        target.setblocking(False)
        limits = description['limits']
        activity = time.monotonic()
        while True:
            inner.checkpoint()
            if time.monotonic()-activity >= limits['idleTimeoutSeconds']: raise TunnelUnavailable()
            deadline = min(session.deadline, time.monotonic()+1)
            ready, _, _ = select.select([target, carrier], [], [], .1)
            if target in ready:
                frame=budget.frame(CHUNK)
                raw = target.recv(frame.capacity)
                if not raw: result['outcome']='closed'; break
                budget.charge(len(raw));result['charged_bytes'] += len(raw)
                if result['charged_bytes'] > limits['maxBytesPerChannel']: raise TunnelUnavailable()
                inner.write(raw, deadline)
                result['client_written_bytes'] += len(raw); activity=time.monotonic()
                raw=None;frame.close();frame=None
            if carrier in ready or carrier.pending() or inner.ssl.pending():
                if carrier in ready or carrier.pending(): inner.feed(deadline)
                while True:
                    frame=budget.frame(CHUNK)
                    try: raw = inner.ssl.read(frame.capacity)
                    except ssl.SSLWantReadError: frame.close();frame=None;break
                    except ssl.SSLWantWriteError: frame.close();frame=None;inner.flush(deadline);break
                    if not raw: result['outcome']='closed'; return result
                    budget.charge(len(raw));result['charged_bytes'] += len(raw)
                    if result['charged_bytes'] > limits['maxBytesPerChannel']: raise TunnelUnavailable()
                    offset=0
                    while offset < len(raw):
                        inner.checkpoint()
                        if time.monotonic() >= deadline: raise TunnelUnavailable()
                        _, writable, _ = select.select([], [target], [], .1)
                        if target in writable:
                            count=target.send(memoryview(raw)[offset:])
                            if count <= 0: raise TunnelUnavailable()
                            offset+=count;result['target_written_bytes']+=count
                    activity=time.monotonic();raw=None;frame.close();frame=None
        return result
    except Exception as error:
        # Return only a fixed outcome, never a retained traceback containing a
        # payload/private IPC frame. The Actor journal still records consumption.
        error.__traceback__=error.__context__=error.__cause__=None
        result['outcome']='unknown'
        return result
    finally:
        errors=[]
        raw=None
        # Drop actual sockets/BIO owners before releasing a channel reservation.
        for value in (target,carrier):
            if value is not None:
                try:value.close()
                except Exception:errors.append('socket_close_unknown')
        if inner is not None:inner.guard=None
        if guard is not None:
            try:guard.close()
            except Exception:errors.append('permit_close_unknown')
        inner=None
        if frame is not None:frame.close()
        if reserved and not errors:budget.release()
        result['cleanup_observed']=not errors
        result['cleanup_errors']=errors
