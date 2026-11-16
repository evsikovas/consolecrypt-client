"""Explicit Linux TUN startup using existing Node custody and one Relay offer.

No enrollment, lease issuance, retry, or restart authorization happens here.
Only the existing Rust worker admits the client's signed inner-TLS operation.
"""
from dataclasses import dataclass
import math
import json
import select
import signal
import subprocess
import os
from pathlib import Path
import re
import ssl
import stat
import sys
import threading
import time

from .device_reader_config import _startup_path
from .identity_gate import GateConfig, strict_json
from .reader_files import read_reader_file


@dataclass(frozen=True, slots=True, repr=False)
class TunnelConfig:
    executable: Path
    executable_sha256: str
    store: Path
    anchor: Path
    owner_pin: Path
    node: Path
    policy: Path
    audit: Path
    certificate: Path
    private_key: Path
    client_ca: Path
    relay_ca: Path
    relay_offer: Path

    @classmethod
    def from_file(cls, path):
        if sys.platform != 'linux':
            raise ValueError('tunnel_linux_required')
        path = Path(path).absolute()
        _startup_path(path)
        obj = strict_json(read_reader_file(path, 16384))
        fields = set(cls.__dataclass_fields__)
        if type(obj) is not dict or set(obj) != fields:
            raise ValueError('invalid_tunnel_configuration')
        for name in fields - {'executable_sha256'}:
            text = obj[name]
            if type(text) is not str or not 0 < len(text) <= 4096 or '\x00' in text:
                raise ValueError('invalid_tunnel_configuration')
            obj[name] = Path(text)
        config = cls(**obj)
        config.validate_files()
        return config

    def __post_init__(self):
        for name in self.__dataclass_fields__:
            if name == 'executable_sha256':
                continue
            path = getattr(self, name)
            if not isinstance(path, Path) or not path.is_absolute() or '..' in path.parts:
                raise ValueError('absolute_tunnel_paths_required')
        if type(self.executable_sha256) is not str or not re.fullmatch('[0-9a-f]{64}', self.executable_sha256):
            raise ValueError('tunnel_worker_digest_required')

    def validate_files(self):
        directories = (self.store, self.anchor, self.node, self.audit)
        for path in directories:
            _startup_path(path, directory=True)
        resolved = tuple(p.resolve(strict=True) for p in directories)
        for i, left in enumerate(resolved):
            if any(left == right or left in right.parents or right in left.parents for right in resolved[i + 1:]):
                raise ValueError('tunnel_state_namespaces_overlap')
        files = (self.executable, self.owner_pin, self.policy, self.certificate,
                 self.private_key, self.client_ca, self.relay_ca, self.relay_offer)
        for path in files:
            _startup_path(path)
        if len({p.resolve(strict=True) for p in files}) != len(files):
            raise ValueError('tunnel_trusted_files_overlap')
        if any(p.resolve(strict=True) == d or d in p.resolve(strict=True).parents for p in files for d in resolved):
            raise ValueError('tunnel_file_in_state_namespace')
        for path in (self.private_key, self.relay_offer):
            info = path.stat()
            if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o600:
                raise ValueError('tunnel_private_file_required')
        if (not os.access(self.executable, os.X_OK)
                or read_reader_file(self.executable, 128 * 1024**2, digest=True) != self.executable_sha256):
            raise ValueError('tunnel_worker_digest_mismatch')
        self.read_offer()  # Validate DATA only; no worker, network, or grant.

    def validate_node_config(self, config, agent_id):
        from cc_agent.tunnel_policy_v2 import load_policy
        if (type(config) is not GateConfig or config.executable_sha256 != self.executable_sha256
                or any(left.resolve(strict=True) != right.resolve(strict=True) for left, right in (
                    (self.executable, config.executable), (self.store, config.store),
                    (self.anchor, config.anchor), (self.owner_pin, config.owner_pin)))):
            raise ValueError('tunnel_node_configuration_mismatch')
        if load_policy(self.policy).value()['agentId'] != agent_id:
            raise ValueError('tunnel_agent_mismatch')

    def read_offer(self):
        _startup_path(self.relay_offer)
        info = self.relay_offer.stat()
        if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o600:
            raise ValueError('tunnel_private_file_required')
        value = strict_json(read_reader_file(self.relay_offer, 4096))
        required = {'origin', 'slotId', 'dialIp', 'capability', 'expiresAt', 'idleSeconds', 'maxCiphertextBytes'}
        if type(value) is not dict or set(value) != required:
            raise ValueError('invalid_tunnel_relay_offer')
        expires = value['expiresAt']
        now = time.time()
        if type(expires) is not int or not math.isfinite(now) or not 0 < expires - now <= 300:
            raise ValueError('tunnel_relay_offer_expired')
        return value

    def __repr__(self):
        return 'TunnelConfig([redacted])'

    def __reduce_ex__(self, protocol):
        raise TypeError('tunnel_config_not_portable')


