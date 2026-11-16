"""Metadata custody for the SAME native WAL, never a grant or a second queue."""
from dataclasses import dataclass
import json
import re

from disk_binding import BoundDiskRoot, DiskBinding, canonical, checked_uuid, need


@dataclass(frozen=True)
class ServiceDiskBinding:
    original: DiskBinding
    service_id: str
    delegation_sha256: str
    policy_revision: int
    revocation_epoch: int
    expires_unix_seconds: int

    @property
    def collector_id(self):
        return self.original.collector_id

    def checked(self):
        self.original.checked()
        need(checked_uuid(self.service_id).version == 4, "service_disk_id")
        need(type(self.delegation_sha256) is str
             and re.fullmatch("[0-9a-f]{64}", self.delegation_sha256), "service_disk_digest")
        for n in (self.policy_revision, self.revocation_epoch):
            need(type(n) is int and 1 <= n <= 18446744073709551615, "service_disk_revision")
        need(type(self.expires_unix_seconds) is int
             and 1 <= self.expires_unix_seconds <= 253402300799, "service_disk_expiry")
        return self

    def bytes(self):
        self.checked()
        return canonical({
            "format": "cc-agent-disk-service-binding/v1",
            "original": json.loads(self.original.bytes()),
            "serviceId": self.service_id,
            "delegationSha256": self.delegation_sha256,
            "policyRevision": self.policy_revision,
            "revocationEpoch": self.revocation_epoch,
            "expiresUnixSeconds": self.expires_unix_seconds,
        })

    def check_bytes(self, raw):
        need(type(raw) is bytes and raw == self.bytes(), "service_disk_binding_changed")


def retained_root(binding):
    """Caller already owns the real service Permit, not an input approval flag.

    BoundDiskRoot performs the same held-directory/session/native-writer locks.
    Its fresh binding requires empty WAL. Existing v1 interactive data or a
    different delegation cannot be silently adopted as this service's WAL.
    """
    need(type(binding) is ServiceDiskBinding, "service_disk_binding")
    return BoundDiskRoot(binding.checked())
