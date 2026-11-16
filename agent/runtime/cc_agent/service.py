"""Small loopback-only telemetry API. Authentication is injected, never configured as scopes."""

from dataclasses import dataclass, field
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import io
import json
import threading
import time
from types import MappingProxyType
from typing import Mapping

from . import __version__
from .audit import AuditLog
from .history import LocalHistory
from .history_reader import BoundHistoryReader
from .http_ingress import HeadBudgetExceeded, HeadReader, IncompleteHead
from .read_expiry import AuthorizationUnavailable, ReadExpiryGuard
from .read_access import ReadAccessUnavailable, ReadAdmission
from .device_reader import (BoundDeviceReader, DeviceReaderAdmission, DeviceReaderAuthRequest,
                            DeviceReaderPrincipal, encode_device_response, extract_reader_envelope)


@dataclass(frozen=True)
class AuthRequest:
    operation: str
    resource: str
    method: str
    path: str
    body: bytes
    headers: Mapping[str, str]


@dataclass(frozen=True)
class Principal:
    agent_id: str
    scopes: frozenset[str]
    verified_at: int | None = field(default=None, kw_only=True)
    verified_expires_at: int | None = field(default=None, kw_only=True)


class AgentRuntime:
    def __init__(self, manifest, authorize=None, collector_status=None, state_dir=None,
                 clock=None, monotonic=None, audit=None, *, history_reader=None, reader_provider=None):
        self.manifest = manifest
        # Validate trusted source/allocation before local persistent state is touched.
        self._history_source = (None if history_reader is None
                                else BoundHistoryReader(history_reader, manifest))
        selected_source = self._history_source
        self._device_source = (None if reader_provider is None else
                               BoundDeviceReader(reader_provider, manifest, self._history_source))
        if self.manifest != manifest or self._history_source is not selected_source:
            raise ValueError("reader_binding_changed")
        self.authorize = authorize or (lambda request: None)
        self.collector_status = collector_status or (lambda: {"state": "disabled"})
        self.clock = clock or time.time
        self.monotonic = monotonic or time.monotonic
        self.started = self.monotonic()
        self.history = LocalHistory(manifest, state_dir, self.clock)
        self.audit_log = AuditLog(state_dir) if audit is None else None
        self.audit = self.audit_log if audit is None else audit
        self.stop_event = threading.Event()

    @property
    def history_reader(self):
        return None if self._history_source is None else self._history_source.reader

    @property
    def reader_provider(self):
        return None if self._device_source is None else self._device_source.provider

    def _collection(self):
        try:
            raw = self.collector_status()
            # Do not echo free-form exporter errors, endpoints, tokens or keys.
            # Native backend is an explicit diagnostic datatype, not authority.
            # This closed projection adds no collector start/selection path.
            if isinstance(raw, dict) and raw.get("backend") is not None:
                if (set(raw) != {"backend", "state", "native_queue"}
                        or raw["backend"] != "prom-agent-hardmode-v1"
                        or type(raw["backend"]) is not str
                        or type(raw["state"]) is not str
                        or raw["state"] not in {"unknown", "running", "degraded", "stopped", "blocked"}):
                    return {"state": "unknown"}
                native = raw["native_queue"]
                fields = {'age_evicted_segments_total', 'byte_evicted_segments_total', 'charged_evicted_wal_bytes_total', 'confirmed_accepted_requests_total', 'confirmed_accepted_samples_total', 'global_unacknowledged_loss_unknown', 'last_validated_root_charge_bytes', 'oldest_retained_segment_residence_seconds', 'physical_evicted_segments_total', 'receiver_permanent_rejected_samples_total', 'sample_nonrecoverable_failures_total', 'sample_pre_send_drops_total', 'sample_retry_attempts_total', 'sample_send_attempts_total', 'shard_pending_samples', 'source_max_enqueued_timestamp_seconds', 'source_max_reported_sent_timestamp_seconds', 'sticky_unavailable', 'system_disk_low_available_rejections_total', 'system_disk_observation_invalid_rejections_total', 'unacknowledged_delivery_lag_seconds', 'unacknowledged_samples_lost_total'}
                if type(native) is not dict or set(native) != fields:
                    return {"state": "unknown"}
                clean = {}
                for name in sorted(fields):
                    item = native[name]
                    if name in {'confirmed_accepted_requests_total', 'confirmed_accepted_samples_total', 'receiver_permanent_rejected_samples_total', 'unacknowledged_delivery_lag_seconds', 'unacknowledged_samples_lost_total'}:
                        if item is not None:
                            return {"state": "unknown"}
                    elif item is not None:
                        if name in {'global_unacknowledged_loss_unknown', 'sticky_unavailable'}:
                            if type(item) is not bool:
                                return {"state": "unknown"}
                        elif name in {'oldest_retained_segment_residence_seconds', 'source_max_enqueued_timestamp_seconds', 'source_max_reported_sent_timestamp_seconds'}:
                            if type(item) not in (int, float) or not 0 <= item < 2**53:
                                return {"state": "unknown"}
                        elif type(item) is not int or not 0 <= item < 2**53:
                            return {"state": "unknown"}
                    clean[name] = item
                if clean["sticky_unavailable"] is True and raw["state"] not in {"degraded", "stopped", "blocked"}:
                    return {"state": "unknown"}
                if raw["state"] == "running" and clean["sticky_unavailable"] is not False:
                    return {"state": "unknown"}
                charge = clean["last_validated_root_charge_bytes"]
                if charge is not None and charge > 536870912 and raw["state"] not in {"blocked", "stopped"}:
                    return {"state": "unknown"}
                return {"state": raw["state"], "backend": "prom-agent-hardmode-v1", "native_queue": clean}
            state = raw.get("state", "unknown") if isinstance(raw, dict) else "unknown"
            if state not in {"disabled", "starting", "running", "degraded", "stopped", "blocked", "unknown"}:
                state = "unknown"
            result = {"state": state}
            block_reason = raw.get("remote_write_block_reason") if isinstance(raw, dict) else None
            if type(block_reason) is str and block_reason == "pinned_collector_buffer_contract_incompatible":
                result["remote_write_block_reason"] = block_reason
            if isinstance(raw, dict) and raw.get("remote_write_production_allowed") is False:
                result["remote_write_production_allowed"] = False
            if (isinstance(raw, dict) and raw.get("remote_write_receiver_response_bounded") is False
                    and type(raw.get("remote_write_receiver_response_block_reason")) is str
                    and raw["remote_write_receiver_response_block_reason"] == "pinned_receiver_error_body_unbounded"):
                result["remote_write_receiver_response_bounded"] = False
                result["remote_write_receiver_response_block_reason"] = "pinned_receiver_error_body_unbounded"
            if (isinstance(raw, dict) and raw.get("remote_write_receiver_destination_pinned") is False
                    and type(raw.get("remote_write_receiver_destination_block_reason")) is str
                    and raw["remote_write_receiver_destination_block_reason"] == "remote_write_receiver_redirect_destination_not_pinned"):
                result["remote_write_receiver_destination_pinned"] = False
                result["remote_write_receiver_destination_block_reason"] = "remote_write_receiver_redirect_destination_not_pinned"
            if isinstance(raw, dict) and (raw.get("last_scrape_success") is None or type(raw.get("last_scrape_success")) is bool):
                result["last_scrape_success"] = raw.get("last_scrape_success")
            for name in ("lag_seconds", "dropped_samples", "remote_write_samples_dropped", "remote_write_packets_dropped", "pending_samples", "buffer_bytes"):
                value = raw.get(name) if isinstance(raw, dict) else None
                if type(value) in (int, float) and 0 <= value < 2**53:
                    result[name] = value
            return result
        except Exception:
            return {"state": "unknown"}

    def heartbeat(self):
        return {"schema_version": 1, "agent_id": self.manifest.agent_id,
                "host_id": self.manifest.host_id, "host_binding": "configured_not_identity_attested",
                "version": __version__, "uptime_seconds": max(0, self.monotonic() - self.started),
                "timestamp": self.clock(), "collection": self._collection(),
                "capabilities": {"protocol": "cc-agent-local/1", "telemetry.read": 1, "local_history": 1},
                "audit": self.audit_log.status() if self.audit_log is not None else {"state": "external_sink"},
                "history_available_while_online_only": True}

    def sample(self):
        snapshot = self.heartbeat()
        state = snapshot["collection"]["state"]
        running = None if state in {"unknown", "degraded", "starting", "blocked"} else state == "running"
        return self.history.append(snapshot["timestamp"], snapshot["uptime_seconds"],
                                   running, collection_state=state)

    def sample_loop(self):
        while not self.stop_event.is_set():
            self.sample()
            self.stop_event.wait(self.manifest.sample_interval_seconds)

    def record_audit(self, reason, resource=None, *, agent_id=None):
        event = {"timestamp": self.clock(), "agent_id": self.manifest.agent_id if agent_id is None else agent_id,
                 "operation": "telemetry.read" if resource is not None else "unavailable",
                 "resource": resource, "reason": reason}
        try:
            return self.audit(event) is not False
        except Exception:
            return False


