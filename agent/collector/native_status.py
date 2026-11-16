"""Read-only diagnostics for one explicitly selected Native WAL backend.

The backend tag selects a datatype, never ownership, collection authority or
activation. Only the existing trusted collector reader may select it. No metric
or HTTP response establishes exact unacknowledged loss or durable delivery.
"""
from dataclasses import asdict, dataclass
from decimal import Decimal, InvalidOperation
import math
import re

BACKEND = "prom-agent-hardmode-v1"
POLICY_FORMAT = "cc-prom-agent-hardmode-operator-policy/v1"
MAX_TEXT_BYTES = 1024 * 1024
MAX_LINES = 8192
MAX_LINE_BYTES = 8192
MAX_NUMBER = 2**53
PHYSICAL_CAP_BYTES = 512 * 1024 * 1024

# Metric unit -> public field. No metric below is an accepted/delivered count.
COUNTERS = {
    "prometheus_remote_storage_samples_total": "sample_send_attempts_total",
    "prometheus_remote_storage_samples_retried_total": "sample_retry_attempts_total",
    "prometheus_remote_storage_samples_failed_total": "sample_nonrecoverable_failures_total",
    "prometheus_remote_storage_samples_dropped_total": "sample_pre_send_drops_total",
    "prometheus_agent_hard_evicted_segments_total": "physical_evicted_segments_total",
    "prometheus_agent_hard_evicted_wal_bytes_total": "charged_evicted_wal_bytes_total",
    "prometheus_agent_hard_byte_evicted_segments_total": "byte_evicted_segments_total",
    "prometheus_agent_hard_age_evicted_segments_total": "age_evicted_segments_total",
}
GAUGES = {
    "prometheus_remote_storage_samples_pending": "shard_pending_samples",
    "prometheus_remote_storage_queue_highest_timestamp_seconds": "source_max_enqueued_timestamp_seconds",
    "prometheus_remote_storage_queue_highest_sent_timestamp_seconds": "source_max_reported_sent_timestamp_seconds",
    "prometheus_agent_hard_charged_bytes": "last_validated_root_charge_bytes",
    "prometheus_agent_hard_oldest_segment_residence_seconds": "oldest_retained_segment_residence_seconds",
    "prometheus_agent_hard_loss_unknown": "global_unacknowledged_loss_unknown",
    "prometheus_agent_hard_unavailable": "sticky_unavailable",
}
DISK_COUNTER = "prometheus_agent_hard_system_disk_guard_rejections_total"
DISK_CAUSES = {
    "low_available": "system_disk_low_available_rejections_total",
    "observation_invalid": "system_disk_observation_invalid_rejections_total",
}
UNKNOWN_FIELDS = (
    "confirmed_accepted_requests_total", "confirmed_accepted_samples_total",
    "receiver_permanent_rejected_samples_total", "unacknowledged_samples_lost_total",
    "unacknowledged_delivery_lag_seconds",
)
BOOL_FIELDS = {"global_unacknowledged_loss_unknown", "sticky_unavailable"}
FLOAT_FIELDS = {
    "source_max_enqueued_timestamp_seconds", "source_max_reported_sent_timestamp_seconds",
    "oldest_retained_segment_residence_seconds",
}
NUMERIC_FIELDS = set(COUNTERS.values()) | set(GAUGES.values()) | set(DISK_CAUSES.values())
PUBLIC_FIELDS = NUMERIC_FIELDS | set(UNKNOWN_FIELDS)
_ROW = re.compile(r"([a-zA-Z_:][a-zA-Z0-9_:]*)(\{.*\})?\s+([^\s]+)\Z")
_LABEL = re.compile(r'([a-zA-Z_][a-zA-Z0-9_]*)="((?:[^"\\]|\\[\\"n])*)"')
_NUMBER = re.compile(r"[+]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\Z")


class NativeStatusError(ValueError):
    """Fixed public code only, without labels, endpoints or upstream text."""


def _require(condition):
    if not condition:
        raise NativeStatusError("native_status_unavailable")


def _labels(raw):
    if raw is None:
        return {}
    _require(raw.startswith("{") and raw.endswith("}"))
    raw = raw[1:-1]
    values = {}
    cursor = 0
    while cursor < len(raw):
        match = _LABEL.match(raw, cursor)
        _require(match is not None)
        key, value = match.groups()
        _require(key not in values and len(values) < 16 and len(value) <= 2048)
        # Values are internal grouping keys only; never returned to callers.
        values[key] = value
        cursor = match.end()
        if cursor < len(raw):
            _require(raw[cursor] == ",")
            cursor += 1
            _require(cursor < len(raw))
    return values


def _number(raw, integer):
    _require(type(raw) is str and len(raw) <= 128 and _NUMBER.fullmatch(raw))
    try:
        value = Decimal(raw)
    except InvalidOperation:
        raise NativeStatusError("native_status_unavailable") from None
    _require(value.is_finite() and 0 <= value < MAX_NUMBER)
    if integer:
        _require(value == value.to_integral_value())
        return int(value)
    result = float(value)
    _require(math.isfinite(result))
    return result


