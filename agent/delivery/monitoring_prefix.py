"""Explicit monitoring-node configuration in an owned prefix, without host installation.

Uses the same MonitoringPlan as monitoring-node.yml. It starts no service/process,
creates no account/mount, downloads nothing and never deletes metric storage.
"""

import argparse
from dataclasses import fields
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

DIRECTORY = Path(__file__).resolve().parent
sys.path.insert(0, str(DIRECTORY))
from monitoring_plan import MonitoringPlan, PROMETHEUS_VERSION, PROMETHEUS_LINUX_AMD64_SHA256
from stage import _safe_path, _write, digest
from monitoring_transaction import MANAGED,LEDGER,load_previous,target_mapping,commit

MARKER = ".cc-monitoring-prefix.json"


def initialize(path):
    path = _safe_path(path)
    if path == Path(path.anchor) or len(path.parts) < 3:
        raise ValueError("monitoring prefix too broad")
    path.mkdir(mode=0o700, parents=False, exist_ok=True)
    if any(path.iterdir()):
        raise ValueError("initialize only an empty owned prefix")
    (path / MARKER).write_text(json.dumps({"format": "cc-monitoring-prefix/v1", "path": str(path.resolve())}), encoding="utf-8")
    return path


def root(path):
    path = _safe_path(path)
    marker = _safe_path(path / MARKER)
    if not marker.is_file() or marker.stat().st_size > 1024:
        raise ValueError("owned monitoring prefix marker required")
    value = json.loads(marker.read_bytes())
    if value != {"format": "cc-monitoring-prefix/v1", "path": str(path.resolve())}:
        raise ValueError("monitoring prefix binding mismatch")
    return path


def parse_plan(data):
    if not isinstance(data, dict) or set(data) != {"cc_monitoring_plan", "cc_prometheus_version", "cc_prometheus_sha256", "cc_prometheus_config"}:
        raise ValueError("reviewed MonitoringPlan variables required")
    if data["cc_prometheus_version"] != PROMETHEUS_VERSION or data["cc_prometheus_sha256"] != PROMETHEUS_LINUX_AMD64_SHA256:
        raise ValueError("unsupported Prometheus supply pin")
    raw = data["cc_monitoring_plan"]
    plan = MonitoringPlan(**{field.name: raw[field.name] for field in fields(MonitoringPlan)})
    if data != plan.variables():
        raise ValueError("plan/config/estimate differs from the shared model")
    return plan


def checked_binary(path, expected):
    path = Path(path)
    if not path.is_absolute():
        raise ValueError("absolute pinned binary required")
    path = _safe_path(path)
    if not path.is_file() or path.stat().st_size > 256 * 1024 * 1024:
        raise ValueError("explicit pinned binary required")
    with path.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != expected:
            raise ValueError("monitoring executable digest mismatch")
    return path


def render(prefix, plan, binary, port):
    if type(port) is not int or not 1024 <= port <= 65535:
        raise ValueError("unprivileged loopback port required")
    config = plan.config()
    config["rule_files"] = [str(prefix / "etc/consolecrypt-monitoring/rules.yml")]
    config["scrape_configs"][0]["static_configs"][0]["targets"] = ["127.0.0.1:" + str(port)]
    command = [str(binary), "--config.file=" + str(prefix / "etc/consolecrypt-monitoring/prometheus.yml"),
               "--storage.tsdb.path=" + str(prefix / "var/lib/consolecrypt-monitoring/data"),
               "--storage.tsdb.retention.time=" + str(plan.retention_days) + "d",
               "--storage.tsdb.retention.size=" + str(plan.retention_gib) + "GB",
               "--web.listen-address=127.0.0.1:" + str(port), "--web.max-connections=64", "--query.max-concurrency=4"]
    if plan.receiver_enabled:
        command.append("--web.enable-remote-write-receiver")
    status = {"format": "cc-monitoring-prefix-plan/v1", "plan": plan.variables(),
              "requested_receiver_enabled": plan.receiver_enabled, "starts_process": False,
              "system_service_applied": False, "physical_capacity_certified": False,
              "prefix_fixture_only": True, "data_directory": str(prefix / "var/lib/consolecrypt-monitoring/data")}
    encode = lambda value: json.dumps(value, sort_keys=True, indent=2).encode() + b"\n"
    return {"etc/consolecrypt-monitoring/prometheus.yml": encode(config),
            "etc/consolecrypt-monitoring/rules.yml": (DIRECTORY / "rules.yml").read_bytes(),
            "monitoring-command.json": encode(command), "monitoring-plan.json": encode(status)}


