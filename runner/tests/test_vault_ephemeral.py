import hashlib
import unittest
from types import SimpleNamespace
from consolecrypt_runner.vault_ephemeral import reference_matches


class EphemeralVaultTests(unittest.TestCase):
    def test_virtual_reference_is_bound_to_exact_snapshot_and_no_saved_reference(self):
        snapshot = SimpleNamespace(files=[], digest='a' * 64)
        expected = hashlib.sha256(b'cc-ansible-vault-ephemeral/v1\0' + snapshot.digest.encode()).hexdigest()
        self.assertTrue(reference_matches(snapshot, expected))
        self.assertFalse(reference_matches(SimpleNamespace(files=[], digest='b' * 64), expected))
        self.assertFalse(reference_matches(snapshot, '0' * 64))
        self.assertFalse(reference_matches(snapshot, None))

    def test_existing_reference_never_falls_back_to_virtual_reference(self):
        snapshot = SimpleNamespace(files=[{'path': '.consolecrypt-secret-inputs.json', 'sha256': 'c' * 64}], digest='a' * 64)
        virtual = hashlib.sha256(b'cc-ansible-vault-ephemeral/v1\0' + snapshot.digest.encode()).hexdigest()
        self.assertTrue(reference_matches(snapshot, 'c' * 64))
        self.assertFalse(reference_matches(snapshot, virtual))

    def test_malformed_snapshot_digest_is_rejected(self):
        for digest in ['A' * 64, 'a' * 65, '../path', None]:
            self.assertFalse(reference_matches(SimpleNamespace(files=[], digest=digest), 'a' * 64))


if __name__ == '__main__':
    unittest.main()
