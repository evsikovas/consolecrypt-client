"""Trusted reader boundaries; fake sources make no authority claims."""

from contextlib import contextmanager
from dataclasses import replace
import io
import json
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cc_agent import AgentRuntime, Manifest, Principal, create_server


AGENT = "be42e704-c1ac-4c4f-9dc5-b08d384e6ab0"
HOST = "36fdf84b-e33a-46f3-b9b4-e301215b1dd3"
RESOURCE = "818fa242-3be2-49fc-bc08-e6d729287e4f"
COLLECTION = "bf4601b3-8c9f-40bc-aa50-65fb974b4b8b"


def manifest():
    return Manifest.from_dict({"schema_version": 1, "agent_id": AGENT, "host_id": HOST,
        "port": 0, "history_resource_id": RESOURCE, "history": {"max_bytes": 32768}})


class Reader:
    def __init__(self):
        self.binding = dict(agent_id=AGENT, host_id=HOST, history_resource_id=RESOURCE,
            collection_resource_id=COLLECTION, plan_sha256="a" * 64,
            agent_reserved_bytes=32768, snapshot_reserved_bytes=65536,
            total_max_bytes=268435456, max_points=5760, max_age_seconds=86400,
            history_kind="fixed_os_snapshot", schema_version=1)
        self.data = dict(agent_id=AGENT, host_id=HOST, resource_id=RESOURCE,
            history_kind="fixed_os_snapshot", schema_version=1, points=[])
        self.calls = 0
        self.validations = []
        self.action = None
        self.validation_action = None

    def descriptor(self):
        return dict(self.binding)

    def validate_runtime_binding(self, **kwargs):
        self.validations.append(kwargs)
        if self.validation_action:
            self.validation_action()

    def query(self):
        self.calls += 1
        if self.action:
            self.action()
        return self.data


@contextmanager
def serving(runtime):
    # Exercise the actual HTTP parser/handler without host socket permission.
    # Real socket + signed proof acceptance belongs to the separate Linux fixture.
    with patch("cc_agent.service._BoundedServer",
            side_effect=lambda address, handler: SimpleNamespace(handler=handler)):
        server = create_server(runtime)
    def get(path="/v1/history", body=None):
        body = body or b""
        wire = (f"GET {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {len(body)}\r\n\r\n".encode()
                + body)
        class Connection:
            def __init__(self):
                self.output = bytearray()
                self.input = io.BytesIO(wire)
            def makefile(self, *args):
                return io.BytesIO(wire)
            def settimeout(self, *args):
                pass
            def sendall(self, data):
                self.output.extend(data)
            def recv_into(self, buffer):
                return self.input.readinto(buffer)
        connection = Connection()
        server.handler(connection, ("127.0.0.1", 0), server)
        headers, payload = bytes(connection.output).split(b"\r\n\r\n", 1)
        return int(headers.split(b" ")[1]), json.loads(payload)
    yield get


