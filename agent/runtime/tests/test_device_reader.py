"""Actual parser + trusted ordering stubs, not Native device authorization."""
import base64
import copy
from dataclasses import replace
import io
import json
from pathlib import Path
import pickle
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cc_agent import AgentRuntime, Manifest, Principal, create_server
from cc_agent.device_reader import (BoundDeviceReaderProvider, DeviceReaderAdmission,
    DeviceReaderAuthRequest, DeviceReaderPrincipal, DeviceReaderProvider,
    build_reader_targets, encode_device_response)
from cc_agent.history_reader import BoundHistoryReader
from cc_agent.read_access import ReadAccessUnavailable, ReadAdmission
from test_history_reader import Reader
from test_read_access import StubGuard

AGENT = "44444444-4444-4444-8444-444444444444"
HOST = "33333333-3333-4333-8333-333333333333"
DEVICE = "99999999-9999-4999-8999-999999999999"
READ = "66666666-6666-4666-8666-666666666666"
CANARY = b"PRIVATE_READER_CREDENTIAL_CANARY"


def manifest():
    return Manifest.from_dict(dict(schema_version=1, agent_id=AGENT, host_id=HOST,
                                  port=0, history_resource_id=READ, history=dict(max_bytes=32768)))


def principal(target, **overrides):
    fields = dict(reader_device_id=DEVICE, target_agent_id=AGENT, operation="telemetry.read",
        resource=target.resource, target_descriptor_hash=target.digest,
        verified_at=100, verified_expires_at=200)
    return DeviceReaderPrincipal(**{**fields, **overrides})


class StubBound(BoundDeviceReaderProvider):
    def __init__(self, targets, events):
        self._targets, self.events = targets, events
        self.requests, self.guards = [], []
        self.result = self.action = self.guard_action = self.targets_action = None

    @property
    def targets(self):
        if self.targets_action:
            self.targets_action()
        return self._targets

    def begin(self, request):
        self.events.append("begin")
        self.requests.append(request)
        if self.action:
            self.action()
        if self.result:
            return self.result(request)
        guard = StubGuard()
        guard.events = self.events
        self.guards.append(guard)
        if self.guard_action:
            self.guard_action(guard)
        return DeviceReaderAdmission(principal(request.target), guard)


class StubProvider(DeviceReaderProvider):
    def __init__(self, events):
        self.events = events

    def bind_targets(self, targets):
        self.bound = StubBound(targets, self.events)
        return self.bound


def parser(runtime):
    # The real handler/stdlib parser runs on a bounded fake socket, no network.
    with patch("cc_agent.service._BoundedServer",
            side_effect=lambda address, handler: SimpleNamespace(handler=handler)):
        server = create_server(runtime)
    def read(path="/v1/history", headers=None, body=b"", method="GET"):
        if headers is None:
            headers = [("X-ConsoleCrypt-Reader", base64.urlsafe_b64encode(CANARY).decode().rstrip("="))]
        head = (f"{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {len(body)}\r\n"
                + "".join(f"{k}: {v}\r\n" for k, v in headers) + "\r\n").encode()
        wire = head + body
        class Connection:
            def __init__(self):
                self.input, self.output = io.BytesIO(wire), bytearray()
            def makefile(self, *args):
                return io.BytesIO(wire)
            def settimeout(self, *args):
                pass
            def sendall(self, data):
                self.output.extend(data)
            def recv_into(self, data):
                return self.input.readinto(data)
        connection = Connection()
        server.handler(connection, ("127.0.0.1", 1), server)
        response, raw = bytes(connection.output).split(b"\r\n\r\n", 1)
        return int(response.split(b" ")[1]), json.loads(raw)
    return read


