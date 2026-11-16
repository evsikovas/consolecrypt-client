"""One owned WSS bitpipe to the ROOT-installed loopback SSH service.

This module does not authenticate SSH, sign/admit, bootstrap, poll, reconnect or
resume anything. Existing pinned SSH and its fixed subsystem retain authority.
There is deliberately no destination, command, path or protocol-message input.
"""
from dataclasses import dataclass
import errno
import math
import selectors
import socket
import ssl
import threading
import time

from .wss_carrier import CarrierUnavailable, RelayOffer, WssCarrier

_SSH_ENDPOINT = ('127.0.0.1', 22)
_CHUNK = 8192
_TICK = .05
_CONNECT_SECONDS = 1
_ENDS = frozenset({'stopped', 'expired', 'idle', 'budget_exhausted', 'ssh_eof',
                   'relay_eof', 'connect_failed', 'transport_failed', 'cleanup_unknown'})


class OutboundControlUnavailable(OSError):
    def __init__(self):
        super().__init__('outbound_control_input')


class _End(Exception):
    def __init__(self, reason):
        if reason not in _ENDS:
            reason = 'transport_failed'
        self.reason = reason
        super().__init__(reason)


@dataclass(frozen=True, slots=True)
class OutboundObservation:
    """Local lower-bound accounting only; never an SSH ACK or resume permit."""
    end: str
    trigger: str
    bytes_from_ssh: int
    bytes_from_relay: int
    bytes_to_ssh: int
    bytes_to_relay: int
    ssh_closed: bool | None
    carrier_closed: bool | None

    @property
    def resume_authority(self):
        return False

    @property
    def remote_drain_observed(self):
        return False

    @property
    def forwarding_counters_exact(self):
        # A failed send may have reached the network before its error. Counts
        # never imply peer receipt, SSH execution or complete remote delivery.
        return False


class _Meter:
    __slots__ = ('deadline', 'idle', 'maximum', 'last', 'from_ssh', 'from_relay',
                 'to_ssh', 'to_relay')

    def __init__(self, offer):
        deadline, idle, maximum = offer.deadline, offer.idle, offer.max_bytes
        if (type(deadline) not in (int, float) or not math.isfinite(deadline)
                or type(idle) is not int or not 1 <= idle <= 30
                or type(maximum) is not int or not 1 <= maximum <= 8 * 1024**2):
            raise OutboundControlUnavailable()
        self.deadline, self.idle, self.maximum = deadline, idle, maximum
        # Initial idle starts BEFORE carrier open/connect, never after setup.
        # RelayOffer retains its absolute expiry but no receipt-time idle clock.
        self.last = time.monotonic()
        self.from_ssh = self.from_relay = self.to_ssh = self.to_relay = 0

    def check(self, stop):
        now = time.monotonic()
        if stop.is_set():
            raise _End('stopped')
        if now >= self.deadline:
            raise _End('expired')
        if now >= self.last + self.idle:
            raise _End('idle')

    def wait(self, stop, ceiling=None):
        self.check(stop)
        now = time.monotonic()
        end = min(self.deadline, self.last + self.idle,
                  ceiling if ceiling is not None else self.deadline)
        return max(0, min(_TICK, end - now))

    def credit(self):
        return self.maximum - self.from_ssh - self.from_relay

    def received(self, direction, count):
        if type(count) is not int or not 1 <= count <= min(_CHUNK, self.credit()):
            raise _End('transport_failed')
        # Read bytes are charged BEFORE any forwarding. Uncertain/cancelled
        # copies are discarded, never refunded/replayed into another carrier.
        if direction == 'ssh':
            self.from_ssh += count
        else:
            self.from_relay += count
        self.last = time.monotonic()

    def progress(self):
        self.last = time.monotonic()


def _carrier_current(carrier):
    carrier._own()
    # The maintained wake fd stays readable for buffered data. While local SSH
    # is backpressured, do not consume a second chunk just to discover close.
    # This is the same narrow read-only cv state used by maintained TUN code.
    # NEVER hold cv across send/recv (their maintained implementation takes it).
    with carrier._cv:
        if carrier._failed or carrier._closed:
            raise _End('transport_failed')
        if carrier._ended:
            raise _End('relay_eof')


def _connect_ssh(meter, stop):
    stream = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        stream.setblocking(False)
        meter.check(stop)
        status = stream.connect_ex(_SSH_ENDPOINT)
        if status == 0:
            return stream
        if status not in {errno.EINPROGRESS, errno.EWOULDBLOCK, errno.EALREADY, errno.EINTR}:
            raise _End('connect_failed')
        end = min(time.monotonic() + _CONNECT_SECONDS, meter.deadline, meter.last + meter.idle)
        with selectors.DefaultSelector() as selected:
            selected.register(stream, selectors.EVENT_WRITE)
            while True:
                meter.check(stop)
                if time.monotonic() >= end:
                    raise _End('connect_failed')
                if selected.select(meter.wait(stop, end)):
                    if stream.getsockopt(socket.SOL_SOCKET, socket.SO_ERROR) != 0:
                        raise _End('connect_failed')
                    meter.check(stop)
                    return stream
    except BaseException:
        stream.close()
        raise


def _interest(selected, obj, mask, direction):
    try:
        key = selected.get_key(obj)
    except KeyError:
        key = None
    if not mask:
        if key is not None:
            selected.unregister(obj)
    elif key is None:
        selected.register(obj, mask, direction)
    elif key.events != mask:
        selected.modify(obj, mask, direction)


