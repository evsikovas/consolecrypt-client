"""Render a reviewable monitoring-node plan; never contacts or installs a host."""

import argparse
from dataclasses import asdict, dataclass
import ipaddress
import json
from pathlib import Path
import re
import sys

METRICS_TRUST_NOTICE = 'Metrics snapshots are accessible to and processed by the monitoring storage within its trust boundary. They do not automatically inherit ConsoleCrypt vault end-to-end encryption. Treat the monitoring node, storage administrators and its backups as a separate trust boundary; configure access and storage protection explicitly.'

PROMETHEUS_VERSION = "3.16.0"
PROMETHEUS_LINUX_AMD64_SHA256 = "68727009472a2c02e1ff7f062fd5b3e637285bffc8fc2d340c76fb449d6e0f34"


@dataclass(frozen=True)
class MonitoringPlan:
    cpu_cores: int = 2
    memory_mib: int = 2048
    disk_gib: int = 50
    retention_days: int = 15
    retention_gib: int = 40
    receiver_enabled: bool = False
    alertmanager_enabled: bool = False
    expected_samples_per_second: int = 2000

    def __post_init__(self):
        for name, low, high in (("cpu_cores", 2, 64), ("memory_mib", 2048, 262144),
                                ("disk_gib", 10, 10000), ("retention_days", 1, 365),
                                ("retention_gib", 1, 9000), ("expected_samples_per_second", 1, 1000000)):
            value = getattr(self, name)
            if type(value) is not int or not low <= value <= high:
                raise ValueError(f"invalid {name}")
        if type(self.receiver_enabled) is not bool or type(self.alertmanager_enabled) is not bool:
            raise ValueError("receiver and Alertmanager require explicit booleans")
        if self.retention_gib > self.disk_gib * 0.8:
            raise ValueError("retain at least 20% storage headroom")
        if self.estimated_sample_bytes > self.retention_gib * 1024**3:
            raise ValueError("sample estimate exceeds planned retention; increase disk or reduce ingestion")

    @property
    def estimated_sample_bytes(self):
        # Published Prometheus estimate is 1-2 B/sample. This estimate excludes
        # series index, head, WAL, compaction scratch and filesystem overhead.
        return 2 * self.expected_samples_per_second * self.retention_days * 86400

    def config(self):
        result = {"global": {"scrape_interval": "15s", "evaluation_interval": "15s"},
                  "rule_files": ["/etc/consolecrypt-monitoring/rules.yml"],
                  "scrape_configs": [{"job_name": "monitoring_node", "scrape_interval": "15s",
                                      "scrape_timeout": "10s", "body_size_limit": "8MB", "sample_limit": 20000,
                                      "label_limit": 24, "label_name_length_limit": 128,
                                      "label_value_length_limit": 256,
                                      "static_configs": [{"targets": ["127.0.0.1:9090"]}]}]}
        if self.alertmanager_enabled:
            result["alerting"] = {"alertmanagers": [{"static_configs": [{"targets": ["127.0.0.1:9093"]}]}]}
        return result

    def variables(self):
        return {"cc_monitoring_plan": {**asdict(self), "estimated_sample_bytes": self.estimated_sample_bytes,
                                       "estimate_excludes": ["series index", "head", "WAL", "compaction", "filesystem"],
                                       "reference_benchmark": "not_measured"},
                "cc_prometheus_version": PROMETHEUS_VERSION,
                "cc_prometheus_sha256": PROMETHEUS_LINUX_AMD64_SHA256,
                "cc_prometheus_config": self.config()}

    def render(self, output):
        directory = Path(output)
        directory.mkdir(mode=0o700, parents=False, exist_ok=False)
        (directory / "TRUST_NOTICE.txt").write_text(METRICS_TRUST_NOTICE + "\n", encoding="utf-8")
        (directory / "plan.json").write_text(json.dumps(self.variables(), indent=2) + "\n", encoding="utf-8")
        (directory / "prometheus.yml").write_text(json.dumps(self.config(), indent=2) + "\n", encoding="utf-8")
        return directory


def main():
    parser = argparse.ArgumentParser(description="Generate reviewable Prometheus node sizing/config; no host access")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--receiver", action="store_true")
    parser.add_argument("--alertmanager", action="store_true", help="use existing loopback Alertmanager; does not install it")
    parser.add_argument("--cpu-cores", type=int, default=2)
    parser.add_argument("--memory-mib", type=int, default=2048)
    parser.add_argument("--disk-gib", type=int, default=50)
    parser.add_argument("--retention-days", type=int, default=15)
    parser.add_argument("--retention-gib", type=int, default=40)
    parser.add_argument("--samples-per-second", type=int, default=2000)
    args = parser.parse_args()
    plan = MonitoringPlan(args.cpu_cores, args.memory_mib, args.disk_gib, args.retention_days,
                          args.retention_gib, args.receiver, args.alertmanager, args.samples_per_second)
    output = plan.render(args.output)
    print(METRICS_TRUST_NOTICE, file=sys.stderr)
    print(output)


if __name__ == "__main__":
    main()
