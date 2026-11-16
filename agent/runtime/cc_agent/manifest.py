"""Bounded public configuration. A manifest never confers authorization."""

from dataclasses import dataclass
import ipaddress
import json
from pathlib import Path
import uuid


class ManifestError(ValueError):
    pass


def integer(value, name, minimum, maximum):
    if type(value) is not int or not minimum <= value <= maximum:
        raise ManifestError(f"invalid {name}")
    return value


def identifier(value, name):
    try:
        result = str(uuid.UUID(value))
    except (ValueError, TypeError, AttributeError):
        raise ManifestError(f"invalid {name}") from None
    if value != result:
        raise ManifestError(f"noncanonical {name}")
    return result


@dataclass(frozen=True)
class Manifest:
    agent_id: str
    host_id: str
    bind_host: str = "127.0.0.1"
    port: int = 18080
    sample_interval_seconds: int = 15
    history_max_bytes: int = 256 * 1024 * 1024
    history_max_age_seconds: int = 86400
    history_max_points: int = 5760
    heartbeat_resource_id: str = ""
    history_resource_id: str = ""

    @classmethod
    def from_dict(cls, data):
        keys = {"schema_version", "agent_id", "host_id", "bind_host", "port",
                "sample_interval_seconds", "history", "heartbeat_resource_id", "history_resource_id"}
        if (not isinstance(data, dict) or set(data) - keys
                or type(data.get("schema_version")) is not int or data.get("schema_version") != 1):
            raise ManifestError("unsupported manifest schema or fields")
        agent_id = identifier(data.get("agent_id"), "agent_id")
        host_id = identifier(data.get("host_id"), "host_id")
        bind = data.get("bind_host", "127.0.0.1")
        try:
            address = ipaddress.ip_address(bind)
        except (ValueError, TypeError):
            raise ManifestError("numeric loopback bind required") from None
        if not address.is_loopback or address.version != 4:
            raise ManifestError("this prototype supports IPv4 loopback only")
        history = data.get("history", {})
        if not isinstance(history, dict) or set(history) - {"max_bytes", "max_age_seconds", "max_points"}:
            raise ManifestError("invalid history configuration")
        namespace = uuid.UUID(agent_id)
        heartbeat_id = identifier(data.get("heartbeat_resource_id", str(uuid.uuid5(namespace, "heartbeat-v1"))), "heartbeat_resource_id")
        history_id = identifier(data.get("history_resource_id", str(uuid.uuid5(namespace, "history-v1"))), "history_resource_id")
        if heartbeat_id == history_id:
            raise ManifestError("endpoint resources must differ")
        return cls(agent_id, host_id, str(address),
                   integer(data.get("port", 18080), "port", 0, 65535),
                   integer(data.get("sample_interval_seconds", 15), "sample interval", 1, 300),
                   integer(history.get("max_bytes", 256 * 1024 * 1024), "history bytes", 1024, 256 * 1024 * 1024),
                   integer(history.get("max_age_seconds", 86400), "history age", 1, 86400),
                   integer(history.get("max_points", 5760), "history points", 1, 5760),
                   heartbeat_id, history_id)

    @classmethod
    def load(cls, path):
        path = Path(path)
        if path.stat().st_size > 16384:
            raise ManifestError("manifest too large")
        def unique(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise ManifestError("duplicate manifest field")
                result[key] = value
            return result
        try:
            return cls.from_dict(json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique))
        except (json.JSONDecodeError, UnicodeError):
            raise ManifestError("invalid manifest JSON") from None
