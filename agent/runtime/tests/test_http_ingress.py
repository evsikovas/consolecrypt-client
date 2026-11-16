"""Actual stdlib parser with bounded fake socket reads, no network authority."""

import io
import json
from http.server import BaseHTTPRequestHandler
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cc_agent import AgentRuntime, Manifest, Principal, create_server
from cc_agent.http_ingress import HeadReader, MAX_HEAD_BYTES


AGENT = "be42e704-c1ac-4c4f-9dc5-b08d384e6ab0"
HOST = "36fdf84b-e33a-46f3-b9b4-e301215b1dd3"


class Connection:
    def __init__(self, wire, now, *, fragment=None, step=0):
        self.input, self.output = io.BytesIO(wire), bytearray()
        self.now, self.fragment, self.step = now, fragment, step
        self.reads, self.timeouts, self.writes = [], [], []

    def makefile(self, *args):
        self.initial_file = io.BytesIO()
        return self.initial_file

    def settimeout(self, value):
        self.timeouts.append(value)

    def recv_into(self, buffer):
        self.reads.append((len(buffer), self.input.tell(), self.timeouts[-1]))
        self.now[0] += self.step
        return self.input.readinto(buffer if self.fragment is None else buffer[:self.fragment])

    def sendall(self, data):
        self.writes.append((bytes(data), self.timeouts[-1]))
        self.output.extend(data)


