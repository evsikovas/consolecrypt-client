"""Local update observations for a supervisor/UI; never an update authority.

Requested, verified and staged releases are separate from running code. Only a
fresh heartbeat fetched while an owned Popen is alive can set actual_version.
Remote/TLS process attestation and service management are outside this adapter.
"""

from dataclasses import dataclass
from enum import Enum
import math
import re
import subprocess
import time
import uuid


class Phase(str, Enum):
    REQUESTED = "requested"
    VERIFIED = "verified"
    STAGED = "staged"
    RESTART_PENDING = "restart_pending"
    CONFIRMED = "confirmed"
    ROLLBACK = "rollback"
    UNKNOWN = "unknown"


def version(value):
    if not isinstance(value, str) or not re.fullmatch(r"(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})", value):
        raise ValueError("invalid observed release version")
    return value


@dataclass(frozen=True)
class _Heartbeat:
    pid: int
    agent_id: str
    version: str
    observed_at: float


class UpdateStatus:
    def __init__(self, expected_agent_id, requested_version, *, clock=time.time, max_age_seconds=15):
        if str(uuid.UUID(expected_agent_id)) != expected_agent_id:
            raise ValueError("canonical expected Agent ID required")
        if type(max_age_seconds) is not int or not 1 <= max_age_seconds <= 30:
            raise ValueError("observation age budget")
        self.expected_agent_id = expected_agent_id
        self.requested_version = version(requested_version)
        self.phase = Phase.REQUESTED
        self.verified_version = None
        self.staged_version = None
        self.verified_artifact_sha256 = None
        self.rollback_version = None
        self.failure_reason = None
        self.failed_process_exit_code = None
        self.last_observed_version = None
        self._process = None
        self._heartbeat = None
        self.clock, self.max_age_seconds = clock, max_age_seconds

    def verified(self, release):
        if self.phase != Phase.REQUESTED or not isinstance(release, dict):
            raise ValueError("release verification transition")
        if (version(release.get("displayVersion")) != self.requested_version
                or not isinstance(release.get("artifactSha256"), str)
                or not re.fullmatch(r"[0-9a-f]{64}", release["artifactSha256"])):
            raise ValueError("verified release context mismatch")
        self.verified_version = self.requested_version
        self.verified_artifact_sha256 = release["artifactSha256"]
        self.phase = Phase.VERIFIED

    def staged(self, staged_release):
        if (self.phase != Phase.VERIFIED or staged_release.get("version") != self.verified_version
                or staged_release.get("artifact_sha256") != self.verified_artifact_sha256):
            raise ValueError("staging confirmation mismatch")
        self.staged_version = self.verified_version
        self.phase = Phase.STAGED

    def restart_pending(self):
        if self.phase != Phase.STAGED:
            raise ValueError("release is not staged")
        self.phase = Phase.RESTART_PENDING

    def start_failed(self, process):
        if not isinstance(process, subprocess.Popen) or process.poll() in (None, 0):
            raise ValueError("owned failed process required")
        self.failed_process_exit_code = process.returncode
        self.failure_reason = "new_process_start_failed"
        if self._heartbeat is None:
            self.phase = Phase.UNKNOWN

    def rollback_requested(self, previous_version):
        self.rollback_version = version(previous_version)
        self.phase = Phase.ROLLBACK

    def _unknown(self, reason):
        self._heartbeat = None
        self._process = None
        self.failure_reason = reason
        self.phase = Phase.UNKNOWN

    def observe(self, process, fetch_heartbeat):
        """fetch_heartbeat is the supervisor's fixed owned-loopback read path.

        A manifest, desired version, process-exists flag or arbitrary remote URL
        is not an observation API. The callback belongs to the trusted adapter;
        this method does not supply response-signing or remote attestation.
        """
        if not isinstance(process, subprocess.Popen) or process.poll() is not None:
            self._unknown("owned_process_unavailable")
            return False
        try:
            heartbeat = fetch_heartbeat()
            observed_at = self.clock()
            if (not isinstance(heartbeat, dict) or heartbeat.get("agent_id") != self.expected_agent_id
                    or type(heartbeat.get("schema_version")) is not int or heartbeat["schema_version"] != 1 or process.poll() is not None
                    or type(observed_at) not in (int, float) or not math.isfinite(observed_at) or observed_at < 0):
                self._unknown("heartbeat_context_unavailable")
                return False
            observed_version = version(heartbeat.get("version"))
        except (OSError, ValueError, TypeError, KeyError):
            self._unknown("heartbeat_unavailable")
            return False
        self._process = process
        self._heartbeat = _Heartbeat(process.pid, self.expected_agent_id, observed_version, observed_at)
        self.last_observed_version = observed_version
        target = self.rollback_version if self.phase == Phase.ROLLBACK else self.requested_version
        if self.phase in {Phase.RESTART_PENDING, Phase.ROLLBACK} and observed_version == target:
            self.phase = Phase.CONFIRMED
            self.failure_reason = None
        return True

    def snapshot(self):
        heartbeat = self._heartbeat
        if heartbeat is not None:
            age = self.clock() - heartbeat.observed_at
            if self._process.poll() is not None:
                self._unknown("owned_process_exited")
            elif not 0 <= age <= self.max_age_seconds:
                self._unknown("heartbeat_stale")
        heartbeat = self._heartbeat
        return {"format": "cc-agent-update-status/v1", "phase": self.phase.value,
                "expected_agent_id": self.expected_agent_id, "requested_version": self.requested_version,
                "verified_version": self.verified_version, "staged_version": self.staged_version,
                "verified_artifact_sha256": self.verified_artifact_sha256,
                "actual_running_version": heartbeat.version if heartbeat else None,
                "last_observed_version": self.last_observed_version,
                "rollback_requested_version": self.rollback_version,
                "confirmation_kind": "rollback" if self.rollback_version else "update",
                "failure_reason": self.failure_reason, "failed_process_exit_code": self.failed_process_exit_code,
                "observation": {"source": "owned_process_loopback_heartbeat", "pid": heartbeat.pid,
                                "observed_at": heartbeat.observed_at, "max_age_seconds": self.max_age_seconds} if heartbeat else None,
                "issues_authority": False, "remote_transport_attested": False}
