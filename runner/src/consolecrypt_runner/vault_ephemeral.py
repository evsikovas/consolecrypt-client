"""Public snapshot binding for a Native-owned one-launch Vault password.

This hash grants no password access. The existing original Native phase/job
broker still owns the secret, expiry, one-use budgets and signed-job checks.
"""
import hashlib
import re


def reference_matches(snapshot, expected):
    if not isinstance(expected, str) or not re.fullmatch(r'[0-9a-f]{64}', expected):
        return False
    reference = next((f for f in snapshot.files if f['path'] == '.consolecrypt-secret-inputs.json'), None)
    if reference is not None:
        return reference['sha256'] == expected
    if not isinstance(snapshot.digest, str) or not re.fullmatch(r'[0-9a-f]{64}', snapshot.digest):
        return False
    digest = hashlib.sha256(b'cc-ansible-vault-ephemeral/v1\0' + snapshot.digest.encode('ascii')).hexdigest()
    return expected == digest
