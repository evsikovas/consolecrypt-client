"""Trusted local ordering stubs, explicitly not Native Permit evidence."""

from dataclasses import asdict, is_dataclass, replace
import copy
from http.server import BaseHTTPRequestHandler
import json
from pathlib import Path
import pickle
import sys
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cc_agent import AgentRuntime, Manifest, Principal
from cc_agent.read_access import ReadAccessGuard, ReadAccessUnavailable, ReadAdmission
from test_history_reader import serving


AGENT = "be42e704-c1ac-4c4f-9dc5-b08d384e6ab0"
HOST = "36fdf84b-e33a-46f3-b9b4-e301215b1dd3"
DEFAULT = object()


class StubGuard(ReadAccessGuard):
    """Only an ordering stub; no Native handle, authority or transport."""
    def __init__(self):
        self.events, self.contexts = [], []
        self.phase, self.retired = "begun", 0
        self.check_action = self.finalize_action = None
        self.check_result = self.finalize_result = None
        self.retire_error = None
        self.expected = None
        self._canary = "PRIVATE_HANDLE_CANARY"

    def check(self, **context):
        self.events.append("check")
        self.contexts.append(("check", context))
        if self.phase != "begun" or self.expected is not None and context != self.expected:
            raise ReadAccessUnavailable()
        self.phase = "checked"
        if self.check_action:
            self.check_action()
        return self.check_result

    def finalize(self, **context):
        self.events.append("finalize")
        self.contexts.append(("finalize", context))
        if self.phase != "checked" or self.expected is not None and context != self.expected:
            raise ReadAccessUnavailable()
        self.phase = "finalized"
        if self.finalize_action:
            self.finalize_action()
        return self.finalize_result

    def retire(self):
        self.events.append("retire")
        self.retired += 1
        self.phase = "retired"
        if self.retire_error:
            raise self.retire_error