def _supervised_runtime_pin():
    # Closed system-Python paths; the reviewed package pin selects exactly one.
    # A real verified build must supply this package-owned PUBLIC pin; no minting.
    root = Path(__file__).resolve().parents[1]
    pin = root / 'python/supervisor-runtime.json'
    _startup_path(pin)
    value = strict_json(read_reader_file(pin, 4096))
    if (type(value) is not dict or set(value) != {'format', 'pythonExecutable', 'pythonSha256'}
            or value['format'] != 'cc-tunnel-supervisor-runtime/v1'
            or value['pythonExecutable'] not in ('/usr/bin/python3.11', '/usr/bin/python3.12')
            or type(value['pythonSha256']) is not str
            or re.fullmatch('[0-9a-f]{64}', value['pythonSha256']) is None
            or value['pythonSha256'] == '0' * 64):
        raise ValueError('supervised_runtime_unsupported')
    runtime = Path(value['pythonExecutable'])
    _startup_path(runtime)
    if read_reader_file(runtime, 128 * 1024**2, digest=True) != value['pythonSha256']:
        raise ValueError('supervised_runtime_unsupported')


def _group_members(group):
    # Only one bounded post-exit inspection, not a new authority/liveness watcher.
    end = time.monotonic() + .5
    members = []
    with os.scandir('/proc') as entries:
        for index, entry in enumerate(entries):
            if index >= 65536 or time.monotonic() >= end:
                raise ValueError('supervised_group_unknown')
            if not entry.name.isdecimal():
                continue
            try:
                with open('/proc/' + entry.name + '/stat', 'rb') as source:
                    raw = source.read(4097)
                if len(raw) > 4096:
                    raise ValueError('supervised_group_unknown')
                tail = raw[raw.rfind(b')') + 2:].split()
                if len(tail) < 3:
                    raise ValueError('supervised_group_unknown')
                if int(tail[2]) == group:
                    members.append(int(entry.name))
            except FileNotFoundError:
                continue
    return members


