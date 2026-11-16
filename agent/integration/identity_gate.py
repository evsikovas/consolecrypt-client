"""One-shot Ed25519 admission before an exact read-only telemetry handler.

The header carries signed statements, never a scope boolean or a vault token.
This adapter deliberately supports only two fixed GET resources. Identity v1
binds an operation/resource, not an arbitrary HTTP method/path/body transcript.
"""
from __future__ import annotations

import base64
import binascii
from dataclasses import dataclass
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import sys
import time
from typing import Callable
import uuid

MAX_BUNDLE_BYTES = 10 * 1024
HEADER = "x-consolecrypt-proof"


class _TrustedClockInvalid(ValueError):
    """Distinguish unavailable local time from an invalid request."""


def strict_json(raw: str | bytes):
    def object_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON key")
            result[key] = value
        return result

    return json.loads(raw, object_pairs_hook=object_pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError("nonfinite JSON")))


@dataclass(frozen=True)
class GateConfig:
    executable: Path
    executable_sha256: str
    store: Path
    anchor: Path
    owner_pin: Path
    timeout_seconds: float = 5.0

    def __post_init__(self):
        for path in (self.executable, self.store, self.anchor, self.owner_pin):
            if not isinstance(path, Path) or not path.is_absolute():
                raise ValueError("identity paths must be trusted absolute paths")
        if not re.fullmatch(r"[0-9a-f]{64}", self.executable_sha256):
            raise ValueError("identity executable SHA-256 required")
        if not 0 < self.timeout_seconds <= 10:
            raise ValueError("identity timeout outside budget")
        if self.store.resolve() == self.anchor.resolve():
            raise ValueError("checkpoint anchor must be separate from journal")

    @classmethod
    def from_file(cls, path: Path):
        if path.stat().st_size > 4096:
            raise ValueError("identity gate config too large")
        obj = strict_json(path.read_bytes())
        return cls.from_dict(obj)

    @classmethod
    def from_dict(cls, obj):
        if not isinstance(obj, dict) or set(obj) != {
            "executable", "executable_sha256", "store", "anchor", "owner_pin"
        }:
            raise ValueError("invalid identity gate configuration")
        return cls(**{key: Path(value) if key != "executable_sha256" else value
                      for key, value in obj.items()})