class HistoryReaderTests(unittest.TestCase):
    def runtime(self, reader, **kwargs):
        self.admissions, self.audits = [], []
        def authorize(request):
            self.admissions.append(request)
            return Principal(AGENT, frozenset({"telemetry.read"}))
        return AgentRuntime(manifest(), history_reader=reader, authorize=authorize,
            audit=lambda event: self.audits.append(event), **kwargs)

    def test_invalid_binding_before_local_history_side_effects(self):
        cases = [("agent_id", HOST), ("host_id", AGENT), ("history_resource_id", COLLECTION),
            ("collection_resource_id", "00000000-0000-0000-0000-000000000000"),
            ("collection_resource_id", RESOURCE), ("plan_sha256", "A" * 64),
            ("agent_reserved_bytes", True), ("agent_reserved_bytes", 65536),
            ("snapshot_reserved_bytes", 0), ("snapshot_reserved_bytes", 268435456),
            ("total_max_bytes", 268435457), ("max_points", True), ("max_points", 5761),
            ("max_age_seconds", 0), ("history_kind", "agent_gauges"), ("schema_version", True)]
        for key, value in cases:
            with self.subTest(key=key, value=value):
                reader = Reader()
                reader.binding[key] = value
                with patch("cc_agent.service.LocalHistory") as writer:
                    with self.assertRaises(ValueError):
                        self.runtime(reader)
                    writer.assert_not_called()
        reader = Reader()
        reader.binding["extra"] = "not allowed"
        with self.assertRaises(ValueError):
            self.runtime(reader)

    def test_default_agent_resource_and_heartbeat_collision_rejected(self):
        for resource in (str(uuid.uuid5(uuid.UUID(AGENT), "history-v1")), manifest().heartbeat_resource_id):
            cfg = replace(manifest(), history_resource_id=resource)
            reader = Reader()
            reader.binding["history_resource_id"] = resource
            with self.subTest(resource=resource), self.assertRaises(ValueError):
                AgentRuntime(cfg, history_reader=reader)
        reader = Reader()
        reader.binding["agent_reserved_bytes"] = 268435456
        with self.assertRaises(ValueError):
            AgentRuntime(replace(manifest(), history_max_bytes=268435456), history_reader=reader)

    def test_source_revalidated_before_listen(self):
        reader = Reader()
        runtime = self.runtime(reader)
        reader.binding["plan_sha256"] = "b" * 64
        with patch("cc_agent.service._BoundedServer") as server:
            with self.assertRaises(ValueError):
                create_server(runtime)
            server.assert_not_called()

    def test_prelisten_validator_manifest_replacement_never_binds(self):
        reader = Reader()
        runtime = self.runtime(reader)
        reader.validation_action = lambda: setattr(runtime, "manifest",
            replace(runtime.manifest, bind_host="0.0.0.0"))
        with patch("cc_agent.service._BoundedServer") as server:
            with self.assertRaises(ValueError):
                create_server(runtime)
            server.assert_not_called()

    def test_writer_preserved_read_selection_and_single_admission_audit(self):
        reader = Reader()
        runtime = self.runtime(reader, clock=lambda: 100.0)
        self.assertIs(runtime.history_reader, reader)
        with self.assertRaises(AttributeError):
            runtime.history_reader = Reader()
        runtime.sample()
        self.assertEqual(len(runtime.history.query()["points"]), 1)
        with serving(runtime) as get:
            runtime.history = Mock()  # The writer cannot substitute the selected datasource.
            reader.query = Mock(side_effect=AssertionError("method replaced after capture"))
            status, body = get()
            self.assertEqual((status, body), (200, reader.data))
            runtime.history.query.assert_not_called()
        self.assertEqual(reader.calls, 1)
        self.assertEqual(len(self.admissions), 1)
        self.assertEqual(self.admissions[0].resource, RESOURCE)
        self.assertEqual([e["reason"] for e in self.audits], ["admitted"])
        self.assertEqual(len(reader.validations), 4)  # Constructor, prelisten, pre/post read.
        self.assertEqual(reader.validations[-1], dict(agent_id=AGENT, host_id=HOST,
            history_resource_id=RESOURCE, agent_history_budget_bytes=32768))

    def test_reader_failures_fixed_503_without_retry_or_fallback(self):
        for case in ("exception", "nonfinite", "oversize", "postdrift", "wrongresource", "toomany"):
            with self.subTest(case=case):
                reader = Reader()
                if case == "exception":
                    reader.action = Mock(side_effect=OSError("https://SECRET/?token=CANARY"))
                elif case == "nonfinite":
                    reader.data["points"] = [{"timestamp": float("nan")}]
                elif case == "oversize":
                    reader.data["untrusted"] = "CANARY" * 50000
                elif case == "postdrift":
                    reader.action = lambda: reader.binding.update(plan_sha256="b" * 64)
                elif case == "wrongresource":
                    reader.data["resource_id"] = COLLECTION
                else:
                    reader.data["points"] = [{}] * 101
                runtime = self.runtime(reader)
                runtime.history.query = Mock(side_effect=AssertionError("fallback"))
                with serving(runtime) as get:
                    self.assertEqual(get(), (503, {"error": "history_unavailable"}))
                self.assertEqual(reader.calls, 1)
                self.assertEqual(len(self.admissions), 1)
                self.assertEqual([e["reason"] for e in self.audits], ["admitted"])
                runtime.history.query.assert_not_called()

    def test_before_read_drift_or_validation_failure_queries_no_data(self):
        for case in ("descriptor", "manifest", "source"):
            with self.subTest(case=case):
                reader = Reader()
                runtime = self.runtime(reader)
                with serving(runtime) as get:
                    if case == "descriptor":
                        reader.binding["max_points"] -= 1
                    elif case == "manifest":
                        runtime.manifest = replace(runtime.manifest, history_max_bytes=65536)
                    else:
                        # Captured method reads live source state on every validation.
                        reader.validations = None
                    self.assertEqual(get(), (503, {"error": "history_unavailable"}))
                self.assertEqual(reader.calls, 0)
                self.assertEqual(len(self.admissions), 1)

    def test_during_query_live_manifest_replacement_is_unavailable(self):
        reader = Reader()
        runtime = self.runtime(reader)
        reader.action = lambda: setattr(runtime, "manifest",
            replace(runtime.manifest, history_max_bytes=65536))
        with serving(runtime) as get:
            self.assertEqual(get(), (503, {"error": "history_unavailable"}))
            # Route/resource selection stays fixed even after live drift.
            runtime.manifest = replace(runtime.manifest, history_resource_id=COLLECTION)
            self.assertEqual(get(), (503, {"error": "history_unavailable"}))
        self.assertEqual(reader.calls, 1)
        self.assertEqual([r.resource for r in self.admissions], [RESOURCE, RESOURCE])
        self.assertEqual([e["reason"] for e in self.audits], ["admitted", "admitted"])

    def test_denial_audit_failure_and_invalid_routes_do_not_read(self):
        reader = Reader()
        runtime = self.runtime(reader)
        with serving(runtime) as get:
            self.assertEqual(get("/v1/history?limit=1")[0], 403)
            self.assertEqual(get(body=b"x")[0], 400)
            self.assertEqual(len(self.admissions), 0)
            runtime.authorize = lambda request: None
            self.assertEqual(get()[0], 403)
            runtime.authorize = lambda request: Principal(AGENT, frozenset({"telemetry.read"}))
            runtime.audit = lambda event: False
            self.assertEqual(get(), (503, {"error": "audit_unavailable"}))
        self.assertEqual(reader.calls, 0)

    def test_default_history_response_stays_local(self):
        runtime = self.runtime(None, clock=lambda: 100.0)
        runtime.sample()
        expected = runtime.history.query()
        with serving(runtime) as get:
            self.assertEqual(get(), (200, expected))
        self.assertIsNone(runtime.history_reader)


if __name__ == "__main__":
    unittest.main()
