"""Report loaded runtime config separately from a newly requested profile.

No file change silently applies collector settings or confirms an update. The
existing heartbeat version comes from running code, never the desired profile.
"""
import hashlib
import json
from dataclasses import asdict
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "runtime"))
from cc_agent.manifest import Manifest
from cc_agent.service import AgentRuntime
from .profile_manifest import ProfileManifest


def runtime_digest(manifest):
    # Hash the effective configuration, including resolved defaults and resource
    # IDs, rather than the spelling of the requested JSON projection.
    effective = {"schema_version": 1, **asdict(Manifest.from_dict(manifest))}
    raw = json.dumps(effective, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    return hashlib.sha256(b"consolecrypt.agent.runtime-config.v1\0" + raw).hexdigest()


class ProfileRuntime(AgentRuntime):
    def __init__(self, profile_path, profile, **kwargs):
        self.profile_path = Path(profile_path)
        self.loaded_profile_digest = profile.digest
        runtime = profile.runtime_manifest()
        self.applied_runtime_digest = runtime_digest(runtime)
        self.loaded_collection_mode = profile.data["collection"]["mode"]
        super().__init__(Manifest.from_dict(runtime), **kwargs)

    def configuration_status(self):
        requested = None
        state = "invalid_requested_profile"
        try:
            requested = ProfileManifest.load(self.profile_path).digest
            state = "matches_loaded" if requested == self.loaded_profile_digest else "changed_pending_restart"
        except (ValueError, TypeError, OSError, RecursionError):
            pass
        return {"format": "cc-agent-config-status/v1", "requested_profile_digest": requested,
                "loaded_profile_digest": self.loaded_profile_digest,
                "applied_runtime_config_digest": self.applied_runtime_digest,
                "applied_profile_digest": None, "requested_state": state,
                "collector_application": "not_requested" if self.loaded_collection_mode in
                    {"agent_only", "existing_prometheus"} else "pending_external_or_authorized_apply",
                "full_profile_application_confirmed": False}

    def heartbeat(self):
        result = super().heartbeat()
        result["configuration"] = self.configuration_status()
        return result
