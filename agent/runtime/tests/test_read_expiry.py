"""Local verified-expiry observations; no issuer, transport or authority."""

from dataclasses import FrozenInstanceError
from http.server import BaseHTTPRequestHandler
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cc_agent import AgentRuntime, Manifest, Principal
from cc_agent.read_expiry import AuthorizationUnavailable, MAX_TIMESTAMP, ReadExpiryGuard
from test_history_reader import serving


AGENT = "be42e704-c1ac-4c4f-9dc5-b08d384e6ab0"
HOST = "36fdf84b-e33a-46f3-b9b4-e301215b1dd3"


class ReadExpiryTests(unittest.TestCase):
    def principal(self, at=100, expires=200):
        return Principal(AGENT, frozenset({"telemetry.read"}),
                         verified_at=at, verified_expires_at=expires)

    def runtime(self, principal):
        self.admissions, self.audits = [], []
        def authorize(request):
            self.admissions.append(request)
            return principal
        cfg = Manifest.from_dict(dict(schema_version=1, agent_id=AGENT, host_id=HOST, port=0))
        runtime = AgentRuntime(cfg, authorize=authorize, audit=lambda e: self.audits.append(e),
                               clock=lambda: 123.0)
        runtime.history.query = Mock(wraps=runtime.history.query)
        runtime.heartbeat = Mock(wraps=runtime.heartbeat)
        return runtime

    def assert_one_admission_audit(self):
        self.assertEqual(len(self.admissions), 1)
        self.assertEqual([e["reason"] for e in self.audits], ["admitted"])

    def test_principal_keyword_fields_preserve_legacy_positional_frozen_api(self):
        principal = Principal(AGENT, frozenset({"telemetry.read"}))
        self.assertIsNone(principal.verified_at)
        self.assertIsNone(principal.verified_expires_at)
        with self.assertRaises(TypeError):
            Principal(AGENT, frozenset(), 100, 200)
        with self.assertRaises(FrozenInstanceError):
            principal.verified_at = 100
        with patch("cc_agent.read_expiry._os_now", side_effect=AssertionError("legacy has no clock guarantee")) as clock:
            ReadExpiryGuard(None, None).check()
            clock.assert_not_called()

    def test_exact_carrier_pair_validation_and_timestamp_bounds(self):
        cases = [(None, 200), (100, None), (True, 200), (100, False), (100.0, 200),
            (100, 200.0), ("100", 200), (100, "CANARY"), ([], 200), (100, {}),
            (-1, 200), (0, MAX_TIMESTAMP + 1), (10**1000, MAX_TIMESTAMP),
            (100, 100), (200, 100), (MAX_TIMESTAMP, MAX_TIMESTAMP)]
        with patch("cc_agent.read_expiry._os_now") as clock:
            for at, expires in cases:
                with self.subTest(at=at, expires=expires), self.assertRaises(AuthorizationUnavailable):
                    ReadExpiryGuard(at, expires)
            clock.assert_not_called()
        with patch("cc_agent.read_expiry._os_now", return_value=0.125):
            self.assertIsNone(ReadExpiryGuard(0, MAX_TIMESTAMP).check())

    def test_raw_fractional_floor_expiry_and_invalid_os_clock(self):
        with patch("cc_agent.read_expiry._os_now", side_effect=[100.75, 100.50]):
            guard = ReadExpiryGuard(100, 200)
            with self.assertRaises(AuthorizationUnavailable):
                guard.check()
        for now in (None, True, "CANARY", [], {}, float("nan"), float("inf"),
                    -float("inf"), -1, MAX_TIMESTAMP + 1, 10**1000, 99.9, 200, 201):
            with self.subTest(now=now), patch("cc_agent.read_expiry._os_now", return_value=now):
                with self.assertRaises(AuthorizationUnavailable) as failure:
                    ReadExpiryGuard(100, 200)
                self.assertEqual(str(failure.exception), "authorization_unavailable")
        with patch("cc_agent.read_expiry._os_now", side_effect=OSError("CANARY secret URL")):
            with self.assertRaises(AuthorizationUnavailable):
                ReadExpiryGuard(100, 200)

    def test_successive_checks_advance_raw_highwater(self):
        with patch("cc_agent.read_expiry._os_now", side_effect=[100.25, 100.75, 100.50]):
            guard = ReadExpiryGuard(100, 200)
            self.assertIsNone(guard.check())
            with self.assertRaises(AuthorizationUnavailable):
                guard.check()

    def test_valid_carrier_both_endpoints_and_legacy_none_keep_one_admission(self):
        for path in ("/v1/history", "/v1/heartbeat"):
            for legacy in (False, True):
                with self.subTest(path=path, legacy=legacy):
                    runtime = self.runtime(self.principal(None, None) if legacy else self.principal())
                    with patch("cc_agent.read_expiry._os_now", side_effect=[150.25, 150.50]) as clock:
                        with serving(runtime) as get:
                            status, data = get(path)
                        self.assertEqual(status, 200)
                        self.assertNotIn("authorization_unavailable", json.dumps(data))
                        self.assertEqual(clock.call_count, 0 if legacy else 2)
                    self.assert_one_admission_audit()
                    self.assertEqual(runtime.history.query.call_count, int(path == "/v1/history"))
                    self.assertEqual(runtime.heartbeat.call_count, int(path == "/v1/heartbeat"))

    def test_malformed_or_expired_prequery_carrier_returns_fixed_503_after_one_audit(self):
        for principal, now in ((self.principal(None, 200), 150), (self.principal(True, 200), 150),
                               (self.principal(), 200), (self.principal(), 99),
                               (self.principal(), float("nan"))):
            with self.subTest(principal=principal, now=now):
                runtime = self.runtime(principal)
                with patch("cc_agent.read_expiry._os_now", return_value=now):
                    with serving(runtime) as get:
                        self.assertEqual(get(), (503, {"error": "authorization_unavailable"}))
                self.assert_one_admission_audit()
                runtime.history.query.assert_not_called()
                runtime.heartbeat.assert_not_called()

    def test_expiry_during_data_query_or_heartbeat_suppresses_data_without_retry(self):
        for path in ("/v1/history", "/v1/heartbeat"):
            with self.subTest(path=path):
                runtime, now = self.runtime(self.principal()), [150.0]
                if path == "/v1/history":
                    def query():
                        now[0] = 200.0
                        return {"points": [{"CANARY": "private result"}]}
                    runtime.history.query.side_effect = query
                else:
                    def collector_status():
                        now[0] = 200.0
                        return {"state": "disabled"}
                    runtime.collector_status = collector_status
                with patch("cc_agent.read_expiry._os_now", side_effect=lambda: now[0]):
                    with serving(runtime) as get:
                        self.assertEqual(get(path), (503, {"error": "authorization_unavailable"}))
                self.assert_one_admission_audit()
                self.assertEqual(runtime.history.query.call_count, int(path == "/v1/history"))
                self.assertEqual(runtime.heartbeat.call_count, int(path == "/v1/heartbeat"))

    def test_final_guard_after_serialization_before_first_response_status(self):
        for after in (200.0, 149.75):
            for path in ("/v1/history", "/v1/heartbeat"):
                with self.subTest(after=after, path=path):
                    runtime, now = self.runtime(self.principal()), [150.25]
                    original_dump, original_status = json.dumps, BaseHTTPRequestHandler.send_response
                    statuses, encoded_candidates = [], []
                    def encode(data, *args, **kwargs):
                        result = original_dump(data, *args, **kwargs)
                        if "error" not in data:
                            encoded_candidates.append(result)
                            now[0] = after
                        return result
                    def response_status(handler, code, *args, **kwargs):
                        statuses.append(code)
                        return original_status(handler, code, *args, **kwargs)
                    with patch("cc_agent.read_expiry._os_now", side_effect=lambda: now[0]), \
                            patch("cc_agent.service.json.dumps", side_effect=encode), \
                            patch.object(BaseHTTPRequestHandler, "send_response", response_status):
                        with serving(runtime) as get:
                            self.assertEqual(get(path), (503, {"error": "authorization_unavailable"}))
                    self.assertEqual(len(encoded_candidates), 1)
                    self.assertEqual(statuses, [503])
                    self.assert_one_admission_audit()

    def test_post_clock_exception_noecho_and_runtime_clock_cannot_extend_expiry(self):
        for clock in ([150, OSError("CANARY secret URL")], [200]):
            runtime = self.runtime(self.principal())
            runtime.clock = lambda: 101  # Telemetry time cannot extend the guard.
            with patch("cc_agent.read_expiry._os_now", side_effect=clock):
                with serving(runtime) as get:
                    self.assertEqual(get(), (503, {"error": "authorization_unavailable"}))
            self.assert_one_admission_audit()
            self.assertEqual(runtime.history.query.call_count, int(len(clock) == 2))

    def test_audit_failure_precedes_clock_guard_and_data(self):
        runtime = self.runtime(self.principal())
        runtime.audit = lambda event: self.audits.append(event) or False
        with patch("cc_agent.read_expiry._os_now") as clock:
            with serving(runtime) as get:
                self.assertEqual(get(), (503, {"error": "audit_unavailable"}))
            clock.assert_not_called()
        self.assert_one_admission_audit()
        runtime.history.query.assert_not_called()


if __name__ == "__main__":
    unittest.main()