def _run_supervised_once(config, stop):
    """Launch ONE fixed Rust supervisor. No Python target I/O or grant fallback."""
    config.validate_files()
    _supervised_runtime_pin()
    if any(not str(getattr(config, name)).isascii() for name in (
            'audit', 'policy', 'certificate', 'private_key', 'client_ca', 'relay_ca', 'relay_offer')):
        raise ValueError('supervised_path_unsupported')
    # One ephemeral name. Unknown owners keep the file and block later starts;
    # successful observed cleanup removes only this exact owned inode.
    path = config.audit / 'supervised-child-config.json'
    value = dict(format='cc-tunnel-supervised-child-config/v1', **{
        key: str(getattr(config, key)) for key in (
            'policy', 'certificate', 'private_key', 'client_ca', 'relay_ca', 'relay_offer')})
    raw = json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode('ascii') + b'\n'
    if len(raw) > 16384:
        raise ValueError('supervised_configuration_oversize')
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as output:
        output.write(raw)
        output.flush()
        os.fsync(output.fileno())
        identity = os.fstat(output.fileno())
    observed = path.lstat()
    if ((observed.st_dev, observed.st_ino, observed.st_size) != (identity.st_dev, identity.st_ino, len(raw))
            or path.is_symlink()):
        raise ValueError('supervised_configuration_unknown')
    process = None
    reader = None
    result = bytearray()
    overflow = []
    cleaned = False
    reaped = False
    try:
        argv = [str(config.executable), 'worker-tunnel-supervised-sqlite',
                '--db-dir', str(config.store), '--anchor-dir', str(config.anchor),
                '--owner-pin', str(config.owner_pin), '--node-dir', str(config.node),
                '--policy', str(config.policy), '--child-config', str(path)]
        process = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, cwd='/tmp', env={'LANG': 'C.UTF-8'},
            start_new_session=True, close_fds=True, bufsize=0)

        def read_result():
            try:
                while True:
                    block = process.stdout.read(1024)
                    if not block:
                        return
                    if len(result) + len(block) > 4096:
                        overflow.append(True)
                        return
                    result.extend(block)
            except Exception:
                overflow.append(True)

        reader = threading.Thread(target=read_result, name='cc-supervised-result', daemon=True)
        reader.start()
        os.set_blocking(process.stdin.fileno(), False)
        end = time.monotonic() + 310  # Never changes the original signed expiry.
        stop_deadline = None
        status = None
        while time.monotonic() < end:
            # WNOWAIT keeps the original leader PID reserved until group cleanup.
            status = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            if status is not None:
                break
            if overflow:
                raise ValueError('supervised_result_unknown')
            if stop.is_set() and stop_deadline is None:
                control = memoryview(b'{"action":"stop"}\n')
                write_end = time.monotonic() + .5
                while control:
                    if time.monotonic() >= write_end:
                        raise ValueError('supervised_stop_unknown')
                    _, writable, _ = select.select([], [process.stdin], [], .05)
                    if writable:
                        try:
                            count = os.write(process.stdin.fileno(), control)
                        except BlockingIOError:
                            continue
                        if count <= 0:
                            raise ValueError('supervised_stop_unknown')
                        control = control[count:]
                stop_deadline = time.monotonic() + 20
            if stop_deadline is not None and time.monotonic() >= stop_deadline:
                raise ValueError('supervised_stop_unknown')
            time.sleep(.05)
        if status is None:
            raise ValueError('supervised_deadline')
        members = _group_members(process.pid)
        if any(pid != process.pid for pid in members):
            raise ValueError('supervised_group_unknown')
        reader.join(.5)
        if reader.is_alive() or overflow or not result.endswith(b'\n') or result.count(b'\n') != 1:
            raise ValueError('supervised_result_unknown')
        terminal = strict_json(bytes(result))
        if (type(terminal) is not dict or set(terminal) != {'format', 'outcome', 'cleanupObserved', 'drainRecorded'}
                or terminal['format'] != 'cc-tunnel-supervised-result/v1'
                or terminal['outcome'] not in ('stopped', 'unknown')
                or type(terminal['cleanupObserved']) is not bool or type(terminal['drainRecorded']) is not bool
                or status.si_code != os.CLD_EXITED or status.si_status != 0 or terminal['outcome'] != 'stopped'
                or terminal['cleanupObserved'] is not True or terminal['drainRecorded'] is not True):
            raise ValueError('supervised_result_unknown')
        process.wait(timeout=1)
        reaped = True
        cleaned = True  # Physical/process cleanup only; never used as Node authority.
        return True
    finally:
        if process is not None:
            if not cleaned and not reaped and process.returncode is None:
                # The leader has not been reaped on the unknown paths above, so
                # this fresh owned PG cannot have been recycled to another owner.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=1)
            process.stdin.close()
            if reader is not None:
                reader.join(.5)
                if reader.is_alive():
                    raise ValueError('supervised_reader_unknown')
            process.stdout.close()
        if cleaned:
            current = path.lstat()
            if (current.st_dev, current.st_ino) != (identity.st_dev, identity.st_ino) or path.is_symlink():
                raise ValueError('supervised_configuration_unknown')
            path.unlink()