def apply(prefix, data, binary, binary_sha256, promtool, promtool_sha256, port, *, check=False, create_data_directory=True, _recover_missing=False):
    if type(create_data_directory) is not bool:raise ValueError('explicit data-directory creation flag required')
    if type(_recover_missing) is not bool:raise ValueError('explicit boolean recovery option required')
    prefix = root(prefix)
    data_directory = _safe_path(prefix / "var/lib/consolecrypt-monitoring/data")
    plan = parse_plan(data)
    binary = checked_binary(binary, binary_sha256)
    promtool = checked_binary(promtool, promtool_sha256)
    wanted = render(prefix, plan, binary, port)
    target_mapping(wanted)
    previous,missing=load_previous(prefix,recover_missing=_recover_missing,allow_initial=True)
    changed=[name for name in MANAGED if previous.get(name)!=wanted[name]]
    # Validate a temporary path-adjusted copy before changing any live artifact.
    with tempfile.TemporaryDirectory(prefix=".monitoring-check-", dir=prefix) as temporary:
        draft = Path(temporary)
        rules = draft / "rules.yml"
        rules.write_bytes(wanted["etc/consolecrypt-monitoring/rules.yml"])
        config = json.loads(wanted["etc/consolecrypt-monitoring/prometheus.yml"])
        config["rule_files"] = [str(rules)]
        config_path = draft / "prometheus.yml"
        config_path.write_text(json.dumps(config), encoding="utf-8")
        for arguments in ([promtool, "check", "rules", rules], [promtool, "check", "config", config_path]):
            result = subprocess.run([str(arg) for arg in arguments], stdin=subprocess.DEVNULL,
                                    capture_output=True, timeout=20, check=False)
            if result.returncode:
                raise ValueError("pinned promtool rejected monitoring configuration")
    if not check:
        backup = _safe_path(prefix / "config-backup")
        backup.mkdir(mode=0o700, exist_ok=True)
        additions = []
        for name in changed:
            path = prefix / name
            if path.exists():
                value = path.read_bytes()
                target = _safe_path(backup / (digest(value) + ".txt"))
                if target.exists() and digest(target.read_bytes()) != digest(value):
                    raise ValueError("modified configuration backup")
                if not target.exists():
                    additions.append((target, value))
        if len(list(backup.iterdir())) + len(additions) > 16:
            raise ValueError("configuration backup budget exceeded")
        for target, value in additions:
            _write(target, value, 0o600)
        if create_data_directory:
            data_directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        commit(prefix,wanted,previous,missing)
    return {"format": "cc-monitoring-prefix-apply/v1", "changed": bool(changed), "changed_files": changed,
            "check_mode": check, "process_started": False, "system_service_applied": False,
            "metrics_deleted": False, "accounts_or_mounts_changed": False, "physical_capacity_certified": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", type=Path, required=True)
    parser.add_argument("--prometheus-binary", type=Path, required=True)
    parser.add_argument("--prometheus-sha256", required=True)
    parser.add_argument("--promtool", type=Path, required=True)
    parser.add_argument("--promtool-sha256", required=True)
    parser.add_argument("--port", type=int, default=9090)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    raw = sys.stdin.buffer.read(65537)
    if len(raw) > 65536:
        parser.error("monitoring plan input budget")
    print(json.dumps(apply(args.prefix, json.loads(raw), args.prometheus_binary, args.prometheus_sha256,
                           args.promtool, args.promtool_sha256, args.port, check=args.check), sort_keys=True))


if __name__ == "__main__":
    main()
