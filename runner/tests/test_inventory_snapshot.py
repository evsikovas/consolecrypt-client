from dataclasses import replace
import unittest
import uuid

from consolecrypt_runner.errors import RunnerError
from consolecrypt_runner.inventory import StaticInventory, merge_inventories, parse_yaml_inventory


class InventorySnapshotTests(unittest.TestCase):
    def test_domain_login_and_long_credential_chain_roundtrip_as_data(self):
        inventory=parse_yaml_inventory("all:\n  hosts:\n    target:\n      ansible_host: example.test\n      ansible_user: 'CORP\\alice@example.test'\n",'domain-fixture')
        inventory.hosts['target']=replace(inventory.hosts['target'],host_id=str(uuid.uuid4()),credential_refs=tuple(str(uuid.uuid4()) for _ in range(34)))
        restored=StaticInventory.from_snapshot(inventory.to_snapshot())
        self.assertEqual(restored.hosts['target'].username,r'CORP\alice@example.test')
        self.assertEqual(restored.hosts['target'].credential_refs,inventory.hosts['target'].credential_refs)

    def setUp(self):
        self.inventory = parse_yaml_inventory('''all:
  children:
    web:
      hosts:
        web_a:
          ansible_host: 192.0.2.1
          ansible_port: 2222
          ansible_user: deploy
        web_b:
          ansible_host: 192.0.2.2
          ansible_user: deploy
''', 'fixture.yaml')
        for alias, host in list(self.inventory.hosts.items()):
            self.inventory.hosts[alias] = replace(host, host_id=str(uuid.uuid4()), credential_refs=(str(uuid.uuid4()),))

    def test_roundtrip_keeps_binding_provenance_and_never_exports_credentials(self):
        snapshot = self.inventory.to_snapshot()
        restored = StaticInventory.from_snapshot(snapshot)
        self.assertEqual(restored.digest, self.inventory.digest)
        self.assertEqual(restored.hosts['web_a'].host_id, self.inventory.hosts['web_a'].host_id)
        self.assertNotIn('credential_refs', str(restored.standard_json()))
        snapshot['hosts'][0]['host_id'] = str(uuid.uuid4())
        self.assertNotEqual(StaticInventory.from_snapshot(snapshot).digest, restored.digest)

    def test_runtime_inventory_is_limited_to_exact_approved_bindings(self):
        host = self.inventory.hosts['web_a']
        target = {'alias': host.alias, 'host_id': host.host_id, 'address': host.address, 'port': host.port,
                  'username': host.username, 'credential_refs': list(host.credential_refs)}
        runtime = self.inventory.runtime_inventory({'targets': [target]})
        self.assertEqual(set(runtime['all']['hosts']), {'web_a'})
        self.assertEqual(runtime['all']['hosts']['web_a']['consolecrypt_host_id'], host.host_id)
        self.assertEqual(set(runtime['web']['hosts']), {'web_a'})
        for change in ({'host_id': str(uuid.uuid4())}, {'address': '192.0.2.99'}, {'port': 22}, {'credential_refs': []}):
            with self.subTest(change=change), self.assertRaises(RunnerError):
                self.inventory.runtime_inventory({'targets': [target | change]})

    def test_different_bound_id_cannot_merge_by_matching_display_alias_or_endpoint(self):
        other = StaticInventory.from_snapshot(self.inventory.to_snapshot())
        other.hosts['web_a'] = replace(other.hosts['web_a'], host_id=str(uuid.uuid4()))
        with self.assertRaises(RunnerError): merge_inventories([self.inventory, other])
        same = merge_inventories([self.inventory, StaticInventory.from_snapshot(self.inventory.to_snapshot())])
        self.assertEqual(same.hosts['web_a'].host_id, self.inventory.hosts['web_a'].host_id)