def _pump(carrier, stream, stop, meter):
    """Private component seam; public entry always creates fixed local socket.

    At most one <=8192-byte application chunk per direction. Carrier's own
    maintained bounded parser/TLS buffers are separate and remain unchanged.
    """
    to_ssh = b''
    ssh_offset = 0
    to_relay = None
    stream.setblocking(False)
    with selectors.DefaultSelector() as selected:
        while True:
            meter.check(stop)
            _carrier_current(carrier)
            if to_relay is not None:
                meter.check(stop)
                _carrier_current(carrier)
                count = carrier.send(to_relay)
                # Maintained send is all-or-error, not a readiness/partial-write
                # socket API. Any violation is uncertain: do NOT replay suffix.
                if type(count) is not int or count != len(to_relay):
                    raise _End('transport_failed')
                meter.to_relay += count
                meter.progress()
                to_relay = None
                meter.check(stop)
                _carrier_current(carrier)
            remaining = meter.credit()
            if remaining == 0 and not to_ssh:
                raise _End('budget_exhausted')
            ssh_mask = (selectors.EVENT_READ if remaining > 0 and to_relay is None else 0)
            if to_ssh:
                ssh_mask |= selectors.EVENT_WRITE
            _interest(selected, stream, ssh_mask, 'ssh')
            # fileno is ONLY the carrier READ wake socket, never send writable.
            _interest(selected, carrier, selectors.EVENT_READ if remaining > 0 and not to_ssh else 0, 'relay')
            events = selected.select(meter.wait(stop))
            for key, mask in events:
                meter.check(stop)
                _carrier_current(carrier)
                if key.data == 'ssh':
                    if mask & selectors.EVENT_WRITE and to_ssh:
                        try:
                            count = stream.send(memoryview(to_ssh)[ssh_offset:])
                        except (BlockingIOError, InterruptedError):
                            count = None
                        if count is not None:
                            if type(count) is not int or not 1 <= count <= len(to_ssh) - ssh_offset:
                                raise _End('transport_failed')
                            ssh_offset += count
                            meter.to_ssh += count
                            meter.progress()
                            if ssh_offset == len(to_ssh):
                                to_ssh, ssh_offset = b'', 0
                    if mask & selectors.EVENT_READ and to_relay is None and meter.credit() > 0:
                        try:
                            raw = stream.recv(min(_CHUNK, meter.credit()))
                        except (BlockingIOError, InterruptedError):
                            continue
                        if not raw:
                            raise _End('ssh_eof')
                        meter.received('ssh', len(raw))
                        to_relay = raw
                elif not to_ssh and meter.credit() > 0:
                    try:
                        raw = carrier.recv(min(_CHUNK, meter.credit()))
                    except (socket.timeout, BlockingIOError, InterruptedError):
                        continue
                    if type(raw) is not bytes:
                        raise _End('transport_failed')
                    if not raw:
                        raise _End('relay_eof')
                    meter.received('relay', len(raw))
                    to_ssh, ssh_offset = raw, 0
                meter.check(stop)
                _carrier_current(carrier)


def bridge_once(offer, context, stop):
    """Consume exactly one RelayOffer; all ownership remains on this thread.

    No destination override, process spawning, retry, TLS/SSH payload inspection,
    half-close replay, reusable carrier or resume authority is returned.
    """
    if (type(offer) is not RelayOffer or type(context) is not ssl.SSLContext
            or context.verify_mode != ssl.CERT_REQUIRED or not context.check_hostname
            or type(stop) is not threading.Event):
        raise OutboundControlUnavailable()
    meter = _Meter(offer)
    carrier = stream = None
    reason = 'transport_failed'
    ssh_closed = carrier_closed = None
    cleanup_unknown = False
    try:
        if stop.is_set():
            # Burn this unconnected one-use offer too; clearing a cancellation
            # must not turn this invocation into a retry of its old capability.
            offer._take()
            raise _End('stopped')
        meter.check(stop)
        carrier = WssCarrier(offer, context, stop=stop)
        meter.check(stop)
        _carrier_current(carrier)
        stream = _connect_ssh(meter, stop)
        _pump(carrier, stream, stop, meter)
    except _End as end:
        reason = end.reason
    except Exception as error:
        # CarrierNetworkLoss deliberately receives NO special/resumable status.
        # Never include arbitrary network/library exception text or credentials.
        reason = 'transport_failed'
        if carrier is None and type(error) is CarrierUnavailable and error.args == ('carrier_cleanup',):
            # __init__ retains/cleans its own partial resources. If its owned
            # join explicitly fails, there is no returned carrier to inspect.
            carrier_closed = False
            cleanup_unknown = True
    finally:
        # No half-close: first EOF/stop/error retires both owned endpoints.
        # Attempt each cleanup independently even if the other raises.
        if stream is not None:
            try:
                stream.close()
                ssh_closed = stream.fileno() == -1
                cleanup_unknown |= not ssh_closed
            except BaseException:
                ssh_closed = False
                cleanup_unknown = True
        if carrier is not None:
            try:
                carrier.close() # Maintained abort + exact owned thread join.
                carrier_closed = carrier.fileno() == -1
                cleanup_unknown |= not carrier_closed
            except BaseException:
                carrier_closed = False
                cleanup_unknown = True
    return OutboundObservation('cleanup_unknown' if cleanup_unknown else reason, reason,
        meter.from_ssh, meter.from_relay, meter.to_ssh, meter.to_relay, ssh_closed, carrier_closed)
