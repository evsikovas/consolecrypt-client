"""Private durable job acceptance, bounded events, and explicit recovery states."""
from __future__ import annotations

from contextlib import contextmanager
from bisect import bisect_right
from datetime import datetime, timezone
import fcntl
import hashlib
import os
from pathlib import Path
import stat
import shutil
import tempfile
import threading

from .approval import DOMAIN, VerifiedManifest
from .errors import RunnerError, require
from .protocol import encode, decode, uuid_field, MAX_FRAME_BYTES
from .probe import environment_receipt
from . import artifact_retention as history
import time

MAX_OUTPUT_BYTES = 50 * 1024 * 1024
MAX_TOTAL_OUTPUT_BYTES = 1024 * 1024 * 1024
MAX_JOBS = 4096
TRANSITIONS = {
    "queued": {"running", "cancelled", "interrupted"},
    "running": {"cancelling", "succeeded", "failed", "interrupted", "unknown"},
    "cancelling": {"cancelled", "failed", "interrupted", "unknown", "succeeded"},
    "unknown": {"succeeded", "failed", "cancelled", "interrupted"},
}


def utc_now():
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _private_directory(path: Path):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid() and info.st_mode & 0o077 == 0,
            "private_storage", "Runner directory must be an owned private directory, not a link")


def atomic_json(path: Path, value, *, max_bytes=MAX_FRAME_BYTES):
    body = encode(value)
    require(len(body) <= max_bytes, "record_size", "Job record exceeds limit")
    fd, name = tempfile.mkstemp(prefix=".record-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(body)
            output.flush()
            os.fsync(output.fileno())
        os.replace(name, path)
        parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def read_json(path: Path, *, max_bytes=MAX_FRAME_BYTES):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    with os.fdopen(fd, "rb") as source:
        require(os.fstat(source.fileno()).st_size <= max_bytes, "record_size", "Job record exceeds limit")
        return decode(source.read(max_bytes + 1), max_bytes=max_bytes)


def redact(value, secrets):
    if isinstance(value, dict):
        return {redact(key, secrets): redact(child, secrets) for key, child in value.items()}
    if isinstance(value, list):
        return [redact(child, secrets) for child in value]
    if isinstance(value, str):
        for secret in sorted((item for item in secrets if item), key=len, reverse=True):
            value = value.replace(secret, "[REDACTED]")
        return value
    if value is not None and str(value) in secrets:
        return "[REDACTED]"
    return value


class JobStore:
    def __init__(self, root: Path, *, output_limit=MAX_OUTPUT_BYTES,
                 total_output_limit=MAX_TOTAL_OUTPUT_BYTES, max_jobs=MAX_JOBS):
        require(type(output_limit) is int and 0 < output_limit <= MAX_OUTPUT_BYTES and
                type(total_output_limit) is int and 0 < total_output_limit <= MAX_TOTAL_OUTPUT_BYTES and
                type(max_jobs) is int and 0 < max_jobs <= MAX_JOBS,
                'store_limits', 'Invalid job storage limits')
        self.root = Path(root).absolute()
        _private_directory(self.root)
        for name in ("jobs", "nonces"):
            _private_directory(self.root / name)
        self._mutex = threading.RLock()
        self._lock_fd = os.open(self.root / ".lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        self._owner_fd = os.open(self.root / ".owner", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            fcntl.flock(self._owner_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            self.close()
            raise RunnerError("runner_already_active", "A helper already owns this job store") from error
        self.output_limit = output_limit
        self.total_output_limit, self.max_jobs = total_output_limit, max_jobs
        self._total_output_bytes, self._job_count = 0, 0
        # Sparse, bounded by the existing aggregate journal byte quota. Rebuilt
        # from fsynced journals once on startup; no separate mutable index file.
        self._event_offsets = {}
        self._counted_output_bytes = {}
        try:
            self._recover_journals()
        except Exception:
            self.close()
            raise

    def close(self):
        if getattr(self, "_owner_fd", None) is not None:
            os.close(self._owner_fd)
            self._owner_fd = None
        if self._lock_fd is not None:
            os.close(self._lock_fd)
            self._lock_fd = None

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    @contextmanager
    def locked(self):
        with self._mutex:
            require(self._lock_fd is not None, "store_closed", "Job store is closed")
            fcntl.flock(self._lock_fd, fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(self._lock_fd, fcntl.LOCK_UN)

    def job_directory(self, job_id):
        uuid_field(job_id, "job ID")
        directory = self.root / "jobs" / job_id
        if directory.exists() or directory.is_symlink():
            info = directory.lstat()
            require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid() and info.st_mode & 0o077 == 0,
                    "private_storage", "Invalid job directory")
        return directory

    def accept(self, verified: VerifiedManifest, *, revision_id=None, environment=None):
        manifest = verified.value
        admission = verified.admission_audit
        job_id = manifest["job_id"]
        receipt = environment_receipt(environment, manifest['environment_digest']) if environment is not None else None
        if revision_id is not None:
            uuid_field(revision_id, 'revision ID')
        with self.locked():
            directory = self.job_directory(job_id)
            state_path = directory / "state.json"
            if state_path.exists():
                state = read_json(state_path)
                require(state["manifest_digest"] == verified.digest, "job_conflict", "Job ID already has another manifest")
                reference = directory / 'revision.json'
                if revision_id is not None and reference.exists():
                    require(read_json(reference) == {'version': 1, 'revision_id': revision_id,
                            'revision_digest': manifest['revision_digest']}, 'job_conflict',
                            'Accepted job already references another snapshot')
                return state, False
            require(self._job_count < self.max_jobs, 'job_quota',
                    'Retained job limit reached; no job was accepted')
            require(shutil.disk_usage(self.root).free >= 128 * 1024 * 1024,
                    'job_disk', 'Insufficient reserved disk space for a job')
            nonce_path = self.root / "nonces" / uuid_field(manifest["nonce"], "approval nonce")
            nonce = {"job_id": job_id, "manifest_digest": verified.digest, "expires_at": manifest["expires_at"]}
            if nonce_path.exists():
                require(read_json(nonce_path) == nonce, "approval_replay", "Approval nonce was already used for another job")
            else:
                atomic_json(nonce_path, nonce)
            _private_directory(directory)
            atomic_json(directory / "manifest.json", manifest)
            # Persist the exact private snapshot before durable admission/ack.
            # Missing legacy or partial metadata remains conservatively pinned.
            if revision_id is not None:
                atomic_json(directory / 'revision.json', {'version': 1, 'revision_id': revision_id,
                            'revision_digest': manifest['revision_digest']})
            if admission is not None:
                from .admission_audit import persist_once
                persist_once(directory/'admission.json',admission,manifest,verified.digest)
            state = {"version": 1, "job_id": job_id, "manifest_digest": verified.digest,
                     "state": "queued", "sequence": 0, "output_bytes": 0, "output_truncated": False,
                     "accepted_at": utc_now(), "updated_at": utc_now(), "result": None}
            if receipt is not None:
                state['environment'] = receipt
            atomic_json(state_path, state)
            self._job_count += 1
            return state, True

    def admission_audit(self, job_id):
        # Read-only metadata. Missing legacy data is explicitly unrecorded;
        # NEVER infer an initiator from a retry/current grant or repair it.
        from .admission_audit import read
        with self.locked():
            directory=self.job_directory(job_id)
            state=read_json(directory/'state.json')
            manifest=read_json(directory/'manifest.json')
            digest=hashlib.sha256(DOMAIN+encode(manifest)).hexdigest()
            require(digest==state['manifest_digest'] and manifest['job_id']==job_id,
                'admission_audit','Original job metadata changed')
            return read(directory/'admission.json',manifest,digest)

    def references_revision(self, revision_id, digest):
        uuid_field(revision_id, 'revision ID')
        with self.locked():
            for child in (self.root / 'jobs').iterdir():
                directory = self.job_directory(child.name)
                manifest = read_json(directory / 'manifest.json')
                if manifest['revision_digest'] != digest:
                    continue
                reference = directory / 'revision.json'
                if not reference.exists():
                    return True
                value = read_json(reference)
                require(set(value) == {'version', 'revision_id', 'revision_digest'} and
                        type(value['version']) is int and value['version'] == 1 and
                        value['revision_digest'] == digest, 'revision_reference',
                        'Invalid retained snapshot reference')
                uuid_field(value['revision_id'], 'revision ID')
                if value['revision_id'] == revision_id:
                    state=self._retire_history_locked(directory,read_json(directory/'state.json'),int(time.time()))
                    if history.payload_reference_expired(state,manifest,int(time.time())):
                        continue
                    return True
            return False

    def _retire_history_locked(self,directory,state,now):
        state,deleted=history.retire(directory,state,now)
        if history.view(state) is not None:
            # The marker can have committed before a failed unlink/fsync in
            # this SAME process. Reclaim its actually counted bytes once, even
            # when the retry observes that the file was already removed.
            counted=self._counted_output_bytes.get(state['job_id'],0)
            require(0<=counted<=self._total_output_bytes,'history_quota','History byte accounting changed')
            self._total_output_bytes-=counted
            self._counted_output_bytes[state['job_id']]=0
            self._event_offsets[state['job_id']]=[]
        return state

    def retire_history(self,*,now=None):
        """Internal bounded maintenance; no job/nonce/authority removal or pin grant."""
        now=int(time.time()) if now is None else now
        with self.locked():
            for count,entry in enumerate((self.root/'jobs').iterdir(),1):
                require(count<=self.max_jobs,'job_quota','Stored job count exceeds quota')
                directory=self.job_directory(entry.name)
                self._retire_history_locked(directory,read_json(directory/'state.json'),now)

    def get(self, job_id):
        with self.locked():
            directory=self.job_directory(job_id)
            return self._retire_history_locked(directory,read_json(directory/'state.json'),int(time.time()))

    def transition(self, job_id, next_state, *, result=None, reconciliation=False):
        with self.locked():
            path = self.job_directory(job_id) / "state.json"
            state = read_json(path)
            require(next_state in TRANSITIONS.get(state["state"], set()), "job_transition", "Invalid job state transition")
            require(state["state"] != "unknown" or reconciliation, "reconciliation_required", "Unknown outcome needs runner evidence")
            marker=history.validate_state(state)
            if marker is not None:
                result,marker=history.reconcile_retired_result(path.parent,state,result)
                state['artifact_retirement']=marker
            changed_at=utc_now()
            state.update(state=next_state, updated_at=changed_at, result=result)
            # Retired output does not erase or block later genuine UNKNOWN
            # reconciliation. It never reopens the old journal/cursor.
            if marker is None:state['history_activity_at']=changed_at
            history.validate_state(state)
            atomic_json(path, state)
            return state

    def append_event(self, job_id, event_type, payload, *, secrets=()):
        require(isinstance(event_type, str) and len(event_type) <= 64, "event_type", "Invalid event type")
        # Exact masking is intentionally not advertised as protection against
        # arbitrary encoded/transformed secrets in user-controlled project code.
        payload = redact(payload, tuple(secrets))
        with self.locked():
            directory = self.job_directory(job_id)
            state = read_json(directory / "state.json")
            require(history.validate_state(state) is None,"history_retired","Raw history already retired")
            event = {"version": 1, "job_id": job_id, "sequence": state["sequence"] + 1,
                     "timestamp": utc_now(), "type": event_type, "payload": payload}
            raw = encode(event) + b"\n"
            require(len(raw) <= MAX_FRAME_BYTES - 8192, "event_size", "Event exceeds framed reply budget")
            if (state["output_bytes"] + len(raw) > self.output_limit or
                    self._total_output_bytes + len(raw) > self.total_output_limit or
                    shutil.disk_usage(self.root).free < 128 * 1024 * 1024):
                state["output_truncated"] = True
                atomic_json(directory / "state.json", state)
                return None
            fd = os.open(directory / "events.jsonl", os.O_APPEND | os.O_CREAT | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "ab") as output:
                output.write(raw)
                output.flush()
                os.fsync(output.fileno())
            # Count durable bytes before writing the cursor: even if the state
            # update fails, a subsequent append cannot exceed the global budget.
            self._total_output_bytes += len(raw)
            self._counted_output_bytes[job_id]=self._counted_output_bytes.get(job_id,0)+len(raw)
            if (event['sequence']-1)%128==0:
                self._event_offsets.setdefault(job_id,[]).append((event['sequence'],state['output_bytes']))
            state.update(sequence=event["sequence"], output_bytes=state["output_bytes"] + len(raw), updated_at=event["timestamp"], history_activity_at=event["timestamp"])
            atomic_json(directory / "state.json", state)
            return event

    def _recover_journals(self):
        # A crash may occur after the event's fsync but before its state record.
        # Rebuild sequence/cursor once on startup, never duplicate the last ID.
        with self.locked():
            for directory in (self.root / "jobs").iterdir():
                require(self._job_count < self.max_jobs, 'job_quota', 'Stored job count exceeds quota')
                self._job_count += 1
                directory = self.job_directory(directory.name)
                if not (directory / "state.json").exists():
                    if not (directory / 'manifest.json').exists():
                        # Only an empty, owned UUID directory can be removed.
                        # Unexpected contents fail closed rather than deleting
                        # something whose provenance is no longer established.
                        directory.rmdir()
                        self._job_count -= 1
                        continue
                    manifest = read_json(directory / 'manifest.json')
                    require(manifest.get('job_id') == directory.name,
                            'job_corrupt', 'Incomplete acceptance has another job ID')
                    # Never replay after a restart, even when the acceptance
                    # acknowledgement was lost before state.json became durable.
                    state = {'version': 1, 'job_id': directory.name,
                             'manifest_digest': hashlib.sha256(DOMAIN + encode(manifest)).hexdigest(),
                             'state': 'interrupted', 'sequence': 0, 'output_bytes': 0,
                             'output_truncated': False, 'accepted_at': utc_now(),
                             'updated_at': utc_now(), 'result': {'reason': 'acceptance_interrupted'}}
                    atomic_json(directory / 'state.json', state)
                state = read_json(directory / "state.json")
                marker=history.validate_state(state)
                if marker is not None:
                    # Complete only an already committed, immutable-bound retirement.
                    history.retire(directory,state,int(time.time()))
                    self._event_offsets[directory.name]=[]
                    self._counted_output_bytes[directory.name]=0
                    continue
                state.setdefault('history_activity_at',state['updated_at'])
                sequence, byte_count = 0, 0
                last_activity=state['history_activity_at']
                offsets=[]
                if (directory / "events.jsonl").exists():
                    fd = os.open(directory / "events.jsonl", os.O_RDONLY | os.O_NOFOLLOW)
                    with os.fdopen(fd, "rb") as source:
                        require(os.fstat(fd).st_size <= self.output_limit, "event_size", "Event journal exceeds quota")
                        while raw := source.readline(MAX_FRAME_BYTES + 1):
                            require(len(raw) <= MAX_FRAME_BYTES and raw.endswith(b"\n"), "event_corrupt", "Incomplete event journal")
                            event = decode(raw)
                            require(event["job_id"] == directory.name and event["sequence"] == sequence + 1,
                                    "event_corrupt", "Event journal sequence conflict")
                            if sequence%128==0:offsets.append((sequence+1,byte_count))
                            if history.timestamp(event['timestamp'])>history.timestamp(last_activity):last_activity=event['timestamp']
                            sequence += 1
                            byte_count += len(raw)
                require(byte_count>0 or (state['sequence']==0 and state['output_bytes']==0),
                        'event_corrupt','Missing unretired journal cannot reset the cursor')
                state.update(sequence=sequence, output_bytes=byte_count,history_activity_at=last_activity)
                self._event_offsets[directory.name]=offsets
                self._total_output_bytes += byte_count
                self._counted_output_bytes[directory.name]=byte_count
                require(self._total_output_bytes <= self.total_output_limit,
                        'event_size', 'Aggregate event journal quota exceeded')
                atomic_json(directory / "state.json", state)
                self._retire_history_locked(directory,state,int(time.time()))

    def events(self, job_id, *, after=0, limit=100):
        require(type(after) is int and after >= 0 and type(limit) is int and 1 <= limit <= 500,
                "event_cursor", "Invalid event cursor or page size")
        with self.locked():
            directory = self.job_directory(job_id)
            state = read_json(directory / "state.json")
            state=self._retire_history_locked(directory,state,int(time.time()))
            retirement=history.view(state)
            if retirement is not None:
                return {"job_id":job_id,"manifest_digest":state["manifest_digest"],"events":[],"cursor":after,
                    "output_truncated":state["output_truncated"],"state":state["state"],"artifact_retirement":retirement}
            events = []
            used = 0
            if (directory / "events.jsonl").exists():
                fd = os.open(directory / "events.jsonl", os.O_RDONLY | os.O_NOFOLLOW)
                with os.fdopen(fd, "rb") as source:
                    offsets=self._event_offsets.get(job_id,[])
                    index=bisect_right(offsets,(after+1,2**63-1))-1
                    if index>=0:source.seek(offsets[index][1])
                    while raw := source.readline(MAX_FRAME_BYTES + 1):
                        require(len(raw) <= MAX_FRAME_BYTES and raw.endswith(b"\n"), "event_corrupt", "Event journal is incomplete")
                        event = decode(raw)
                        if event["sequence"] <= after:
                            continue
                        if len(events) >= limit or used + len(raw) > MAX_FRAME_BYTES - 4096:
                            break
                        require(event["job_id"] == job_id, "event_corrupt", "Event belongs to another job")
                        events.append(event)
                        used += len(raw)
            return {"job_id":job_id,"manifest_digest":state["manifest_digest"],
                    "events": events, "cursor": events[-1]["sequence"] if events else after,
                    "output_truncated": state["output_truncated"], "state": state["state"]}

    def _discard_legacy_input(self, directory, state):
        # Exclusive JobStore owner/recovery only. This fixed artifact carries
        # no nonce/approval/revision authority and is never replayed. Future or
        # unverifiable state is preserved for explicit recovery, not swept.
        if (type(state.get('version')) is not int or state['version'] != 1 or
                state.get('job_id') != directory.name or state.get('state') not in
                {'queued', 'running', 'cancelling', 'succeeded', 'failed', 'unknown', 'cancelled', 'interrupted'}):
            return
        expected = directory.lstat()
        fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            held = os.fstat(fd)
            require((held.st_dev, held.st_ino) == (expected.st_dev, expected.st_ino),
                    'job_input_retention', 'Legacy input directory changed')
            try:
                item = os.stat('input.json', dir_fd=fd, follow_symlinks=False)
            except FileNotFoundError:
                return
            require(stat.S_ISREG(item.st_mode) and item.st_uid == os.getuid() and item.st_nlink == 1
                    and stat.S_IMODE(item.st_mode) == 0o600 and item.st_size <= 6*1024*1024,
                    'job_input_retention', 'Legacy input is not an owned private artifact')
            manifest = read_json(directory / 'manifest.json')
            if (type(manifest.get('version')) is not int or manifest['version'] != 1 or
                    manifest.get('job_id') != directory.name or
                    hashlib.sha256(DOMAIN + encode(manifest)).hexdigest() != state.get('manifest_digest')):
                return
            os.unlink('input.json', dir_fd=fd)
            os.fsync(fd)
            after = directory.lstat()
            require((after.st_dev, after.st_ino) == (held.st_dev, held.st_ino),
                    'job_input_retention', 'Legacy input directory changed')
        finally:
            os.close(fd)

    def recover_interactive(self):
        """Never retry an operation just because its old helper disappeared."""
        recovered = []
        with self.locked():
            for directory in (self.root / "jobs").iterdir():
                uuid_field(directory.name, "job ID")
                state = read_json(self.job_directory(directory.name) / "state.json")
                current = state["state"]
                if current in ("running", "cancelling", "queued"):
                    # A helper restart is not fresh sensitive-output activity.
                    state.setdefault('history_activity_at',state['updated_at'])
                    state.update(state="unknown" if current != "queued" else "interrupted",
                                 updated_at=utc_now(), result={"reason": "helper_restarted"})
                    atomic_json(directory / "state.json", state)
                    recovered.append(state)
                self._discard_legacy_input(directory, state)
        return recovered
