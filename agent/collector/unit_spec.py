"""Closed systemd argv for the proposed Linux delegated-user subset.

Internal component data only. This module admits no operation and constructs no
Permit. Selection requires the future SAME Rust MetricsCollect session owner.
No subprocess is started by importing this module or constructing a spec.
"""
from dataclasses import dataclass
import re
import uuid

ROOT = "/run/consolecrypt-agent/volatile"
ENTRY = "/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector/volatile_unit_entry.py"
SLICE = "app-cc-agent-volatile.slice"
MEMORY = 536870912
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
    "ReadOnlyPaths=/tmp /var/tmp /dev/shm",
    "Environment=LANG=C",
    "UnsetEnvironment=LD_PRELOAD LD_LIBRARY_PATH GLIBC_TUNABLES PYTHONPATH PYTHONHOME",
)
SHOW = (
    "Id", "LoadState", "Description", "InvocationID", "ControlGroup", "MainPID", "ActiveState", "SubState",
    "Result", "MemoryMax", "MemorySwapMax", "TasksMax", "CPUQuotaPerSecUSec",
    "Type", "ExitType", "KillMode", "KillSignal", "Restart", "RuntimeMaxUSec", "ExecMainCode", "ExecMainStatus",
)


@dataclass(frozen=True)
class UnitSpec:
    session: str
    config_sha256: str
    runtime_seconds: int

    @classmethod
    def from_private_handoff(cls, session: str, config_sha256: str, original_remaining_ns: int):
        # The caller is the Rust owner's private launcher, never request JSON.
        if not re.fullmatch("[0-9a-f]{64}", config_sha256):
            raise ValueError("volatile_config_digest")
        if not re.fullmatch("[0-9a-f]{32}", session) or uuid.UUID(hex=session).version != 4:
            raise ValueError("volatile_session")
        if type(original_remaining_ns) is not int or not 12_000_000_000 <= original_remaining_ns <= 300_000_000_000:
            raise ValueError("volatile_original_window")
        # Ten seconds of launch is deducted ONCE. No status/retry can renew it.
        return cls(session, config_sha256, original_remaining_ns // 1_000_000_000 - 10)

    @property
    def name(self):
        if not re.fullmatch("[0-9a-f]{32}", self.session) or uuid.UUID(hex=self.session).version != 4:
            raise ValueError("volatile_session")
        return "cc-agent-volatile-" + self.session + ".service"

    def start_argv(self):
        if not 2 <= self.runtime_seconds <= 290:
            raise ValueError("volatile_runtime")
        argv = ["/usr/bin/systemd-run", "--user", "--quiet", "--expand-environment=no",
                "--unit=" + self.name, "--slice=" + SLICE]
        argv += ["--property=" + value for value in PROPERTIES]
        argv += ["--property=Description=cc-volatile:" + self.session + ":" + self.config_sha256,
                 "--property=RuntimeMaxSec=" + str(self.runtime_seconds) + "s",
                 "--property=WorkingDirectory=" + ROOT + "/" + self.session,
                 "--property=ReadWritePaths=" + ROOT + "/" + self.session,
                 "--", "/usr/bin/python3.12", "-I", "-B", ENTRY,
                 "--session=" + self.session, "--config-sha256=" + self.config_sha256]
        return argv

    def show_argv(self):
        return ["/usr/bin/systemctl", "--user", "--no-pager", "show", self.name,
                "--property=" + ",".join(SHOW)]

    def stop_argv(self):
        # Caller MUST revalidate the retained InvocationID/cgroup/PID-start
        # tuple immediately before this fixed unit operation. No supplied PID.
        return ["/usr/bin/systemctl", "--user", "--no-pager", "stop", self.name]


def parse_show(payload: bytes, spec: UnitSpec, *, check_limits=True):
    if len(payload) > 8192:
        raise ValueError("volatile_unit_observation")
    pairs = [line.split("=", 1) for line in payload.decode("ascii").splitlines()]
    if any(len(row) != 2 for row in pairs) or len(pairs) != len({row[0] for row in pairs}):
        raise ValueError("volatile_unit_observation")
    row = dict(pairs)
    if not set(row) <= set(SHOW) or row.get("Id") != spec.name:
        raise ValueError("volatile_unit_identity")
    if row.get("LoadState") == "not-found":
        return None
    if set(row) != set(SHOW):
        raise ValueError("volatile_unit_observation")
    if row["Description"] != "cc-volatile:" + spec.session + ":" + spec.config_sha256 or not re.fullmatch("[0-9a-f]{32}", row["InvocationID"]):
        raise ValueError("volatile_unit_identity")
    if check_limits and (row["MemoryMax"] != str(MEMORY) or row["MemorySwapMax"] != "0" or row["TasksMax"] != "64" or row["CPUQuotaPerSecUSec"] != "1s" or row["Restart"] != "no"):
        raise ValueError("volatile_unit_limits")
    if check_limits and (row["Type"] != "exec" or row["ExitType"] != "main" or row["KillMode"] != "control-group" or row["KillSignal"] != "9"):
        raise ValueError("volatile_unit_exit_policy")
    # This is post-start observation, not the before-exec cap gate. The fixed
    # inert entry below performs the ACTUAL kernel/mount gate before vmagent.
    return row
