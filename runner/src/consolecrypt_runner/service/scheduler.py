"""Private logical scheduling only. No command, manifest, credential or dispatch."""
from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass
from datetime import date, datetime, time, timezone
from enum import Enum
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import threading
import stat
import sys
import uuid
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

MAX_SCHEDULES = 128
MAX_SLOTS = 16384  # Tombstones are never silently pruned/reused.
MAX_DEFINITION_BYTES = 4096
MAX_UTC = 253402300799
MAX_REVISION = 9007199254740991
MAX_DRIFT_SECONDS = 5


class SchedulerError(Exception):
    def __init__(self, code: str):
        self.code = code
        super().__init__(code)


def require(value, code):
    if not value:
        raise SchedulerError(code)


def identifier(value):
    require(type(value) is str and len(value) == 36, "invalid_id")
    try:
        parsed = uuid.UUID(value)
    except ValueError:
        raise SchedulerError("invalid_id") from None
    require(parsed.int != 0 and str(parsed) == value, "invalid_id")
    return value


def integer(value, low, high, code="invalid_number"):
    require(type(value) is int and low <= value <= high, code)
    return value


def unique_object(pairs):
    value = {}
    for key, child in pairs:
        require(key not in value, "duplicate_field")
        value[key] = child
    return value


@dataclass(frozen=True)
class Definition:
    zone: str
    hour: int
    minute: int
    weekdays: tuple[int, ...] = (0, 1, 2, 3, 4, 5, 6)
    grace_seconds: int = 60
    gap_policy: str = "skip"
    fold_policy: str = "first"
    missed_policy: str = "skip"

    def __post_init__(self):
        require(type(self.zone) is str and 0 < len(self.zone) <= 128 and self.zone.isascii()
                and not self.zone.startswith("/") and all(
                    part and part not in (".", "..") and
                    all(c.isalnum() or c in "_+-" for c in part)
                    for part in self.zone.split("/")), "invalid_zone")
        try:
            ZoneInfo(self.zone)
        except (ZoneInfoNotFoundError, ValueError):
            raise SchedulerError("zone_unavailable") from None
        integer(self.hour, 0, 23)
        integer(self.minute, 0, 59)
        integer(self.grace_seconds, 0, 60)
        require(type(self.weekdays) is tuple and 0 < len(self.weekdays) <= 7
                and all(type(v) is int and 0 <= v <= 6 for v in self.weekdays)
                and len(set(self.weekdays)) == len(self.weekdays), "invalid_weekdays")
        require(self.gap_policy == "skip" and self.fold_policy == "first"
                and self.missed_policy == "skip", "unsupported_policy")

    def encoded(self):
        return json.dumps({"version": 1, "zone": self.zone, "hour": self.hour,
            "minute": self.minute, "weekdays": sorted(self.weekdays),
            "graceSeconds": self.grace_seconds, "gapPolicy": self.gap_policy,
            "foldPolicy": self.fold_policy, "missedPolicy": self.missed_policy},
            sort_keys=True, separators=(",", ":"))

    @staticmethod
    def parse(raw):
        require(type(raw) is str and len(raw.encode()) <= MAX_DEFINITION_BYTES, "definition_budget")
        try:
            value = json.loads(raw, object_pairs_hook=unique_object)
        except (ValueError, RecursionError):
            raise SchedulerError("invalid_definition") from None
        require(type(value) is dict and set(value) == {"version", "zone", "hour", "minute",
            "weekdays", "graceSeconds", "gapPolicy", "foldPolicy", "missedPolicy"}, "invalid_definition")
        require(type(value["version"]) is int and value["version"] == 1, "unsupported_version")
        require(type(value["weekdays"]) is list, "invalid_weekdays")
        return Definition(value["zone"], value["hour"], value["minute"], tuple(value["weekdays"]),
                          value["graceSeconds"], value["gapPolicy"], value["foldPolicy"], value["missedPolicy"])


