"""One public configuration for the client and an external Ansible role.

Validation/planning is read-only, never an identity or privilege grant. Existing
exporters retain external ownership even when an Agent has broader scopes.
"""
from __future__ import annotations
import argparse
import hashlib
import ipaddress
import json
from pathlib import Path
import re
import sys
from urllib.parse import urlsplit
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "runtime"))
from cc_agent.manifest import Manifest
from .identity_gate import strict_json

MAX_PROFILE_BYTES = 32 * 1024


class ProfileError(ValueError):
    pass


def require(condition, code):
    if not condition:
        raise ProfileError(code)


def obj(value, allowed, required=()):
    require(isinstance(value, dict) and set(value) <= set(allowed) and set(required) <= set(value),
            "unsupported_profile_fields")
    return value


def identifier(value):
    try:
        parsed = uuid.UUID(value)
    except (ValueError, TypeError, AttributeError):
        raise ProfileError("invalid_profile_identity") from None
    require(parsed.int and str(parsed) == value, "invalid_profile_identity")
    return value


def name(value):
    require(isinstance(value, str) and re.fullmatch(r"[a-zA-Z][a-zA-Z0-9_-]{0,31}", value), "invalid_profile_name")
    return value


def backend_url(value):
    require(isinstance(value, str) and len(value) <= 512 and all(33 <= ord(c) < 127 for c in value),
            "invalid_backend_url")
    try:
        parsed = urlsplit(value)
        port = parsed.port
    except ValueError:
        raise ProfileError("invalid_backend_url") from None
    require(parsed.scheme in {"http", "https"} and bool(parsed.hostname) and
            parsed.username is None and parsed.password is None and not parsed.query and not parsed.fragment,
            "invalid_backend_url")
    require(port is None or 1 <= port <= 65535, "invalid_backend_url")
    if parsed.scheme == "http":
        try:
            require(ipaddress.ip_address(parsed.hostname).is_loopback, "insecure_backend_denied")
        except ValueError:
            raise ProfileError("insecure_backend_denied") from None
    return value


