"""Purpose-specific device-reader composition, never a signer or authority.

Only a concrete native verifier can issue a successful admission. These types
bind public runtime facts and keep credentials out of representations/copies.
"""
from abc import ABC, abstractmethod
import base64
import binascii
import hashlib
import json
import re
import uuid

from .read_access import ReadAccessGuard, ReadAccessUnavailable
from .read_expiry import MAX_TIMESTAMP


HEADER = "x-consolecrypt-reader"
MAX_ENVELOPE_BYTES = 8192
_DOMAIN = b"consolecrypt.agent.reader.target.v1\0"


class _Opaque:
    __slots__ = ()

    def __setattr__(self, name, value):
        raise AttributeError("immutable_device_reader_value")

    def __delattr__(self, name):
        raise AttributeError("immutable_device_reader_value")

    def __repr__(self):
        return type(self).__name__ + "([redacted])"

    def __reduce_ex__(self, protocol):
        raise TypeError("device_reader_value_not_portable")


def _uuid(value):
    if type(value) is not str or str(uuid.UUID(value)) != value or uuid.UUID(value).int == 0:
        raise ValueError("invalid_reader_identity")
    return value


def _digest(value):
    if type(value) is not str or re.fullmatch("[0-9a-f]{64}", value) is None:
        raise ValueError("invalid_reader_target")
    return value


class ReaderTarget(_Opaque):
    """Built from actual runtime facts, not from signed or caller assertions."""
    __slots__ = ("_canonical", "_digest", "_action", "_resource", "_agent", "_bytes", "_points")

    def __init__(self, descriptor):
        raw = json.dumps(descriptor, sort_keys=True, separators=(",", ":"),
                         ensure_ascii=False, allow_nan=False).encode("utf-8")
        for name, value in (("_canonical", raw), ("_digest", hashlib.sha256(_DOMAIN + raw).hexdigest()),
                ("_action", descriptor["action"]), ("_resource", descriptor["resourceId"]),
                ("_agent", descriptor["agentId"]), ("_bytes", descriptor["maxResponseBytes"]),
                ("_points", descriptor["maxQueryPoints"])):
            object.__setattr__(self, name, value)

    def descriptor(self):
        return json.loads(self._canonical)

    digest = property(lambda self: self._digest)
    action = property(lambda self: self._action)
    resource = property(lambda self: self._resource)
    agent_id = property(lambda self: self._agent)
    max_response_bytes = property(lambda self: self._bytes)
    max_query_points = property(lambda self: self._points)

    def __eq__(self, other):
        return type(other) is ReaderTarget and self._canonical == other._canonical

    def __hash__(self):
        return hash(self._canonical)


def build_reader_targets(manifest, history_source=None):
    """The only runtime builder: no Native approval, path or key inference."""
    for value in (manifest.agent_id, manifest.host_id, manifest.heartbeat_resource_id,
                  manifest.history_resource_id):
        _uuid(value)
    if manifest.heartbeat_resource_id == manifest.history_resource_id:
        raise ValueError("invalid_reader_target")
    for value, minimum, maximum in ((manifest.sample_interval_seconds, 1, 300),
            (manifest.history_max_bytes, 1024, 256 * 1024 * 1024),
            (manifest.history_max_points, 1, 5760), (manifest.history_max_age_seconds, 1, 86400)):
        if type(value) is not int or not minimum <= value <= maximum:
            raise ValueError("invalid_reader_target")
    heartbeat = dict(agent_id=manifest.agent_id, host_id=manifest.host_id,
                     heartbeat_resource_id=manifest.heartbeat_resource_id,
                     history_kind="heartbeat", schema_version=1)
    if history_source is None:
        history = dict(agent_id=manifest.agent_id, host_id=manifest.host_id,
            history_resource_id=manifest.history_resource_id,
            sample_interval_seconds=manifest.sample_interval_seconds,
            max_bytes=manifest.history_max_bytes, max_points=manifest.history_max_points,
            max_age_seconds=manifest.history_max_age_seconds,
            history_kind="agent_gauge_history", schema_version=1)
        kind, points = "localAgentHistoryV1", 1000
    else:
        history_source.validate(manifest)
        history = dict(history_source.binding)  # Exact existing twelve public fields.
        kind, points = "fixedOsHistoryV1", 100
    def target(action, resource, kind, descriptor, cap, points):
        return ReaderTarget(dict(format="cc-reader-target/v1", agentId=manifest.agent_id,
            configuredHostId=manifest.host_id, action=action, resourceId=resource,
            dataset=dict(kind=kind, schemaVersion=1, descriptor=descriptor),
            maxResponseBytes=cap, maxQueryPoints=points))
    return (target("heartbeatRead", manifest.heartbeat_resource_id, "heartbeatV1", heartbeat, 65536, 0),
            target("historyRead", manifest.history_resource_id, kind, history, 262144, points))