@dataclass(frozen=True)
class Clock:
    utc_second: int
    monotonic_ns: int
    epoch: str  # Original process/clock observation identity, NOT authority.

    def __post_init__(self):
        integer(self.utc_second, 0, MAX_UTC)
        integer(self.monotonic_ns, 0, 9223372036854775807)
        identifier(self.epoch)

    @staticmethod
    def from_utc(value, monotonic_ns, epoch):
        require(type(value) is datetime and value.tzinfo is timezone.utc, "utc_required")
        require(value.microsecond == 0, "whole_second_required")
        return Clock(int(value.timestamp()), monotonic_ns, epoch)


class Status(str, Enum):
    RESERVED = "reserved"
    NOT_DUE = "not_due"
    SEEN = "already_seen"
    SKIPPED_GAP = "skipped_gap"
    SKIPPED_MISSED = "skipped_missed"
    SKIPPED_OVERLAP = "skipped_overlap"
    SKIPPED_ACTIVATION = "skipped_before_activation"
    CLOCK_BASELINE = "clock_restart_baseline"
    CLOCK_FORWARD = "clock_forward_skip"
    CLOCK_ROLLBACK = "clock_rollback_blocked"


@dataclass(frozen=True)
class Reservation:
    schedule_id: str
    schedule_revision: int
    local_slot: str
    due_utc: int
    token: str
    definition_digest: str
    # Logical reservation only. It does not contain/authorize an executable job.


@dataclass(frozen=True)
class Tick:
    status: Status
    reservation: Reservation | None = None


@dataclass(frozen=True)
class ScheduleView:
    schedule_id: str
    revision: int
    definition: Definition
    not_before_utc: int
    enabled: bool


@dataclass(frozen=True)
class UnresolvedView:
    schedule_id: str
    original_revision: int
    local_slot: str
    due_utc: int
    observed_utc: int
    definition_digest: str
    # No token, material, executable payload or restart/release authority.


def first_occurrence(definition: Definition, local_day: date):
    """UTC round trip rejects missing wall times; fold chooses first, not both."""
    wall = datetime.combine(local_day, time(definition.hour, definition.minute))
    zone = ZoneInfo(definition.zone)
    possible = set()
    for fold in (0, 1):
        try:
            candidate = wall.replace(tzinfo=zone, fold=fold).astimezone(timezone.utc)
        except (OverflowError, ValueError):
            raise SchedulerError("slot_out_of_range") from None
        if candidate.astimezone(zone).replace(tzinfo=None) == wall:
            possible.add(int(candidate.timestamp()))
    return min(possible) if possible else None


SCHEMA = [
    """CREATE TABLE clock_state(id INTEGER PRIMARY KEY CHECK(id=1),
        highwater INTEGER NOT NULL, epoch TEXT NOT NULL, monotonic_ns INTEGER NOT NULL,
        eligibility_floor INTEGER NOT NULL)""",
    """CREATE TABLE schedules(id TEXT PRIMARY KEY, revision INTEGER NOT NULL,
        definition TEXT NOT NULL, not_before INTEGER NOT NULL, enabled INTEGER NOT NULL CHECK(enabled IN(0,1)))""",
    """CREATE TABLE slots(schedule_id TEXT NOT NULL REFERENCES schedules(id), local_slot TEXT NOT NULL,
        schedule_revision INTEGER NOT NULL, due_utc INTEGER, observed_utc INTEGER NOT NULL,
        state TEXT NOT NULL, token TEXT, definition TEXT NOT NULL,
        definition_digest TEXT NOT NULL, PRIMARY KEY(schedule_id,local_slot))""",
    "CREATE UNIQUE INDEX one_unresolved_slot ON slots(schedule_id) WHERE state='reserved'",
]


