"""One-use, owner-provisioned Relay carrier. No Node admission or reconnect.

Requires the reviewed websockets 17.4 distribution. InnerTLS owns application
authorization and plaintext budgets. This adapter counts inner-TLS ciphertext.
"""
import asyncio
import base64
import ipaddress
import logging
import math
import os
import re
import socket
import ssl
import threading
import time
import uuid
from dataclasses import dataclass
from urllib.parse import urlsplit

import websockets
from websockets import http11
from websockets.asyncio.client import ClientConnection, connect
from websockets.frames import Frame, Opcode
from websockets.exceptions import ConnectionClosedError, ConnectionClosedOK

FRAME = 16384
LOGGER = logging.Logger('cc.relay.carrier.silent', level=logging.CRITICAL + 1)
LOGGER.disabled = True
LOGGER.propagate = False


class CarrierUnavailable(OSError):
    def __init__(self, code='carrier_unavailable'):
        # Only closed public literals; never chain a library exception.
        if code not in {'carrier_unavailable', 'carrier_input', 'carrier_timeout', 'carrier_cleanup'}:
            code = 'carrier_unavailable'
        super().__init__(code)


class CarrierNetworkLoss(CarrierUnavailable):
    """Private actual recv cause only; never retirement or authorization."""


def _finite(value):
    try:
        return type(value) in (int, float) and math.isfinite(value)
    except OverflowError:
        return False


def _trust_snapshot(context):
    # Copy public CA material only into a private context. A caller changing its
    # input context after open cannot silently disable verification mid-connect.
    authorities = context.get_ca_certs(binary_form=True)
    if not 1 <= len(authorities) <= 512 or sum(map(len, authorities)) > 1024**2:
        raise CarrierUnavailable('carrier_input')
    value = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    value.minimum_version = ssl.TLSVersion.TLSv1_2
    value.load_verify_locations(cadata=''.join(ssl.DER_cert_to_PEM_cert(x) for x in authorities))
    return value


@dataclass(frozen=True)
class RelayEndpoint:
    """Trusted local configuration; never derived from a received capability."""
    origin: str
    slot_id: str
    dial_ip: str

    def uri(self):
        try:
            if type(self.origin) is not str or not 1 <= len(self.origin) <= 256:
                raise ValueError()
            p = urlsplit(self.origin)
            if (p.scheme != 'wss' or not p.hostname or p.username is not None
                    or p.password is not None or p.path or p.query or p.fragment
                    or not self.origin.isascii() or '%' in self.origin or '\\' in self.origin):
                raise ValueError()
            host = p.hostname
            # Canonical DNS ASCII or canonical IP, no alternate URI spellings.
            try:
                ip = ipaddress.ip_address(host)
                if str(ip) != host:
                    raise ValueError()
                authority = '[' + host + ']' if ip.version == 6 else host
            except ValueError:
                if (not re.fullmatch(r'[a-z0-9](?:[a-z0-9.-]{0,251}[a-z0-9])?', host)
                        or any(not label or len(label) > 63 or label.startswith('-')
                               or label.endswith('-') for label in host.split('.'))):
                    raise ValueError()
                authority = host
            port = p.port or 443
            if not 1 <= port <= 65535:
                raise ValueError()
            expected = 'wss://' + authority + ((':' + str(port)) if p.port is not None else '')
            if self.origin != expected:
                raise ValueError()
            if type(self.slot_id) is not str or str(uuid.UUID(self.slot_id)) != self.slot_id:
                raise ValueError()
            if self.slot_id == str(uuid.UUID(int=0)):
                raise ValueError()
            if type(self.dial_ip) is not str or str(ipaddress.ip_address(self.dial_ip)) != self.dial_ip:
                raise ValueError()
            return self.origin + '/v1/relay/slots/' + self.slot_id + '/agent'
        except (ValueError, TypeError, AttributeError):
            raise CarrierUnavailable('carrier_input') from None


