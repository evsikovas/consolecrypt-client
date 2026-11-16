"""DRAFT component IO tests: scripted carrier + real owned socketpairs.

No WSS/SSH authentication, Actor, Node admission, remote execution or NAT
deployment proof is fabricated. The public bridge still has no endpoint input.
"""
import base64
import errno
from pathlib import Path
import socket
import ssl
import sys
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

AGENT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(AGENT), str(AGENT / 'runtime')]
from cc_agent import outbound_control as control
from cc_agent.wss_carrier import CarrierUnavailable, RelayEndpoint, RelayOffer


class ScriptedCarrier:
    """Data-only wake/IO adapter; never represents actual WSS or authorization."""
    def __init__(self, send_error=None, short=False, cleanup_error=False):
        self._owner = threading.current_thread()
        self._cv = threading.Condition()
        self._failed = self._closed = self._ended = False
        self.read, self.wake = socket.socketpair()
        self.read.setblocking(False); self.wake.setblocking(False)
        self.incoming = b''
        self.sent = []
        self.send_error, self.short, self.cleanup_error = send_error, short, cleanup_error
        self.sent_event = threading.Event()
        self.close_owner = None

    def _own(self):
        if self._owner is not threading.current_thread():
            raise RuntimeError('wrong_component_owner')

    def fileno(self):
        self._own(); return self.read.fileno()

    def inject(self, raw):
        with self._cv:
            if self.incoming:
                raise AssertionError('one_test_handoff')
            self.incoming = raw; self.wake.send(b'1')

    def recv(self, count):
        self._own()
        with self._cv:
            if not self.incoming:
                raise socket.timeout()
            raw, self.incoming = self.incoming[:count], self.incoming[count:]
            if not self.incoming:
                self.read.recv(1)
            return raw

    def send(self, raw):
        self._own(); self.sent.append(bytes(raw)); self.sent_event.set()
        if self.send_error:
            raise self.send_error
        return len(raw) - 1 if self.short else len(raw)

    def close(self):
        self._own(); self.close_owner = threading.current_thread()
        self._closed = True; self.read.close(); self.wake.close()
        if self.cleanup_error:
            raise CarrierUnavailable('carrier_cleanup')


class ShortLocalSocket:
    def __init__(self, stream):
        self.stream = stream
        self.calls = 0

    def fileno(self): return self.stream.fileno()
    def setblocking(self, value): self.stream.setblocking(value)
    def recv(self, count): return self.stream.recv(count)
    def close(self): self.stream.close()

    def send(self, value):
        self.calls += 1
        if self.calls % 2 == 0:
            raise BlockingIOError()
        return self.stream.send(value[:3])


class Clock:
    def __init__(self):
        self.current = time.monotonic()
    def monotonic(self): return self.current