class Store:
    """Owned Linux directory, FULL SQLite commits, bounded metadata, no job store.

    Private permissions are an other-UID boundary, not isolation from a malicious
    same-UID process. No migration, automatic tombstone cleanup or reset exists.
    """
    def __init__(self, root: Path, *, _existing_only=False):
        self._mutex = threading.RLock()
        self.db = None
        self._schema_ready = False
        try:
            self._open(root, _existing_only)
        except BaseException as error:
            self.close()
            if isinstance(error, (OSError, sqlite3.Error)):
                raise SchedulerError("private_storage_unavailable") from None
            raise

    def _open(self, root, existing_only):
        require(sys.platform == "linux" and hasattr(os, "O_NOFOLLOW"), "unsupported_platform")
        self.root = Path(root).absolute()
        if not existing_only:
            self.root.mkdir(mode=0o700, exist_ok=True)
        info = self.root.lstat()
        require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
                and info.st_mode & 0o077 == 0, "private_directory_required")
        self._root_identity = (info.st_dev, info.st_ino)
        path = self.root / "schedule.sqlite"
        if existing_only:
            # No absent-file/schema adoption, hot-journal recovery or unknown
            # sibling cleanup during restoration of an enrolled service.
            with os.scandir(self.root) as entries:
                names = []
                for entry in entries:
                    require(len(names) < 1, "unsupported_schema")
                    names.append(entry.name)
            require(names == ["schedule.sqlite"], "unsupported_schema")
            fd = os.open(path, os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC)
        else:
            try:
                fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
            except FileExistsError:
                fd = os.open(path, os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            before = os.fstat(fd)
            require(stat.S_ISREG(before.st_mode) and before.st_size <= 64*1024*1024 and before.st_uid == os.getuid()
                    and before.st_nlink == 1 and before.st_mode & 0o077 == 0, "private_database_required")
            if existing_only:
                require(before.st_size>=100 and os.pread(fd,16,0)==b"SQLite format 3\0", "unsupported_schema")
            self._database_identity = (before.st_dev, before.st_ino)
            self.db = sqlite3.connect(path.as_uri() + "?mode=rw", uri=True, timeout=1,
                                      isolation_level=None, check_same_thread=False)
            after = path.lstat()
            require((before.st_dev, before.st_ino) == (after.st_dev, after.st_ino)
                    and stat.S_ISREG(after.st_mode), "database_replaced")
        finally:
            os.close(fd)
        try:
            self.db.execute("PRAGMA foreign_keys=ON")
            self.db.execute("PRAGMA trusted_schema=OFF")
            self.db.execute("PRAGMA synchronous=FULL")
            require(self.db.execute("PRAGMA page_size").fetchone()[0] == 4096, "unsupported_page_size")
            require(self.db.execute("PRAGMA journal_mode").fetchone()[0] == "delete", "unsupported_journal_mode")
            with self.transaction():
                version = self.db.execute("PRAGMA user_version").fetchone()[0]
                tables = {r[0] for r in self.db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT GLOB 'sqlite_*' LIMIT 4")}
                if version == 0:
                    require(not existing_only and not tables, "unsupported_schema")
                    for command in SCHEMA:
                        self.db.execute(command)
                    self.db.execute("PRAGMA user_version=1")
                else:
                    require(version == 1 and tables == {"clock_state", "schedules", "slots"}, "unsupported_schema")
            self.db.execute("PRAGMA max_page_count=16384")  # connection growth limit, no row/schema mutation.
            self._schema_ready = True
        except BaseException:
            self.db.close()
            raise

    def close(self):
        with self._mutex:
            if self.db is not None:
                self.db.close()
                self.db = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    @contextmanager
    def transaction(self):
        # One existing connection, complete transaction custody across helper
        # workers. No SQLite use/close may interleave inside BEGIN..COMMIT.
        with self._mutex:
            require(self.db is not None, "store_closed")
            self._check_files()
            try:
                self.db.execute("BEGIN IMMEDIATE")
                if self._schema_ready:
                    self._check_schema()
                yield
                self._check_files()
                self.db.execute("COMMIT")
            except BaseException as error:
                if self.db.in_transaction:
                    self.db.execute("ROLLBACK")
                if isinstance(error, sqlite3.Error):
                    raise SchedulerError("storage_unavailable") from None
                raise

    def _check_schema(self):
        version = self.db.execute("PRAGMA user_version").fetchone()[0]
        require(version == 1, "unsupported_schema")
        tables = {r[0] for r in self.db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT GLOB 'sqlite_*' LIMIT 4")}
        require(tables == {"clock_state", "schedules", "slots"}, "unsupported_schema")

    def _check_files(self):
        try:
            self._check_files_opened()
        except OSError:
            raise SchedulerError("storage_path_changed") from None

    def _check_files_opened(self):
        info = self.root.lstat()
        require(stat.S_ISDIR(info.st_mode) and (info.st_dev, info.st_ino) == self._root_identity
                and info.st_uid == os.getuid() and info.st_mode & 0o077 == 0, "directory_replaced")
        database = (self.root / "schedule.sqlite").lstat()
        require(stat.S_ISREG(database.st_mode)
                and (database.st_dev, database.st_ino) == self._database_identity
                and database.st_uid == os.getuid() and database.st_mode & 0o077 == 0
                and database.st_nlink == 1, "database_replaced")

    def _clock(self, clock: Clock):
        require(type(clock) is Clock, "clock_required")
        previous = self.db.execute("SELECT highwater,epoch,monotonic_ns,eligibility_floor FROM clock_state WHERE id=1").fetchone()
        if previous is None:
            self.db.execute("INSERT INTO clock_state VALUES(1,?,?,?,?)",
                            (clock.utc_second, clock.epoch, clock.monotonic_ns, -1))
            return None, -1
        highwater, epoch, monotonic, floor = previous
        if clock.utc_second < highwater or (clock.epoch == epoch and clock.monotonic_ns < monotonic):
            return Status.CLOCK_ROLLBACK, floor
        if clock.epoch != epoch:
            status, floor = Status.CLOCK_BASELINE, max(floor, clock.utc_second)
        else:
            elapsed = (clock.monotonic_ns - monotonic) // 1_000_000_000
            drift = clock.utc_second - highwater - elapsed
            if drift < -MAX_DRIFT_SECONDS:
                return Status.CLOCK_ROLLBACK, floor
            status = Status.CLOCK_FORWARD if drift > MAX_DRIFT_SECONDS else None
            if status:
                floor = max(floor, clock.utc_second)
        self.db.execute("UPDATE clock_state SET highwater=?,epoch=?,monotonic_ns=?,eligibility_floor=? WHERE id=1",
                        (clock.utc_second, clock.epoch, clock.monotonic_ns, floor))
        return status, floor

    def save(self, schedule_id: str, expected_revision: int, definition: Definition, clock: Clock,
             *, enabled: bool = True):
        identifier(schedule_id)
        integer(expected_revision, 0, MAX_REVISION - 1)
        require(type(definition) is Definition, "definition_required")
        require(type(enabled) is bool, "enabled_required")
        with self.transaction():
            row = self.db.execute("SELECT revision,definition FROM schedules WHERE id=?", (schedule_id,)).fetchone()
            require((row[0] if row else 0) == expected_revision, "revision_changed")
            if row is not None:
                Definition.parse(row[1])  # Future definition bytes are never downgraded by save.
            if row is None:
                require(self.db.execute("SELECT count(*) FROM schedules").fetchone()[0] < MAX_SCHEDULES, "schedule_quota")
            status, _ = self._clock(clock)
            require(status != Status.CLOCK_ROLLBACK, "clock_rollback")
            revision = expected_revision + 1
            self.db.execute("INSERT INTO schedules VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,definition=excluded.definition,not_before=excluded.not_before,enabled=excluded.enabled",
                            (schedule_id, revision, definition.encoded(), clock.utc_second, int(enabled)))
            return revision

    def tick(self, schedule_id: str, expected_revision: int, clock: Clock):
        identifier(schedule_id)
        integer(expected_revision, 1, MAX_REVISION)
        with self.transaction():
            row = self.db.execute("SELECT revision,definition,not_before,enabled FROM schedules WHERE id=?", (schedule_id,)).fetchone()
            require(row is not None and row[0] == expected_revision, "revision_changed")
            definition = Definition.parse(row[1])
            status, floor = self._clock(clock)
            if status:
                return Tick(status)
            if not row[3]:
                return Tick(Status.NOT_DUE)
            try:
                now = datetime.fromtimestamp(clock.utc_second, timezone.utc).astimezone(ZoneInfo(definition.zone))
            except (OverflowError, ValueError):
                raise SchedulerError("clock_out_of_zone_range") from None
            if now.weekday() not in definition.weekdays:
                return Tick(Status.NOT_DUE)
            local_slot = f"{now.date().isoformat()}T{definition.hour:02}:{definition.minute:02}"
            due = first_occurrence(definition, now.date())
            if due is None and (now.hour, now.minute) < (definition.hour, definition.minute):
                return Tick(Status.NOT_DUE)
            if due is not None and clock.utc_second < due:
                return Tick(Status.NOT_DUE)
            if self.db.execute("SELECT 1 FROM slots WHERE schedule_id=? AND local_slot=?", (schedule_id, local_slot)).fetchone():
                return Tick(Status.SEEN)
            require(self.db.execute("SELECT count(*) FROM slots").fetchone()[0] < MAX_SLOTS, "slot_quota")
            if due is None:
                result = Status.SKIPPED_GAP
            elif due < row[2] or due <= floor:
                result = Status.SKIPPED_ACTIVATION
            elif clock.utc_second - due > definition.grace_seconds:
                result = Status.SKIPPED_MISSED
            elif self.db.execute("SELECT 1 FROM slots WHERE schedule_id=? AND state='reserved'", (schedule_id,)).fetchone():
                result = Status.SKIPPED_OVERLAP
            else:
                result = Status.RESERVED
            token = str(uuid.uuid4()) if result is Status.RESERVED else None
            encoded = definition.encoded()
            digest = hashlib.sha256(encoded.encode()).hexdigest()
            self.db.execute("INSERT INTO slots VALUES(?,?,?,?,?,?,?,?,?)", (schedule_id, local_slot,
                expected_revision, due, clock.utc_second, result.value, token, encoded, digest))
            reservation = Reservation(schedule_id, expected_revision, local_slot, due, token, digest) if token else None
            return Tick(result, reservation)

    def resolve(self, reservation: Reservation, disposition: str):
        """Record caller observation, NOT verified job success. Lost token stays blocked.

        No auto-expiry/retry/recovery release: future original service admission
        and explicit reconciliation must supply their own independent evidence.
        """
        require(type(reservation) is Reservation, "reservation_required")
        require(disposition in ("not_started", "finished"), "unsupported_disposition")
        identifier(reservation.schedule_id)
        identifier(reservation.token)
        with self.transaction():
            current = self.db.execute("SELECT definition FROM schedules WHERE id=?", (reservation.schedule_id,)).fetchone()
            require(current is not None, "revision_changed")
            Definition.parse(current[0])
            # The current schedule and the retained original slot can have
            # DIFFERENT schema/revisions. Never interpret/finish a future or
            # corrupted original using only today's supported schedule.
            original = self.db.execute("SELECT definition,definition_digest FROM slots WHERE schedule_id=? AND local_slot=? AND schedule_revision=? AND due_utc=? AND token=? AND definition_digest=? AND state='reserved'",
                (reservation.schedule_id, reservation.local_slot,
                 reservation.schedule_revision, reservation.due_utc,
                 reservation.token, reservation.definition_digest)).fetchone()
            require(original is not None, "reservation_changed")
            Definition.parse(original[0])
            require(type(original[1]) is str and original[1] ==
                    hashlib.sha256(original[0].encode()).hexdigest(), "invalid_slot_metadata")
            changed = self.db.execute("UPDATE slots SET state=? WHERE schedule_id=? AND local_slot=? AND schedule_revision=? AND due_utc=? AND token=? AND definition_digest=? AND state='reserved'",
                (disposition, reservation.schedule_id, reservation.local_slot,
                 reservation.schedule_revision, reservation.due_utc, reservation.token,
                 reservation.definition_digest)).rowcount
            require(changed == 1, "reservation_changed")

    def schedules(self):
        """Bounded immutable recovery metadata; does not tick, claim or authorize."""
        with self.transaction():
            rows = self.db.execute("SELECT id,revision,definition,not_before,enabled FROM schedules ORDER BY id LIMIT ?",
                                   (MAX_SCHEDULES + 1,)).fetchall()
            require(len(rows) <= MAX_SCHEDULES, "schedule_quota")
            views = []
            for schedule_id, revision, definition, not_before, enabled in rows:
                identifier(schedule_id)
                integer(revision, 1, MAX_REVISION)
                integer(not_before, 0, MAX_UTC)
                require(type(enabled) is int and enabled in (0, 1), "invalid_schedule_metadata")
                views.append(ScheduleView(schedule_id, revision, Definition.parse(definition), not_before, bool(enabled)))
            return tuple(views)

    def unresolved(self, schedule_id, expected_revision):
        """Original unresolved slot only, without its token or an auto-release path."""
        identifier(schedule_id)
        integer(expected_revision, 1, MAX_REVISION)
        with self.transaction():
            current = self.db.execute("SELECT revision,definition FROM schedules WHERE id=?", (schedule_id,)).fetchone()
            require(current is not None and current[0] == expected_revision, "revision_changed")
            Definition.parse(current[1])
            rows = self.db.execute("SELECT schedule_revision,local_slot,due_utc,observed_utc,definition,definition_digest FROM slots WHERE schedule_id=? AND state='reserved' LIMIT 2",
                                   (schedule_id,)).fetchall()
            require(len(rows) <= 1, "invalid_slot_metadata")
            if not rows:
                return None
            revision, local_slot, due, observed, definition, digest = rows[0]
            integer(revision, 1, MAX_REVISION)
            integer(due, 0, MAX_UTC)
            integer(observed, 0, MAX_UTC)
            require(type(local_slot) is str and len(local_slot) == 16, "invalid_slot_metadata")
            try:
                wall = datetime.fromisoformat(local_slot)
            except ValueError:
                raise SchedulerError("invalid_slot_metadata") from None
            require(wall.tzinfo is None and wall.isoformat(timespec="minutes") == local_slot, "invalid_slot_metadata")
            Definition.parse(definition)
            require(type(digest) is str and digest == hashlib.sha256(definition.encode()).hexdigest(), "invalid_slot_metadata")
            return UnresolvedView(schedule_id, revision, local_slot, due, observed, digest)

    @classmethod
    def open_existing(cls, root):
        return cls(root, _existing_only=True)

    def preflight_existing(self):
        """Bounded read-only schema/metadata pass, before JobStore recovery."""
        schedules = self.schedules()
        with self.transaction():
            clock = self.db.execute("SELECT highwater,epoch,monotonic_ns,eligibility_floor FROM clock_state WHERE id=1").fetchone()
            require(clock is not None, "invalid_clock_metadata")
            integer(clock[0], 0, MAX_UTC); identifier(clock[1]); integer(clock[2], 0, 2**63-1)
            integer(clock[3], -1, MAX_UTC)
            rows = self.db.execute("SELECT schedule_id,local_slot,schedule_revision,due_utc,observed_utc,state,token,definition,definition_digest FROM slots LIMIT ?", (MAX_SLOTS+1,)).fetchall()
            require(len(rows) <= MAX_SLOTS, "slot_quota")
            known = {s.schedule_id for s in schedules}
            for sid, local, rev, due, observed, state, token, definition, digest in rows:
                identifier(sid); integer(rev, 1, MAX_REVISION); integer(observed, 0, MAX_UTC)
                require(sid in known and type(local) is str and len(local)==16, "invalid_slot_metadata")
                try: wall = datetime.fromisoformat(local)
                except (TypeError, ValueError): raise SchedulerError("invalid_slot_metadata") from None
                require(wall.tzinfo is None and wall.isoformat(timespec="minutes")==local, "invalid_slot_metadata")
                parsed = Definition.parse(definition)
                require(type(digest) is str and digest==hashlib.sha256(definition.encode()).hexdigest(), "invalid_slot_metadata")
                require(state in ("reserved", "finished", "not_started", "skipped_gap", "skipped_missed", "skipped_overlap", "skipped_before_activation"), "invalid_slot_metadata")
                if due is not None: integer(due, 0, MAX_UTC)
                if state in ("reserved", "finished", "not_started"): identifier(token)
                else: require(token is None, "invalid_slot_metadata")
            return clock[0]