class DeviceReaderTests(unittest.TestCase):
    def runtime(self, *, reader=None, enabled=True, audit_ok=True):
        self.events, self.audits = [], []
        self.provider = StubProvider(self.events)
        self.node = Mock(return_value=Principal(AGENT, frozenset({"telemetry.read"})))
        def audit(event):
            self.events.append("audit")
            self.audits.append(dict(event))
            return audit_ok
        runtime = AgentRuntime(manifest(), authorize=self.node, audit=audit, clock=lambda: 150,
            reader_provider=self.provider if enabled else None, history_reader=reader)
        runtime.history.query = Mock(side_effect=lambda: self.events.append("query") or {"points": []})
        return runtime

    def get(self, runtime, *args, **kwargs):
        with patch("cc_agent.read_expiry._os_now", return_value=150.25):
            return parser(runtime)(*args, **kwargs)

    def test_full_public_targets_match_fixed_os_golden_and_detach(self):
        reader = Reader()
        reader.binding.update(agent_id=AGENT, host_id=HOST, history_resource_id=READ,
            collection_resource_id="55555555-5555-4555-8555-555555555555", total_max_bytes=98304,
            plan_sha256="ab" * 32)
        targets = build_reader_targets(manifest(), BoundHistoryReader(reader, manifest()))
        hb, fixed = [t.descriptor() for t in targets]
        self.assertEqual((len(hb), len(hb["dataset"]["descriptor"])), (8, 5))
        self.assertEqual(len(fixed["dataset"]["descriptor"]), 12)
        self.assertEqual(targets[1].digest, "c407219c4d4960600f9505f5a8d3cd980d9b5fba1322de345fe4167123450083")
        fixed["dataset"]["descriptor"]["max_points"] = 1
        self.assertEqual(targets[1].descriptor()["dataset"]["descriptor"]["max_points"], 5760)
        local = build_reader_targets(manifest())[1].descriptor()
        self.assertEqual(len(local["dataset"]["descriptor"]), 9)
        self.assertEqual((local["maxQueryPoints"], local["dataset"]["descriptor"]["max_points"]), (1000, 5760))
        self.assertEqual((hb["maxResponseBytes"], fixed["maxResponseBytes"]), (65536, 262144))

    def test_values_redact_credentials_and_cannot_mutate_copy_or_pickle(self):
        target = build_reader_targets(manifest())[1]
        request = DeviceReaderAuthRequest(target, CANARY)
        subject = principal(target)
        admission = DeviceReaderAdmission(subject, StubGuard())
        for value in (target, request, subject, admission):
            self.assertNotIn(CANARY.decode(), repr(value))
            self.assertNotIn(DEVICE, repr(value))
            for copier in (copy.copy, copy.deepcopy, pickle.dumps):
                with self.assertRaises(TypeError):
                    copier(value)
            with self.assertRaises(AttributeError):
                value.extra = True
        with self.assertRaises(TypeError):
            DeviceReaderAdmission(Principal(AGENT, frozenset()), StubGuard())
        for fields in (dict(reader_device_id="00000000-0000-0000-0000-000000000000"),
                       dict(verified_at=True), dict(verified_expires_at=100)):
            with self.assertRaises(ValueError):
                principal(target, **fields)

    def test_bad_provider_or_bind_type_denies_before_state_creation(self):
        with patch("cc_agent.service.LocalHistory") as history:
            with self.assertRaises(TypeError):
                AgentRuntime(manifest(), reader_provider=lambda: True)
            class Bad(StubProvider):
                def bind_targets(self, targets):
                    return True
            with self.assertRaises(TypeError):
                AgentRuntime(manifest(), reader_provider=Bad([]))
            history.assert_not_called()

    def test_success_is_separate_one_admission_audit_guard_and_exact_context(self):
        runtime = self.runtime()
        status, _ = self.get(runtime)
        self.assertEqual(status, 200)
        self.assertEqual(self.events, ["begin", "audit", "check", "query", "finalize", "retire"])
        self.node.assert_not_called()
        self.assertEqual([e["reason"] for e in self.audits], ["admitted"])
        bound = self.provider.bound
        self.assertEqual(bound.requests[0].envelope, CANARY)
        guard = bound.guards[0]
        self.assertEqual(guard.retired, 1)
        for _, context in guard.contexts:
            self.assertEqual(context, dict(agent_id=AGENT, operation="telemetry.read", resource=READ))

    def test_node_header_stays_legacy_and_reader_family_never_falls_back(self):
        runtime = self.runtime()
        self.assertEqual(self.get(runtime, headers=[("X-ConsoleCrypt-Proof", "unchanged_node_syntax")])[0], 200)
        self.assertEqual(self.node.call_args.args[0].headers["x-consolecrypt-proof"], "unchanged_node_syntax")
        self.assertEqual(self.provider.bound.requests, [])
        runtime = self.runtime()
        self.provider.bound.result = lambda request: None
        self.assertEqual(self.get(runtime), (403, {"error": "scope_denied"}))
        self.node.assert_not_called()
        self.assertEqual([e["reason"] for e in self.audits], ["scope_denied"])

    def test_absent_provider_and_unknown_begin_are_fixed_503_without_node(self):
        for enabled in (False, True):
            runtime = self.runtime(enabled=enabled)
            if enabled:
                self.provider.bound.action = Mock(side_effect=RuntimeError(CANARY.decode()))
            status, data = self.get(runtime)
            self.assertEqual((status, data), (503, {"error": "authorization_unavailable"}))
            self.node.assert_not_called()
            self.assertEqual([e["reason"] for e in self.audits], ["operation_unavailable"])
            runtime.history.query.assert_not_called()

    def test_mixed_duplicate_and_noncanonical_headers_deny_before_either_provider(self):
        good = base64.urlsafe_b64encode(CANARY).decode().rstrip("=")
        cases = [[("X-ConsoleCrypt-Reader", good), ("X-ConsoleCrypt-Proof", "node")],
                 [("X-ConsoleCrypt-Reader", good), ("x-consolecrypt-reader", good)]]
        cases += [[("X-ConsoleCrypt-Reader", raw)] for raw in ("", "Zg==", "Zh", "/w", "a")]
        for headers in cases:
            with self.subTest(headers=headers):
                runtime = self.runtime()
                self.assertEqual(self.get(runtime, headers=headers)[0], 400)
                self.assertEqual(self.provider.bound.requests, [])
                self.node.assert_not_called()

    def test_envelope_exact_limit_and_opaque_bytes(self):
        runtime = self.runtime()
        maximum = base64.urlsafe_b64encode(b"x" * 8192).decode().rstrip("=")
        self.assertEqual(self.get(runtime, headers=[("X-ConsoleCrypt-Reader", maximum)])[0], 200)
        self.assertEqual(len(self.provider.bound.requests[0].envelope), 8192)
        runtime = self.runtime()
        oversized = base64.urlsafe_b64encode(b"x" * 8193).decode().rstrip("=")
        self.assertEqual(self.get(runtime, headers=[("X-ConsoleCrypt-Reader", oversized)])[0], 400)
        self.assertEqual(self.provider.bound.requests, [])

    def test_forbidden_route_query_body_and_method_never_begin(self):
        for options in (dict(path="/v1/history?limit=1"), dict(body=b"{}"), dict(method="POST")):
            runtime = self.runtime()
            self.assertIn(self.get(runtime, **options)[0], (400, 403))
            self.assertEqual(self.provider.bound.requests, [])
            self.node.assert_not_called()

    def test_generic_principal_or_node_admission_is_never_device_authority(self):
        for result in (True, Principal(AGENT, frozenset({"telemetry.read"})),
                       ReadAdmission(Principal(AGENT, frozenset({"telemetry.read"})), StubGuard())):
            runtime = self.runtime()
            self.provider.bound.result = lambda request: result
            self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
            self.node.assert_not_called()
            if isinstance(result, ReadAdmission):
                self.assertEqual(result.guard.retired, 1)

    def test_wrong_device_principal_target_resource_or_hash_is_403_and_retires(self):
        for fields in (dict(target_agent_id=DEVICE), dict(resource=DEVICE), dict(target_descriptor_hash="0" * 64)):
            runtime = self.runtime()
            guard = StubGuard()
            self.provider.bound.result = lambda request: DeviceReaderAdmission(principal(request.target, **fields), guard)
            self.assertEqual(self.get(runtime), (403, {"error": "scope_denied"}))
            self.assertEqual(guard.retired, 1)
            self.assertEqual(guard.contexts, [])
            self.assertEqual(self.audits[0]["agent_id"], AGENT)

    def test_audit_or_source_failure_retires_without_finalize(self):
        runtime = self.runtime(audit_ok=False)
        self.assertEqual(self.get(runtime), (503, {"error": "audit_unavailable"}))
        self.assertEqual(self.events, ["begin", "audit", "retire"])
        runtime = self.runtime()
        runtime.history.query.side_effect = OSError(CANARY.decode())
        self.assertEqual(self.get(runtime), (503, {"error": "history_unavailable"}))
        self.assertEqual(self.events, ["begin", "audit", "check", "retire"])

    def test_device_response_caps_and_point_limits_precede_finalize_node_unchanged(self):
        for path, data in (("/v1/heartbeat", {"huge": "x" * 65536}),
                           ("/v1/history", {"points": [], "huge": "x" * 262144}),
                           ("/v1/history", {"points": [None] * 1001}),
                           ("/v1/history", {"points": [], "bad": float("nan")})):
            runtime = self.runtime()
            if path.endswith("heartbeat"):
                runtime.heartbeat = Mock(return_value=data)
            else:
                runtime.history.query = Mock(return_value=data)
            self.assertEqual(self.get(runtime, path), (503, {"error": "authorization_unavailable"}))
            self.assertNotIn("finalize", self.events)
            self.assertEqual(self.provider.bound.guards[0].retired, 1)
        runtime = self.runtime()
        runtime.heartbeat = Mock(return_value={"huge": "x" * 65536})
        self.assertEqual(self.get(runtime, "/v1/heartbeat", headers=[("X-ConsoleCrypt-Proof", "node")])[0], 200)

    def test_encoded_utf8_limit_is_exact_and_query_cap_does_not_truncate(self):
        target = build_reader_targets(manifest())[0]
        # ASCII JSON string quotes cost two bytes; equality is allowed.
        self.assertEqual(len(encode_device_response("x" * 65534, target)), 65536)
        with self.assertRaises(ReadAccessUnavailable):
            encode_device_response("x" * 65535, target)
        with self.assertRaises(ReadAccessUnavailable):
            encode_device_response("\U0001f600" * 6000, target)
        data = {"points": [None] * 1001}
        with self.assertRaises(ReadAccessUnavailable):
            encode_device_response(data, build_reader_targets(manifest())[1])
        self.assertEqual(len(data["points"]), 1001)

    def test_manifest_drift_during_begin_audits_startup_agent_and_never_queries(self):
        runtime = self.runtime()
        self.provider.bound.action = lambda: setattr(runtime, "manifest", replace(runtime.manifest, agent_id=DEVICE))
        self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
        self.assertEqual([e["agent_id"] for e in self.audits], [AGENT])
        self.assertEqual(self.events, ["begin", "audit", "retire"])
        runtime.history.query.assert_not_called()

    def test_manifest_drift_after_query_stop_and_finalize_failure_suppress_data(self):
        for mode in ("manifest", "stop", "finalize"):
            runtime = self.runtime()
            if mode == "manifest":
                runtime.history.query.side_effect = lambda: (setattr(runtime, "manifest",
                    replace(runtime.manifest, history_max_bytes=65536)) or {"points": []})
            elif mode == "stop":
                runtime.history.query.side_effect = lambda: (runtime.stop_event.set() or {"points": []})
            else:
                self.provider.bound.guard_action = lambda guard: setattr(guard, "finalize_action",
                    Mock(side_effect=ReadAccessUnavailable()))
            self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
            self.assertEqual(self.provider.bound.guards[0].retired, 1)
            self.assertEqual([e["reason"] for e in self.audits], ["admitted"])

    def test_binding_mutation_during_prelisten_validation_never_binds(self):
        runtime = self.runtime()
        self.provider.bound.targets_action = lambda: setattr(runtime, "manifest",
            replace(runtime.manifest, history_max_bytes=65536))
        with patch("cc_agent.service._BoundedServer") as server:
            with self.assertRaises(ValueError):
                create_server(runtime)
            server.assert_not_called()

    def test_prelisten_validator_cannot_replace_selected_local_writer(self):
        runtime = self.runtime()
        self.provider.bound.targets_action = lambda: setattr(runtime, "history", SimpleNamespace(manifest=manifest()))
        with patch("cc_agent.service._BoundedServer") as server:
            with self.assertRaises(ValueError):
                create_server(runtime)
            server.assert_not_called()

    def test_bound_target_drift_before_begin_is_unavailable_without_consumption(self):
        runtime = self.runtime()
        read = parser(runtime)
        self.provider.bound._targets = tuple(reversed(self.provider.bound._targets))
        self.assertEqual(read(), (503, {"error": "authorization_unavailable"}))
        self.assertEqual(self.provider.bound.requests, [])
        self.node.assert_not_called()

    def test_postencoding_and_postfinalize_stop_are_checked_before_first_200(self):
        for phase in ("encoding", "finalize"):
            runtime = self.runtime()
            if phase == "finalize":
                self.provider.bound.guard_action = lambda guard: setattr(guard, "finalize_action", runtime.stop_event.set)
            original = encode_device_response
            def encoded(data, target):
                raw = original(data, target)
                if phase == "encoding":
                    runtime.stop_event.set()
                return raw
            with patch("cc_agent.service.encode_device_response", side_effect=encoded):
                self.assertEqual(self.get(runtime), (503, {"error": "authorization_unavailable"}))
            self.assertEqual("finalize" in self.events, phase == "finalize")
            self.assertEqual(self.provider.bound.guards[0].retired, 1)

    def test_source_descriptor_change_during_device_query_is_no_data(self):
        reader = Reader()
        reader.binding.update(agent_id=AGENT, host_id=HOST, history_resource_id=READ)
        reader.data.update(agent_id=AGENT, host_id=HOST, resource_id=READ)
        runtime = self.runtime(reader=reader)
        reader.action = lambda: reader.binding.update(max_age_seconds=3600)
        self.assertEqual(self.get(runtime), (503, {"error": "history_unavailable"}))
        self.assertEqual(self.provider.bound.guards[0].retired, 1)
        self.assertNotIn("finalize", self.events)

    def test_expiry_during_postfinalize_source_validation_suppresses_first_200(self):
        reader = Reader()
        reader.binding.update(agent_id=AGENT, host_id=HOST, history_resource_id=READ)
        reader.data.update(agent_id=AGENT, host_id=HOST, resource_id=READ)
        runtime = self.runtime(reader=reader)
        observed = {"now": 150.25, "final_validation": False}
        def validate():
            # Only the runtime's fixed OS observation changes. This stub is
            # ordering evidence, not consumption or real Native grant proof.
            if "finalize" in self.events:
                observed.update(now=200.0, final_validation=True)
        reader.validation_action = validate
        read = parser(runtime)
        with patch("cc_agent.read_expiry._os_now", side_effect=lambda: observed["now"]):
            self.assertEqual(read(), (503, {"error": "authorization_unavailable"}))
        self.assertTrue(observed["final_validation"])
        self.assertEqual(self.events.count("finalize"), 1)
        self.assertEqual(self.provider.bound.guards[0].retired, 1)
        self.assertEqual(reader.calls, 1)
        self.assertEqual([e["reason"] for e in self.audits], ["admitted"])


if __name__ == "__main__":
    unittest.main()