class ReadAccessTests(unittest.TestCase):
    def runtime(self, guard=None, principal=DEFAULT, begin_error=None):
        if principal is DEFAULT:
            principal = Principal(AGENT, frozenset({"telemetry.read"}),
                                  verified_at=100, verified_expires_at=200)
        self.events = guard.events if guard is not None else []
        self.requests, self.audits = [], []
        admission = ReadAdmission(principal, guard) if guard is not None else principal
        def authorize(request):
            self.events.append("begin")
            self.requests.append(request)
            if begin_error:
                raise begin_error
            return admission
        def audit(event):
            self.events.append("audit")
            self.audits.append(dict(event))
        cfg = Manifest.from_dict(dict(schema_version=1, agent_id=AGENT, host_id=HOST, port=0))
        runtime = AgentRuntime(cfg, authorize=authorize, audit=audit, clock=lambda: 123.0)
        def query():
            self.events.append("query")
            return {"points": []}
        runtime.history.query = Mock(side_effect=query)
        original = runtime.heartbeat
        def heartbeat():
            self.events.append("heartbeat")
            return original()
        runtime.heartbeat = Mock(side_effect=heartbeat)
        return runtime

    def get(self, runtime, path="/v1/history"):
        with serving(runtime) as get:
            return get(path)

    def assert_admitted_once(self):
        self.assertEqual(len(self.requests), 1)
        self.assertEqual([e["reason"] for e in self.audits], ["admitted"])

    def test_wrapper_is_immutable_nonserializing_redacted_and_requires_typed_guard(self):
        guard = StubGuard()
        principal = Principal(AGENT, frozenset())
        admission = ReadAdmission(principal, guard)
        self.assertFalse(is_dataclass(admission))
        self.assertIs(admission.principal, principal)
        self.assertIs(admission.guard, guard)
        with self.assertRaises(AttributeError):
            admission.guard = StubGuard()
        with self.assertRaises(AttributeError):
            del admission._guard
        with self.assertRaises(TypeError):
            asdict(admission)
        with self.assertRaises(TypeError):
            json.dumps(admission)
        for value in (admission, guard):
            for copier in (copy.copy, copy.deepcopy, pickle.dumps):
                with self.assertRaises(TypeError):
                    copier(value)
        with self.assertRaises(TypeError):
            ReadAdmission(principal, True)
        with self.assertRaises(TypeError):
            ReadAccessGuard()
        self.assertNotIn(guard._canary, repr(admission) + repr(guard) + repr(principal))

    def test_success_orders_actual_context_audit_expiry_query_encoding_finalize_and_retire(self):
        guard = StubGuard()
        runtime = self.runtime(guard)
        original_dump, original_status = json.dumps, BaseHTTPRequestHandler.send_response
        def clock():
            self.events.append("clock")
            return 150.0
        def encode(*args, **kwargs):
            result = original_dump(*args, **kwargs)
            self.events.append("encode")
            return result
        def status(handler, code, *args, **kwargs):
            self.events.append("status:" + str(code))
            return original_status(handler, code, *args, **kwargs)
        with patch("cc_agent.read_expiry._os_now", side_effect=clock), \
                patch("cc_agent.service.json.dumps", side_effect=encode), \
                patch.object(BaseHTTPRequestHandler, "send_response", status):
            self.assertEqual(self.get(runtime), (200, {"points": []}))
        self.assertEqual(self.events, ["begin", "audit", "clock", "check", "clock", "query",
            "encode", "clock", "finalize", "clock", "status:200", "retire"])
        context = dict(agent_id=AGENT, operation="telemetry.read", resource=runtime.manifest.history_resource_id)
        self.assertEqual(guard.contexts, [("check", context), ("finalize", context)])
        self.assert_admitted_once()
        self.assertEqual(guard.retired, 1)
        self.assertNotIn(guard._canary, json.dumps(self.audits))

    def test_heartbeat_and_legacy_modes_remain_distinct(self):
        guard = StubGuard()
        runtime = self.runtime(guard)
        with patch("cc_agent.read_expiry._os_now", return_value=150):
            self.assertEqual(self.get(runtime, "/v1/heartbeat")[0], 200)
        self.assertEqual(guard.contexts[-1][1]["resource"], runtime.manifest.heartbeat_resource_id)
        self.assertEqual(runtime.heartbeat.call_count, 1)
        runtime.history.query.assert_not_called()
        self.assertEqual(guard.retired, 1)
        legacy = self.runtime(principal=Principal(AGENT, frozenset({"telemetry.read"})))
        legacy.stop_event.set()  # New guard checkpoints do not change legacy behavior.
        with patch("cc_agent.read_expiry._os_now", side_effect=AssertionError("legacy clock")):
            self.assertEqual(self.get(legacy)[0], 200)
        self.assert_admitted_once()

    def test_typed_begin_unavailable_is_coarse_unavailable_not_fake_admitted(self):
        runtime = self.runtime(begin_error=ReadAccessUnavailable())
        self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
        self.assertEqual(len(self.requests), 1)
        self.assertEqual([e["reason"] for e in self.audits], ["operation_unavailable"])
        runtime.history.query.assert_not_called()
        denied = self.runtime(principal=None)
        self.assertEqual(self.get(denied), (403, {"error": "scope_denied"}))
        self.assertEqual([e["reason"] for e in self.audits], ["scope_denied"])

    def test_bad_principal_audit_failure_and_missing_bounded_metadata_all_retire(self):
        cases = [(None, 403, "scope_denied"),
                 (Principal(HOST, frozenset({"telemetry.read"})), 403, "scope_denied"),
                 (Principal(AGENT, frozenset()), 403, "scope_denied"),
                 (Principal(AGENT, frozenset({"telemetry.read"})), 503, "authorization_unavailable")]
        for principal, status, reason in cases:
            with self.subTest(principal=principal):
                guard = StubGuard()
                runtime = self.runtime(guard, principal)
                self.assertEqual(self.get(runtime), (status, {"error": reason}))
                self.assertEqual(guard.contexts, [])
                self.assertEqual(guard.retired, 1)
                self.assertEqual(self.audits[0]["agent_id"], AGENT)
                runtime.history.query.assert_not_called()
        guard = StubGuard()
        runtime = self.runtime(guard)
        runtime.audit = lambda e: self.audits.append(e) or False
        self.assertEqual(self.get(runtime), (503, {"error": "audit_unavailable"}))
        self.assertEqual(guard.contexts, [])
        self.assertEqual(guard.retired, 1)
        self.assert_admitted_once()

    def test_guard_strict_none_return_and_wrong_resource_do_not_grant(self):
        for phase in ("check", "finalize"):
            for result in (True, False, {}, "PRIVATE_HANDLE_CANARY"):
                with self.subTest(phase=phase, result=result):
                    guard = StubGuard()
                    setattr(guard, phase + "_result", result)
                    runtime = self.runtime(guard)
                    with patch("cc_agent.read_expiry._os_now", return_value=150):
                        self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
                    self.assert_admitted_once()
                    self.assertEqual(runtime.history.query.call_count, int(phase == "finalize"))
                    self.assertEqual(guard.retired, 1)
        guard = StubGuard()
        guard.expected = dict(agent_id=AGENT, operation="telemetry.read", resource=HOST)
        runtime = self.runtime(guard)
        with patch("cc_agent.read_expiry._os_now", return_value=150):
            self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
        runtime.history.query.assert_not_called()
        self.assertEqual(guard.retired, 1)

    def test_live_agent_drift_after_begin_is_503_with_startup_agent_audit_and_retire(self):
        guard = StubGuard()
        runtime = self.runtime(guard)
        original_begin = runtime.authorize
        def begin_then_change_agent(request):
            result = original_begin(request)
            runtime.manifest = replace(runtime.manifest, agent_id=HOST)
            return result
        runtime.authorize = begin_then_change_agent
        self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
        self.assert_admitted_once()
        self.assertEqual(self.audits[0]["agent_id"], AGENT)
        self.assertEqual(guard.contexts, [])
        self.assertEqual(guard.retired, 1)
        runtime.history.query.assert_not_called()
        unavailable = self.runtime()
        def unavailable_after_change(request):
            self.requests.append(request)
            unavailable.manifest = replace(unavailable.manifest, agent_id=HOST)
            raise ReadAccessUnavailable()
        unavailable.authorize = unavailable_after_change
        self.assertEqual(self.get(unavailable), (503, {"error": "authorization_unavailable"}))
        self.assertEqual([e["reason"] for e in self.audits], ["operation_unavailable"])
        self.assertEqual(self.audits[0]["agent_id"], AGENT)

    def test_guard_exception_before_or_after_data_never_echoes_and_retires(self):
        for phase in ("check", "finalize"):
            with self.subTest(phase=phase):
                guard = StubGuard()
                def fail():
                    raise OSError("PRIVATE_HANDLE_CANARY http://secret/")
                setattr(guard, phase + "_action", fail)
                runtime = self.runtime(guard)
                with patch("cc_agent.read_expiry._os_now", return_value=150):
                    self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
                self.assertEqual(runtime.history.query.call_count, int(phase == "finalize"))
                self.assertEqual(guard.retired, 1)
                self.assert_admitted_once()

    def test_source_and_encoding_failure_retire_without_finalize(self):
        for failure in ("source", "encode"):
            guard = StubGuard()
            runtime = self.runtime(guard)
            runtime.history.query.side_effect = OSError("PRIVATE_HANDLE_CANARY") if failure == "source" else None
            if failure == "encode":
                runtime.history.query.return_value = {"unencodable": object()}
            with patch("cc_agent.read_expiry._os_now", return_value=150):
                status, data = self.get(runtime)
            self.assertEqual(status, 503)
            self.assertEqual(data, {"error": "history_unavailable" if failure == "source" else "authorization_unavailable"})
            self.assertNotIn("finalize", guard.events)
            self.assertEqual(guard.retired, 1)
            self.assert_admitted_once()

    def test_new_guard_checkpoints_cancel_or_deny_manifest_drift(self):
        for phase in ("initial", "check", "query", "finalize"):
            for mutation in ("cancel", "manifest"):
                with self.subTest(phase=phase, mutation=mutation):
                    guard = StubGuard()
                    runtime = self.runtime(guard)
                    def mutate():
                        if mutation == "cancel":
                            runtime.stop_event.set()
                        else:
                            runtime.manifest = replace(runtime.manifest, history_max_bytes=32768)
                    if phase == "initial":
                        # Set after startup selection was captured by create_server.
                        with serving(runtime) as get, patch("cc_agent.read_expiry._os_now", return_value=150):
                            mutate()
                            self.assertEqual(get(), (503, {"error": "authorization_unavailable"}))
                    else:
                        if phase == "query":
                            def query():
                                mutate()
                                return {"points": []}
                            runtime.history.query.side_effect = query
                        else:
                            setattr(guard, phase + "_action", mutate)
                        with patch("cc_agent.read_expiry._os_now", return_value=150):
                            self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
                    self.assertEqual(runtime.history.query.call_count, int(phase in {"query", "finalize"}))
                    self.assertEqual(guard.retired, 1)
                    self.assert_admitted_once()

    def test_expiry_after_native_finalize_reply_suppresses_first_200(self):
        guard, now = StubGuard(), [150.0]
        runtime = self.runtime(guard)
        guard.finalize_action = lambda: now.__setitem__(0, 200.0)
        statuses, original = [], BaseHTTPRequestHandler.send_response
        def status(handler, code, *args, **kwargs):
            statuses.append(code)
            return original(handler, code, *args, **kwargs)
        with patch("cc_agent.read_expiry._os_now", side_effect=lambda: now[0]), \
                patch.object(BaseHTTPRequestHandler, "send_response", status):
            self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
        self.assertEqual(statuses, [503])
        self.assertEqual([p for p, _ in guard.contexts], ["check", "finalize"])
        self.assertEqual(guard.retired, 1)

    def test_disconnect_and_cleanup_error_cannot_reopen_or_replace_outcome(self):
        guard = StubGuard()
        runtime = self.runtime(guard)
        with patch("cc_agent.read_expiry._os_now", return_value=150), \
                patch("socketserver._SocketWriter.write", side_effect=BrokenPipeError("closed peer")):
            with self.assertRaises(BrokenPipeError):
                self.get(runtime)
        self.assertEqual(guard.retired, 1)
        for deny in (False, True):
            guard = StubGuard()
            guard.retire_error = OSError("PRIVATE_HANDLE_CANARY")
            if deny:
                guard.check_result = True
            runtime = self.runtime(guard)
            with patch("cc_agent.read_expiry._os_now", return_value=150):
                self.assertEqual(self.get(runtime)[0], 503 if deny else 200)
            self.assertEqual(guard.retired, 1)

    def test_reusing_same_wrapper_does_not_reopen_one_shot_guard(self):
        guard = StubGuard()
        runtime = self.runtime(guard)
        with patch("cc_agent.read_expiry._os_now", return_value=150), serving(runtime) as get:
            self.assertEqual(get()[0], 200)
            self.assertEqual(get(), (503, {"error": "authorization_unavailable"}))
        self.assertEqual(runtime.history.query.call_count, 1)
        self.assertEqual(guard.retired, 2)  # One cleanup attempt per returned wrapper.


if __name__ == "__main__":
    unittest.main()
