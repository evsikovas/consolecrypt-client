"""Fixed TLS/WSS child transport; Node authority and target I/O stay in Rust.

The pipe exchanges are consistency/lifetime facts from one owned child, never
a deserialized Permit or a boolean authorizer. There is no subprocess or target
socket API in this module.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import re
import select
import ssl
import stat
import threading
import time

from integration.device_reader_config import _startup_path
from integration.reader_files import read_reader_file
from .tunnel import InnerTLS, ServerTLS, TunnelUnavailable, _canonical, _strict
from .tunnel_policy_v2 import PolicyV2
from .wss_carrier import CarrierNetworkLoss, RelayEndpoint, RelayOffer, WssCarrier

FRAME = 24576
DATA = 8192
_BASE = {'format', 'session', 'seq', 'kind'}
_FIELDS = {
    'start': {'configPath', 'inputPipeInode', 'outputPipeInode'},
    'challenge': {'proof', 'policyHash', 'expiresAt'},
    'accepted': {'proof', 'expiresAt', 'policyHash', 'idleExpiresAt'},
    'poll': {'idleExpiresAt'}, 'deliver': {'hex', 'idleExpiresAt'}, 'stop': set(),
}


def _os_now():
    return time.time()


def _pipe_identity(fd):
    info = os.fstat(fd)
    if not stat.S_ISFIFO(info.st_mode) or info.st_ino <= 0 or info.st_uid != os.getuid():
        raise TunnelUnavailable()
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid)


def _hex(value, digits):
    if (type(value) is not str or len(value) != digits
            or re.fullmatch('[0-9a-f]+', value) is None or value == '0' * digits):
        raise TunnelUnavailable()
    return value


def _path(value):
    if type(value) is not str or not 0 < len(value) <= 4096 or not value.isascii():
        raise TunnelUnavailable()
    path = Path(value)
    if not path.is_absolute() or '..' in path.parts or path.resolve(strict=True) != path:
        raise TunnelUnavailable()
    _startup_path(path)
    return path


class OwnedPipe:
    """Nonportable original pipe owner plus fixed clock/sequence boundaries."""
    def __init__(self):
        self._pid, self._parent = os.getpid(), os.getppid()
        self._input, self._output = _pipe_identity(0), _pipe_identity(1)
        if self._input == self._output:
            raise TunnelUnavailable()
        os.set_blocking(0, False)
        os.set_blocking(1, False)
        self._session, self._seq, self._phase = None, 0, 'boot'
        self.policy = None
        self.stop = threading.Event()
        self._last_wall = None
        self._transport_failed = False
        self._wall_origin, self._mono_origin = _os_now(), time.monotonic()
        if (type(self._wall_origin) not in (int, float) or not math.isfinite(self._wall_origin)
                or not 0 <= self._wall_origin <= 253402300799):
            raise TunnelUnavailable()
        self._deadline = self._mono_origin + 5
        self._expires = self._wall_origin + 5
        self._lease_expires = None
        self._idle_expires = None
        self._idle_deadline = None
        self._idle = None

    def _owner(self):
        if (os.getpid() != self._pid or os.getppid() != self._parent
                or _pipe_identity(0) != self._input or _pipe_identity(1) != self._output):
            raise TunnelUnavailable()

    def check(self, include_idle=True):
        self._owner()
        wall = _os_now()
        mono = time.monotonic()
        if (type(wall) not in (int, float) or not math.isfinite(wall)
                or not 0 <= wall <= 253402300799 or wall >= self._expires
                or self._last_wall is not None and wall < self._last_wall
                or mono >= self._deadline or self.stop.is_set() or self._transport_failed
                or include_idle and self._idle_expires is not None
                and (wall >= self._idle_expires or mono >= self._idle_deadline)):
            raise TunnelUnavailable()
        self._last_wall = wall
        if self.policy is not None:
            self.policy.check()

    def deadline(self, seconds, include_idle=True):
        self.check(include_idle=include_idle)
        result = min(self._deadline, time.monotonic() + seconds)
        if include_idle and self._idle_deadline is not None:
            result = min(result, self._idle_deadline)
        return result

    def request(self):
        raw = bytearray()
        # Only Rust's real DATA debit can advance idle. Read its next checked
        # bound before applying old idle, while original TTL/pipe owner stay fixed.
        end = self.deadline(1 if self._phase == 'active' else 5, include_idle=False)
        while b'\n' not in raw:
            self.check(include_idle=False)
            if time.monotonic() >= end:
                raise TunnelUnavailable()
            ready, _, _ = select.select([0], [], [], min(.1, max(0, end - time.monotonic())))
            if not ready:
                continue
            try:
                block = os.read(0, FRAME + 1 - len(raw))
            except BlockingIOError:
                continue
            if not block:
                raise TunnelUnavailable()  # EOF is never graceful Stop.
            raw.extend(block)
            if len(raw) > FRAME:
                raise TunnelUnavailable()
        value = _strict(bytes(raw), FRAME)
        if (not raw.endswith(b'\n') or raw.count(b'\n') != 1 or b'\r' in raw
                or type(value) is not dict
                or json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode('ascii') + b'\n' != raw
                or value.get('format') != 'cc-tunnel-supervised/v1'
                or type(value.get('kind')) is not str or value['kind'] not in _FIELDS
                or set(value) != _BASE | _FIELDS[value['kind']]):
            raise TunnelUnavailable()
        session = _hex(value.get('session'), 32)
        if (type(value.get('seq')) is not int or value['seq'] != self._seq + 1
                or not 1 <= value['seq'] <= 2**64 - 1):
            raise TunnelUnavailable()
        if self._session is None:
            if value['kind'] != 'start' or value['seq'] != 1:
                raise TunnelUnavailable()
            self._session = session
        elif session != self._session or value['kind'] == 'start':
            raise TunnelUnavailable()
        self._seq = value['seq']
        self.check(include_idle=False)
        return value

    def reply(self, kind, **fields):
        value = dict(format='cc-tunnel-supervised/v1', session=self._session,
                     seq=self._seq, kind=kind, **fields)
        raw = json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode('ascii') + b'\n'
        if len(raw) > FRAME:
            raise TunnelUnavailable()
        offset = 0
        cleanup_only = kind == 'stopped'
        end = self.deadline(1, include_idle=not cleanup_only)
        while offset < len(raw):
            self.check(include_idle=not cleanup_only)
            if time.monotonic() >= end:
                raise TunnelUnavailable()
            _, ready, _ = select.select([], [1], [], min(.1, max(0, end - time.monotonic())))
            if ready:
                try:
                    count = os.write(1, memoryview(raw)[offset:])
                except BlockingIOError:
                    continue
                if count <= 0:
                    raise TunnelUnavailable()
                offset += count
        self.check(include_idle=not cleanup_only)

    def configure(self, request):
        for field, actual in (('inputPipeInode', self._input[1]), ('outputPipeInode', self._output[1])):
            if type(request[field]) is not int or request[field] != actual:
                raise TunnelUnavailable()
        config_path = _path(request['configPath'])
        config = _strict(read_reader_file(config_path, 16384))
        names = {'policy', 'certificate', 'private_key', 'client_ca', 'relay_ca', 'relay_offer'}
        if (type(config) is not dict or set(config) != names | {'format'}
                or config['format'] != 'cc-tunnel-supervised-child-config/v1'):
            raise TunnelUnavailable()
        files = {name: _path(config[name]) for name in names}
        if len(set(files.values())) != len(files):
            raise TunnelUnavailable()
        for name in ('private_key', 'relay_offer'):
            info = files[name].stat()
            if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o600:
                raise TunnelUnavailable()
        self.policy = PolicyV2(files['policy'])
        relay = _strict(read_reader_file(files['relay_offer'], 4096), 4096)
        if type(relay) is not dict or set(relay) != {
                'origin', 'slotId', 'dialIp', 'capability', 'expiresAt', 'idleSeconds', 'maxCiphertextBytes'}:
            raise TunnelUnavailable()
        wall = _os_now()
        expires = relay['expiresAt']
        if type(expires) is not int or not 0 < expires - wall <= 300:
            raise TunnelUnavailable()
        self._expires = min(expires, wall + self.policy.value()['limits']['maxTtlSeconds'])
        self._deadline = self._mono_origin + (self._expires - self._wall_origin)
        self._idle = self.policy.value()['limits']['idleTimeoutSeconds']
        self.check()
        return files, relay

    def narrow_expiry(self, expires):
        if type(expires) is not int or not 0 < expires <= 253402300799:
            raise TunnelUnavailable()
        if self._lease_expires is not None and self._lease_expires != expires:
            raise TunnelUnavailable()
        self._lease_expires = expires
        self._expires = min(self._expires, expires)
        self._deadline = min(self._deadline, self._mono_origin + (self._expires - self._wall_origin))
        self.check()

    def adopt_idle(self, expires):
        self.check(include_idle=False)
        wall = _os_now()
        if (type(expires) is not int or self._lease_expires is None
                or not wall < expires <= self._lease_expires
                or expires > wall + self._idle
                or self._idle_expires is not None and expires < self._idle_expires):
            raise TunnelUnavailable()
        deadline = self._mono_origin + (expires - self._wall_origin)
        if expires == self._idle_expires:
            deadline = min(deadline, self._idle_deadline)
        self._idle_expires, self._idle_deadline = expires, deadline
        self.check()

    def activate(self, idle_expires):
        self.adopt_idle(idle_expires)
        self._phase = 'active'

    def __repr__(self):
        return 'OwnedTunnelPipe([redacted])'

    def __reduce_ex__(self, protocol):
        raise TypeError('owned_tunnel_pipe_not_portable')


def _read_tls_json(inner, owned):
    raw = bytearray()
    end = owned.deadline(5)
    while b'\n' not in raw:
        owned.check()
        if time.monotonic() >= end:
            raise TunnelUnavailable()
        try:
            block = inner.ssl.read(min(16385 - len(raw), 16384))
        except ssl.SSLWantReadError:
            inner.feed(end)
            continue
        except ssl.SSLWantWriteError:
            inner.flush(end)
            continue
        if not block:
            raise TunnelUnavailable()
        raw.extend(block)
        if len(raw) > 16384:
            raise TunnelUnavailable()
    if not raw.endswith(b'\n') or raw.count(b'\n') != 1 or b'\r' in raw:
        raise TunnelUnavailable()
    owned.check()
    return _strict(bytes(raw))


def _write_tls_json(inner, owned, value):
    raw = _canonical(value) + b'\n'
    if len(raw) > 16384:
        raise TunnelUnavailable()
    inner.write(raw, owned.deadline(5))
    owned.check()


def _data(value, maximum):
    if (type(value) is not str or len(value) % 2 or not 0 < len(value) <= 2 * maximum
            or re.fullmatch('[0-9a-f]+', value) is None):
        raise TunnelUnavailable()
    return bytes.fromhex(value)


def _proof_fields(proof, policy):
    # Structural consistency only: Rust verifies the Node signature/Actor state.
    if (type(proof) is not dict or set(proof) != {'body', 'signature'}
            or type(proof['body']) is not dict
            or set(proof['body']) != {'header', 'agentId', 'keyEpoch', 'leaseHash', 'nonce', 'issuedAt'}):
        raise TunnelUnavailable()
    body, route = proof['body'], policy.value()
    header = body['header']
    if (type(header) is not dict or set(header) != {'version', 'context', 'ownerEpoch', 'checkpoint'}
            or type(header['version']) is not int or header['version'] != 1
            or header['context'] != route['context']
            or any(type(header[k]) is not int or not 0 <= header[k] <= 2**64 - 1
                   for k in ('ownerEpoch', 'checkpoint'))
            or body['agentId'] != route['agentId']
            or type(body['keyEpoch']) is not int or body['keyEpoch'] != route['keyEpoch']
            or type(body['issuedAt']) is not int or not 0 <= body['issuedAt'] <= 253402300799):
        raise TunnelUnavailable()
    _hex(proof['signature'], 128)
    return _hex(body['nonce'], 32), _hex(body['leaseHash'], 64)


def _challenge_echo(inner, owned, proof, expires):
    """One exact prepared request on one real verified SSLObject; no Actor here."""
    actual_ssl = inner.ssl
    nonce, lease_hash = _proof_fields(proof, owned.policy)
    owned.narrow_expiry(expires)
    _write_tls_json(inner, owned, dict(format='cc-agent-tunnel-channel-challenge/v2',
        proof=proof, policyHash=owned.policy.digest, expiresAt=expires))
    echo = _read_tls_json(inner, owned)
    expected = dict(format='cc-agent-tunnel-channel-echo/v2', nonce=nonce,
        policyHash=owned.policy.digest, leaseHash=lease_hash, deviceId=owned.policy.value()['clientDeviceId'])
    if echo != expected or inner.ssl is not actual_ssl:
        raise TunnelUnavailable()
    return expected




def _check_carrier(carrier, owned):
    # Existing maintained carrier failures have priority over buffered TLS alert.
    # No tls=true input or alternate carrier implementation is accepted here.
    try:
        if type(carrier) is not WssCarrier:
            raise TunnelUnavailable()
        carrier._own()
        with carrier._cv:
            if carrier._failed or carrier._ended or carrier._closed or not carrier._live():
                raise TunnelUnavailable()
    except Exception:
        owned._transport_failed = True
        raise


class _BoundTransportLoss(TunnelUnavailable):
    """Private request only. Only Rust can prove ownership retirement/CAS."""


def _observe_transport_loss(carrier, inner, owned, actual_ssl):
    owned.check()
    if (owned._phase != 'active' or owned.policy.wrapper()['resumeOriginal'] is not True
            or type(carrier) is not WssCarrier or inner.carrier is not carrier
            or inner.ssl is not actual_ssl or not isinstance(actual_ssl, ssl.SSLObject)
            or actual_ssl.version() != 'TLSv1.3' or not inner._handshake_done):
        return False
    carrier._own()
    with carrier._cv:
        return (carrier._network_lost and not carrier._closed and carrier._live()
                and not carrier._in and actual_ssl.pending() == 0
                and inner.incoming.pending == 0 and inner.outgoing.pending == 0
                and carrier._ws is not None and not carrier._ws._denied)


def _read_active_tls(inner, owned, actual_ssl, maximum):
    """Actual TLS record observation only; no admission or drain authority.

    InnerTLS.feed rejects raw carrier EOF and never writes BIO EOF. Before any
    local TLS shutdown, an empty SSLObject read (or SSLZeroReturnError) therefore
    observes the peer's authenticated close_notify, not a transport close. Any
    failed observation is sticky for this original owner.
    """
    wanted_read = False
    try:
        owned.check()
        if (inner.ssl is not actual_ssl or not isinstance(actual_ssl, ssl.SSLObject)
                or actual_ssl.version() != 'TLSv1.3' or not inner._handshake_done):
            raise TunnelUnavailable()
        inner.checkpoint()
        limits = owned.policy.value()['limits']
        if (type(maximum) is not int or not 1 <= maximum <= min(DATA, limits['maxFrameBytes'],
                limits['maxQueuedBytesPerChannel'], limits['maxQueuedBytesTotal'])):
            raise TunnelUnavailable() # SSL.read(0) must never masquerade as alert.
        if (inner.ssl is not actual_ssl or not isinstance(actual_ssl, ssl.SSLObject)
                or actual_ssl.version() != 'TLSv1.3' or not inner._handshake_done):
            raise TunnelUnavailable()
        end = owned.deadline(.25)
        # Observe TLS errors/alerts FIRST. Only exact SSLWantRead permits an
        # actual carrier read to classify its own typed network-loss cause.
        try:
            data = actual_ssl.read(maximum)
        except ssl.SSLWantReadError:
            wanted_read = True
            ready = (inner.carrier.pending()
                     or select.select([inner.carrier], [], [], .05)[0])
            if ready:
                inner.feed(end)
            data = None
        except ssl.SSLWantWriteError:
            inner.flush(end)
            data = None
        except ssl.SSLZeroReturnError:
            data = b''
        owned.check()
        if inner.ssl is not actual_ssl:
            raise TunnelUnavailable()
        inner.checkpoint()
        if inner.ssl is not actual_ssl:
            raise TunnelUnavailable()
        # Linearize this original read against the maintained loss monitor.
        # Only an actual WantRead/no-data observation may request retirement;
        # payload, TLS alert, pending bytes and every denied cause stay Unknown.
        carrier = inner.carrier
        if type(carrier) is not WssCarrier:
            _check_carrier(carrier, owned)
        else:
            with carrier._cv:
                if wanted_read and data is None and _observe_transport_loss(carrier, inner, owned, actual_ssl):
                    raise _BoundTransportLoss()
                _check_carrier(carrier, owned)
        return data
    except _BoundTransportLoss:
        raise
    except CarrierNetworkLoss:
        if wanted_read and _observe_transport_loss(inner.carrier, inner, owned, actual_ssl):
            raise _BoundTransportLoss() from None
        owned._transport_failed = True
        raise
    except Exception:
        owned._transport_failed = True
        raise


def _finish_original_tls_stop(inner, owned, actual_ssl, end):
    # The flag is produced only by this SAME SSLObject's authenticated alert;
    # the caller is the SAME original Rust parent's typed stop pipe command.
    try:
        owned.check(include_idle=False)
        if (owned._phase != 'stop_requested' or type(inner) is not InnerTLS
                or inner.ssl is not actual_ssl or not isinstance(actual_ssl, ssl.SSLObject)
                or actual_ssl.version() != 'TLSv1.3' or not inner._handshake_done):
            raise TunnelUnavailable()
        while True:
            owned.check(include_idle=False)
            if time.monotonic() >= end or inner.ssl is not actual_ssl:
                raise TunnelUnavailable()
            inner.checkpoint()  # Original credential/policy/idle guards unchanged.
            try:
                actual_ssl.unwrap()
                break
            except ssl.SSLWantReadError:
                inner.feed(end)  # Existing raw EOF rejection, never write BIO EOF.
            except ssl.SSLWantWriteError:
                inner.flush(end)
        inner.flush(end)  # Emit the SAME SSLObject's reciprocal close_notify first.
        owned.check(include_idle=False)
        if inner.ssl is not actual_ssl or time.monotonic() >= end:
            raise TunnelUnavailable()
    except BaseException:
        owned._transport_failed = True
        raise


def run_child():
    """One fixed package child, one pipe owner, one actual SSLObject; no retry."""
    owned = OwnedPipe()
    carrier = inner = None
    clean_stop = False
    try:
        request = owned.request()
        if request['kind'] != 'start':
            raise TunnelUnavailable()
        files, relay = owned.configure(request)
        context = ssl.create_default_context(cadata=read_reader_file(files['relay_ca'], 65536).decode('ascii'))
        received = time.monotonic()
        ttl = int(relay['expiresAt'] - _os_now())
        offer = RelayOffer(RelayEndpoint(relay['origin'], relay['slotId'], relay['dialIp']), relay['capability'],
            received_at=received, expires_in_seconds=ttl, idle_seconds=relay['idleSeconds'],
            max_ciphertext_bytes=relay['maxCiphertextBytes'])
        relay = None
        tls = ServerTLS._for_owned_supervised_child(certificate=files['certificate'],
            private_key=files['private_key'], client_ca=files['client_ca'], policy=owned.policy, owned=owned)
        carrier = WssCarrier(offer, context, stop=owned.stop)
        inner = InnerTLS(carrier, tls, owned.policy, owned.stop)
        peer = inner.handshake()
        owned.check()
        actual_ssl = inner.ssl
        opened = _read_tls_json(inner, owned)
        if (type(opened) is not dict or set(opened) != {'format', 'mode', 'binding'}
                or opened['format'] != 'cc-agent-tunnel-open/v2'
                or opened['mode'] not in ('admit', 'resume')):
            raise TunnelUnavailable()
        owned.policy.require_mode(opened['mode'])
        owned.reply('opened', peerSha256=peer, mode=opened['mode'], binding=opened['binding'])
        challenge = owned.request()
        if challenge['kind'] != 'challenge' or challenge['policyHash'] != owned.policy.digest:
            raise TunnelUnavailable()
        proof = challenge['proof']
        pending = _canonical(proof)
        if inner.ssl is not actual_ssl:
            raise TunnelUnavailable()
        expected = _challenge_echo(inner, owned, proof, challenge['expiresAt'])
        owned.reply('echo', **{key: value for key, value in expected.items() if key != 'format'}, peerSha256=peer)
        accepted = owned.request()
        if (accepted['kind'] != 'accepted' or _canonical(accepted['proof']) != pending
                or accepted['policyHash'] != owned.policy.digest
                or type(accepted['expiresAt']) is not int
                or accepted['expiresAt'] != challenge['expiresAt'] or inner.ssl is not actual_ssl):
            raise TunnelUnavailable()
        owned.activate(accepted['idleExpiresAt'])
        # Parent has done real Actor CAS and target connect. This child has no Permit.
        _write_tls_json(inner, owned, dict(format='cc-agent-tunnel-open-ack/v2', proof=proof,
            policyHash=owned.policy.digest, expiresAt=accepted['expiresAt']))
        owned.reply('accepted')
        maximum = min(DATA, owned.policy.value()['limits']['maxFrameBytes'],
            owned.policy.value()['limits']['maxQueuedBytesPerChannel'],
            owned.policy.value()['limits']['maxQueuedBytesTotal'])
        while True:
            request = owned.request()
            if inner.ssl is not actual_ssl:
                raise TunnelUnavailable()
            kind = request['kind']
            if owned._phase in ('stop_requested', 'loss_requested') and kind != 'stop':
                raise TunnelUnavailable()
            if kind == 'poll':
                owned.adopt_idle(request['idleExpiresAt'])
                try:
                    data = _read_active_tls(inner, owned, actual_ssl, maximum)
                except _BoundTransportLoss:
                    # This is not EOF/drain/Permit. Keep original pipes alive
                    # and await the exact parent's stop after target shutdown.
                    owned._phase = 'loss_requested'
                    owned.reply('loss_request', **{key: value for key, value in expected.items()
                        if key != 'format'}, peerSha256=peer)
                    continue
                if data is None:
                    owned.reply('idle')
                elif data == b'':
                    # V2 ONLY: Core must send close_notify solely for explicit
                    # owner stop; all other exits abort. This is REQUEST, not drain.
                    owned._phase = 'stop_requested'
                    owned.reply('stop_requested', **{key: value for key, value in expected.items()
                        if key != 'format'}, peerSha256=peer)
                else:
                    owned.reply('data', hex=data.hex())
            elif kind == 'deliver':
                owned.adopt_idle(request['idleExpiresAt'])
                data = _data(request['hex'], maximum)
                inner.write(data, owned.deadline(1))
                owned.check()
                owned.reply('delivered', bytes=len(data))
            elif kind == 'stop':
                # Only Rust parent Stop follows intent or a bound loss request.
                if owned._phase == 'loss_requested':
                    owned.check()
                    if (inner.ssl is not actual_ssl or type(carrier) is not WssCarrier
                            or not carrier._network_lost or carrier._closed):
                        raise TunnelUnavailable()
                else:
                    _check_carrier(carrier, owned)
                if owned._phase == 'stop_requested':
                    # ONE original remaining bound for TLS alert + WS close;
                    # neither step renews TTL/idle or manufactures a drain ACK.
                    end = owned.deadline(1, include_idle=False)
                    _finish_original_tls_stop(inner, owned, actual_ssl, end)
                    carrier._close_after_original_stop(end)
                else:
                    carrier.close()  # Loss/cancel/ordinary abort semantics unchanged.
                carrier = None
                inner.guard = None
                inner = None
                actual_ssl = tls = context = offer = pending = proof = None
                owned.reply('stopped')
                clean_stop = True
                return
            else:
                raise TunnelUnavailable()
    finally:
        if carrier is not None:
            carrier.close()
        if inner is not None:
            inner.guard = None
        inner = None
        owned.stop.set()
        if not clean_stop:
            raise TunnelUnavailable()
