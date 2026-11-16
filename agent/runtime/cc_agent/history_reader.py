"""Trusted local read-source binding, never request-selected authority."""

import json
import re
from types import MappingProxyType
import uuid


MAX_HISTORY_RESPONSE_BYTES = 262144
_TOTAL = 256 * 1024 * 1024
_FIELDS = frozenset(("agent_id", "host_id", "history_resource_id", "collection_resource_id",
    "plan_sha256", "agent_reserved_bytes", "snapshot_reserved_bytes", "total_max_bytes",
    "max_points", "max_age_seconds", "history_kind", "schema_version"))


class BoundHistoryReader:
    """A pinned local object/protocol; its metadata grants no permission.

    Capture bound methods as well as descriptor. This is accidental-drift
    containment for trusted code, not a sandbox against Python code execution.
    """

    def __init__(self, reader, manifest):
        self.reader = reader
        self._manifest = manifest
        self._descriptor = reader.descriptor
        self._validate = reader.validate_runtime_binding
        self._query = reader.query
        if not all(callable(method) for method in (self._descriptor, self._validate, self._query)):
            raise ValueError("invalid_history_reader")
        descriptor = self._descriptor()
        self._check_descriptor(descriptor, manifest)
        self.binding = MappingProxyType(dict(descriptor))
        self.validate(manifest)

    @staticmethod
    def _check_descriptor(data, manifest):
        if type(data) is not dict or set(data) != _FIELDS:
            raise ValueError("invalid_history_binding")
        for key in ("agent_id", "host_id", "history_resource_id", "collection_resource_id"):
            value = data[key]
            if type(value) is not str or str(uuid.UUID(value)) != value or uuid.UUID(value).int == 0:
                raise ValueError("invalid_history_binding")
        if (data["agent_id"] != manifest.agent_id or data["host_id"] != manifest.host_id
                or data["history_resource_id"] != manifest.history_resource_id
                or data["history_resource_id"] in {manifest.heartbeat_resource_id,
                    data["collection_resource_id"], str(uuid.uuid5(uuid.UUID(manifest.agent_id), "history-v1"))}
                or type(data["plan_sha256"]) is not str
                or re.fullmatch("[0-9a-f]{64}", data["plan_sha256"]) is None
                or type(data["history_kind"]) is not str or data["history_kind"] != "fixed_os_snapshot"
                or type(data["schema_version"]) is not int or data["schema_version"] != 1):
            raise ValueError("invalid_history_binding")
        for key, minimum, maximum in (("agent_reserved_bytes", 1024, _TOTAL),
                ("snapshot_reserved_bytes", 32768, _TOTAL), ("total_max_bytes", 1024, _TOTAL),
                ("max_points", 1, 5760), ("max_age_seconds", 1, 86400)):
            if type(data[key]) is not int or not minimum <= data[key] <= maximum:
                raise ValueError("invalid_history_binding")
        if (data["agent_reserved_bytes"] != manifest.history_max_bytes
                or data["agent_reserved_bytes"] + data["snapshot_reserved_bytes"] > data["total_max_bytes"]):
            raise ValueError("invalid_history_binding")

    def validate(self, manifest):
        if manifest != self._manifest:
            raise ValueError("history_binding_changed")
        current = self._descriptor()
        self._check_descriptor(current, manifest)
        if current != self.binding:
            raise ValueError("history_binding_changed")
        if self._validate(agent_id=manifest.agent_id, host_id=manifest.host_id,
                history_resource_id=manifest.history_resource_id,
                agent_history_budget_bytes=manifest.history_max_bytes) is not None:
            raise ValueError("invalid_history_validation")

    def query(self, manifest):
        self.validate(manifest)
        data = self._query()
        if (type(data) is not dict or type(data.get("points")) is not list or len(data["points"]) > 100
                or data.get("agent_id") != self.binding["agent_id"]
                or data.get("host_id") != self.binding["host_id"]
                or data.get("resource_id") != self.binding["history_resource_id"]
                or data.get("history_kind") != "fixed_os_snapshot"
                or type(data.get("schema_version")) is not int or data["schema_version"] != 1):
            raise ValueError("invalid_history_response")
        # Bound encoded output and detach it from provider-owned mutable data.
        chunks, size = [], 0
        for chunk in json.JSONEncoder(separators=(",", ":"), allow_nan=False).iterencode(data):
            raw = chunk.encode("utf-8")
            size += len(raw)
            if size > MAX_HISTORY_RESPONSE_BYTES:
                raise ValueError("history_response_too_large")
            chunks.append(raw)
        result = json.loads(b"".join(chunks))
        self.validate(manifest)
        return result