class OutboundControlTests(unittest.TestCase):
    def offer(self, maximum=4096):
        capability = 'ccrl_' + base64.urlsafe_b64encode(b'\x21' * 32).decode('ascii').rstrip('=')
        return RelayOffer(RelayEndpoint('wss://example.invalid', str(uuid.uuid4()), '127.0.0.1'),
            capability, received_at=time.monotonic(), expires_in_seconds=60,
            idle_seconds=30, max_ciphertext_bytes=maximum)

    def start(self, *, maximum=4096, short_local=False, clock=None, **carrier_options):
        left, right = socket.socketpair(); right.settimeout(2)
        local = ShortLocalSocket(left) if short_local else left
        offer, stop, opened = self.offer(maximum), threading.Event(), threading.Event()
        state = {}
        def factory(actual, context, *, stop):
            actual._take() # Test-only one-use behavior; no actual WSS connection.
            state['carrier'] = ScriptedCarrier(**carrier_options)
            return state['carrier']
        def connected(meter, event):
            opened.set(); return local
        def run():
            try:
                state['result'] = control.bridge_once(offer, ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT), stop)
            except BaseException as error:
                state['error'] = error
        patches = [patch.object(control, 'WssCarrier', side_effect=factory),
                   patch.object(control, '_connect_ssh', side_effect=connected)]
        if clock is not None:
            patches.append(patch.object(control, 'time', clock))
        for item in patches: item.start()
        worker = threading.Thread(target=run, name='owned-outbound-component', daemon=False)
        def cleanup():
            stop.set()
            if worker.ident is not None: worker.join(3)
            right.close(); left.close()
            for item in reversed(patches): item.stop()
            self.assertFalse(worker.is_alive(), 'owned component must retire')
        self.addCleanup(cleanup)
        worker.start()
        self.assertTrue(opened.wait(1))
        return state, right, stop, worker, offer, local

    def joined(self, state, worker):
        worker.join(3); self.assertFalse(worker.is_alive())
        self.assertNotIn('error', state)
        result = state['result']
        self.assertIs(result.resume_authority, False)
        self.assertIs(result.remote_drain_observed, False)
        self.assertIs(result.forwarding_counters_exact, False)
        return result

    def test_real_socket_short_writes_and_backpressure_preserve_both_byte_streams(self):
        state, peer, stop, worker, _, local = self.start(short_local=True)
        raw = b'one bounded inbound chunk with many partial local writes'
        state['carrier'].inject(raw)
        # Keep both directions active while the local writer repeatedly blocks.
        peer.sendall(b'SSH application bytes')
        received = bytearray()
        while len(received) < len(raw): received.extend(peer.recv(len(raw)-len(received)))
        self.assertEqual(bytes(received), raw)
        self.assertTrue(state['carrier'].sent_event.wait(1))
        self.assertEqual(b''.join(state['carrier'].sent), b'SSH application bytes')
        stop.set(); result = self.joined(state, worker)
        self.assertGreater(local.calls, 2)
        self.assertEqual(result.bytes_from_relay, len(raw))
        self.assertEqual(result.bytes_to_ssh, len(raw))
        self.assertTrue(result.ssh_closed and result.carrier_closed)
        self.assertIs(state['carrier'].close_owner, worker)

    def test_actual_local_eof_closes_both_owned_sockets_without_half_close_replay(self):
        state, peer, _, worker, _, _ = self.start()
        peer.shutdown(socket.SHUT_WR)
        result = self.joined(state, worker)
        self.assertEqual(result.end, 'ssh_eof')
        self.assertTrue(result.ssh_closed and result.carrier_closed)
        self.assertEqual(peer.recv(1), b'')

    def test_exact_read_budget_is_charged_before_forward_then_both_sides_close(self):
        state, peer, _, worker, _, _ = self.start(maximum=5)
        state['carrier'].inject(b'12345')
        received = bytearray()
        while len(received) < 5:
            part = peer.recv(5-len(received)); self.assertTrue(part); received.extend(part)
        self.assertEqual(bytes(received), b'12345')
        result = self.joined(state, worker)
        self.assertEqual(result.end, 'budget_exhausted')
        self.assertEqual((result.bytes_from_relay, result.bytes_to_ssh), (5, 5))
        self.assertEqual(result.bytes_from_ssh, 0)
        self.assertTrue(result.ssh_closed and result.carrier_closed)

    def test_stop_event_retirement_does_not_create_a_resume_result(self):
        state, _, stop, worker, _, _ = self.start()
        stop.set(); result = self.joined(state, worker)
        self.assertEqual(result.end, 'stopped')
        self.assertTrue(result.ssh_closed and result.carrier_closed)

    def test_original_absolute_expiry_remains_terminal_with_no_reconnect(self):
        clock = Clock()
        state, _, _, worker, offer, _ = self.start(clock=clock)
        clock.current = offer.deadline
        result = self.joined(state, worker)
        self.assertEqual(result.end, 'expired')
        self.assertEqual(state['carrier'].sent, [])
        with self.assertRaises(CarrierUnavailable): offer._take()

    def test_no_data_or_readiness_poll_renews_original_idle_window(self):
        clock = Clock()
        state, _, _, worker, _, _ = self.start(clock=clock)
        clock.current += 30
        result = self.joined(state, worker)
        self.assertEqual(result.end, 'idle')
        self.assertEqual(result.bytes_from_ssh + result.bytes_from_relay, 0)

    def test_generic_network_error_is_redacted_and_never_replayed_or_resumable(self):
        state, peer, _, worker, offer, _ = self.start(send_error=OSError('private error text'))
        peer.sendall(b'once only')
        self.assertTrue(state['carrier'].sent_event.wait(1))
        result = self.joined(state, worker)
        self.assertEqual(result.end, 'transport_failed')
        self.assertEqual(state['carrier'].sent, [b'once only'])
        self.assertNotIn('private error text', repr(result))
        with self.assertRaises(CarrierUnavailable): offer._take()

    def test_unexpected_short_carrier_send_discards_suffix_instead_of_replay(self):
        state, peer, _, worker, _, _ = self.start(short=True)
        peer.sendall(b'never retransmit an uncertain suffix')
        result = self.joined(state, worker)
        self.assertEqual(result.end, 'transport_failed')
        self.assertEqual(len(state['carrier'].sent), 1)
        self.assertEqual(result.bytes_to_relay, 0) # Lower bound, no delivery claim.

    def test_cleanup_join_failure_stays_unknown_even_if_local_fd_closed(self):
        state, _, stop, worker, _, _ = self.start(cleanup_error=True)
        stop.set(); result = self.joined(state, worker)
        self.assertEqual((result.end, result.trigger), ('cleanup_unknown', 'stopped'))
        self.assertIs(result.carrier_closed, False)
        self.assertIs(result.ssh_closed, True)

    def test_local_connect_has_only_fixed_endpoint_and_closes_refused_socket(self):
        class Refused:
            destination = None
            closed = False
            def setblocking(self, value): self.blocking = value
            def connect_ex(self, destination): self.destination = destination; return errno.ECONNREFUSED
            def close(self): self.closed = True
        local = Refused()
        with patch.object(control.socket, 'socket', return_value=local):
            with self.assertRaises(control._End) as caught:
                control._connect_ssh(control._Meter(self.offer()), threading.Event())
        self.assertEqual(caught.exception.reason, 'connect_failed')
        self.assertEqual(local.destination, ('127.0.0.1', 22))
        self.assertTrue(local.closed)


if __name__ == '__main__':
    unittest.main()
