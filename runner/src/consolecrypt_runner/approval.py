"""Execution authority is an enrolled key and its grant, never model output."""
from __future__ import annotations

import base64
from dataclasses import dataclass
import hashlib
import re
import time

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from .errors import RunnerError, require
from .protocol import encode, uuid_field, MAX_FRAME_BYTES
from .snapshot import relative_path
from .production_owner import verify_owner_confirmation

DOMAIN = b"ConsoleCrypt JobManifest v1\x00"
DIGEST = re.compile(r"^[0-9a-f]{64}$")
MANIFEST_FIELDS = frozenset({"version", "job_id", "tenant_id", "runner_id", "project_id",
    "revision_digest", "inventory_digest", "environment_digest", "parameters_digest", "playbook",
    "policy", "mode", "targets", "issued_at", "expires_at", "access_expires_at", "nonce"})
POLICY_FIELDS = frozenset({"strict_isolation", "cancel_on_lock", "timeout_seconds", "forks", "production"})
TARGET_FIELDS = frozenset({"host_id", "alias", "address", "port", "username", "route_digest", "credential_refs"})


@dataclass(frozen=True)
class TrustGrant:
    public_key: bytes
    tenant_id: str
    allowed_hosts: frozenset[str]
    allowed_projects: frozenset[str]
    scopes: frozenset[str]
    expires_at: int
    max_job_seconds: int = 3600


@dataclass(frozen=True)
class VerifiedManifest:
    canonical: bytes
    digest: str
    signer_id: str
    admission_audit: bytes | None = None

    @property
    def value(self):
        # A caller cannot mutate the authority that was actually verified.
        from .protocol import decode
        return decode(self.canonical)


def _text(value, maximum):
    return isinstance(value, str) and 0 < len(value) <= maximum and not any(ord(c) < 32 or ord(c) == 127 for c in value)