class ProfileManifest:
    def __init__(self, canonical):
        self._canonical = canonical

    @classmethod
    def load(cls, path):
        path = Path(path)
        require(path.stat().st_size <= MAX_PROFILE_BYTES, "profile_size_limit")
        with path.open("rb") as source:
            raw = source.read(MAX_PROFILE_BYTES + 1)
        require(len(raw) <= MAX_PROFILE_BYTES, "profile_size_limit")
        return cls.from_dict(strict_json(raw))

    @classmethod
    def from_dict(cls, raw):
        raw = obj(raw, {"format", "identity", "labels", "collection", "runtime"},
                  {"format", "identity", "labels", "collection"})
        require(raw["format"] == "cc-agent-profile/v1", "unsupported_profile_format")
        identity = obj(raw["identity"], {"host_id", "agent_id", "public_key_fingerprint"}, {"host_id"})
        host = identifier(identity["host_id"])
        agent = identity.get("agent_id")
        fingerprint = identity.get("public_key_fingerprint")
        if agent is not None:
            identifier(agent)
            require(isinstance(fingerprint, str) and re.fullmatch(r"[0-9a-f]{64}", fingerprint)
                    and fingerprint != "0" * 64, "public_key_fingerprint_required")
        else:
            require(fingerprint is None, "fingerprint_without_agent")
        labels = obj(raw["labels"], {"site", "environment"}, {"site", "environment"})
        labels = {key: name(labels[key]) for key in ("site", "environment")}
        collection = obj(raw["collection"], {"mode", "base_profile", "exporters", "backend", "buffer",
                                               "scrape_interval_seconds"}, {"mode"})
        mode = collection["mode"]
        require(isinstance(mode, str) and mode in {"agent_only", "existing_prometheus", "pull", "remote_write"}, "unsupported_collection_mode")
        require(agent is not None or mode == "existing_prometheus", "agent_identity_required")
        base = collection.get("base_profile", "none")
        require(isinstance(base, str) and base in {"none", "linux_basic"}, "unsupported_base_profile")
        require("buffer" not in collection or mode == "remote_write", "unexpected_delivery_buffer")
        exporters = collection.get("exporters", [])
        require(isinstance(exporters, list) and len(exporters) <= 16, "exporter_count_limit")
        normalized, ids, urls = [], set(), set()
        for exporter in exporters:
            exporter = obj(exporter, {"id", "kind", "url", "ownership", "enabled", "sample_limit", "series_limit",
                "response_bytes", "label_limit", "artifact", "management"}, {"id", "kind", "url", "ownership"})
            eid = name(exporter["id"])
            require(type(exporter.get("enabled", True)) is bool, "invalid_exporter_enabled")
            require(isinstance(exporter["ownership"], str) and exporter["ownership"] in {"external", "agent", "reviewedAnsible"}, "invalid_exporter_ownership")
            require(isinstance(exporter["kind"], str) and exporter["kind"] in {"node_exporter", "application", "cadvisor"}, "unsupported_exporter_kind")
            require(isinstance(exporter["url"], str), "invalid_exporter_url")
            require(eid not in ids and exporter["url"] not in urls, "duplicate_exporter_denied")
            ids.add(eid)
            urls.add(exporter["url"])
            item = {**exporter, "enabled": exporter.get("enabled", True)}
            if item["ownership"] == "external":
                require("artifact" not in exporter, "external_exporter_lifecycle_denied")
                require("management" not in exporter, "unsupported_profile_fields")
            elif item["ownership"] == "reviewedAnsible":
                from collector.manager import CollectorError, validate_reviewed_ansible_component
                try:
                    item["management"] = validate_reviewed_ansible_component(item, approved_host_id=host)
                except CollectorError as error:
                    raise ProfileError(str(error)) from None
            else:
                require("management" not in exporter, "unsupported_profile_fields")
                artifact = obj(item.get("artifact"), {"version", "sha256"}, {"version", "sha256"})
                require(isinstance(artifact["version"], str) and len(artifact["version"]) <= 64
                        and re.fullmatch(r"v?[0-9]+\.[0-9]+\.[0-9]+", artifact["version"])
                        and isinstance(artifact["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", artifact["sha256"]),
                        "pinned_managed_exporter_required")
            normalized.append(item)
        enabled = [item for item in normalized if item["enabled"]]
        if mode in {"agent_only", "existing_prometheus"}:
            require(not enabled and base == "none" and "buffer" not in collection
                    and "scrape_interval_seconds" not in collection, "connection_only_cannot_collect")
        else:
            require(bool(enabled), "enabled_exporter_required")
        if base == "linux_basic":
            require(sum(item["kind"] == "node_exporter" for item in enabled) == 1,
                    "linux_basic_requires_one_explicit_node_exporter")
        backend = collection.get("backend")
        if mode in {"existing_prometheus", "remote_write"}:
            backend = obj(backend, {"url"}, {"url"})
            backend = {"url": backend_url(backend["url"])}
        else:
            require(backend is None, "unexpected_backend")
        runtime = obj(raw.get("runtime", {}), {"bind_host", "port", "sample_interval_seconds", "history"})
        require("bind_host" not in runtime or isinstance(runtime["bind_host"], str), "runtime_bind_must_be_string")
        data = {"format": raw["format"], "identity": {"host_id": host, "agent_id": agent,
            "public_key_fingerprint": fingerprint}, "labels": labels,
            "collection": {**collection, "mode": mode, "base_profile": base, "exporters": normalized},
            "runtime": runtime}
        if backend is not None:
            data["collection"]["backend"] = backend
        result = cls(json.dumps(data, sort_keys=True, separators=(",", ":"), allow_nan=False).encode())
        require(len(result._canonical) <= MAX_PROFILE_BYTES, "profile_size_limit")
        if agent is not None:
            Manifest.from_dict(result.runtime_manifest())
        # Reuse the actual collector validator without trusting scope strings as
        # authorization. This is purely configuration validation, with no I/O.
        if normalized:
            from collector import validate_manifest
            for item in normalized:
                target = {key: value for key, value in item.items() if key not in {"ownership", "enabled", "artifact", "management"}}
                validate_manifest({"schema_version": 1, "mode": "pull", "targets": [target],
                    "labels": {"cc_host_id": host, **labels}}, granted_scopes={"metrics.collect"},
                    allowed_endpoints={target["url"]}, approved_host_id=host)
        if mode in {"pull", "remote_write"}:
            from collector import validate_manifest
            manifest = result.collector_manifest()
            allowed = {item["url"] for item in manifest["targets"]}
            if mode == "remote_write":
                allowed.add(manifest["receiver"]["url"])
            validate_manifest(manifest, granted_scopes={"metrics.collect"}, allowed_endpoints=allowed,
                              approved_host_id=host)
        return result

    @property
    def data(self):
        return json.loads(self._canonical)

    @property
    def digest(self):
        return hashlib.sha256(b"consolecrypt.agent.profile.v1\0" + self._canonical).hexdigest()

    def runtime_manifest(self):
        data = self.data
        require(data["identity"]["agent_id"] is not None, "agent_not_requested")
        return {"schema_version": 1, "agent_id": data["identity"]["agent_id"],
                "host_id": data["identity"]["host_id"], **data["runtime"]}

    def collector_manifest(self):
        data = self.data
        collection = data["collection"]
        if collection["mode"] not in {"pull", "remote_write"}:
            return None
        targets = []
        for item in collection["exporters"]:
            if item["enabled"]:
                targets.append({key: value for key, value in item.items()
                                if key not in {"ownership", "enabled", "artifact", "management"}})
        result = {"schema_version": 1, "mode": collection["mode"], "targets": targets,
                  "labels": {"cc_host_id": data["identity"]["host_id"], **data["labels"]}}
        if "scrape_interval_seconds" in collection:
            result["scrape_interval_seconds"] = collection["scrape_interval_seconds"]
        if collection["mode"] == "remote_write":
            result["receiver"] = collection["backend"]
            if "buffer" in collection:
                result["buffer"] = collection["buffer"]
        return result

    def plan(self):
        data = self.data
        mode = data["collection"]["mode"]
        actions = []
        blockers = []
        if mode == "existing_prometheus":
            actions.append({"kind": "connect_existing_prometheus", "starts_collection": False,
                            "requires_capability_probe": True})
        elif mode == "agent_only":
            actions.append({"kind": "configure_agent_telemetry", "requires_signed_identity": True})
        else:
            for exporter in data["collection"]["exporters"]:
                if not exporter["enabled"]:
                    continue
                external = exporter["ownership"] == "external"
                reviewed_ansible = exporter["ownership"] == "reviewedAnsible"
                actions.append({"kind": "review_ansible_component" if reviewed_ansible else "attach_exporter" if external else "review_managed_exporter",
                    "id": exporter["id"], "ownership": exporter["ownership"],
                    "lifecycle_permissions": [],
                    "required_lifecycle_scope": None if external or reviewed_ansible else "exporters.manage",
                    "starts_exporter": False, "updates_exporter": False, "deletes_exporter": False})
                if reviewed_ansible:
                    actions[-1].update({"managedBy": "reviewedAnsible", "component_state": "declared",
                        "state_source": "configuration_only", "observed_state": None,
                        "component_manifest": exporter["management"]["manifest"],
                        "manifestSha256": exporter["management"]["manifestSha256"],
                        "operations_requiring_native_confirmation": ["inspect", "install", "start", "stop", "remove"],
                        "requires_separate_operator_job": True, "agent_container_runtime_access": False})
                elif not external:
                    blockers.append("managed_exporter_installation_not_implemented")
                    if exporter["kind"] in {"node_exporter", "cadvisor"}:
                        from collector import exporter_privileges
                        actions[-1]["privilege_review"] = exporter_privileges(exporter["kind"])
                    else:
                        actions[-1]["privilege_review"] = {"enabled": False, "privileges": "explicit_application_review_required"}
            if mode == "remote_write":
                blockers.append("strict_remote_write_byte_age_policy_unverified")
                blockers.append("pinned_vmagent_physical_byte_cap_disproven")
                blockers.append("pinned_collector_receiver_redirect_destination_not_pinned")
                blockers.append("pinned_collector_receiver_error_body_unbounded")
        return {"format": "cc-agent-plan/v1", "profile_digest": self.digest,
                "mode": mode, "actions": actions, "blockers": sorted(set(blockers)),
                "applies_changes": False, "agent_required": mode != "existing_prometheus",
                "required_agent_scopes": ["metrics.collect"] if mode in {"pull", "remote_write"} else [],
                "host_binding": "configured_not_identity_attested",
                "declared_identity_is_not_registration": True}


def validate_fleet(profiles):
    require(isinstance(profiles, (list, tuple)) and 1 <= len(profiles) <= 1024, "fleet_limit")
    seen = {"host_id": set(), "agent_id": set(), "public_key_fingerprint": set()}
    for profile in profiles:
        require(isinstance(profile, ProfileManifest), "invalid_fleet_profile")
        identity = profile.data["identity"]
        for key, values in seen.items():
            value = identity[key]
            if value is not None:
                require(value not in values, "duplicate_fleet_" + key)
                values.add(value)
    return True


def main():
    parser = argparse.ArgumentParser(description="Validate/render one public Agent profile; never install or fetch")
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--output", choices=("plan", "runtime", "collector", "profile"), default="plan")
    args = parser.parse_args()
    profile = ProfileManifest.load(args.manifest)
    result = {"plan": profile.plan, "runtime": profile.runtime_manifest,
              "collector": profile.collector_manifest, "profile": lambda: profile.data}[args.output]()
    print(json.dumps(result, sort_keys=True, separators=(",", ":"), allow_nan=False))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, TypeError, OSError, RecursionError) as error:
        print(json.dumps({"error": str(error) if isinstance(error, ValueError) else "invalid_profile"}), file=sys.stderr)
        raise SystemExit(1)
