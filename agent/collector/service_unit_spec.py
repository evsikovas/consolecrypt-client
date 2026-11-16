"""One explicitly delegated original service run; no admission/renewal method."""
from dataclasses import dataclass
import re
import uuid

from disk_unit_spec import DiskUnitSpec, ENTRY as INTERACTIVE_ENTRY

MAX_SECONDS = 86400
ENTRY = "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/service_unit_entry.py"


@dataclass(frozen=True)
class ServiceUnitSpec(DiskUnitSpec):
    @classmethod
    def from_private_handoff(cls, session, collector_id, config_sha256, original_remaining_ns):
        if (type(session) is not str or re.fullmatch("[0-9a-f]{32}", session) is None
                or uuid.UUID(hex=session).version != 4):
            raise ValueError("service_session")
        if type(collector_id) is not str:
            raise ValueError("service_collector")
        try:
            resource = uuid.UUID(collector_id)
        except ValueError as error:
            raise ValueError("service_collector") from error
        if resource.int == 0 or str(resource) != collector_id or resource.version != 8:
            raise ValueError("service_collector")
        if type(config_sha256) is not str or re.fullmatch("[0-9a-f]{64}", config_sha256) is None:
            raise ValueError("service_config_digest")
        if (type(original_remaining_ns) is not int
                or not 12_000_000_000 <= original_remaining_ns <= MAX_SECONDS * 1_000_000_000):
            raise ValueError("service_original_window")
        return cls(session, collector_id, config_sha256, original_remaining_ns // 1_000_000_000 - 10)

    def checked(self):
        # Same fixed fields, original remaining duration, and reserved cleanup
        # margin. The independent service statement/Actor verifies authority.
        if type(self.runtime_seconds) is not int:
            raise ValueError("service_runtime")
        expected = self.from_private_handoff(
            self.session, self.collector_id, self.config_sha256,
            (self.runtime_seconds + 10) * 1_000_000_000)
        if expected != self:
            raise ValueError("service_runtime")
        return self

    def start_argv(self):
        # Reuse the existing exact cap/mount/cgroup/argv projection. Only this
        # fixed entry validates a service handoff; old entry still rejects>300s.
        args = super().start_argv()
        if args.count(INTERACTIVE_ENTRY) != 1:
            raise ValueError("service_entry")
        args[args.index(INTERACTIVE_ENTRY)] = ENTRY
        return args
