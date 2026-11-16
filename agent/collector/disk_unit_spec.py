"""Fixed disk-profile unit projection, callable only by the existing Permit owner.

This builds no queue and admits no operation. The normal maintained Prometheus
owns WAL retention; its 7200s residence is never a process/lease lifetime.
"""
from dataclasses import dataclass
import re
import uuid

PROFILE = "maintained-prometheus-disk-512mib-2h-v1"
SESSION_ROOT = "/run/consolecrypt-agent/disk"
STORE_ROOT = "/var/lib/consolecrypt-agent/metrics-disk"
ENTRY = "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/disk_unit_entry.py"
SLICE = "app-cc-agent-disk.slice"
MEMORY_BYTES = 536870912
ROOT_BYTES = 536870912
RESIDENCE_SECONDS = 7200

PROPERTIES = (
    "Type=exec", "ExitType=main", "Restart=no", "RuntimeRandomizedExtraSec=0",
    "MemoryAccounting=yes", "MemoryMax=536870912", "MemorySwapMax=0",
    "TasksAccounting=yes", "TasksMax=64", "CPUAccounting=yes", "CPUQuota=100%",
    "CPUQuotaPeriodSec=100ms", "OOMPolicy=stop", "KillMode=control-group",
    "KillSignal=SIGKILL", "FinalKillSignal=SIGKILL", "SendSIGKILL=yes",
    "TimeoutStartSec=10s", "TimeoutStopSec=1s", "LimitCORE=0",
    "StandardInput=null", "StandardOutput=null", "StandardError=null",
    "NoNewPrivileges=yes", "ProtectSystem=strict", "ProtectHome=yes",
    "PrivateDevices=yes", "PrivateUsers=yes", "RestrictSUIDSGID=yes", "LockPersonality=yes",
    "ReadOnlyPaths=/tmp /var/tmp /dev/shm", "Environment=LANG=C",
    "UnsetEnvironment=LD_PRELOAD LD_LIBRARY_PATH GLIBC_TUNABLES PYTHONPATH PYTHONHOME",
)


@dataclass(frozen=True)
class DiskUnitSpec:
    session: str
    collector_id: str
    config_sha256: str
    runtime_seconds: int

    @classmethod
    def from_private_handoff(cls, session, collector_id, config_sha256, original_remaining_ns):
        # No caller proof/ready/drained field. Actual Rust Actor retains its
        # original Permit and supplies one decreasing BOOTTIME budget.
        if type(session) is not str or not re.fullmatch("[0-9a-f]{32}", session) or uuid.UUID(hex=session).version != 4:
            raise ValueError("disk_session")
        if type(collector_id) is not str:
            raise ValueError("disk_collector")
        try:
            resource = uuid.UUID(collector_id)
        except ValueError as error:
            raise ValueError("disk_collector") from error
        if resource.int == 0 or str(resource) != collector_id or resource.version != 8:
            raise ValueError("disk_collector")
        if type(config_sha256) is not str or not re.fullmatch("[0-9a-f]{64}", config_sha256):
            raise ValueError("disk_config_digest")
        if type(original_remaining_ns) is not int or not 12_000_000_000 <= original_remaining_ns <= 300_000_000_000:
            raise ValueError("disk_original_window")
        return cls(session, collector_id, config_sha256, original_remaining_ns // 1_000_000_000 - 10)

    def checked(self):
        # Recheck direct dataclass construction before creating any argv.
        expected = self.from_private_handoff(self.session, self.collector_id, self.config_sha256,
                                            (self.runtime_seconds + 10) * 1_000_000_000)
        if type(self.runtime_seconds) is not int or expected != self:
            raise ValueError("disk_runtime")
        return self

    @property
    def name(self):
        return "cc-agent-disk-" + self.session + ".service"

    @property
    def directory(self):
        return SESSION_ROOT + "/" + self.session

    @property
    def wal_root(self):
        # Derived only from the full-manifest address, never a supplied path.
        return STORE_ROOT + "/" + self.collector_id + "/wal"

    def start_argv(self):
        self.checked()
        args = ["/usr/bin/systemd-run", "--user", "--quiet", "--expand-environment=no",
                "--unit=" + self.name, "--slice=" + SLICE]
        args += ["--property=" + item for item in PROPERTIES]
        args += ["--property=Description=cc-disk:" + self.session + ":" + self.config_sha256,
                 "--property=RuntimeMaxSec=" + str(self.runtime_seconds) + "s",
                 "--property=WorkingDirectory=" + self.directory,
                 "--property=ReadWritePaths=" + self.directory + " " + self.wal_root,
                 "--", "/usr/bin/python3.12", "-I", "-B", ENTRY,
                 "--session=" + self.session, "--config-sha256=" + self.config_sha256]
        return args

    def show_argv(self):
        self.checked()
        return ["/usr/bin/systemctl", "--user", "--no-pager", "show", self.name,
                "--property=Id,LoadState,Description,InvocationID,ControlGroup,MainPID,ActiveState,SubState,Result,MemoryMax,MemorySwapMax,TasksMax,CPUQuotaPerSecUSec,Type,ExitType,KillMode,KillSignal,Restart,RuntimeMaxUSec,ExecMainCode,ExecMainStatus"]

    def stop_argv(self):
        self.checked()
        return ["/usr/bin/systemctl", "--user", "--no-pager", "stop", self.name]


SHOW = (
    "Id", "LoadState", "Description", "InvocationID", "ControlGroup", "MainPID", "ActiveState", "SubState",
    "Result", "MemoryMax", "MemorySwapMax", "TasksMax", "CPUQuotaPerSecUSec",
    "Type", "ExitType", "KillMode", "KillSignal", "Restart", "RuntimeMaxUSec", "ExecMainCode", "ExecMainStatus",
)

def parse_show(payload: bytes, spec: DiskUnitSpec, *, check_limits=True):
    if len(payload) > 8192:
        raise ValueError("disk_unit_observation")
    pairs = [line.split("=", 1) for line in payload.decode("ascii").splitlines()]
    if any(len(row) != 2 for row in pairs) or len(pairs) != len({row[0] for row in pairs}):
        raise ValueError("disk_unit_observation")
    row = dict(pairs)
    if not set(row) <= set(SHOW) or row.get("Id") != spec.name:
        raise ValueError("disk_unit_identity")
    if row.get("LoadState") == "not-found":
        return None
    if set(row) != set(SHOW):
        raise ValueError("disk_unit_observation")
    if row["Description"] != "cc-disk:" + spec.session + ":" + spec.config_sha256 or not re.fullmatch("[0-9a-f]{32}", row["InvocationID"]):
        raise ValueError("disk_unit_identity")
    if check_limits and (row["MemoryMax"] != str(MEMORY_BYTES) or row["MemorySwapMax"] != "0" or row["TasksMax"] != "64" or row["CPUQuotaPerSecUSec"] != "1s" or row["Restart"] != "no"):
        raise ValueError("disk_unit_limits")
    if check_limits and (row["Type"] != "exec" or row["ExitType"] != "main" or row["KillMode"] != "control-group" or row["KillSignal"] != "9"):
        raise ValueError("disk_unit_exit_policy")
    # This is post-start observation, not the before-exec cap gate. The fixed
    # inert entry below performs the ACTUAL kernel/mount gate before vmagent.
    return row
