"""Classifier DATA faults using maintained Sans-I/O parser; no real transport,
Permit, TLS peer, Node signature, target/drain or acceptance is manufactured.
"""
import asyncio
import os
import sys
import threading
import time
import unittest
from pathlib import Path
from types import SimpleNamespace

AGENT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(AGENT))
sys.path.insert(0, str(AGENT / 'runtime'))
from cc_agent.wss_carrier import CarrierNetworkLoss, CarrierUnavailable, WssCarrier, _BoundedConnection
from websockets.client import ClientProtocol
from websockets.exceptions import ConnectionClosedError, ProtocolError
from websockets.frames import Close
from websockets.protocol import OPEN
from websockets.uri import parse_uri


class NetworkLossClassificationTests(unittest.IsolatedAsyncioTestCase):
    def fixture(self):
        protocol = ClientProtocol(parse_uri('wss://example.invalid/'), state=OPEN)
        connection = _BoundedConnection(protocol)
        carrier = WssCarrier.__new__(WssCarrier)
        carrier._ws = connection
        carrier._closed = False
        carrier._stop = threading.Event()
        carrier._paired = True
        carrier._started = carrier._last = time.monotonic()
        carrier._metadata = SimpleNamespace(deadline=time.monotonic() + 10, idle=10)
        carrier._cv = threading.Condition()
        carrier._pid, carrier._owner = os.getpid(), threading.current_thread()
        carrier._task = asyncio.current_task()
        carrier._network_lost = carrier._network_loss_denied = False
        carrier._network_loss_cancel_requested = False
        carrier._network_loss_cancel_task = None
        carrier._in = b''
        carrier._ended = carrier._failed = False
        return carrier, protocol

    async def test_only_exact_maintained_abnormal_no_close_event_is_a_loss_request(self):
        carrier, protocol = self.fixture()
        error = ConnectionClosedError(None, None)
        self.assertTrue(carrier._network_loss(error))  # Classification only, NOT physical retirement.
        for wrong in [EOFError(), OSError(), RuntimeError(), ConnectionClosedError(Close(1000, ''), None),
                      ConnectionClosedError(None, Close(1002, ''))]:
            self.assertFalse(carrier._network_loss(wrong))
        carrier._ws._denied = True
        self.assertFalse(carrier._network_loss(error))

    async def test_actual_maintained_malformed_frame_cannot_be_reclassified_as_network_loss(self):
        carrier, protocol = self.fixture()
        # A server frame may not be masked. Drive the actual maintained parser.
        protocol.receive_data(b'\x82\x80\x00\x00\x00\x00')
        self.assertIsInstance(protocol.parser_exc, ProtocolError)
        self.assertFalse(carrier._network_loss(ConnectionClosedError(None, None)))

    async def test_original_expiry_owner_stop_and_real_close_frame_have_priority(self):
        carrier, protocol = self.fixture()
        error = ConnectionClosedError(None, None)
        carrier._metadata.deadline = time.monotonic() - 1
        self.assertFalse(carrier._network_loss(error))
        carrier._metadata.deadline = time.monotonic() + 10
        carrier._stop.set()
        self.assertFalse(carrier._network_loss(error))
        carrier._stop.clear()
        protocol.close_rcvd = Close(1000, '')
        self.assertFalse(carrier._network_loss(error))

    async def test_prior_monitor_loss_cannot_mask_later_cap_generic_or_parser_failure(self):
        for failure in [CarrierUnavailable(), RuntimeError('data fault'), ProtocolError('data fault')]:
            carrier, protocol = self.fixture()
            carrier._network_lost = True  # The exact earlier positive fact in the reported race.
            carrier._network_loss_cancel_requested = True
            carrier._network_loss_cancel_task = carrier._task
            carrier._record_failure(failure)  # SAME handler called before _main's async finally.
            self.assertFalse(carrier._network_lost)
            self.assertTrue(carrier._network_loss_denied)
            carrier._record_failure(ConnectionClosedError(None, None))
            self.assertFalse(carrier._network_lost)  # Later close cannot revive UNKNOWN.
            carrier._ended = True
            with self.assertRaises(CarrierUnavailable) as denied:
                carrier.recv(1)  # Actual maintained recv branch, no socket/network fixture.
            self.assertNotIsInstance(denied.exception, CarrierNetworkLoss)


if __name__ == '__main__':
    unittest.main()