def verify_envelope(envelope: dict, grants: dict[str, TrustGrant], *, runner_id: str,
                    tenant_id: str, environment_digest: str, now: int | None = None) -> VerifiedManifest:
    now = int(time.time()) if now is None else now
    require(isinstance(envelope, dict) and set(envelope) in ({"signer_id", "manifest", "signature"}, {"signer_id", "manifest", "signature", "owner_confirmation"}),
            "approval_shape", "Invalid signed approval envelope")
    signer = uuid_field(envelope["signer_id"], "signer ID")
    grant = grants.get(signer)
    require(grant is not None and grant.expires_at > now and grant.tenant_id == tenant_id,
            "untrusted_signer", "Signer is not enrolled or its grant has expired")
    require("jobs.execute" in grant.scopes, "scope_denied", "Signer cannot execute jobs")
    manifest = envelope["manifest"]
    require(isinstance(manifest, dict) and set(manifest) == MANIFEST_FIELDS,
            "manifest_shape", "Invalid job manifest fields")
    require(type(manifest["version"]) is int and manifest["version"] == 1, "manifest_version", "Unsupported manifest version")
    for name in ("job_id", "tenant_id", "runner_id", "project_id", "nonce"):
        uuid_field(manifest[name], name)
    require(manifest["tenant_id"] == tenant_id and manifest["runner_id"] == runner_id,
            "identity_mismatch", "Approval belongs to another tenant or runner")
    require(manifest["project_id"] in grant.allowed_projects, "project_denied", "Project is not in the enrolled grant")
    playbook = relative_path(manifest['playbook'])
    require(str(playbook) == manifest['playbook'] and playbook.suffix in ('.yml', '.yaml'),
            'playbook_path', 'A canonical YAML playbook path is required')
    for name in ("revision_digest", "inventory_digest", "environment_digest", "parameters_digest"):
        require(isinstance(manifest[name], str) and DIGEST.fullmatch(manifest[name]) is not None,
                "manifest_digest", "Invalid manifest digest")
    require(manifest["environment_digest"] == environment_digest, "environment_changed", "Runner environment changed since approval")
    issued, expiry = manifest["issued_at"], manifest["expires_at"]
    require(type(issued) is int and type(expiry) is int and issued <= now + 30 and
            now - issued <= 600 and now < expiry <= issued + 600,
            "approval_expired", "Approval expired or clock is outside its window")
    require(manifest["mode"] in ("check", "apply"), "run_mode", "Unsupported execution mode")
    policy = manifest["policy"]
    require(isinstance(policy, dict) and set(policy) == POLICY_FIELDS, "policy_shape", "Invalid execution policy")
    require(all(type(policy[name]) is bool for name in ("strict_isolation", "cancel_on_lock", "production")),
            "policy_shape", "Policy flags must be booleans")
    require(type(policy["timeout_seconds"]) is int and 1 <= policy["timeout_seconds"] <= grant.max_job_seconds,
            "policy_timeout", "Job timeout exceeds the enrolled grant")
    access_expiry = manifest['access_expires_at']
    require(type(access_expiry) is int and now < access_expiry <= issued + policy['timeout_seconds'] and
            access_expiry <= grant.expires_at, 'access_expired', 'Job access lease exceeds its signed policy or enrolled grant')
    require(type(policy["forks"]) is int and 1 <= policy["forks"] <= 5, "policy_forks", "Invalid job parallelism")
    targets = manifest["targets"]
    require(isinstance(targets, list) and 0 < len(targets) <= 10000, "target_limit", "Invalid target count")
    seen_ids, seen_aliases = set(), set()
    for target in targets:
        require(isinstance(target, dict) and set(target) == TARGET_FIELDS, "target_shape", "Invalid target manifest")
        host = uuid_field(target["host_id"], "Host ID")
        require(host in grant.allowed_hosts and host not in seen_ids, "target_denied", "Target is outside grant or duplicated")
        seen_ids.add(host)
        alias = target["alias"]
        require(isinstance(alias, str) and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_-]{0,95}", alias) is not None and
                alias not in seen_aliases, "target_alias", "Invalid or duplicate target alias")
        seen_aliases.add(alias)
        require(_text(target["address"], 253) and not target["address"].startswith("-") and
                all(c not in target["address"] for c in (" ", "\t", "/", "@")), "target_address", "Invalid target address")
        require(type(target["port"]) is int and 1 <= target["port"] <= 65535, "target_port", "Invalid target port")
        require(_text(target["username"], 128) and re.fullmatch(r"[A-Za-z0-9_.@\\-]+", target["username"]) is not None and
                not target["username"].startswith("-"), "target_username", "Invalid SSH username")
        require(isinstance(target["route_digest"], str) and DIGEST.fullmatch(target["route_digest"]) is not None,
                "route_digest", "Invalid approved route digest")
        refs = target["credential_refs"]
        require(isinstance(refs, list) and len(refs) <= 64, "credential_refs", "Invalid credential references")
        for reference in refs:
            uuid_field(reference, "credential reference")
        require(len(set(refs)) == len(refs), 'credential_refs', 'Duplicate credential references')
    raw = DOMAIN + encode(manifest)
    require(len(raw) <= MAX_FRAME_BYTES - 1024, "manifest_size", "Manifest exceeds limit")
    try:
        signature = base64.b64decode(envelope["signature"], validate=True)
        require(len(signature) == 64, "approval_signature", "Invalid approval signature")
        Ed25519PublicKey.from_public_bytes(grant.public_key).verify(signature, raw)
    except (InvalidSignature, ValueError, TypeError) as error:
        raise RunnerError("approval_signature", "Invalid approval signature") from error
    if policy['production']:
        verify_owner_confirmation(envelope, manifest, grant, signer, DOMAIN)
    else:
        require('owner_confirmation' not in envelope, 'approval_shape', 'Unexpected production owner evidence')
    from .admission_audit import derive
    digest=hashlib.sha256(raw).hexdigest()
    owner_digest=hashlib.sha256(encode(envelope['owner_confirmation'])).hexdigest() if policy['production'] else None
    audit=derive(manifest,digest,signer,owner_digest)
    return VerifiedManifest(canonical=encode(manifest), digest=digest, signer_id=signer, admission_audit=audit)
