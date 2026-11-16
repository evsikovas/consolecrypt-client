"""Concrete private reader-worker bridge; no issuer, signer or Node-proof cast."""
import base64
import json
import math
import os
import re
import sys
import time
import uuid

from cc_agent.device_reader import (BoundDeviceReaderProvider, DeviceReaderAdmission,
    DeviceReaderAuthRequest, DeviceReaderPrincipal, DeviceReaderProvider, ReaderTarget)
from cc_agent.read_access import ReadAccessUnavailable
from cc_agent.read_expiry import MAX_TIMESTAMP

from .device_reader_config import ReaderGateConfig
from .identity_gate import strict_json
from .reader_channel import READER_DENIED, ReaderWorkerChannel
from .reader_files import read_reader_file
from .sqlite_custody_gate import _NativeReadGuard, _remaining

_META = frozenset({"readerDeviceId", "targetAgentId", "operation", "associationHash",
    "nativeProfileId", "nativeHostId", "nativeBindingId", "nativeBindingRevision",
    "targetDescriptorHash", "expiresAt", "handle"})


def _uuid(value):
    return (type(value) is str and uuid.UUID(value).int != 0
            and str(uuid.UUID(value)) == value)


def _hex(value, *, nonzero=False):
    return (type(value) is str and re.fullmatch("[0-9a-f]{64}", value) is not None
            and (not nonzero or value != "0" * 64))


def _canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode("utf-8")


def _clock_sample(clock):
    value = clock()
    if (type(value) not in (int, float) or not 0 <= value <= MAX_TIMESTAMP
            or not math.isfinite(value)):
        raise ReadAccessUnavailable()
    return value


class _ReaderGuard(_NativeReadGuard):
    """Reuse owner/deadline/one-shot IPC mechanics, never a native Node Permit.

    The bound process runs only worker-reader-sqlite; every phase below carries
    the separate reader context and calls its actual ReaderActor.
    """
    __slots__ = ("_reader", "_association", "_target_hash")

    def __init__(self, channel, process, metadata, target, deadline):
        super().__init__(channel, process, metadata["handle"], target.agent_id,
            {"kind": "telemetryRead", "source": target.resource}, deadline)
        self._reader = metadata["readerDeviceId"]
        self._association = metadata["associationHash"]
        self._target_hash = target.digest

    def _exchange(self, action, handle):
        names = {"check": "readerCheck", "finalize": "readerFinalize", "retire": "readerRetire"}
        process, frame = self._channel.exchange_bound({"action": names[action],
            "handle": handle, "readerDeviceId": self._reader, "targetAgentId": self._agent,
            "associationHash": self._association, "targetDescriptorHash": self._target_hash,
            "operation": self._operation}, expected_process=self._process,
            timeout=self._remaining())
        phase = {"check": "checked", "finalize": "finalized", "retire": "retired"}[action]
        if process is not self._process or frame != {"ok": True, "metadata": {"phase": phase}}:
            raise ReadAccessUnavailable()
        self._remaining()

    def __repr__(self):
        return "NativeDeviceReadGuard([redacted])"


class NativeDeviceReaderProvider(DeviceReaderProvider):
    """Explicit trusted embedding only. Binding never initializes stores/keys."""
    def __init__(self, config, *, clock=time.time):
        if sys.platform != "linux":
            raise ValueError("device reader requires Linux")
        if type(config) is not ReaderGateConfig:
            raise TypeError("typed_reader_config_required")
        self._config, self._clock = config, clock
        self._pid, self._bound = os.getpid(), None

    def bind_targets(self, targets):
        if os.getpid() != self._pid or self._bound is not None:
            raise ValueError("reader_provider_already_bound_or_foreign")
        if (type(targets) is not tuple or len(targets) != 2
                or any(type(target) is not ReaderTarget for target in targets)
                or [target.action for target in targets] != ["heartbeatRead", "historyRead"]):
            raise ValueError("invalid_reader_target_set")
        expected = {"format": "cc-reader-target-set/v1",
                    "targets": [target.descriptor() for target in targets]}
        path = self._config.target
        if path.is_symlink():
            raise ValueError("invalid_reader_target_file")
        raw = read_reader_file(path, 16384)
        if _canonical(strict_json(raw)) != _canonical(expected):
            raise ValueError("reader_target_file_mismatch")
        self._bound = _BoundNativeReader(self._config, targets, self._clock)
        return self._bound

    def close(self):
        if os.getpid() != self._pid:
            raise ReadAccessUnavailable()
        if self._bound is not None:
            self._bound.close()

    def __repr__(self):
        return "NativeDeviceReaderProvider([redacted])"


