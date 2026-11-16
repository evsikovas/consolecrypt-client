import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cc_agent import AgentRuntime, Manifest, ManifestError, Principal, create_server
from cc_agent.audit import AuditLog


AGENT = "be42e704-c1ac-4c4f-9dc5-b08d384e6ab0"
HOST = "36fdf84b-e33a-46f3-b9b4-e301215b1dd3"


def config(**overrides):
    data = {"schema_version": 1, "agent_id": AGENT, "host_id": HOST, "port": 0}
    data.update(overrides)
    return Manifest.from_dict(data)


class RuntimeTests(unittest.TestCase):
    def test_manifest_rejects_authority_and_external_binds(self):
        for override in ({"scopes": ["jobs.execute"]}, {"bind_host": "0.0.0.0"},
                         {"bind_host": "localhost"}, {"schema_version": True},
                         {"port": True}, {"history": {"max_bytes": 268435457}}):
            with self.subTest(override=override), self.assertRaises(ManifestError):
                config(**override)

    def test_resource_routes_distinct_and_stable(self):
        a, b = config(), config()
        self.assertEqual(a.heartbeat_resource_id, b.heartbeat_resource_id)
        self.assertNotEqual(a.heartbeat_resource_id, a.history_resource_id)

    def test_bounded_history_survives_restart_and_reports_eviction(self):
        now = [100.0]
        cfg = config(history={"max_bytes": 1024, "max_points": 4, "max_age_seconds": 5})
        with tempfile.TemporaryDirectory(prefix="cc-agent-runtime-") as directory:
            runtime = AgentRuntime(cfg, state_dir=directory, clock=lambda: now[0], monotonic=lambda: now[0])
            for i in range(10):
                now[0] = 100.0 + i
                runtime.sample()
            result = runtime.history.query()
            self.assertLessEqual(len(result["points"]), 4)
            self.assertGreater(result["evicted_points"], 0)
            self.assertLessEqual(sum(p.stat().st_size for p in Path(directory).iterdir()), 1024)
            restored = AgentRuntime(cfg, state_dir=directory, clock=lambda: now[0])
            self.assertEqual(restored.history.query()["points"], result["points"])
            now[0] = 200.0
            self.assertEqual(restored.history.query()["points"], [])

    def test_history_query_actual_range_matches_returned_tail(self):
        now = [100.0]
        runtime = AgentRuntime(config(history={"max_bytes": 1048576, "max_points": 1100,
            "max_age_seconds": 86400}), clock=lambda: now[0])
        for index in range(1005):
            now[0] = 100.0 + index
            runtime.history.append(now[0], float(index), None)
        result = runtime.history.query()
        self.assertEqual(len(result["points"]), 1000)
        self.assertTrue(result["truncated"])
        self.assertEqual((result["actual_start"], result["actual_end"]), (105.0, 1104.0))
        self.assertEqual(result["actual_start"], result["points"][0]["timestamp"])
        self.assertEqual(result["actual_end"], result["points"][-1]["timestamp"])
        smaller = runtime.history.query(limit=3)
        self.assertEqual([p["timestamp"] for p in smaller["points"]], [1102.0, 1103.0, 1104.0])
        self.assertEqual((smaller["actual_start"], smaller["actual_end"]), (1102.0, 1104.0))
        self.assertTrue(smaller["truncated"])
        for field in ("stored_bytes", "evicted_points", "budget_bytes", "effective_data_budget_bytes"):
            self.assertEqual(smaller[field], result[field])

    def test_history_query_actual_range_after_age_prune_and_empty(self):
        now = [100.0]
        runtime = AgentRuntime(config(history={"max_bytes": 65536, "max_points": 100,
            "max_age_seconds": 5}), clock=lambda: now[0])
        for index in range(11):
            now[0] = 100.0 + index
            runtime.history.append(now[0], float(index), None)
        tail = runtime.history.query(limit=2)
        self.assertEqual([p["timestamp"] for p in tail["points"]], [109.0, 110.0])
        self.assertEqual((tail["actual_start"], tail["actual_end"]), (109.0, 110.0))
        self.assertTrue(tail["truncated"])
        now[0] = 115.0
        single = runtime.history.query(limit=2)
        self.assertEqual([p["timestamp"] for p in single["points"]], [110.0])
        self.assertEqual((single["actual_start"], single["actual_end"]), (110.0, 110.0))
        self.assertFalse(single["truncated"])
        now[0] = 116.0
        empty = runtime.history.query(limit=2)
        self.assertEqual(empty["points"], [])
        self.assertIsNone(empty["actual_start"])
        self.assertIsNone(empty["actual_end"])
        self.assertFalse(empty["truncated"])
        self.assertGreater(empty["evicted_points"], tail["evicted_points"])

    def test_history_restore_rejects_overflowing_numeric_record_without_repair(self):
        huge = int("9" * 400)
        prefix = {"timestamp": 100.0, "agent_uptime_seconds": 1.0, "agent_collector_running": None}
        for field in ("timestamp", "agent_uptime_seconds"):
            with self.subTest(field=field), tempfile.TemporaryDirectory(prefix="cc-history-numeric-") as directory:
                bad = dict(prefix, timestamp=101.0)
                bad[field] = huge
                record = json.dumps(bad, separators=(",", ":")).encode() + b"\n"
                self.assertLessEqual(len(record), 1024)
                original = json.dumps(prefix, separators=(",", ":")).encode() + b"\n" + record
                path = Path(directory) / "history.jsonl"
                path.write_bytes(original)
                runtime = AgentRuntime(config(), state_dir=directory, clock=lambda: 102.0)
                result = runtime.history.query()
                self.assertEqual(result["storage_state"], "corrupt_history_refused")
                self.assertEqual(result["points"], [])
                self.assertEqual(result["stored_bytes"], 0)
                self.assertEqual(path.read_bytes(), original)
                runtime.sample()  # Memory sampling does not repair/replace corrupt disk data.
                self.assertEqual(runtime.history.query()["storage_state"], "corrupt_history_refused")
                self.assertEqual(path.read_bytes(), original)
                self.assertFalse((Path(directory) / ".history.pending").exists())
            self.assertFalse(Path(directory).exists())

    def test_history_append_overflowing_numeric_rejects_before_mutation(self):
        runtime = AgentRuntime(config(), clock=lambda: 100.0)
        self.assertTrue(runtime.history.append(100.0, 1.0, None))
        before = runtime.history.query()
        huge = int("9" * 400)
        for timestamp, uptime in ((huge, 2.0), (101.0, huge)):
            with self.subTest(timestamp_huge=timestamp == huge):
                with self.assertRaises(ValueError):
                    runtime.history.append(timestamp, uptime, None)
                self.assertEqual(runtime.history.query(), before)
        self.assertTrue(runtime.history.append(101.0, 2.0, True))
        self.assertEqual(len(runtime.history.query()["points"]), 2)

    def test_corrupt_history_preserved_and_write_failure_visible(self):
        with tempfile.TemporaryDirectory(prefix="cc-agent-runtime-") as directory:
            path = Path(directory) / "history.jsonl"
            path.write_bytes(b"bad history\n")
            runtime = AgentRuntime(config(), state_dir=directory)
            runtime.sample()
            self.assertEqual(runtime.history.query()["storage_state"], "corrupt_history_refused")
            self.assertEqual(path.read_bytes(), b"bad history\n")
        with tempfile.TemporaryDirectory(prefix="cc-agent-runtime-") as directory:
            runtime = AgentRuntime(config(), state_dir=directory)
            with patch("cc_agent.history.os.replace", side_effect=OSError("full")):
                runtime.sample()
            self.assertEqual(runtime.history.query()["storage_state"], "write_failed_memory_only")
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_clock_rollback_pauses_append(self):
        now = [100.0]
        runtime = AgentRuntime(config(), clock=lambda: now[0])
        runtime.sample()
        now[0] = 99
        self.assertFalse(runtime.sample())
        self.assertEqual(runtime.history.query()["storage_state"], "clock_rollback_paused")

    def test_unknown_collector_is_missing_gauge_not_zero(self):
        runtime = AgentRuntime(config(), collector_status=lambda: {"state": "unknown"})
        runtime.sample()
        self.assertIsNone(runtime.history.query()["points"][0]["agent_collector_running"])

    def test_pending_history_file_is_preserved_without_unbounded_temp_growth(self):
        with tempfile.TemporaryDirectory(prefix="cc-agent-history-") as directory:
            pending = Path(directory) / ".history.pending"
            pending.write_bytes(b"old crash object")
            runtime = AgentRuntime(config(), state_dir=directory)
            runtime.sample()
            runtime.sample()
            self.assertEqual(pending.read_bytes(), b"old crash object")
            self.assertEqual(len(list(Path(directory).iterdir())), 1)
            self.assertEqual(runtime.history.query()["storage_state"], "write_failed_memory_only")

    def test_local_audit_is_redacted_bounded_and_restartable(self):
        with tempfile.TemporaryDirectory(prefix="cc-agent-audit-") as directory:
            log = AuditLog(directory)
            event = {"timestamp": 1, "agent_id": AGENT, "operation": "unavailable", "resource": None, "reason": "operation_unavailable"}
            for index in range(300):
                self.assertTrue(log({**event, "timestamp": index}))
            self.assertLessEqual(log.status()["events"], 256)
            self.assertLessEqual(log.status()["bytes"], 32768)
            self.assertGreater(log.status()["evicted_events"], 0)
            self.assertFalse(log({**event, "token": "SECRET"}))
            data = (Path(directory) / "audit.jsonl").read_bytes()
            self.assertNotIn(b"SECRET", data)
            self.assertEqual(AuditLog(directory).events(), log.events())
            self.assertFalse(log.status()["tamper_proof"])

    def test_audit_disk_failure_and_corruption_are_visible(self):
        with tempfile.TemporaryDirectory(prefix="cc-agent-audit-") as directory:
            log = AuditLog(directory)
            event = {"timestamp": 1, "agent_id": AGENT, "operation": "unavailable", "resource": None, "reason": "scope_denied"}
            with patch("cc_agent.audit.os.replace", side_effect=OSError("full")):
                self.assertFalse(log(event))
            self.assertEqual(log.status()["state"], "write_failed_memory_only")
            self.assertEqual(list(Path(directory).iterdir()), [])
            (Path(directory) / "audit.jsonl").write_bytes(b"corrupt")
            log = AuditLog(directory)
            self.assertFalse(log(event))
            self.assertEqual(log.status()["state"], "corrupt_audit_refused")

    def test_collector_errors_never_leak_free_text(self):
        runtime = AgentRuntime(config(), collector_status=lambda: {"state": "running", "token": "SECRET", "error": "SECRET", "buffer_bytes": 4})
        self.assertNotIn("SECRET", json.dumps(runtime.heartbeat()))
        self.assertEqual(runtime.heartbeat()["collection"], {"state": "running", "buffer_bytes": 4, "last_scrape_success": None})

    def test_scrape_failure_remains_distinct_from_running_process(self):
        runtime = AgentRuntime(config(), collector_status=lambda: {"state": "running", "last_scrape_success": False})
        runtime.sample()
        self.assertFalse(runtime.heartbeat()["collection"]["last_scrape_success"])
        self.assertEqual(runtime.history.query()["points"][0]["agent_collector_running"], 1)

    def test_queue_drop_counter_does_not_invent_total_loss(self):
        runtime = AgentRuntime(config(), collector_status=lambda: {
            "state": "running", "remote_write_samples_dropped": 0, "remote_write_packets_dropped": 3, "dropped_samples": None})
        status = runtime.heartbeat()["collection"]
        self.assertEqual(status["remote_write_samples_dropped"], 0)
        self.assertEqual(status["remote_write_packets_dropped"], 3)
        self.assertNotIn("dropped_samples", status)

    def test_remote_write_diagnostics_are_fixed_and_never_promote_permission(self):
        runtime = AgentRuntime(config(), collector_status=lambda: {"state": "blocked",
            "remote_write_block_reason": "pinned_collector_buffer_contract_incompatible",
            "remote_write_production_allowed": False})
        status = runtime.heartbeat()["collection"]
        self.assertEqual(status["remote_write_block_reason"], "pinned_collector_buffer_contract_incompatible")
        self.assertIs(status["remote_write_production_allowed"], False)
        for proposed in (True, 0, "false"):
            runtime = AgentRuntime(config(), collector_status=lambda: {"state": "running",
                "remote_write_block_reason": "SECRET_FREEFORM_REASON", "remote_write_production_allowed": proposed})
            status = runtime.heartbeat()["collection"]
            self.assertNotIn("remote_write_production_allowed", status)
            self.assertNotIn("remote_write_block_reason", status)
            self.assertNotIn("SECRET_FREEFORM_REASON", json.dumps(status))

    def test_receiver_response_diagnostic_requires_exact_false_pair(self):
        primary={"state":"blocked","remote_write_block_reason":"pinned_collector_buffer_contract_incompatible",
                 "remote_write_production_allowed":False}
        flag="remote_write_receiver_response_bounded"
        reason="remote_write_receiver_response_block_reason"
        fixed="pinned_receiver_error_body_unbounded"
        runtime=AgentRuntime(config(),collector_status=lambda:{**primary,flag:False,reason:fixed})
        status=runtime.heartbeat()["collection"]
        self.assertIs(status[flag],False)
        self.assertEqual(status[reason],fixed)
        for proposed,explanation in [(True,fixed),(0,fixed),("false",fixed),(False,"https://secret:token@example.invalid/error"),(False,None),(None,fixed)]:
            with self.subTest(proposed=proposed,explanation=explanation):
                runtime=AgentRuntime(config(),collector_status=lambda:{**primary,flag:proposed,reason:explanation})
                status=runtime.heartbeat()["collection"]
                self.assertNotIn(flag,status)
                self.assertNotIn(reason,status)
                self.assertIs(status["remote_write_production_allowed"],False)
                self.assertEqual(status["remote_write_block_reason"],primary["remote_write_block_reason"])
                self.assertNotIn("secret:token",json.dumps(status))

    def test_receiver_destination_diagnostic_only_exact_false_fixed_pair(self):
        flag = "remote_write_receiver_destination_pinned"
        reason = "remote_write_receiver_destination_block_reason"
        fixed = "remote_write_receiver_redirect_destination_not_pinned"
        primary = {"state": "blocked", "remote_write_production_allowed": False,
                   "remote_write_block_reason": "pinned_collector_buffer_contract_incompatible"}
        for proposed in (False, 0, True, "false", None, 1):
            for explanation in (fixed, "unknown", "https://secret:token@example.invalid/redirect", None, {"url": "secret:token"}):
                with self.subTest(flag=proposed, reason=explanation):
                    runtime = AgentRuntime(config(), collector_status=lambda: {
                        **primary, flag: proposed, reason: explanation, "raw_endpoint": "secret:token"})
                    status = runtime.heartbeat()["collection"]
                    if proposed is False and type(explanation) is str and explanation == fixed:
                        self.assertIs(status[flag], False)
                        self.assertEqual(status[reason], fixed)
                    else:
                        self.assertNotIn(flag, status)
                        self.assertNotIn(reason, status)
                    self.assertIs(status["remote_write_production_allowed"], False)
                    self.assertEqual(status["remote_write_block_reason"], primary["remote_write_block_reason"])
                    self.assertNotIn("secret:token", json.dumps(status))

    def test_http_real_socket_default_denial_and_per_request_revocation(self):
        allowed = [False]
        requests = []
        def authorize(request):
            requests.append(request)
            if allowed[0]:
                return Principal(AGENT, frozenset({"telemetry.read"}))
        runtime = AgentRuntime(config(), authorize=authorize)
        runtime.sample()
        server = create_server(runtime)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        def get(path, method="GET", body=None, headers=None):
            connection = http.client.HTTPConnection(*server.server_address, timeout=2)
            connection.request(method, path, body=body, headers=headers or {})
            response = connection.getresponse()
            result = response.status, response.read()
            connection.close()
            return result
        try:
            self.assertEqual(get("/v1/heartbeat")[0], 403)
            allowed[0] = True
            status, body = get("/v1/heartbeat")
            self.assertEqual(status, 200)
            self.assertEqual(json.loads(body)["agent_id"], AGENT)
            self.assertEqual(requests[-1].resource, runtime.manifest.heartbeat_resource_id)
            self.assertEqual(get("/v1/history")[0], 200)
            self.assertEqual(requests[-1].resource, runtime.manifest.history_resource_id)
            for path in ("/v1/jobs", "/v1/tunnel", "/v1/update", "/v1/history?limit=10", "/v1/heartbeat/", "/v1/../heartbeat"):
                self.assertEqual(get(path)[0], 403)
            self.assertEqual(get("/v1/heartbeat", "POST")[0], 403)
            self.assertEqual(get("/v1/heartbeat", body=b"x")[0], 400)
            allowed[0] = False
            self.assertEqual(get("/v1/history")[0], 403)
            self.assertEqual(get("/var/run/docker.sock?SECRET_TOKEN")[0], 403)
            audit = runtime.audit_log.events()
            self.assertIn("operation_unavailable", {event["reason"] for event in audit})
            self.assertIn("scope_denied", {event["reason"] for event in audit})
            self.assertNotIn("SECRET_TOKEN", json.dumps(audit))
        finally:
            server.shutdown()
            server.server_close()
            thread.join(2)

    def test_wrong_agent_and_no_scope_denied(self):
        for principal in (Principal(HOST, frozenset({"telemetry.read"})), Principal(AGENT, frozenset())):
            runtime = AgentRuntime(config(), authorize=lambda request: principal)
            server = create_server(runtime)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                client = http.client.HTTPConnection(*server.server_address, timeout=2)
                client.request("GET", "/v1/heartbeat")
                response = client.getresponse()
                self.assertEqual(response.status, 403)
                response.read()
                client.close()
            finally:
                server.shutdown()
                server.server_close()
                thread.join(2)

    def test_cli_heartbeat_is_runnable(self):
        runtime_dir = Path(__file__).resolve().parents[1]
        result = subprocess.run([sys.executable, "-m", "cc_agent", "--manifest", "manifest.example.json", "--once"],
                                cwd=runtime_dir, capture_output=True, text=True, timeout=5, check=True)
        self.assertEqual(json.loads(result.stdout)["collection"]["state"], "disabled")


if __name__ == "__main__":
    unittest.main()