class RelayOffer:
    """Process-local one-connect lease. No copying, retry, renewal or authority."""
    __slots__ = ('_metadata', '_token', '_lock', '_pid', '_taken')

    def __init__(self, endpoint, capability, *, received_at, expires_in_seconds,
                 idle_seconds, max_ciphertext_bytes):
        if type(endpoint) is not RelayEndpoint:
            raise CarrierUnavailable('carrier_input')
        endpoint.uri()
        if type(capability) is not str or not re.fullmatch(r'ccrl_[A-Za-z0-9_-]{43}', capability):
            raise CarrierUnavailable('carrier_input')
        raw = base64.urlsafe_b64decode(capability[5:] + '=')
        if len(raw) != 32 or base64.urlsafe_b64encode(raw).decode('ascii').rstrip('=') != capability[5:]:
            raise CarrierUnavailable('carrier_input')
        now = time.monotonic()
        if (not _finite(received_at) or not 0 <= received_at <= now
                or type(expires_in_seconds) is not int or not 1 <= expires_in_seconds <= 300
                or type(idle_seconds) is not int or not 1 <= idle_seconds <= 30
                or type(max_ciphertext_bytes) is not int or not 1 <= max_ciphertext_bytes <= 8 * 1024**2
                or now >= received_at + expires_in_seconds):
            raise CarrierUnavailable('carrier_input')
        self._metadata = _OfferMetadata(RelayEndpoint(endpoint.origin, endpoint.slot_id, endpoint.dial_ip),
                                       received_at + expires_in_seconds, idle_seconds, max_ciphertext_bytes)
        self._token, self._lock, self._pid, self._taken = capability, threading.Lock(), os.getpid(), False

    def __repr__(self):
        return 'RelayOffer([redacted])'

    @property
    def endpoint(self):
        return self._metadata.endpoint

    @property
    def deadline(self):
        return self._metadata.deadline

    @property
    def idle(self):
        return self._metadata.idle

    @property
    def max_bytes(self):
        return self._metadata.max_bytes

    def __reduce_ex__(self, protocol):
        raise TypeError('carrier_not_serializable')

    def __copy__(self):
        raise TypeError('carrier_not_copyable')

    def __deepcopy__(self, memo):
        raise TypeError('carrier_not_copyable')

    def _take(self):
        with self._lock:
            if self._pid != os.getpid() or self._taken or time.monotonic() >= self.deadline:
                raise CarrierUnavailable()
            self._taken = True
            value, self._token = self._token, None
            return self._metadata, value


@dataclass(frozen=True, slots=True)
class _OfferMetadata:
    endpoint: RelayEndpoint
    deadline: float
    idle: int
    max_bytes: int


class _NoRedirect(connect):
    def process_redirect(self, exc):
        # Returning an exception denies ALL status redirects, including same-origin.
        return CarrierUnavailable()


