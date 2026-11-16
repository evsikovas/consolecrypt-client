"""Bounded configuration and lifecycle of a maintained upstream collector.

This adapter accepts authority already verified by the identity dispatcher. A
scope string in a request is not proof of authorization. The dispatcher must
bind the full manifest digest, endpoints, host ID, lease and current checkpoint
to its permit, recheck expiry/revocation, and stop this child when it expires.
No keys, vault access, service manager, shell, or arbitrary executable entry
point is exposed here. Strict remote-write is deliberately unavailable until
the hard byte/age resource guarantees have passed isolated Linux tests.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import ipaddress
import json
import math
import os
from pathlib import Path
import re
import subprocess
import time
import urllib.parse
import urllib.request
import uuid

from .supply import check_path, lock, verify_binary
from .native_status import BACKEND as NATIVE_BACKEND, NativeCollectorStatus, state_from_native_metrics

VERSION = "v1.156.0"
MAX_BUFFER_BYTES = 512 * 1024 * 1024
MAX_BUFFER_AGE = 2 * 60 * 60
# Pinned v1.156.0 source minimum; the actual Linux counterexample is recorded
# in evidence/linux-full-queue. This is pending-offset accounting, not a
# supported physical512MiB quota. A version change needs new evidence.
UPSTREAM_MINIMUM_BUFFER_BYTES = (32 * 1024 * 1024 + 8) * 16
MAX_TARGETS = 16
_NAME = re.compile(r"[a-zA-Z][a-zA-Z0-9_-]{0,31}\Z")


class CollectorError(ValueError):
    """Stable public error code; never include URLs, secrets or upstream logs."""


def _require(condition: bool, code: str) -> None:
    if not condition:
        raise CollectorError(code)


def _keys(data: object, allowed: set[str], required: set[str] = frozenset()) -> dict:
    _require(isinstance(data, dict), "invalid_object")
    _require(set(data) <= allowed and required <= set(data), "unsupported_field")
    return data


def _integer(value: object, minimum: int, maximum: int) -> int:
    _require(type(value) is int and minimum <= value <= maximum, "invalid_budget")
    return value


def _name(value: object) -> str:
    _require(isinstance(value, str) and _NAME.fullmatch(value) is not None, "invalid_label")
    return value


def _endpoint(value: object, *, scrape: bool) -> tuple[str, bool]:
    _require(isinstance(value, str) and len(value) <= 512, "invalid_endpoint")
    _require(not any(ord(c) < 33 or ord(c) > 126 for c in value), "invalid_endpoint")
    try:
        parsed = urllib.parse.urlsplit(value)
        address = ipaddress.ip_address(parsed.hostname or "")
        port = parsed.port
    except ValueError:
        raise CollectorError("literal_ip_required") from None
    _require(parsed.scheme in ("http", "https") and port is not None and 1 <= port <= 65535, "invalid_endpoint")
    _require(getattr(address, "scope_id", None) is None, "endpoint_zone_id_denied")
    _require(parsed.username is None and parsed.password is None and not parsed.query and not parsed.fragment, "endpoint_secret_or_query_denied")
    _require(not address.is_unspecified and not address.is_multicast and not address.is_link_local, "endpoint_address_denied")
    _require(parsed.scheme == "https" or address.is_loopback, "insecure_non_loopback_denied")
    _require(parsed.path == "/metrics" if scrape else parsed.path in ("/api/v1/write", "/api/v1/push"), "endpoint_path_denied")
    host = "[" + address.compressed + "]" if address.version == 6 else address.compressed
    canonical = f"{parsed.scheme}://{host}:{port}{parsed.path}"
    _require(value == canonical, "endpoint_noncanonical")
    return canonical, address.is_loopback


def validate_reviewed_ansible_component(exporter: dict, *, approved_host_id: str) -> dict:
    """Closed declared cAdvisor definition, never an observed state or grant.

    The collector can only scrape this endpoint. Fixed Ansible lifecycle jobs
    independently bind the same full definition/hash to signed Host inventory
    and actual owned-container readback; no Docker/root/job capability is added.
    """
    _require(exporter.get('ownership') == 'reviewedAnsible' and exporter.get('kind') == 'cadvisor',
             'reviewed_ansible_component_denied')
    management = _keys(exporter.get('management'), {'manifest', 'manifestSha256'}, {'manifest', 'manifestSha256'})
    fields = {'schemaVersion', 'component', 'managedBy', 'hostId', 'owner', 'collectorVersion',
              'imageManifestSha256', 'imageId', 'bindIp', 'port', 'metricsPath'}
    manifest = _keys(management['manifest'], fields, fields)
    _require(type(manifest['schemaVersion']) is int and manifest['schemaVersion'] == 1
             and manifest['component'] == 'cadvisor' and manifest['managedBy'] == 'reviewedAnsible'
             and manifest['collectorVersion'] == '0.60.6', 'reviewed_ansible_component_denied')
    for value in (approved_host_id, manifest['hostId'], manifest['owner']):
        try:
            parsed = uuid.UUID(value)
        except (ValueError, TypeError, AttributeError):
            raise CollectorError('reviewed_ansible_identity_denied') from None
        _require(parsed.int != 0 and str(parsed) == value, 'reviewed_ansible_identity_denied')
    _require(manifest['hostId'] == approved_host_id and uuid.UUID(manifest['owner']).version == 4,
             'reviewed_ansible_host_denied')
    image_hash, image_id = manifest['imageManifestSha256'], manifest['imageId']
    _require(isinstance(image_hash, str) and re.fullmatch(r'[0-9a-f]{64}', image_hash)
             and image_hash != '0' * 64 and isinstance(image_id, str)
             and re.fullmatch(r'sha256:[0-9a-f]{64}', image_id) and image_id[7:] != '0' * 64,
             'reviewed_ansible_artifact_denied')
    artifact = _keys(exporter.get('artifact'), {'version', 'sha256'}, {'version', 'sha256'})
    _require(artifact == {'version': '0.60.6', 'sha256': image_hash}, 'reviewed_ansible_artifact_denied')
    port = _integer(manifest['port'], 1024, 65535)
    endpoint = f'http://127.0.0.1:{port}/metrics'
    _require(manifest['bindIp'] == '127.0.0.1' and manifest['metricsPath'] == '/metrics'
             and exporter.get('url') == endpoint, 'reviewed_ansible_endpoint_denied')
    _endpoint(endpoint, scrape=True)
    raw = json.dumps(manifest, sort_keys=True, separators=(',', ':'), ensure_ascii=True, allow_nan=False).encode('ascii')
    _require(len(raw) <= 2048 and isinstance(management['manifestSha256'], str)
             and re.fullmatch(r'[0-9a-f]{64}', management['manifestSha256'])
             and hashlib.sha256(raw).hexdigest() == management['manifestSha256'], 'reviewed_ansible_manifest_hash_denied')
    return {'manifest': json.loads(raw), 'manifestSha256': management['manifestSha256']}


@dataclass(frozen=True)
class Target:
    id: str
    url: str
    kind: str
    sample_limit: int
    series_limit: int
    response_bytes: int
    label_limit: int
    loopback: bool


@dataclass(frozen=True)
class CollectorPlan:
    mode: str
    targets: tuple[Target, ...]
    host_id: str
    site: str
    environment: str
    scrape_interval_seconds: int
    receiver_url: str | None
    receiver_loopback: bool
    buffer_bytes: int
    buffer_age_seconds: int

    @property
    def labels(self) -> dict[str, str]:
        return {"cc_host_id": self.host_id, "site": self.site, "environment": self.environment}


def validate_manifest(data: dict, *, granted_scopes, allowed_endpoints, approved_host_id: str) -> CollectorPlan:
    """Validate an explicit allowlist, bounded targets and scopes without I/O.

    IP literals deliberately prevent DNS rebinding. HTTPS receivers can use the
    platform CA store. Arbitrary auth/CA file paths, discovery, proxy config,
    relabel expressions and secret resolution are not accepted in this version.
    """
    data = _keys(data, {"schema_version", "mode", "targets", "labels", "scrape_interval_seconds", "receiver", "buffer"},
                 {"schema_version", "mode", "targets", "labels"})
    _require(type(data["schema_version"]) is int and data["schema_version"] == 1, "unsupported_schema")
    _require("metrics.collect" in granted_scopes, "scope_denied")
    mode = data["mode"]
    _require(mode in ("pull", "remote_write"), "unsupported_mode")
    labels = _keys(data["labels"], {"cc_host_id", "site", "environment"}, {"cc_host_id", "site", "environment"})
    try:
        parsed_host_id = uuid.UUID(labels["cc_host_id"])
        host_id = str(parsed_host_id)
    except (ValueError, TypeError, AttributeError):
        raise CollectorError("invalid_host_id") from None
    _require(parsed_host_id.int != 0, "invalid_host_id")
    _require(host_id == labels["cc_host_id"] == approved_host_id, "host_binding_denied")
    site, environment = _name(labels["site"]), _name(labels["environment"])
    interval = _integer(data.get("scrape_interval_seconds", 15), 1, 300)
    raw_targets = data["targets"]
    _require(isinstance(raw_targets, list) and 1 <= len(raw_targets) <= MAX_TARGETS, "target_limit")
    allowed = frozenset(allowed_endpoints)
    targets, ids, urls = [], set(), set()
    for raw in raw_targets:
        raw = _keys(raw, {"id", "url", "kind", "sample_limit", "series_limit", "response_bytes", "label_limit"}, {"id", "url"})
        target_id = _name(raw["id"])
        url, loopback = _endpoint(raw["url"], scrape=True)
        _require(url in allowed, "endpoint_not_authorized")
        _require(target_id not in ids and url not in urls, "duplicate_collection_denied")
        ids.add(target_id)
        urls.add(url)
        kind = raw.get("kind", "application")
        _require(kind in ("application", "node_exporter", "cadvisor"), "unsupported_exporter")
        targets.append(Target(target_id, url, kind,
                              _integer(raw.get("sample_limit", 10000), 1, 50000),
                              _integer(raw.get("series_limit", 10000), 1, 50000),
                              _integer(raw.get("response_bytes", 4 * 1024 * 1024), 256, 8 * 1024 * 1024),
                              _integer(raw.get("label_limit", 32), 4, 64), loopback))
    receiver_url, receiver_loopback = None, False
    if mode == "remote_write":
        receiver = _keys(data.get("receiver"), {"url"}, {"url"})
        receiver_url, receiver_loopback = _endpoint(receiver["url"], scrape=False)
        _require(receiver_url in allowed, "receiver_not_authorized")
    else:
        _require("receiver" not in data and "buffer" not in data, "pull_remote_write_conflict")
    buffer = _keys(data.get("buffer", {}), {"max_bytes", "max_age_seconds"})
    buffer_bytes = _integer(buffer.get("max_bytes", MAX_BUFFER_BYTES), 1024 * 1024, MAX_BUFFER_BYTES)
    buffer_age = _integer(buffer.get("max_age_seconds", MAX_BUFFER_AGE), 60, MAX_BUFFER_AGE)
    return CollectorPlan(mode, tuple(targets), host_id, site, environment,
                         interval, receiver_url, receiver_loopback, buffer_bytes, buffer_age)


def _scrape_configs(plan: CollectorPlan, *, vmagent: bool) -> list[dict]:
    result = []
    for target in plan.targets:
        parsed = urllib.parse.urlsplit(target.url)
        config = {
            "job_name": "cc_" + target.id,
            "scheme": parsed.scheme,
            "metrics_path": parsed.path,
            "scrape_interval": f"{plan.scrape_interval_seconds}s",
            "scrape_timeout": f"{min(10, plan.scrape_interval_seconds)}s",
            "honor_labels": False,
            "honor_timestamps": True,
            "follow_redirects": False,
            "sample_limit": target.sample_limit,
            "label_limit": target.label_limit,
            "static_configs": [{"targets": [parsed.netloc], "labels": plan.labels}],
            # Force approved host/environment labels even if an exporter forges
            # them. Exporter responses never become host identity authority.
            "metric_relabel_configs": [{"target_label": key, "replacement": value} for key, value in plan.labels.items()],
        }
        if vmagent:
            config["series_limit"] = target.series_limit
            config["max_scrape_size"] = str(target.response_bytes)
        else:
            config["body_size_limit"] = str(target.response_bytes) + "B"
            config["label_name_length_limit"] = 128
            config["label_value_length_limit"] = 256
        result.append(config)
    return result


def render_pull_config(plan: CollectorPlan) -> dict:
    """Configuration fragment for an existing compatible Prometheus.

    This does not expose or start exporters and does not alter an existing
    Prometheus. Per-target series cardinality limits are vmagent-specific;
    pull mode publishes sample/body/label limits, not a false cardinality claim.
    """
    _require(plan.mode == "pull", "wrong_mode")
    return {"scrape_configs": _scrape_configs(plan, vmagent=False)}


def exporter_privileges(kind: str) -> dict:
    metadata = lock()["optional_exporters"]
    _require(kind in metadata, "unsupported_exporter")
    entry = metadata[kind]
    return {"name": kind, "version": entry["version"], "license": entry["license"],
            "enabled": False, "required_scope": "exporters.manage",
            "automatic_install_available": False, "privileges": entry["privileges"]}


@dataclass(frozen=True)
class StrictSupport:
    schema_version: int = 1
    collector: str = "vmagent"
    version: str = VERSION
    reason_code: str = "pinned_collector_buffer_contract_incompatible"
    required_max_bytes: int = MAX_BUFFER_BYTES
    byte_minimum: int = UPSTREAM_MINIMUM_BUFFER_BYTES
    physical_byte_limit_supported: bool = False
    required_max_age_seconds: int = MAX_BUFFER_AGE
    age_supported: bool = False
    oldest_unacknowledged_policy_verified: bool = False
    receiver_destination_pinned: bool = False
    receiver_destination_reason_code: str = "remote_write_receiver_redirect_destination_not_pinned"
    production_allowed: bool = False
    receiver_response_bounded: bool = False
    receiver_response_reason_code: str = "pinned_receiver_error_body_unbounded"

    def to_dict(self) -> dict:
        return asdict(self)


def strict_support() -> StrictSupport:
    """Public static diagnostics for the pinned component, never authorization.

    No filesystem, endpoint, secret or manifest values are included. The
    measured byte incompatibility, missing age contract and receiver-response
    gap keep production disabled even when a test-only child is running.
    """
    return StrictSupport()


@dataclass(frozen=True)
class CollectorStatus:
    mode: str
    running: bool
    version: str
    state: str
    last_scrape_success: bool | None = None
    pending_bytes: int | None = None
    samples_dropped: int | None = None
    remote_write_samples_dropped: int | None = None
    remote_write_packets_dropped: int | None = None
    queue_bytes_dropped: int | None = None
    requests_accepted: int | None = None
    delivery_lag_seconds: float | None = None
    buffer_guarantee: str = "incompatible_strict_512MiB_2h"
    strict_support: StrictSupport | None = None

    def to_dict(self) -> dict:
        return asdict(self)


def state_from_metrics(text: str, *, running: bool, mode: str = "remote_write",
                       backend: str = "vmagent-v1.156.0") -> CollectorStatus | NativeCollectorStatus:
    """Read only selected numeric health metrics, not a general metrics parser.

    Missing counters and exact delivery lag remain unknown. The upstream queue
    drop counter is remote_write_samples_dropped, not aggregate loss. Aggregate
    samples_dropped stays unknown because scrape/cardinality failures and
    rejected compressed blocks are not included in that counter. Packet drops
    count rejected blocks, never inferred sample counts. HTTP 2xx counts
    receiver acceptance, not durable ingestion or successful storage of every
    series. No labels, URL or secret-bearing upstream text is returned.
    """
    # Datatype selection only; the owned collector reader supplies this value.
    if backend == NATIVE_BACKEND:
        _require(mode == "remote_write", "native_backend_mode_mismatch")
        return state_from_native_metrics(text, backend=backend, running=running)
    _require(type(backend) is str and backend == "vmagent-v1.156.0", "unsupported_backend")
    _require(isinstance(text, str) and len(text.encode("utf-8")) <= 1024 * 1024, "status_limit")
    values: dict[str, list[float]] = {}
    wanted = {"vmagent_remotewrite_pending_data_bytes", "vmagent_remotewrite_samples_dropped_total",
              "vmagent_remotewrite_packets_dropped_total",
              "vm_persistentqueue_bytes_dropped_total", "vmagent_remotewrite_requests_total",
              "vm_promscrape_scrapes_failed_total", "vm_promscrape_scrapes_total"}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        name = re.split(r"[\s{]", line, 1)[0]
        if name not in wanted:
            continue
        # Labels may contain spaces; the numeric value is the final token.
        try:
            value = float(line.rsplit(None, 1)[1])
        except (ValueError, IndexError):
            continue
        if not math.isfinite(value) or value < 0:
            continue
        if name == "vmagent_remotewrite_requests_total" and 'status_code="2XX"' not in line:
            continue
        values.setdefault(name, []).append(value)
    def total(name):
        return int(sum(values[name])) if name in values else None
    return CollectorStatus(mode, running, VERSION, "running" if running else "stopped",
                           pending_bytes=total("vmagent_remotewrite_pending_data_bytes"),
                           remote_write_samples_dropped=total("vmagent_remotewrite_samples_dropped_total"),
                           remote_write_packets_dropped=total("vmagent_remotewrite_packets_dropped_total"),
                           queue_bytes_dropped=total("vm_persistentqueue_bytes_dropped_total"),
                           requests_accepted=total("vmagent_remotewrite_requests_total"),
                           strict_support=strict_support() if mode == "remote_write" else None,
                           buffer_guarantee="incompatible_strict_512MiB_2h" if mode == "remote_write" else "not_applicable")


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, new_url):
        return None


class CollectorSupervisor:
    """Own exactly one digest-pinned child. Never control user-owned processes.

    Production strict remote-write is fail closed. Test-only mode requires only
    loopback endpoints, disables disk queue, and is intended for a bounded
    temporary integration harness, never a service/package configuration.
    """
    def __init__(self, plan: CollectorPlan, binary: Path, platform: str,
                 workdir: Path, *, allow_test_only_remote_write: bool = False):
        self.plan, self.binary, self.platform = plan, Path(binary), platform
        self.workdir = Path(workdir)
        self.test_only = allow_test_only_remote_write
        self.process: subprocess.Popen | None = None
        self.port: int | None = None
        self._last_error: str | None = None
        self._started = None

    def start(self, *, port: int) -> None:
        _require(self.plan.mode == "remote_write", "pull_has_no_local_collector")
        _require(self.test_only, "strict_remote_write_unavailable_byte_age_quota")
        _require(self.plan.receiver_loopback and all(t.loopback for t in self.plan.targets), "test_mode_requires_loopback")
        _require(type(port) is int and 1024 <= port <= 65535, "invalid_status_port")
        _require(self.process is None, "already_started")
        binary = verify_binary(self.binary, self.platform)
        check_path(self.workdir)
        self.workdir.mkdir(mode=0o700, parents=True, exist_ok=True)
        check_path(self.workdir)
        # Reuse only an empty harness directory: no overwriting user configs/WAL.
        _require(not any(self.workdir.iterdir()), "nonempty_workdir_denied")
        config = self.workdir / "scrape.json"
        with config.open("x", encoding="utf-8") as output:
            json.dump({"scrape_configs": _scrape_configs(self.plan, vmagent=True)}, output)
        args = [str(binary), f"-promscrape.config={config.resolve()}",
                f"-httpListenAddr=127.0.0.1:{port}",
                "-remoteWrite.url=" + self.plan.receiver_url,
                "-remoteWrite.forcePromProto=true", "-remoteWrite.queues=1",
                "-remoteWrite.disableOnDiskQueue=true", "-remoteWrite.dropSamplesOnOverload=true",
                "-remoteWrite.tmpDataPath=" + str((self.workdir / "queue").resolve()),
                f"-remoteWrite.maxDiskUsagePerURL={self.plan.buffer_bytes}",
                "-remoteWrite.maxHourlySeries=50000", "-remoteWrite.maxDailySeries=50000",
                "-remoteWrite.retryMinInterval=1s", "-remoteWrite.retryMaxInterval=10s",
                "-remoteWrite.flushInterval=1s", "-remoteWrite.sendTimeout=3s",
                "-memory.allowedBytes=67108864", "-maxLabelsPerTimeseries=64",
                "-maxLabelNameLen=128", "-maxLabelValueLen=256", "-loggerLevel=ERROR"]
        # Do not inherit proxy URLs, cloud credentials, VM env overrides or vault
        # environment. No shell, pass_fds, host mounts or privilege escalation.
        env = {key: value for key, value in os.environ.items()
               if key.upper() in {"SYSTEMROOT", "WINDIR", "TEMP", "TMP", "LANG", "LC_ALL"}}
        env["GOMAXPROCS"] = "2"
        try:
            self.process = subprocess.Popen(args, cwd=self.workdir, env=env,
                                            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                            stderr=subprocess.DEVNULL, shell=False,
                                            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            self.port = port
            self._started = time.monotonic()
        except OSError:
            raise CollectorError("collector_start_failed") from None

    def status(self) -> CollectorStatus:
        if self.plan.mode == "pull":
            return CollectorStatus("pull", False, VERSION, "external_prometheus", buffer_guarantee="not_applicable")
        if not self.test_only:
            return CollectorStatus(self.plan.mode, False, VERSION, "blocked", strict_support=strict_support())
        running = self.process is not None and self.process.poll() is None
        if not running:
            return CollectorStatus(self.plan.mode, False, VERSION, "stopped",
                                   buffer_guarantee="test_only_memory_queue_no_durability", strict_support=strict_support())
        try:
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), _NoRedirect())
            with opener.open(f"http://127.0.0.1:{self.port}/metrics", timeout=1) as response:
                payload = response.read(1024 * 1024 + 1)
            _require(len(payload) <= 1024 * 1024, "status_limit")
            result = state_from_metrics(payload.decode("utf-8"), running=True)
            scrape_success = None
            try:
                with opener.open(f"http://127.0.0.1:{self.port}/api/v1/targets", timeout=1) as response:
                    targets_payload = response.read(1024 * 1024 + 1)
                _require(len(targets_payload) <= 1024 * 1024, "status_limit")
                targets = json.loads(targets_payload)["data"]["activeTargets"]
                expected = {target.url for target in self.plan.targets}
                if (isinstance(targets, list) and len(targets) == len(expected)
                        and {target.get("scrapeUrl") for target in targets} == expected
                        and all(target.get("health") in ("up", "down") and target.get("lastScrape")
                                and not target["lastScrape"].startswith("0001") for target in targets)):
                    scrape_success = all(target["health"] == "up" for target in targets)
            except (OSError, ValueError, KeyError, TypeError, AttributeError):
                pass
            return CollectorStatus(**{**result.to_dict(), "last_scrape_success": scrape_success,
                                       "strict_support": strict_support(),
                                       "buffer_guarantee": "test_only_memory_queue_no_durability"})
        except (OSError, ValueError, UnicodeError):
            return CollectorStatus(self.plan.mode, True, VERSION, "starting_or_status_unavailable",
                                   buffer_guarantee="test_only_memory_queue_no_durability", strict_support=strict_support())

    def stop(self) -> None:
        if self.process is None:
            return
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        # Keep owned handle so a second start cannot accidentally reuse queue or
        # config data. Never kill a process identified only by a recycled PID.

    def __enter__(self):
        return self

    def __exit__(self, *unused):
        self.stop()