class _BoundNativeReader(BoundDeviceReaderProvider):
    def __init__(self, config, targets, clock):
        self._config, self._targets, self._clock = config, targets, clock
        self._pid = os.getpid()
        self.channel = ReaderWorkerChannel([str(config.executable), "worker-reader-sqlite",
            "--reader-db-dir", str(config.reader_store),
            "--reader-anchor-dir", str(config.reader_anchor),
            "--reader-issuer-pin", str(config.issuer_pin),
            "--node-db-dir", str(config.node_store), "--node-anchor-dir", str(config.node_anchor),
            "--node-owner-pin", str(config.node_owner_pin), "--target", str(config.target)],
            timeout=config.timeout_seconds)

    @property
    def targets(self):
        return self._targets

    def begin(self, request):
        if os.getpid() != self._pid:
            raise ReadAccessUnavailable()
        if type(request) is not DeviceReaderAuthRequest or request.target not in self._targets:
            return None
        # Always take payload context from captured startup objects.
        target = next(target for target in self._targets if target == request.target)
        guard = None
        try:
            before = _clock_sample(self._clock)
            if self._config.executable.is_symlink():
                raise ReadAccessUnavailable()
            if read_reader_file(self._config.executable, 128 * 1024**2, digest=True) != self._config.executable_sha256:
                raise ReadAccessUnavailable()
            deadline = time.monotonic() + 5.0
            process, frame = self.channel.exchange_bound({"action": "readerBegin",
                "expectedAgentId": target.agent_id,
                "operation": {"kind": "telemetryRead", "source": target.resource},
                "targetDescriptorHash": target.digest,
                "bundle": base64.urlsafe_b64encode(request.envelope).decode("ascii").rstrip("=")},
                timeout=_remaining(self.channel, deadline))
            if frame["ok"] is False:
                if frame["code"] in READER_DENIED:
                    return None
                raise ReadAccessUnavailable()
            metadata = frame["metadata"]
            # Capture safe cleanup context before validating the remaining
            # success schema. Missing unusable context leaves only bounded
            # native TTL/capacity; it never creates an admission.
            if (type(metadata) is not dict or not _hex(metadata.get("handle"), nonzero=True)
                    or not _uuid(metadata.get("readerDeviceId"))
                    or not _hex(metadata.get("associationHash"))):
                raise ReadAccessUnavailable()
            guard = _ReaderGuard(self.channel, process, metadata, target, deadline)
            if (set(metadata) != _META
                    or metadata["targetAgentId"] != target.agent_id
                    or metadata["operation"] != {"kind": "telemetryRead", "source": target.resource}
                    or metadata["targetDescriptorHash"] != target.digest
                    or not all(_uuid(metadata[name]) for name in (
                        "nativeProfileId", "nativeHostId", "nativeBindingId"))
                    or type(metadata["nativeBindingRevision"]) is not int
                    or not 1 <= metadata["nativeBindingRevision"] <= 2**64 - 1
                    or type(metadata["expiresAt"]) is not int
                    or not 0 <= metadata["expiresAt"] <= MAX_TIMESTAMP):
                raise ReadAccessUnavailable()
            after = _clock_sample(self._clock)
            if after < before:
                raise ReadAccessUnavailable()
            guard._remaining()
            principal = DeviceReaderPrincipal(reader_device_id=metadata["readerDeviceId"],
                target_agent_id=target.agent_id, operation="telemetry.read", resource=target.resource,
                target_descriptor_hash=target.digest, verified_at=int(max(before, after)),
                verified_expires_at=metadata["expiresAt"])
            admission = DeviceReaderAdmission(principal, guard)
            guard = None
            return admission
        except Exception:
            if guard is not None:
                try:
                    guard.retire()
                except Exception:
                    pass
            raise ReadAccessUnavailable() from None

    def close(self):
        if os.getpid() != self._pid:
            raise ReadAccessUnavailable()
        self.channel.close()

    def __repr__(self):
        return "BoundNativeDeviceReader([redacted])"
