"""Read-only public profile renderer shared by the client contract and Ansible.

JSON stdin: {"manifest": <cc-agent-profile/v1>, "fleet": [profiles...] optional}.
No credentials, service changes, package download or ownership migration occurs.
"""

import argparse
import json
from pathlib import Path
import sys

AGENT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(AGENT))
from integration.profile_manifest import ProfileManifest, validate_fleet
from integration.identity_gate import strict_json

MAX_INPUT = 2 * 1024 * 1024


def render(data):
    if not isinstance(data, dict) or set(data) - {"manifest", "fleet"} or "manifest" not in data:
        raise ValueError("profile envelope fields")
    profile = ProfileManifest.from_dict(data["manifest"])
    fleet_data = data.get("fleet", [])
    if not isinstance(fleet_data, list) or len(fleet_data) > 64:
        raise ValueError("role fleet limit")
    if not fleet_data:
        fleet_data = [data["manifest"]]
    fleet = [ProfileManifest.from_dict(item) for item in fleet_data]
    validate_fleet(fleet)
    if profile.digest not in {item.digest for item in fleet}:
        raise ValueError("current profile missing from fleet")
    plan = profile.plan()
    runtime = profile.runtime_manifest() if plan["agent_required"] else None
    return {"profile": profile.data, "plan": plan, "runtime": runtime,
            "collector": profile.collector_manifest(),
            "metadata": {"profile_digest": profile.digest, "fleet_profiles_validated": len(fleet),
                         "configuration_only": True, "performs_service_changes": False}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    raw = sys.stdin.buffer.read(MAX_INPUT + 1)
    if len(raw) > MAX_INPUT:
        parser.error("profile input budget")
    try:
        result = render(strict_json(raw))
    except (ValueError, TypeError, KeyError):
        parser.error("invalid public profile or fleet")
    print(json.dumps(result, sort_keys=True, separators=(",", ":"), allow_nan=False))


if __name__ == "__main__":
    main()
