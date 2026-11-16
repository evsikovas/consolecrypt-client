"""Explicit Linux node-read custody; no device identity or serialized Permit.

Handles stay in this private adapter. Only the native actor owns a real Permit.
No operation is retried and a lost worker generation is never rebound.
"""
import math
import os
import re
import sys
import threading
import time

from cc_agent.read_access import ReadAccessGuard, ReadAccessUnavailable, ReadAdmission

from .identity_gate import IdentityGate, _TrustedClockInvalid
from .sqlite_worker_gate import WorkerChannel

_LIFETIME = 5.0
_DENIED = frozenset({"ScopeDenied", "InvalidSignature", "InvalidWire", "InvalidKey", "WrongClaim",
    "WrongEpoch", "WrongCheckpoint", "Expired", "Revoked", "Replay", "NotFound",
    "CapabilityDenied", "Conflict", "WrongState", "InvalidToken", "WrongContext",
    "UnsupportedTeam", "MissingFreshness", "KeyReuse"})


def _remaining(channel, deadline):
    remaining = deadline - time.monotonic()
    if not math.isfinite(remaining) or remaining <= 0:
        raise ReadAccessUnavailable()
    return min(channel.timeout, remaining)


class _NativeReadGuard(ReadAccessGuard):
    __slots__ = ("_channel", "_process", "_handle", "_agent", "_operation", "_resource",
                 "_deadline", "_pid", "_thread", "_phase")

    def __init__(self, channel, process, handle, agent, operation, deadline):
        self._channel, self._process, self._handle = channel, process, handle
        self._agent, self._operation = agent, dict(operation)
        self._resource = operation["source"]
        self._deadline = deadline
        self._pid, self._thread = os.getpid(), threading.current_thread()
        self._phase = "begun"

    def _owner(self):
        if os.getpid() != self._pid or threading.current_thread() is not self._thread:
            raise ReadAccessUnavailable()

    def _remaining(self):
        return _remaining(self._channel, self._deadline)

    def _context(self, agent_id, operation, resource):
        self._owner()  # A foreign finally must not clear another owner's handle.
        if (type(agent_id) is not str or agent_id != self._agent
                or type(operation) is not str or operation != "telemetry.read"
                or type(resource) is not str or resource != self._resource):
            self.retire()
            raise ReadAccessUnavailable()

    def _exchange(self, action, handle):
        process, frame = self._channel.exchange_bound({"action": action, "handle": handle,
            "agentId": self._agent, "operation": self._operation},
            expected_process=self._process, timeout=self._remaining())
        if (process is not self._process or frame != {"ok": True, "metadata": {
                "phase": {"check": "checked", "finalize": "finalized", "retire": "retired"}[action]}}):
            raise ReadAccessUnavailable()
        self._remaining()  # Include queue, response and local scheduling in five seconds.

    def check(self, *, agent_id, operation, resource):
        self._context(agent_id, operation, resource)
        if self._phase != "begun" or self._handle is None:
            self.retire()
            raise ReadAccessUnavailable()
        try:
            self._exchange("check", self._handle)
            self._phase = "checked"
        except Exception:
            self.retire()
            raise ReadAccessUnavailable() from None

    def finalize(self, *, agent_id, operation, resource):
        self._context(agent_id, operation, resource)
        if self._phase != "checked" or self._handle is None:
            self.retire()
            raise ReadAccessUnavailable()
        handle, self._handle = self._handle, None
        self._phase = "unavailable"  # Dispatch uncertainty is terminal, never retried.
        try:
            self._exchange("finalize", handle)
            self._phase = "finalized"
        except Exception:
            raise ReadAccessUnavailable() from None

    def retire(self):
        self._owner()
        handle, self._handle = self._handle, None
        if handle is None:
            return
        self._phase = "closed"  # Local retirement is not a native acknowledgment.
        try:
            # Do not start or close a replacement generation; never retry cleanup.
            if self._channel.process is not self._process or self._process.poll() is not None:
                return
            self._exchange("retire", handle)
            self._phase = "retired"
        except Exception:
            pass  # Unknown cleanup is not an authority acknowledgment or success claim.

    def __repr__(self):
        return "NativeReadGuard([redacted])"

    def __reduce_ex__(self, protocol):
        raise TypeError("private_read_guard_not_portable")


class SqliteCustodyGate(IdentityGate):
    def __init__(self, config, agent_id, resources, **kwargs):
        if sys.platform != "linux":
            raise ValueError("custody provider requires Linux")
        super().__init__(config, agent_id, resources, **kwargs)
        self.channel = WorkerChannel([str(config.executable), "worker-custody-sqlite",
            "--db-dir", str(config.store), "--anchor-dir", str(config.anchor),
            "--owner-pin", str(config.owner_pin)], timeout=min(config.timeout_seconds, _LIFETIME))

    def __call__(self, request):
        try:
            prepared = self._prepare_request(request)
        except _TrustedClockInvalid:
            raise ReadAccessUnavailable() from None
        except (ValueError, TypeError, KeyError, OSError, UnicodeError, RecursionError):
            return None
        if prepared is None:
            return None
        operation, payload, before = prepared
        guard = None
        try:
            if not self._executable_valid():
                raise ReadAccessUnavailable()
            # Start before even waiting for the begin exchange's private lock.
            deadline = time.monotonic() + _LIFETIME
            native = {k: v for k, v in payload.items() if k != "now"}
            process, frame = self.channel.exchange_bound({**native, "action": "begin",
                "agentId": self.agent_id}, timeout=_remaining(self.channel, deadline))
            if frame["ok"] is False:
                if frame["code"] in _DENIED:
                    return None
                raise ReadAccessUnavailable()
            metadata = frame["metadata"]
            if (type(metadata) is not dict
                    or type(metadata.get("handle")) is not str
                    or re.fullmatch("[0-9a-f]{64}", metadata["handle"]) is None
                    or metadata["handle"] == "0" * 64):
                raise ReadAccessUnavailable()
            guard = _NativeReadGuard(self.channel, process, metadata["handle"], self.agent_id,
                                     operation, deadline)
            # A usable private handle permits cleanup even when the rest of
            # the success metadata is malformed. Never trust its context.
            if set(metadata) != {"handle", "agentId", "operation", "expiresAt"}:
                raise ReadAccessUnavailable()
            principal = self._principal_from_metadata(metadata, operation, before)
            guard._remaining()
            if principal is None:
                raise ReadAccessUnavailable()
            result = ReadAdmission(principal, guard)
            guard = None  # Ownership handed off exactly once to runtime finally.
            return result
        except Exception:
            if guard is not None:
                try:
                    guard.retire()
                except Exception:
                    pass
            raise ReadAccessUnavailable() from None

    def close(self):
        self.channel.close()