class DeviceReaderAuthRequest(_Opaque):
    __slots__ = ("_target", "_envelope")

    def __init__(self, target, envelope):
        if type(target) is not ReaderTarget or type(envelope) is not bytes or not 0 < len(envelope) <= MAX_ENVELOPE_BYTES:
            raise ValueError("invalid_reader_request")
        object.__setattr__(self, "_target", target)
        object.__setattr__(self, "_envelope", envelope)

    target = property(lambda self: self._target)
    envelope = property(lambda self: self._envelope)


class DeviceReaderPrincipal(_Opaque):
    __slots__ = ("reader_device_id", "target_agent_id", "operation", "resource",
                 "target_descriptor_hash", "verified_at", "verified_expires_at")

    def __init__(self, *, reader_device_id, target_agent_id, operation, resource,
                 target_descriptor_hash, verified_at, verified_expires_at):
        for value in (reader_device_id, target_agent_id, resource):
            _uuid(value)
        _digest(target_descriptor_hash)
        if (type(operation) is not str or operation != "telemetry.read"
                or type(verified_at) is not int or type(verified_expires_at) is not int
                or not 0 <= verified_at < verified_expires_at <= MAX_TIMESTAMP):
            raise ValueError("invalid_reader_principal")
        for name, value in locals().copy().items():
            if name in self.__slots__:
                object.__setattr__(self, name, value)


class DeviceReaderAdmission(_Opaque):
    __slots__ = ("_principal", "_guard")

    def __init__(self, principal, guard):
        if type(principal) is not DeviceReaderPrincipal or not isinstance(guard, ReadAccessGuard):
            raise TypeError("typed_device_reader_admission_required")
        object.__setattr__(self, "_principal", principal)
        object.__setattr__(self, "_guard", guard)

    principal = property(lambda self: self._principal)
    guard = property(lambda self: self._guard)


class DeviceReaderProvider(ABC):
    @abstractmethod
    def bind_targets(self, targets):
        raise ReadAccessUnavailable()


class BoundDeviceReaderProvider(ABC):
    @property
    @abstractmethod
    def targets(self):
        raise ReadAccessUnavailable()

    @abstractmethod
    def begin(self, request):
        raise ReadAccessUnavailable()


class BoundDeviceReader:
    """Capture a trusted binding and method; no authority comes from an ABC."""
    def __init__(self, provider, manifest, history_source):
        if not isinstance(provider, DeviceReaderProvider):
            raise TypeError("typed_device_reader_provider_required")
        self.provider, self._manifest, self._source = provider, manifest, history_source
        self.targets = build_reader_targets(manifest, history_source)
        self._bound = provider.bind_targets(self.targets)
        if not isinstance(self._bound, BoundDeviceReaderProvider):
            raise TypeError("typed_bound_device_reader_provider_required")
        self.begin = self._bound.begin
        if not callable(self.begin):
            raise TypeError("typed_bound_device_reader_provider_required")
        self.validate(manifest, history_source)

    def validate(self, manifest, history_source):
        if (manifest != self._manifest or history_source is not self._source
                or type(self._bound.targets) is not tuple
                or len(self._bound.targets) != 2
                or any(type(target) is not ReaderTarget for target in self._bound.targets)
                or self._bound.targets != self.targets
                or build_reader_targets(manifest, history_source) != self.targets):
            raise ReadAccessUnavailable()

    def target_for(self, path):
        return self.targets[0 if path == "/v1/heartbeat" else 1]


def extract_reader_envelope(headers):
    """Exclusive family selection; the native actor interprets signed bytes."""
    if HEADER not in headers:
        return None
    value = headers[HEADER]
    if ("x-consolecrypt-proof" in headers or type(value) is not str
            or not 0 < len(value) <= (MAX_ENVELOPE_BYTES * 4 + 2) // 3
            or re.fullmatch("[A-Za-z0-9_-]+", value) is None):
        raise ValueError("invalid_reader_request")
    try:
        raw = base64.b64decode(value + "=" * (-len(value) % 4), altchars=b"-_", validate=True)
    except (ValueError, binascii.Error):
        raise ValueError("invalid_reader_request") from None
    if (not 0 < len(raw) <= MAX_ENVELOPE_BYTES
            or base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=") != value):
        raise ValueError("invalid_reader_request")
    return raw


def encode_device_response(data, target):
    if target.action == "historyRead" and (type(data) is not dict
            or type(data.get("points")) is not list or len(data["points"]) > target.max_query_points):
        raise ReadAccessUnavailable()
    chunks, size = [], 0
    for chunk in json.JSONEncoder(separators=(",", ":"), allow_nan=False).iterencode(data):
        # Retained encoded bytes are bounded, including UTF8 expansion. This is
        # not a heap bound on arbitrary objects produced by trusted Python code.
        for index in range(0, len(chunk), 4096):
            raw = chunk[index:index + 4096].encode("utf-8")
            size += len(raw)
            if size > target.max_response_bytes:
                raise ReadAccessUnavailable()
            chunks.append(raw)
    return b"".join(chunks)
