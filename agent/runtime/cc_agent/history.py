"""Two agreed Agent gauges plus bounded collection-state diagnostics; no OS scrape."""

from collections import deque
import json
import math
import os
from pathlib import Path
import stat
import threading

METRICS = ("agent_uptime_seconds", "agent_collector_running")
# State is diagnostic metadata, not a third numeric metric or collection grant.
# not_running is used ONLY when an older record proves 0 but not its cause.
COLLECTION_STATES = frozenset(("running", "disabled", "stopped", "not_running",
                               "starting", "degraded", "blocked", "unknown"))
_STATE_RUNNING = {name: (1 if name == "running" else 0 if name in
                  {"disabled", "stopped", "not_running"} else None)
                  for name in COLLECTION_STATES}


def _legacy_collection_state(running):
    return "unknown" if running is None else "running" if running == 1 else "not_running"


def _decode_record(raw):
    def unique(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate history field")
            result[key] = value
        return result
    def reject_constant(_):
        raise ValueError("nonfinite history value")
    return json.loads(raw, object_pairs_hook=unique, parse_constant=reject_constant)


def finite_number(value):
    if type(value) not in (int, float):
        return False
    try:
        return math.isfinite(value)
    except OverflowError:
        return False


def reject_links(path):
    for component in (path, *path.parents):
        if component.exists() or component.is_symlink():
            info = component.lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
                raise ValueError("symlink/reparse state path refused")


class LocalHistory:
    def __init__(self, manifest, state_dir=None, clock=None):
        import time
        self.manifest = manifest
        self.clock = clock or time.time
        self._points = deque()
        self._bytes = 0
        self._evicted = 0
        self._lock = threading.RLock()
        self._path = None
        self._persistent_blocked = False
        self.storage_state = "memory"
        if state_dir is not None:
            directory = Path(state_dir)
            reject_links(directory)
            directory.mkdir(mode=0o700, parents=True, exist_ok=True)
            reject_links(directory)
            if os.name != "nt" and (directory.stat().st_mode & 0o077 or directory.stat().st_uid != os.geteuid()):
                raise ValueError("state directory must be private")
            self._path = directory / "history.jsonl"
            reject_links(self._path)
            self._restore()

    @staticmethod
    def _encode(point):
        return json.dumps(point, allow_nan=False, separators=(",", ":")).encode("utf-8") + b"\n"

    def _valid(self, point):
        return (isinstance(point, dict) and set(point) == {"timestamp", *METRICS, "collection_state", "collection_state_version"}
                and all(finite_number(point[name]) for name in ("timestamp", "agent_uptime_seconds"))
                and point["timestamp"] >= 0 and point["agent_uptime_seconds"] >= 0
                and (point["agent_collector_running"] is None or type(point["agent_collector_running"]) is int
                     and point["agent_collector_running"] in (0, 1))
                and type(point["collection_state_version"]) is int
                and point["collection_state_version"] == 1
                and type(point["collection_state"]) is str
                and point["collection_state"] in COLLECTION_STATES
                and point["agent_collector_running"] == _STATE_RUNNING[point["collection_state"]])

    def _restore(self):
        if not self._path.exists():
            return
        if self._path.stat().st_size > self.manifest.history_max_bytes // 2:
            self.storage_state = "oversize_history_refused"
            self._persistent_blocked = True
            return
        try:
            with self._path.open("rb") as stream:
                for line in iter(lambda: stream.readline(1025), b""):
                    if len(line) > 1024:
                        raise ValueError("oversize history record")
                    point = _decode_record(line)
                    if type(point) is dict and set(point) == {"timestamp", *METRICS}:
                        # Old 0 says not running, not disabled/stopped specifically.
                        # Old null never becomes zero or a successful observation.
                        point = {**point, "collection_state":
                                 _legacy_collection_state(point["agent_collector_running"]),
                                 "collection_state_version": 1}
                    if not self._valid(point) or (self._points and point["timestamp"] < self._points[-1][0]["timestamp"]):
                        raise ValueError("invalid history record")
                    encoded = self._encode(point)
                    self._points.append((point, encoded))
                    self._bytes += len(encoded)
                    if len(self._points) > self.manifest.history_max_points:
                        raise ValueError("excess history records")
            self._prune(self.clock())
            self.storage_state = "ready"
        except (ValueError, UnicodeError, OSError):
            self._points.clear()
            self._bytes = 0
            self.storage_state = "corrupt_history_refused"
            self._persistent_blocked = True

    def _prune(self, now):
        while self._points and (len(self._points) > self.manifest.history_max_points
                or self._bytes > self.manifest.history_max_bytes // 2
                or self._points[0][0]["timestamp"] < now - self.manifest.history_max_age_seconds):
            _, encoded = self._points.popleft()
            self._bytes -= len(encoded)
            self._evicted += 1

    def append(self, timestamp, uptime, collection_up, *, collection_state=None):
        running = None if collection_up is None else int(collection_up)
        state = _legacy_collection_state(running) if collection_state is None else collection_state
        point = {"timestamp": timestamp, "agent_uptime_seconds": uptime,
                 "agent_collector_running": running, "collection_state": state,
                 "collection_state_version": 1}
        if not self._valid(point):
            raise ValueError("invalid agreed history metric")
        with self._lock:
            if self._points and timestamp < self._points[-1][0]["timestamp"]:
                self.storage_state = "clock_rollback_paused"
                return False
            encoded = self._encode(point)
            self._points.append((point, encoded))
            self._bytes += len(encoded)
            self._prune(timestamp)
            self._persist()
        return True

    def _persist(self):
        if self._path is None or self._persistent_blocked:
            return
        temporary = None
        try:
            # Reserve half the history budget for atomic replacement. No other
            # files (in particular collector queues) are touched or counted here.
            reject_links(self._path)
            pending = self._path.parent / ".history.pending"
            fd = os.open(pending, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
            temporary = pending
            with os.fdopen(fd, "wb") as stream:
                for _, encoded in self._points:
                    stream.write(encoded)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, self._path)
            temporary = None
            self.storage_state = "ready"
        except (OSError, ValueError):
            self.storage_state = "write_failed_memory_only"
        finally:
            if temporary is not None:
                Path(temporary).unlink(missing_ok=True)

    def query(self, limit=1000):
        if type(limit) is not int or not 1 <= limit <= 1000:
            raise ValueError("history query limit")
        with self._lock:
            self._prune(self.clock())
            points = list(self._points)
            selected = points[-limit:]
            return {"metrics": list(METRICS), "points": [dict(p) for p, _ in selected],
                    "truncated": len(points) > limit, "evicted_points": self._evicted,
                    "stored_bytes": self._bytes, "budget_bytes": self.manifest.history_max_bytes,
                    "effective_data_budget_bytes": self.manifest.history_max_bytes // 2,
                    "max_age_seconds": self.manifest.history_max_age_seconds,
                    "actual_start": selected[0][0]["timestamp"] if selected else None,
                    "actual_end": selected[-1][0]["timestamp"] if selected else None,
                    "storage_state": self.storage_state, "available_while_agent_online_only": True}
