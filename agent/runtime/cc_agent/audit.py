"""Bounded redacted local audit, explicitly not a tamper-proof audit ledger."""

from collections import deque
import json
import os
from pathlib import Path
import threading
import uuid

from .history import finite_number, reject_links

REASONS = {"scope_denied", "operation_unavailable", "invalid_request", "duplicate_header", "admitted"}


class AuditLog:
    MAX_BYTES = 65536
    MAX_EVENTS = 256

    def __init__(self, directory=None):
        self._events = deque()
        self._size = 0
        self.evicted_events = 0
        self.state = "memory_only"
        self._blocked = False
        self._lock = threading.RLock()
        self._path = Path(directory) / "audit.jsonl" if directory is not None else None
        if self._path is not None and self._path.exists():
            try:
                reject_links(self._path)
                if self._path.stat().st_size > self.MAX_BYTES // 2:
                    raise ValueError("invalid audit storage")
                with self._path.open("rb") as stream:
                    for line in iter(lambda: stream.readline(513), b""):
                        event = json.loads(line)
                        if len(line) > 512 or not self._valid(event) or len(self._events) >= self.MAX_EVENTS:
                            raise ValueError("invalid audit event")
                        encoded = self._encode(event)
                        self._events.append((event, encoded))
                        self._size += len(encoded)
                self.state = "ready"
            except (OSError, ValueError, UnicodeError):
                self._blocked = True
                self.state = "corrupt_audit_refused"
                self._events.clear()
                self._size = 0

    @staticmethod
    def _encode(event):
        return json.dumps(event, allow_nan=False, separators=(",", ":")).encode() + b"\n"

    @staticmethod
    def _valid(event):
        try:
            return (isinstance(event, dict) and set(event) == {"timestamp", "agent_id", "operation", "resource", "reason"}
                    and finite_number(event["timestamp"]) and event["timestamp"] >= 0
                    and str(uuid.UUID(event["agent_id"])) == event["agent_id"]
                    and (event["resource"] is None or str(uuid.UUID(event["resource"])) == event["resource"])
                    and event["operation"] in {"telemetry.read", "unavailable"} and event["reason"] in REASONS)
        except (TypeError, ValueError, AttributeError):
            return False

    def __call__(self, event):
        if not self._valid(event) or self._blocked:
            return False
        with self._lock:
            encoded = self._encode(event)
            self._events.append((dict(event), encoded))
            self._size += len(encoded)
            while len(self._events) > self.MAX_EVENTS or self._size > self.MAX_BYTES // 2:
                _, old = self._events.popleft()
                self._size -= len(old)
                self.evicted_events += 1
            if self._path is None:
                return True
            temporary = None
            try:
                reject_links(self._path)
                pending = self._path.parent / ".audit.pending"
                fd = os.open(pending, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
                temporary = pending
                with os.fdopen(fd, "wb") as stream:
                    for _, value in self._events:
                        stream.write(value)
                    stream.flush()
                    os.fsync(stream.fileno())
                os.replace(temporary, self._path)
                temporary = None
                self.state = "ready"
                return True
            except (OSError, ValueError):
                self.state = "write_failed_memory_only"
                return False
            finally:
                if temporary is not None:
                    temporary.unlink(missing_ok=True)

    def status(self):
        with self._lock:
            return {"state": self.state, "events": len(self._events), "evicted_events": self.evicted_events,
                    "bytes": self._size, "budget_bytes": self.MAX_BYTES, "tamper_proof": False}

    def events(self):
        with self._lock:
            return [dict(event) for event, _ in self._events]