class _BoundedConnection(ClientConnection):
    """Maintained parser with hard admission bounds beyond soft max_queue.

    No framing/handshake codec here. Each callback feeds <=1024 bytes into the
    maintained parser; <=4 queued data fragments/32768 payload bytes accepted.
    A peer exceeding this stricter carrier profile is terminally disconnected.
    """
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._frames = 0
        self._bytes = 0
        self._denied = False
        self._callback_frames = 0
        self._outbound_checks = 0
        self._outbound_peak = 0

    def _abort(self):
        self._denied = True
        self.transport.abort()

    def send_data(self):
        # This hook is called by the maintained library's handshake, send and
        # automatic-control paths, even with NO incoming application traffic.
        # write_limit is a watermark: a single native encoded frame plus TLS
        # framing may transiently overshoot it before this post-write check.
        # This is not advertised as a zero-overshoot physical memory ceiling.
        self._outbound_checks += 1
        before = self.transport.get_write_buffer_size()
        self._outbound_peak = max(self._outbound_peak, before)
        if self._denied or before > 16384:
            self._abort()
            raise CarrierUnavailable()
        super().send_data()
        after = self.transport.get_write_buffer_size()
        self._outbound_peak = max(self._outbound_peak, after)
        if after > 32768:
            self._abort()
            raise CarrierUnavailable()

    def data_received(self, data):
        if self._denied:
            return
        if len(data) > 65536:
            self._abort()
            return
        self._callback_frames = 0
        for offset in range(0, len(data), 1024):
            super().data_received(data[offset:offset + 1024])
            if self._denied:
                return
            if self.transport.get_write_buffer_size() > 32768:
                self._abort()
                return

    def process_event(self, event):
        if self._denied:
            return
        if isinstance(event, Frame):
            self._callback_frames += 1
            if self._callback_frames > 128 or event.opcode == Opcode.TEXT:
                self._abort()
                return
            if event.opcode in (Opcode.BINARY, Opcode.CONT):
                if self._frames >= 4 or self._bytes + len(event.data) > 32768:
                    self._abort()
                    return
                self._frames += 1
                self._bytes += len(event.data)
        super().process_event(event)

    def release_fragment(self, count):
        self._frames -= 1
        self._bytes -= count
        if self._frames < 0 or self._bytes < 0:
            self._abort()
            raise CarrierUnavailable()