class IngressTests(unittest.TestCase):
    def runtime(self):
        self.authorizations, self.audits = [], []
        manifest = Manifest.from_dict(dict(schema_version=1, agent_id=AGENT, host_id=HOST, port=0))
        def authorize(request):
            self.authorizations.append(request)
            return Principal(AGENT, frozenset({"telemetry.read"}))
        runtime = AgentRuntime(manifest, authorize=authorize, audit=lambda event: self.audits.append(event))
        runtime.history.query = Mock(wraps=runtime.history.query)
        return runtime

    def handle(self, runtime, wire, *, now=None, fragment=None, step=0, parser=None):
        now = [0.0] if now is None else now
        connection = Connection(wire, now, fragment=fragment, step=step)
        captured = []
        original = HeadReader.__init__
        def remember(reader, *args, **kwargs):
            original(reader, *args, **kwargs)
            captured.append(reader)
        with patch("cc_agent.service._BoundedServer",
                side_effect=lambda address, handler: SimpleNamespace(handler=handler)):
            server = create_server(runtime)
        with patch("cc_agent.http_ingress.time.monotonic", side_effect=lambda: now[0]), \
                patch.object(HeadReader, "__init__", remember):
            if parser is None:
                server.handler(connection, ("127.0.0.1", 0), server)
            else:
                with patch.object(BaseHTTPRequestHandler, "parse_request", parser):
                    server.handler(connection, ("127.0.0.1", 0), server)
        self.assertTrue(connection.initial_file.closed)
        self.assertEqual(len(captured), 1)
        self.assertTrue(captured[0].closed)
        for requested, consumed, _ in connection.reads:
            self.assertLessEqual(requested, MAX_HEAD_BYTES - consumed)
        self.assertLessEqual(connection.input.tell(), MAX_HEAD_BYTES)
        return connection, captured[0]

    def response(self, connection):
        headers, payload = bytes(connection.output).split(b"\r\n\r\n", 1)
        self.assertTrue(headers.startswith(b"HTTP/1."))
        return int(headers.split(b" ")[1]), json.loads(payload)

    def no_admission(self, runtime):
        self.assertEqual(self.authorizations, [])
        self.assertEqual(self.audits, [])
        runtime.history.query.assert_not_called()

    def test_exact_raw_budget_complete_head_admits_once_without_eof_probe(self):
        prefix = b"GET /v1/history HTTP/1.1\r\nHost: localhost\r\nX-ConsoleCrypt-Proof: CANARY\r\nX-Pad: "
        suffix = b"\r\n\r\n"
        wire = prefix + b"x" * (MAX_HEAD_BYTES - len(prefix) - len(suffix)) + suffix
        runtime = self.runtime()
        connection, reader = self.handle(runtime, wire)
        self.assertEqual(reader.received, MAX_HEAD_BYTES)
        self.assertEqual(self.response(connection)[0], 200)
        self.assertEqual(len(self.authorizations), 1)
        self.assertEqual([e["reason"] for e in self.audits], ["admitted"])
        runtime.history.query.assert_called_once_with()
        self.assertNotIn(b"CANARY", connection.output)

    def test_literal_http10_supported_and_pipelined_second_request_not_dispatched(self):
        runtime = self.runtime()
        wire = (b"GET /v1/heartbeat HTTP/1.0\r\n\r\n"
                b"GET /v1/history HTTP/1.1\r\n\r\n")
        connection, _ = self.handle(runtime, wire)
        self.assertEqual(self.response(connection)[0], 200)
        self.assertEqual(len(self.authorizations), 1)
        self.assertEqual(self.authorizations[0].path, "/v1/heartbeat")
        self.assertEqual([e["reason"] for e in self.audits], ["admitted"])
        runtime.history.query.assert_not_called()

    def test_long_line_and_aggregate_overflow_stop_at_raw_cap(self):
        for wire in (b"GET /" + b"CANARY" * 5000,
                     b"GET /v1/history HTTP/1.1\r\nX-Proof: " + b"CANARY" * 5000 + b"\r\n\r\n",
                     b"GET /v1/history HTTP/1.1\r\n" + (b"X-Chunk: " + b"x" * 800 + b"\r\n") * 30 + b"\r\n"):
            with self.subTest(length=len(wire)):
                runtime = self.runtime()
                connection, reader = self.handle(runtime, wire)
                self.assertEqual(self.response(connection), (431, {"error": "request_headers_too_large"}))
                self.assertEqual(reader.received, MAX_HEAD_BYTES)
                self.assertEqual(len(connection.writes), 1)
                self.assertNotIn(b"CANARY", connection.output)
                self.no_admission(runtime)

    def test_stdlib_header_count_error_has_fixed_json_not_raw_parser_detail(self):
        runtime = self.runtime()
        connection, _ = self.handle(runtime,
            b"GET /v1/history HTTP/1.1\r\n" + b"X-CANARY: secret\r\n" * 101 + b"\r\n")
        self.assertEqual(self.response(connection), (431, {"error": "invalid_request"}))
        self.assertEqual(len(connection.writes), 1)
        self.assertNotIn(b"CANARY", connection.output)
        self.no_admission(runtime)

    def test_drip_uses_absolute_deadline_not_per_read_timeout(self):
        runtime = self.runtime()
        connection, _ = self.handle(runtime, b"GET /v1/history HTTP/1.1\r\n\r\n", fragment=1, step=0.6)
        self.assertEqual(connection.output, b"")
        self.assertEqual(len(connection.reads), 4)
        for observed, expected in zip((r[2] for r in connection.reads), (2.0, 1.4, 0.8, 0.2)):
            self.assertAlmostEqual(observed, expected)
        self.no_admission(runtime)

    def test_buffered_parse_crossing_deadline_does_not_dispatch(self):
        runtime, now = self.runtime(), [0.0]
        original = BaseHTTPRequestHandler.parse_request
        def delayed_parse(handler):
            result = original(handler)
            now[0] = 2.0
            return result
        connection, _ = self.handle(runtime, b"GET /v1/history HTTP/1.1\r\n\r\n",
                                    now=now, parser=delayed_parse)
        self.assertEqual(connection.output, b"")
        self.no_admission(runtime)

    def test_eof_missing_head_terminator_and_http09_never_admit(self):
        for wire in (b"", b"GET /v1/history HTTP/1.1\r\nX-ConsoleCrypt-Proof: CANARY\r\n",
                     b"GET /v1/history HTTP/1.1\r\nX-ConsoleCrypt-Proof: CANARY",
                     b"GET /v1/history\r\n"):
            with self.subTest(wire=wire):
                runtime = self.runtime()
                connection, _ = self.handle(runtime, wire)
                if wire:
                    self.assertEqual(self.response(connection), (400, {"error": "invalid_request"}))
                    self.assertEqual(len(connection.writes), 1)
                else:
                    self.assertEqual(connection.output, b"")
                self.assertNotIn(b"CANARY", connection.output)
                self.no_admission(runtime)

    def test_parser_errors_force_framing_and_never_echo_path_or_proof(self):
        cases = [(b"GET /CANARY INVALID\r\nX-Proof: SECRET\r\n\r\n", 400),
                 (b"GET /CANARY HTTP/9.9\r\nX-Proof: SECRET\r\n\r\n", 505),
                 (b"GET /CANARY HTTP/1.2\r\nX-Proof: SECRET\r\n\r\n", 400),
                 (b"GET /CANARY HTTP/1.01\r\nX-Proof: SECRET\r\n\r\n", 400),
                 (b"FUTURE /CANARY HTTP/1.1\r\nX-Proof: SECRET\r\n\r\n", 501)]
        for wire, expected in cases:
            with self.subTest(expected=expected):
                runtime = self.runtime()
                connection, _ = self.handle(runtime, wire)
                self.assertEqual(self.response(connection), (expected, {"error": "invalid_request"}))
                self.assertEqual(len(connection.writes), 1)
                self.assertNotIn(b"CANARY", connection.output)
                self.assertNotIn(b"SECRET", connection.output)
                self.no_admission(runtime)

    def test_parser_failure_after_expiry_does_not_attempt_error_write(self):
        runtime = self.runtime()
        connection, _ = self.handle(runtime, b"BAD CANARY INVALID\r\n", step=2.0)
        self.assertEqual(connection.output, b"")
        self.assertEqual(connection.writes, [])
        self.no_admission(runtime)

    def test_error_frame_is_one_write_with_only_remaining_header_time(self):
        runtime = self.runtime()
        connection, _ = self.handle(runtime, b"GET /CANARY INVALID\r\n", step=1.75)
        self.assertEqual(self.response(connection), (400, {"error": "invalid_request"}))
        self.assertEqual(len(connection.writes), 1)
        self.assertAlmostEqual(connection.writes[0][1], 0.25)
        self.no_admission(runtime)


if __name__ == "__main__":
    unittest.main()
