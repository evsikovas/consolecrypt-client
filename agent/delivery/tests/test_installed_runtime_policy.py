"""DRAFT owned-filesystem/pure policy causals. No systemctl/service/payload use."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('owned_installed_runtime_policy', HERE / 'installed_runtime_policy.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


def config(relay=None, dns=None):
    return {'format': 'cc-agent-reviewed-egress/v1', 'relayIps': relay or [], 'dnsResolverIps': dns or []}


@unittest.skipUnless(sys.platform == 'linux', 'held nofollow/nonblocking source contract requires Linux')
class RuntimePolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='cc-runtime-policy-owned-')
        self.root = Path(self.temp.name)
        class Paths:
            pass
        self.paths = Paths()
        self.paths.egress_config = self.root / 'etc/consolecrypt-agent/egress.json'
        self.paths.runtime_dropin = self.root / ('etc/systemd/system/' + m.UNIT + '.d') / m.DROPIN
        self.paths.runtime_record = self.root / 'opt/consolecrypt-agent-install/runtime-policy-owner.json'
        for parent in (self.paths.egress_config.parent, self.paths.runtime_dropin.parent.parent, self.paths.runtime_record.parent):
            parent.mkdir(parents=True, exist_ok=True)

    def tearDown(self):
        self.temp.cleanup()

    def desired(self, value):
        self.paths.egress_config.write_bytes(m.canonical(value) + b'\n')

    def publish(self, value):
        self.desired(value)
        m.Review(self.paths, uid=None).apply(None)

    def test_absent_config_keeps_loopback_base_without_creating_dropin_or_receipt(self):
        original = m.Review(self.paths, uid=None)
        original.apply(None)
        self.assertIsNone(m.render(None, None))
        self.assertFalse(self.paths.runtime_dropin.exists())
        self.assertFalse(self.paths.runtime_record.exists())

    def test_literal_host_addresses_append_only_without_deny_or_localhost_reset(self):
        value = config(['192.0.2.9', '2001:db8::9'], ['127.0.0.53'])
        self.publish(value)
        actual = self.paths.runtime_dropin.read_bytes()
        self.assertEqual(actual, m.render(value, None))
        self.assertIn(b'IPAddressAllow=192.0.2.9/32\n', actual)
        self.assertIn(b'IPAddressAllow=2001:db8::9/128\n', actual)
        self.assertIn(b'IPAddressAllow=127.0.0.53/32\n', actual)
        self.assertNotIn(b'IPAddressAllow=\n', actual)
        self.assertNotIn(b'IPAddressDeny=', actual)

    def test_closed_address_schema_denies_broad_alias_duplicate_and_discovered_names(self):
        bad = ['0.0.0.0', '::', '0.0.0.0/0', '192.0.2.9/32', 'example.com',
               '2001:DB8::9', 'fe80::1%eth0', '::ffff:192.0.2.9', '224.0.0.1', '255.255.255.255']
        for address in bad:
            with self.subTest(address=address), self.assertRaises(ValueError):
                m.config(config([address]))
        for value in (config(['192.0.2.9'], ['192.0.2.9']), config(['192.0.2.' + str(i) for i in range(1, 18)]),
                      dict(config(), autoResolve=True), dict(config(), relayIps=True)):
            with self.subTest(value=value), self.assertRaises(ValueError):
                m.config(value)

    def test_duplicate_json_and_float_are_not_configuration(self):
        for raw in (b'{"format":"x","format":"x"}', b'{"value":1.0}', b'{"value":NaN}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                m.strict(raw)

    def test_exact_managed_old_config_allows_approved_new_config_without_adopting_unknown_bytes(self):
        self.publish(config(['192.0.2.9']))
        old = self.paths.runtime_dropin.read_bytes()
        self.desired(config(['198.51.100.7']))
        reviewed = m.Review(self.paths, uid=None)
        self.assertEqual(reviewed.old_dropin.raw, old)
        reviewed.apply(None)
        self.assertEqual(self.paths.runtime_dropin.read_bytes(), m.render(config(['198.51.100.7']), None))

    def test_absent_desired_config_removes_only_matching_owned_policy(self):
        self.publish(config(['192.0.2.9']))
        self.paths.egress_config.unlink()
        m.Review(self.paths, uid=None).apply(None)
        self.assertFalse(self.paths.runtime_dropin.exists())
        self.assertFalse(self.paths.runtime_record.exists())

    def test_foreign_extra_or_fixed_filename_without_record_is_never_removed(self):
        self.paths.runtime_dropin.parent.mkdir()
        self.paths.runtime_dropin.write_bytes(b'foreign')
        with self.assertRaises(ValueError):
            m.Review(self.paths, uid=None)
        self.assertEqual(self.paths.runtime_dropin.read_bytes(), b'foreign')
        self.paths.runtime_dropin.unlink()
        other = self.paths.runtime_dropin.parent / '99-unowned.conf'
        other.write_bytes(b'foreign')
        with self.assertRaises(ValueError):
            m.Review(self.paths, uid=None)
        self.assertEqual(other.read_bytes(), b'foreign')

    def test_changed_existing_policy_does_not_gain_ownership_from_new_desired_config(self):
        self.publish(config(['192.0.2.9']))
        self.paths.runtime_dropin.write_bytes(b'foreign changed after install')
        self.desired(config(['198.51.100.7']))
        with self.assertRaises(ValueError):
            m.Review(self.paths, uid=None)
        self.assertEqual(self.paths.runtime_dropin.read_bytes(), b'foreign changed after install')

    def test_config_or_owned_inode_swap_after_preflight_is_refused_before_publication(self):
        self.publish(config(['192.0.2.9']))
        original = self.paths.runtime_dropin.read_bytes()
        reviewed = m.Review(self.paths, uid=None)
        self.desired(config(['198.51.100.7']))
        with self.assertRaises(ValueError):
            reviewed.apply(None)
        self.assertEqual(self.paths.runtime_dropin.read_bytes(), original)
        reviewed = m.Review(self.paths, uid=None)
        replacement = self.paths.runtime_dropin.parent / '.owned-test-replacement'
        replacement.write_bytes(original)
        os.replace(replacement, self.paths.runtime_dropin)
        with self.assertRaises(ValueError):
            reviewed.apply(None)

    def test_first_publication_is_exclusive_and_preserves_racing_foreign_file(self):
        self.desired(config(['192.0.2.9']))
        reviewed = m.Review(self.paths, uid=None)
        original = os.link
        def racing(source, target, **kwargs):
            Path(target).write_bytes(b'foreign racing policy')
            return original(source, target, **kwargs)
        with patch.object(m.os, 'link', side_effect=racing), self.assertRaises(FileExistsError):
            reviewed.apply(None)
        self.assertEqual(self.paths.runtime_dropin.read_bytes(), b'foreign racing policy')
        self.assertFalse(self.paths.runtime_record.exists())

    def test_symlink_hardlink_or_fifo_config_denied_before_blocking_read(self):
        source = self.root / 'owned-source'
        source.write_bytes(m.canonical(config(['192.0.2.9'])))
        self.paths.egress_config.symlink_to(source)
        with self.assertRaises(ValueError):
            m.Review(self.paths, uid=None)
        self.paths.egress_config.unlink()
        os.link(source, self.paths.egress_config)
        with self.assertRaises(ValueError):
            m.Review(self.paths, uid=None)
        self.paths.egress_config.unlink()
        os.mkfifo(self.paths.egress_config)
        with self.assertRaises(ValueError):
            m.Review(self.paths, uid=None)

    def test_collector_profile_uses_only_live_fixed_nss_and_composes_one_file(self):
        profile = {'format': 'cc-agent-installed-collector-profile/v1', 'uid': 1401, 'gid': 1402}
        with patch.object(m, 'service_ids', return_value=(1401, 1402)):
            rendered = m.render(config(['192.0.2.9']), profile)
            self.assertIn(b'Requires=run-consolecrypt\\x2dagent-volatile.mount user@1401.service\n', rendered)
            self.assertIn(b'RuntimeDirectoryPreserve=yes\n', rendered)
            self.assertIn(b'ProtectHome=tmpfs\n', rendered)
            self.assertIn(b'BindReadOnlyPaths=/run/user/1401\n', rendered)
            self.assertNotIn(b'ProtectHome=no', rendered)
            self.assertIn(b'IPAddressAllow=192.0.2.9/32\n', rendered)
        with patch.object(m, 'service_ids', return_value=(1403, 1402)), self.assertRaises(ValueError):
            m.render(None, profile)
        with patch.object(m, 'service_ids', return_value=(1401, 1402)), self.assertRaises(ValueError):
            m.render(None, dict(profile, unit='caller.service'))


if __name__ == '__main__':
    unittest.main()