class WssCarrier:
    """InnerTLS socket-like stream. fileno is ONLY the bounded wake socket.

    One complete binary message handoff; receiver waits for consumption before
    reading another. EOF/failure leaves wake readable until close. No queues of
    pending sends: concurrent send calls are denied, and timed-out send is never
    retried. close aborts transport and joins the one owned event-loop thread.
    """
    def __init__(self, offer, context, *, stop):
        if (type(offer) is not RelayOffer or type(context) is not ssl.SSLContext
                or context.verify_mode != ssl.CERT_REQUIRED or not context.check_hostname
                or type(stop) is not threading.Event):
            raise CarrierUnavailable('carrier_input')
        if (websockets.__version__ != '17.4' or http11.MAX_NUM_HEADERS > 128
                or http11.MAX_LINE_LENGTH > 8192 or http11.MAX_BODY_SIZE > 1048576):
            raise CarrierUnavailable('carrier_input')
        context = _trust_snapshot(context)
        self._pid = os.getpid()
        self._owner = threading.current_thread()
        self._metadata, self._stop = None, stop
        self._cv = threading.Condition()
        self._sending = threading.Lock()
        self._in = b''
        self._ended = self._failed = self._closed = False
        self._closing_original_stop = self._original_close_ok = False
        self._original_close_done = None
        self._network_lost = False
        self._network_loss_denied = False
        self._network_loss_cancel_requested = False
        self._network_loss_cancel_task = None
        self._wake_set = False
        self._loop = self._ws = self._task = None
        self._ready = threading.Event()
        self._space = None
        self._received = self._sent = 0
        self._paired = False
        self._started = self._last = time.monotonic()
        self._read, self._write = socket.socketpair()
        self._read.setblocking(False)
        self._write.setblocking(False)
        self._thread = None
        try:
            # Freeze routing/deadline/budget now; never consult the offer again.
            self._metadata, token = offer._take()
            self._thread = threading.Thread(target=self._run, args=(context, token),
                                            name='cc-wss-owned', daemon=True)
            self._thread.start()
            remaining = min(5.5, self._metadata.deadline - time.monotonic())
            if remaining <= 0 or not self._ready.wait(remaining):
                raise CarrierUnavailable('carrier_timeout')
            with self._cv:
                if self._failed or self._ended or self._ws is None:
                    raise CarrierUnavailable()
        except BaseException:
            self.close()
            raise CarrierUnavailable() from None

    def __repr__(self):
        return 'WssCarrier([redacted])'

    def __reduce_ex__(self, protocol):
        raise TypeError('carrier_not_serializable')

    def __copy__(self):
        raise TypeError('carrier_not_copyable')

    def __deepcopy__(self, memo):
        raise TypeError('carrier_not_copyable')

    def _own(self):
        if self._pid != os.getpid() or self._owner is not threading.current_thread():
            raise CarrierUnavailable()

    def _signal(self):
        # Caller holds cv; never more than one outstanding wake byte.
        if not self._wake_set:
            try:
                self._write.send(b'1')
                self._wake_set = True
            except BlockingIOError:
                self._wake_set = True
            except OSError:
                self._failed = True

    def _empty_wake(self):
        if self._wake_set and not self._in and not self._ended:
            try:
                self._read.recv(1)
            except BlockingIOError:
                pass
            self._wake_set = False

    def _live(self):
        now = time.monotonic()
        return (not self._closed and not self._stop.is_set()
                and now < self._metadata.deadline and now - self._last < self._metadata.idle
                and (self._paired or now - self._started < 15))

    def _network_loss(self, error):
        # Concrete maintained protocol close, not generic OSError/EOF or an
        # exception string. Parser/cap/close frames and original expiry deny.
        return (type(error) is ConnectionClosedError and error.rcvd is None and error.sent is None
                and type(self._ws) is _BoundedConnection and not self._ws._denied
                and self._ws.protocol.close_rcvd is None and self._ws.protocol.close_sent is None
                and type(self._ws.protocol.parser_exc) in (type(None), EOFError)
                and self._paired and self._live())

    def _record_failure(self, error):
        # Called before _main's asynchronous finally can mask an earlier
        # cap/parser/generic exception with monitor cancellation. UNKNOWN wins.
        with self._cv:
            monitor_cancel = (type(error) is asyncio.CancelledError
                and self._network_loss_cancel_requested
                and self._network_loss_cancel_task is self._task
                and self._ws is not None and self._ws.close_code is not None
                and self._network_loss(self._ws.protocol.close_exc))
            if self._network_loss_denied or not (self._network_loss(error) or monitor_cancel):
                self._network_loss_denied = True
                self._network_lost = False
            else:
                self._network_lost = True
            self._failed = not self._closed

    async def _monitor(self):
        # Detect remote close/revoke even while a full handoff waits for its
        # consumer; otherwise recv_streaming isn't polled until that drain.
        while self._live() and self._ws.close_code is None:
            await asyncio.sleep(.05)
        if self._closing_original_stop:
            return  # The one bounded original close coroutine owns this handshake.
        # close_code is available only after the maintained protocol is CLOSED.
        # Capture its actual exception before cancellation can mask it.
        observed = False
        if self._ws.close_code is not None:
            with self._cv:
                observed = not self._network_loss_denied and self._network_loss(self._ws.protocol.close_exc)
                self._network_lost = observed
        self._ws.transport.abort()
        requested = self._task.cancel()  # Exact owned task, not a caller task/PID.
        # No await occurs between cancel and binding this cause: the SAME loop
        # cannot deliver its CancelledError to _main before this is recorded.
        with self._cv:
            self._network_loss_cancel_requested = bool(observed and requested and not self._network_loss_denied)
            self._network_loss_cancel_task = self._task if self._network_loss_cancel_requested else None

    async def _main(self, context, token):
        self._loop = asyncio.get_running_loop()
        self._task = asyncio.current_task()
        self._space = asyncio.Event()
        self._original_close_done = asyncio.Event()
        endpoint = self._metadata.endpoint
        p = urlsplit(endpoint.origin)
        monitor = None
        try:
            if not self._live():
                raise CarrierUnavailable()
            # One await, never async-for; no redirect/proxy/DNS fallback.
            self._ws = await _NoRedirect(endpoint.uri(), ssl=context,
                host=endpoint.dial_ip, port=p.port or 443, server_hostname=p.hostname,
                additional_headers={'Authorization': 'Bearer ' + token},
                proxy=None, compression=None, user_agent_header=None, logger=LOGGER,
                open_timeout=min(5, self._metadata.deadline - time.monotonic()),
                ssl_handshake_timeout=5, ssl_shutdown_timeout=.5,
                close_timeout=.5, ping_interval=None, max_size=FRAME,
                max_queue=(1, 0), write_limit=(16384, 4096), create_connection=_BoundedConnection)
            token = None
            if not self._live():
                raise CarrierUnavailable()
            self._ready.set()
            monitor = asyncio.create_task(self._monitor())
            while self._live():
                block = bytearray()
                async for fragment in self._ws.recv_streaming():
                    self._ws.release_fragment(len(fragment))
                    if type(fragment) is not bytes or len(block) + len(fragment) > FRAME:
                        raise CarrierUnavailable()
                    block.extend(fragment)
                if not block:  # Fixed binary-only profile disallows empty-message floods.
                    raise CarrierUnavailable()
                with self._cv:
                    if not self._live() or self._in or self._sent + self._received + len(block) > self._metadata.max_bytes:
                        raise CarrierUnavailable()
                    self._received += len(block)
                    self._paired = True
                    self._last = time.monotonic()
                    self._in = bytes(block)
                    self._space.clear()
                    self._signal()
                    self._cv.notify_all()
                block = None
                await self._space.wait()
        except BaseException as error:
            if self._closing_original_stop and type(error) is ConnectionClosedOK:
                # Keep asyncio.run's original owner alive until the close
                # coroutine has checked the actual handshake, not just an EOF.
                await self._original_close_done.wait()
                if not self._original_close_ok:
                    self._record_failure(error)
                    raise
            else:
                self._record_failure(error)
                raise
        finally:
            token = None
            if self._ws is not None:
                self._ws.transport.abort()
            if monitor is not None:
                monitor.cancel()
                await asyncio.gather(monitor, return_exceptions=True)

    def _run(self, context, token):
        try:
            operation = self._main(context, token)
            token = None
            asyncio.run(operation)
        except BaseException as error:
            self._record_failure(error)
        finally:
            token = None
            with self._cv:
                self._ended = True
                self._signal()
                self._cv.notify_all()
            self._ready.set()

    def fileno(self):
        self._own()
        return self._read.fileno()

    def pending(self):
        self._own()
        with self._cv:
            return len(self._in)

    def recv(self, count):
        self._own()
        if type(count) is not int or not 1 <= count <= FRAME:
            raise CarrierUnavailable('carrier_input')
        with self._cv:
            end = time.monotonic() + .1
            while not self._in and not self._ended and self._live():
                remaining = end - time.monotonic()
                if remaining <= 0:
                    raise socket.timeout() from None
                self._cv.wait(remaining)
            if not self._live():
                raise CarrierUnavailable()
            if self._failed:
                if self._network_lost and not self._in:
                    raise CarrierNetworkLoss()
                raise CarrierUnavailable()
            if not self._in:
                return b''
            raw, self._in = self._in[:count], self._in[count:]
            if not self._in:
                self._empty_wake()
                self._loop.call_soon_threadsafe(self._space.set)
            return raw

    async def _send(self, raw):
        if not self._live():
            raise CarrierUnavailable()
        try:
            # On timeout abort, THEN cancellation; never resume/duplicate a send.
            operation = asyncio.create_task(self._ws.send(raw))
            done, _ = await asyncio.wait({operation}, timeout=min(1, self._metadata.deadline - time.monotonic()))
            if not done:
                self._ws.transport.abort()
                operation.cancel()
                await asyncio.gather(operation, return_exceptions=True)
                raise CarrierUnavailable('carrier_timeout')
            operation.result()
            if not self._live():
                raise CarrierUnavailable()
            with self._cv:
                self._last = time.monotonic()
            return len(raw)
        except BaseException:
            self._ws.transport.abort()
            raise CarrierUnavailable() from None

    def send(self, value):
        self._own()
        if not isinstance(value, (bytes, bytearray, memoryview)) or not 1 <= len(value) <= FRAME:
            raise CarrierUnavailable('carrier_input')
        if not self._sending.acquire(blocking=False):
            raise CarrierUnavailable()
        try:
            with self._cv:
                if not self._live() or self._ended or self._sent + self._received + len(value) > self._metadata.max_bytes:
                    raise CarrierUnavailable()
                self._sent += len(value)  # Conservative charge before send, never rolled back.
            future = asyncio.run_coroutine_threadsafe(self._send(bytes(value)), self._loop)
            return future.result(timeout=1.2)
        except BaseException:
            self.close()
            raise CarrierUnavailable() from None
        finally:
            self._sending.release()

    async def _finish_original_close(self, end):
        try:
            if time.monotonic() >= end or not self._live():
                raise CarrierUnavailable()
            await asyncio.wait_for(self._ws.close(code=1000, reason=''),
                                   timeout=max(0, end-time.monotonic()))
            protocol = self._ws.protocol
            # Pinned close() suppresses ConnectionClosedError as well as OK.
            # Only this actual, clean, two-sided maintained handshake qualifies.
            if (type(self._ws) is not _BoundedConnection or self._ws._denied
                    or protocol.parser_exc is not None
                    or self._ws.connection_exc is not None
                    or protocol.close_sent is None or protocol.close_sent.code != 1000
                    or protocol.close_rcvd is None or protocol.close_rcvd.code != 1000
                    or type(protocol.close_exc) is not ConnectionClosedOK
                    or not self._ws.connection_lost_waiter.done()):
                raise CarrierUnavailable()
            with self._cv:
                if (time.monotonic() >= end or not self._live() or self._in
                        or self._failed or self._network_lost or self._network_loss_denied):
                    raise CarrierUnavailable()
                self._original_close_ok = True
        finally:
            self._original_close_done.set()

    def _close_after_original_stop(self, end):
        # Private same-owner transport cleanup only. The supervised TLS child
        # calls this AFTER authentic peer alert + actual reciprocal alert flush.
        self._own()
        end = min(end, self._metadata.deadline, time.monotonic()+1)
        future = None
        try:
            with self._cv:
                if (time.monotonic() >= end or not self._live() or self._in
                        or self._failed or self._ended or self._closed
                        or self._closing_original_stop or self._network_lost
                        or self._network_loss_denied or self._ws is None):
                    raise CarrierUnavailable()
                self._closing_original_stop = True
            future = asyncio.run_coroutine_threadsafe(self._finish_original_close(end), self._loop)
            future.result(timeout=max(0, end-time.monotonic()))
            self._thread.join(max(0, end-time.monotonic()))
            if self._thread.is_alive() or not self._original_close_ok or self._failed:
                raise CarrierUnavailable('carrier_cleanup')
            with self._cv:
                self._closed = True
                self._cv.notify_all()
            self._read.close()
            self._write.close()
        except BaseException:
            if future is not None:
                future.cancel()
            self.close()  # Existing abort/join path; no graceful result on error.
            raise CarrierUnavailable('carrier_cleanup') from None

    def close(self):
        self._own()
        with self._cv:
            self._closed = True
            self._in = b''
            self._signal()
            self._cv.notify_all()
        def abort():
            if self._ws is not None:
                self._ws.transport.abort()
            if self._task is not None:
                self._task.cancel()
        try:
            if self._loop is not None and not self._loop.is_closed():
                self._loop.call_soon_threadsafe(abort)
        except RuntimeError:
            pass
        if self._thread is not None and self._thread.ident is not None:
            self._thread.join(2)
            if self._thread.is_alive():
                raise CarrierUnavailable('carrier_cleanup')
        self._read.close()
        self._write.close()