class _BoundedServer(ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = True
    request_queue_size = 8

    def __init__(self, *args, **kwargs):
        self._slots = threading.BoundedSemaphore(8)
        super().__init__(*args, **kwargs)

    def process_request(self, request, client_address):
        if not self._slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self._slots.release()
            raise

    def process_request_thread(self, *args):
        try:
            super().process_request_thread(*args)
        finally:
            self._slots.release()


def create_server(runtime):
    if not ipaddress.IPv4Address(runtime.manifest.bind_host).is_loopback:
        raise ValueError("loopback bind required")
    # Capture selection before HTTPServer binds/listens; .history remains the writer.
    source = runtime._history_source
    device_source = runtime._device_source
    selected_manifest = runtime.manifest
    selected_local_history = runtime.history
    bound_resources = None
    if source is not None:
        source.validate(runtime.manifest)
        if runtime.manifest != selected_manifest:
            raise ValueError("history_binding_changed")
        bound_resources = {"/v1/heartbeat": selected_manifest.heartbeat_resource_id,
                           "/v1/history": selected_manifest.history_resource_id}
        def history_query():
            data = source.query(runtime.manifest)
            # The argument passed into query is a snapshot. Fence the live runtime
            # again in case trusted source code replaced it during projection.
            if runtime.manifest != selected_manifest:
                raise ValueError("history_binding_changed")
            return data
    else:
        history_query = runtime.history.query
    if device_source is not None:
        device_source.validate(runtime.manifest, runtime._history_source)
        if (runtime.manifest != selected_manifest or runtime._history_source is not source
                or runtime._device_source is not device_source or runtime.history is not selected_local_history
                or selected_local_history.manifest != selected_manifest):
            raise ValueError("reader_binding_changed")
        bound_resources = {"/v1/heartbeat": selected_manifest.heartbeat_resource_id,
                           "/v1/history": selected_manifest.history_resource_id}
    class Handler(BaseHTTPRequestHandler):
        server_version = "ConsoleCryptAgent"
        sys_version = ""

        def setup(self):
            super().setup()
            self.rfile.close()
            self._head_reader = HeadReader(self.connection)
            self.rfile = io.BufferedReader(self._head_reader, buffer_size=1024)

        def _head_error(self, status, error):
            self.close_connection = True
            payload = json.dumps({"error": error}, separators=(",", ":")).encode("ascii")
            phrases = {400: "Bad Request", 414: "URI Too Long", 431: "Request Header Fields Too Large",
                       501: "Not Implemented", 505: "HTTP Version Not Supported"}
            frame = (f"HTTP/1.0 {status} {phrases[status]}\r\nContent-Type: application/json\r\n"
                f"Content-Length: {len(payload)}\r\nCache-Control: no-store\r\n"
                "Connection: close\r\n\r\n").encode("ascii") + payload
            try:
                # One bounded write: header/payload cannot each reset the budget.
                self.connection.settimeout(self._head_reader.remaining_seconds())
                self.connection.sendall(frame)
            except (OSError, TimeoutError):
                pass  # Expired/closed peers get no replacement response or retry.

        def send_error(self, code, message=None, explain=None):
            # Ignore stdlib messages which can include raw request lines/headers.
            status = int(code) if type(code) in {int, HTTPStatus} and code in {400, 414, 431, 501, 505} else 400
            self._head_error(status, "invalid_request")

        def handle_one_request(self):
            try:
                super().handle_one_request()
            except HeadBudgetExceeded:
                self._head_error(431, "request_headers_too_large")
            except IncompleteHead:
                self.close_connection = True
                if self._head_reader.received:
                    self._head_error(400, "invalid_request")

        def parse_request(self):
            if not super().parse_request():
                return False
            if self.request_version not in {"HTTP/1.0", "HTTP/1.1"}:
                self._head_error(400, "invalid_request")
                return False
            # Buffered bytes still require a fresh postparse deadline observation.
            self._head_reader.remaining_seconds()
            self.close_connection = True  # Exactly one request; no body/keepalive.
            self.connection.settimeout(2)
            return True

        def log_message(self, *args):
            pass  # Request paths/headers may contain attacker-supplied material.

        def _access_checkpoint(self, access, device_target=None):
            if access is not None and (runtime.stop_event.is_set() or runtime.manifest != selected_manifest):
                raise ReadAccessUnavailable()
            if device_target is not None:
                if (device_source is None or runtime._device_source is not device_source or runtime.stop_event.is_set()
                        or runtime.history is not selected_local_history
                        or selected_local_history.manifest != selected_manifest):
                    raise ReadAccessUnavailable()
                device_source.validate(runtime.manifest, runtime._history_source)
                # A trusted validator may change live state during validation.
                if (runtime.manifest != selected_manifest or runtime.stop_event.is_set()
                        or runtime._device_source is not device_source or runtime._history_source is not source
                        or runtime.history is not selected_local_history
                        or selected_local_history.manifest != selected_manifest):
                    raise ReadAccessUnavailable()

        @staticmethod
        def _access_step(access, phase, context):
            if getattr(access, phase)(**context) is not None:
                raise ReadAccessUnavailable()

        def _reply(self, status, data, *, expiry_guard=None, read_access=None, access_context=None,
                   device_target=None):
            try:
                payload = (encode_device_response(data, device_target) if device_target is not None else
                           json.dumps(data, separators=(",", ":"), allow_nan=False).encode("utf-8"))
            except Exception:
                if read_access is None:
                    raise
                return self._reply(503, {"error": "authorization_unavailable"})
            if status == 200 and read_access is not None:
                try:
                    self._access_checkpoint(read_access, device_target)
                    expiry_guard.check()
                    self._access_step(read_access, "finalize", access_context)
                    expiry_guard.check()
                    self._access_checkpoint(read_access, device_target)
                    if device_target is not None:
                        # Device source validation can perform I/O. Observe OS
                        # time again after that final callback, before headers.
                        expiry_guard.check()
                except Exception:
                    return self._reply(503, {"error": "authorization_unavailable"})
            elif status == 200 and expiry_guard is not None:
                try:
                    expiry_guard.check()
                except AuthorizationUnavailable:
                    return self._reply(503, {"error": "authorization_unavailable"})
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(payload)
            self.close_connection = True

        def do_GET(self):
            resources = bound_resources or {"/v1/heartbeat": runtime.manifest.heartbeat_resource_id,
                                            "/v1/history": runtime.manifest.history_resource_id}
            # Exact static resource routes: no query, fragments, arbitrary paths,
            # request bodies, compression, transfer encoding, or redirects.
            if self.path not in resources or len(self.path) > 128:
                runtime.record_audit("operation_unavailable")
                return self._reply(403, {"error": "operation_unavailable"})
            if (self.headers.get("Content-Length", "0") != "0"
                    or self.headers.get("Transfer-Encoding") is not None
                    or len(str(self.headers).encode("utf-8")) > 16384):
                runtime.record_audit("invalid_request", resources[self.path])
                return self._reply(400, {"error": "invalid_request"})
            if len({name.lower() for name in self.headers}) != len(self.headers):
                runtime.record_audit("duplicate_header", resources[self.path])
                return self._reply(400, {"error": "duplicate_header"})
            request = AuthRequest("telemetry.read", resources[self.path], "GET", self.path, b"",
                                  MappingProxyType({k.lower(): v for k, v in self.headers.items()}))
            try:
                reader_envelope = extract_reader_envelope(request.headers)
            except ValueError:
                runtime.record_audit("invalid_request", resources[self.path],
                                     agent_id=selected_manifest.agent_id)
                return self._reply(400, {"error": "invalid_request"})
            device_mode = reader_envelope is not None
            access = None
            device_target = None
            try:
                try:
                    if device_mode:
                        if device_source is None:
                            raise ReadAccessUnavailable()
                        device_target = device_source.target_for(self.path)
                        self._access_checkpoint(None, device_target)
                        admitted = device_source.begin(DeviceReaderAuthRequest(device_target, reader_envelope))
                    else:
                        admitted = runtime.authorize(request)
                except ReadAccessUnavailable:
                    runtime.record_audit("operation_unavailable", resources[self.path],
                                         agent_id=selected_manifest.agent_id)
                    return self._reply(503, {"error": "authorization_unavailable"})
                except Exception:
                    if device_mode:
                        runtime.record_audit("operation_unavailable", resources[self.path],
                                             agent_id=selected_manifest.agent_id)
                        return self._reply(503, {"error": "authorization_unavailable"})
                    admitted = None
                if device_mode:
                    if type(admitted) is DeviceReaderAdmission:
                        access, principal = admitted.guard, admitted.principal
                    elif admitted is None:
                        principal = None
                    else:
                        # A wrong wrapper is not a device principal. Retire any
                        # typed acquired guard without treating it as authority.
                        if isinstance(admitted, ReadAdmission):
                            access = admitted.guard
                        runtime.record_audit("operation_unavailable", resources[self.path],
                                             agent_id=selected_manifest.agent_id)
                        return self._reply(503, {"error": "authorization_unavailable"})
                elif isinstance(admitted, ReadAdmission):
                    access, principal = admitted.guard, admitted.principal
                else:
                    principal = admitted
                audit_context = {} if access is None and not device_mode else {"agent_id": selected_manifest.agent_id}
                expected_agent = runtime.manifest.agent_id if access is None else selected_manifest.agent_id
                if device_mode:
                    valid_principal = (type(principal) is DeviceReaderPrincipal
                        and principal.target_agent_id == selected_manifest.agent_id
                        and principal.operation == request.operation and principal.resource == request.resource
                        and principal.target_descriptor_hash == device_target.digest)
                else:
                    valid_principal = (isinstance(principal, Principal) and principal.agent_id == expected_agent
                                       and "telemetry.read" in principal.scopes)
                if not valid_principal:
                    runtime.record_audit("scope_denied", resources[self.path], **audit_context)
                    return self._reply(403, {"error": "scope_denied"})
                if not runtime.record_audit("admitted", resources[self.path], **audit_context):
                    return self._reply(503, {"error": "audit_unavailable"})
                context = {"agent_id": selected_manifest.agent_id,
                           "operation": request.operation, "resource": request.resource}
                try:
                    self._access_checkpoint(access, device_target)
                    if access is not None and (principal.verified_at is None or principal.verified_expires_at is None):
                        raise ReadAccessUnavailable()
                    expiry_guard = ReadExpiryGuard(principal.verified_at, principal.verified_expires_at)
                    if access is not None:
                        self._access_step(access, "check", context)
                        expiry_guard.check()
                        self._access_checkpoint(access, device_target)
                except AuthorizationUnavailable:
                    return self._reply(503, {"error": "authorization_unavailable"})
                except Exception:
                    if access is None:
                        raise
                    return self._reply(503, {"error": "authorization_unavailable"})
                if self.path == "/v1/heartbeat":
                    try:
                        data = runtime.heartbeat()
                    except Exception:
                        if access is None:
                            raise
                        return self._reply(503, {"error": "authorization_unavailable"})
                else:
                    try:
                        data = history_query()
                    except Exception:
                        # Admission may already have consumed a one-use proof.
                        return self._reply(503, {"error": "history_unavailable"})
                return self._reply(200, data, expiry_guard=expiry_guard,
                                   read_access=access, access_context=context, device_target=device_target)
            finally:
                if access is not None:
                    try:
                        access.retire()
                    except Exception:
                        pass  # Cleanup cannot replace an earlier result or grant.

        def do_POST(self):
            runtime.record_audit("operation_unavailable")
            return self._reply(403, {"error": "operation_unavailable"})

        do_PUT = do_POST
        do_DELETE = do_POST
        do_PATCH = do_POST
        do_HEAD = do_POST
        do_OPTIONS = do_POST

    bind_manifest = selected_manifest if source is not None or device_source is not None else runtime.manifest
    return _BoundedServer((bind_manifest.bind_host, bind_manifest.port), Handler)
