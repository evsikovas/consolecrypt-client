"""Bounded file-integrity checks for a prototype notice inventory, not legal advice/SBOM validation."""

import hashlib
import json
import re

INVENTORY = "notices/inventory.json"


def validate_notice_inventory(files, *, required=False):
    raw = files.get(INVENTORY)
    if raw is None:
        if required:
            raise ValueError("notice inventory required")
        return None
    if not isinstance(raw, bytes) or len(raw) > 256 * 1024:
        raise ValueError("notice inventory budget")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate inventory field")
            result[key] = value
        return result
    inventory = json.loads(raw, object_pairs_hook=unique)
    if (not isinstance(inventory, dict) or inventory.get("format") != "cc-agent-notice-inventory/v1"
            or not isinstance(inventory.get("components"), list) or len(inventory["components"]) > 512):
        raise ValueError("invalid notice inventory")
    checked = set()
    for component in inventory["components"]:
        if not isinstance(component, dict) or not isinstance(component.get("notices", []), list):
            raise ValueError("invalid component notice list")
        for notice in component.get("notices", []):
            if not isinstance(notice, dict) or set(notice) != {"original_path", "package_path", "sha256"}:
                raise ValueError("invalid notice entry")
            path, expected = notice["package_path"], notice["sha256"]
            if (not isinstance(path, str) or not re.fullmatch(r"notices/text/[0-9a-f]{64}\.txt", path)
                    or not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected)):
                raise ValueError("invalid notice path/digest")
            value = files.get(path)
            if not isinstance(value, bytes) or not value or len(value) > 128 * 1024:
                raise ValueError("required full notice missing")
            if hashlib.sha256(value).hexdigest() != expected:
                raise ValueError("notice digest mismatch")
            checked.add(path)
    return {"components": len(inventory["components"]), "distinct_notice_files": len(checked),
            "file_integrity_checked": True, "legal_certification": False, "validated_sbom": False}