class TunnelRunner:
    """One outbound attempt. Shutdown joins its actual carrier/worker owners."""
    def __init__(self, config):
        if type(config) is not TunnelConfig:
            raise ValueError('invalid_tunnel_configuration')
        self._config = config
        self._stop = threading.Event()
        self._thread = None
        self._cleanup_observed = False
        self._supervised_mode = False

    def start(self):
        if self._thread is not None or self._stop.is_set():
            raise ValueError('tunnel_start_once')
        self._thread = threading.Thread(target=self._run, name='cc-tunnel-owned', daemon=True)
        self._thread.start()

    def _run(self):
        from cc_agent.tunnel_policy_v2 import PolicyV2, load_policy
        try:
            supervised = type(load_policy(self._config.policy)) is PolicyV2
        except Exception as error:
            error.__traceback__ = error.__context__ = error.__cause__ = None
            self._cleanup_observed = False
            self._stop.set()
            return
        if supervised:
            self._supervised_mode = True
            try:
                self._cleanup_observed = _run_supervised_once(self._config, self._stop)
            except Exception as error:
                error.__traceback__ = error.__context__ = error.__cause__ = None
                self._cleanup_observed = False
            finally:
                self._stop.set()
            return
        custody = session = carrier = None
        errors = []
        try:
            from cc_agent.tunnel import Policy, TunnelCustody, RouteSession, ServerTLS, serve_once
            from cc_agent.tunnel_audit import TunnelAuditLog
            from cc_agent.wss_carrier import RelayEndpoint, RelayOffer, WssCarrier
            c = self._config
            c.validate_files()
            policy = Policy(c.policy)
            custody = TunnelCustody(binary=c.executable, binary_sha256=c.executable_sha256,
                database=c.store, anchor=c.anchor, owner_pin=c.owner_pin, node=c.node, policy=policy)
            session = RouteSession(policy, custody, self._stop, audit=TunnelAuditLog(c.audit, policy))
            inner = ServerTLS(certificate=c.certificate, private_key=c.private_key,
                              client_ca=c.client_ca, policy=policy, custody=custody)
            context = ssl.create_default_context(cadata=read_reader_file(c.relay_ca, 65536).decode('ascii'))
            received = time.monotonic()
            offer_data = c.read_offer()
            ttl = int(offer_data['expiresAt'] - time.time())
            offer = RelayOffer(RelayEndpoint(offer_data['origin'], offer_data['slotId'], offer_data['dialIp']),
                offer_data['capability'], received_at=received, expires_in_seconds=ttl,
                idle_seconds=offer_data['idleSeconds'], max_ciphertext_bytes=offer_data['maxCiphertextBytes'])
            offer_data = None
            if self._stop.is_set():
                return
            carrier = WssCarrier(offer, context, stop=self._stop)
            result = serve_once(carrier=carrier, inner_context=inner, session=session)
            if result['cleanup_observed'] is not True:
                errors.append('tunnel_cleanup_unknown')
            carrier = None  # serve_once already attempted its actual close.
        except Exception as error:
            # No traceback/private paths/capability goes into telemetry or logs.
            error.__traceback__ = error.__context__ = error.__cause__ = None
        finally:
            self._stop.set()
            for resource in (carrier, session, custody):
                if resource is not None:
                    try:
                        resource.close()
                    except Exception:
                        errors.append('tunnel_cleanup_unknown')
            self._cleanup_observed = not errors

    def close(self):
        self._stop.set()
        if self._thread is None:
            return
        self._thread.join(timeout=25 if self._supervised_mode else 10)
        if self._thread.is_alive() or not self._cleanup_observed:
            raise ValueError('tunnel_cleanup_unknown')