class IdentityGate:
    def __init__(self, config: GateConfig, agent_id: str, resources: dict[str, str],
                 *, clock: Callable[[], float] = time.time,
                 run: Callable = subprocess.run):
        self.config = config
        self.agent_id = str(uuid.UUID(agent_id))
        if set(resources) != {"/v1/heartbeat", "/v1/history"}:
            raise ValueError("exact telemetry resource map required")
        self.resources = {path: str(uuid.UUID(resource)) for path, resource in resources.items()}
        if len(set(self.resources.values())) != 2:
            raise ValueError("each handler requires a distinct resource")
        self.clock, self.run = clock, run

    def __call__(self, request):
        # Any verifier failure, timeout, corruption, or missing checkpoint denies.
        try:
            return self._admit(request)
        except (ValueError, TypeError, KeyError, OSError, subprocess.SubprocessError,
                binascii.Error, UnicodeError, RecursionError):
            return None

    def _clock_seconds(self):
        from cc_agent.read_expiry import MAX_TIMESTAMP

        try:
            value = self.clock()
            if (type(value) not in (int, float) or not 0 <= value <= MAX_TIMESTAMP
                    or not math.isfinite(value)):
                raise _TrustedClockInvalid("invalid trusted identity clock")
            return int(value)
        except Exception:
            raise _TrustedClockInvalid("invalid trusted identity clock") from None

    def _prepare_request(self, request):
        resource = self.resources.get(request.path)
        if (request.method != "GET" or request.body or resource is None
                or request.operation != "telemetry.read" or request.resource != resource):
            return None
        encoded = {key.lower(): value for key, value in request.headers.items()}.get(HEADER, "")
        if not isinstance(encoded, str) or not 0 < len(encoded) <= MAX_BUNDLE_BYTES * 4 // 3:
            return None
        if not re.fullmatch(r"[A-Za-z0-9_-]+={0,2}", encoded):
            return None
        raw = base64.b64decode(encoded + "=" * (-len(encoded) % 4), altchars=b"-_", validate=True)
        if len(raw) > MAX_BUNDLE_BYTES:
            return None
        bundle = strict_json(raw)
        if not isinstance(bundle, dict) or set(bundle) != {"lease", "proof", "freshness"}:
            return None
        operation = {"kind": "telemetryRead", "source": resource}
        # Precheck context before consuming the one-shot lease. Rust checks all
        # signatures, freshness, certificate epoch, scope, nonce and tombstones.
        if (bundle["lease"]["body"]["agentId"] != self.agent_id
                or bundle["lease"]["body"]["operation"] != operation):
            return None
        now = self._clock_seconds()
        payload = {**bundle, "now": now, "operation": operation}
        return operation, payload, now

    def _executable_valid(self):
        if self.config.executable.is_symlink():
            return False
        with self.config.executable.open("rb") as binary:
            return hashlib.file_digest(binary, "sha256").hexdigest() == self.config.executable_sha256

    def _principal_from_metadata(self, result, operation, before_verify):
        from cc_agent.read_expiry import MAX_TIMESTAMP
        from cc_agent.service import Principal

        if (not isinstance(result, dict) or result.get("agentId") != self.agent_id or result.get("operation") != operation
                or type(result.get("expiresAt")) is not int
                or not 0 <= result["expiresAt"] <= MAX_TIMESTAMP):
            return None
        verified_at = max(before_verify, self._clock_seconds())
        if result["expiresAt"] <= verified_at:
            return None
        # Local observations copied only after real verifier success. These are
        # not a retained Permit, a revocation witness or a caller-supplied grant.
        return Principal(agent_id=self.agent_id, scopes=frozenset({"telemetry.read"}),
                         verified_at=verified_at, verified_expires_at=result["expiresAt"])

    def _admit(self, request):
        prepared = self._prepare_request(request)
        if prepared is None:
            return None
        operation, payload, now = prepared
        if not self._executable_valid():
            return None
        completed = self.run(
            [str(self.config.executable), "verify-request", "--store", str(self.config.store),
             "--anchor", str(self.config.anchor), "--owner-pin", str(self.config.owner_pin)],
            input=json.dumps(payload, separators=(",", ":")).encode(),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=self.config.timeout_seconds,
            check=False, shell=False,
        )
        if completed.returncode or len(completed.stdout) > 4096:
            return None
        return self._principal_from_metadata(strict_json(completed.stdout), operation, now)


def load_gate(path, agent_id, resources):
    """Explicit provider selection; legacy packages do not import worker code."""
    with Path(path).open("rb") as source:
        raw = source.read(4097)
    if len(raw) > 4096:
        raise ValueError("identity gate config too large")
    data = strict_json(raw)
    if not isinstance(data, dict):
        raise ValueError("invalid identity gate config")
    provider = data.pop("provider", "file")
    if not isinstance(provider, str) or provider not in {"file", "sqlite-worker", "sqlite-custody-worker"}:
        raise ValueError("unknown identity provider; no fallback")
    config = GateConfig.from_dict(data)
    if provider == "sqlite-custody-worker":
        if sys.platform != "linux":
            raise ValueError("custody provider requires Linux")
        from .sqlite_custody_gate import SqliteCustodyGate
        return SqliteCustodyGate(config, agent_id, resources)
    if provider == "sqlite-worker":
        from .sqlite_worker_gate import SqliteWorkerGate
        return SqliteWorkerGate(config, agent_id, resources)
    return IdentityGate(config, agent_id, resources)