def _metrics(text):
    _require(type(text) is str and len(text) <= MAX_TEXT_BYTES)
    try:
        _require(len(text.encode("utf-8")) <= MAX_TEXT_BYTES)
    except UnicodeError:
        raise NativeStatusError("native_status_unavailable") from None
    lines = text.splitlines()
    _require(len(lines) <= MAX_LINES)
    selected = set(COUNTERS) | set(GAUGES) | {DISK_COUNTER}
    observed = {}
    metric_types = {}
    identities = set()
    remote_identities = set()
    for line in lines:
        _require(len(line.encode("utf-8")) <= MAX_LINE_BYTES)
        if not line or line.startswith("#"):
            if line.startswith("# TYPE "):
                parts = line.split()
                _require(len(parts) == 4)
                _, _, name, kind = parts
                if name in selected:
                    _require(name not in metric_types)
                    _require(kind == ("counter" if name in COUNTERS or name == DISK_COUNTER else "gauge"))
                    metric_types[name] = kind
            continue
        name = re.split(r"[\s{]", line, 1)[0]
        _require(not name.startswith(("vmagent_", "vm_persistentqueue_")))
        if name not in selected:
            continue
        match = _ROW.fullmatch(line)
        _require(match is not None)
        _, labels, raw = match.groups()
        labels = _labels(labels)
        identity = (name, tuple(sorted(labels.items())))
        _require(identity not in identities)
        identities.add(identity)
        if name.startswith("prometheus_remote_storage_"):
            allowed = {"remote_name", "url"} | ({"reason"} if name.endswith("samples_dropped_total") else set())
            _require(set(labels) <= allowed and {"remote_name", "url"} <= labels.keys())
            remote_identities.add((labels["remote_name"], labels["url"]))
            _require(len(remote_identities) == 1)  # Declared profile has one destination.
        elif name == DISK_COUNTER:
            _require(set(labels) == {"cause"} and labels["cause"] in DISK_CAUSES)
        else:
            _require(not labels)
        integer = name in COUNTERS or name == DISK_COUNTER or name in {
            "prometheus_remote_storage_samples_pending", "prometheus_agent_hard_charged_bytes",
            "prometheus_agent_hard_loss_unknown", "prometheus_agent_hard_unavailable",
        }
        value = _number(raw, integer)
        field = DISK_CAUSES[labels["cause"]] if name == DISK_COUNTER else (COUNTERS | GAUGES)[name]
        if field in BOOL_FIELDS:
            _require(value in (0, 1))
            value = bool(value)
        if field in observed:
            _require(name.endswith("samples_dropped_total"))  # Distinct native reason labels only.
            value += observed[field]
            _require(value < MAX_NUMBER)
        observed[field] = value
    return observed


@dataclass(frozen=True)
class NativeCollectorStatus:
    backend: str
    state: str
    native_queue: dict

    def to_dict(self):
        return asdict(self)


def state_from_native_metrics(text, *, backend, running):
    _require(type(backend) is str and backend == BACKEND and type(running) is bool)
    values = {field: None for field in PUBLIC_FIELDS}
    values.update(_metrics(text))
    state = "stopped" if not running else "unknown"
    if running and values["sticky_unavailable"] is not None:
        state = "degraded" if values["sticky_unavailable"] else "running"
    charge = values["last_validated_root_charge_bytes"]
    if running and charge is not None and charge > PHYSICAL_CAP_BYTES:
        state = "blocked"
    return NativeCollectorStatus(BACKEND, state, values)


def project_native_status(value):
    """Closed public diagnostic shape; exceptions become existing unknown state."""
    _require(type(value) is dict and set(value) == {"backend", "state", "native_queue"})
    _require(value["backend"] == BACKEND and type(value["backend"]) is str)
    _require(type(value["state"]) is str and value["state"] in {"unknown", "running", "degraded", "stopped", "blocked"})
    data = value["native_queue"]
    _require(type(data) is dict and set(data) == PUBLIC_FIELDS)
    clean = {}
    for name in sorted(PUBLIC_FIELDS):
        item = data[name]
        if name in UNKNOWN_FIELDS:
            _require(item is None)
        elif item is not None:
            if name in BOOL_FIELDS:
                _require(type(item) is bool)
            elif name in FLOAT_FIELDS:
                _require(type(item) in (int, float) and math.isfinite(item) and 0 <= item < MAX_NUMBER)
            else:
                _require(type(item) is int and 0 <= item < MAX_NUMBER)
        clean[name] = item
    # Do not allow an arbitrary DTO to label observed sticky failure healthy.
    if clean["sticky_unavailable"] is True:
        _require(value["state"] in {"degraded", "stopped", "blocked"})
    if value["state"] == "running":
        _require(clean["sticky_unavailable"] is False)
    charge = clean["last_validated_root_charge_bytes"]
    if charge is not None and charge > PHYSICAL_CAP_BYTES:
        _require(value["state"] in {"blocked", "stopped"})
    return {"backend": BACKEND, "state": value["state"], "native_queue": clean}
