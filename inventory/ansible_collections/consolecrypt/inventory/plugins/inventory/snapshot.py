"""Explicit static snapshot inventory. Never reads ConsoleCrypt's vault/API."""
from ansible.errors import AnsibleParserError
from ansible.plugins.inventory import BaseInventoryPlugin
from ansible_collections.consolecrypt.inventory.plugins.module_utils.snapshot import read_snapshot, SnapshotError

DOCUMENTATION = r'''
name: snapshot
plugin_type: inventory
short_description: Read an explicitly exported ConsoleCrypt static snapshot
description:
  - Reads only strict local versioned snapshot data.
  - No live vault API, credentials, composition, network or cache integration.
  - Enable this plugin explicitly and pass a C(.consolecrypt.json) snapshot.
author:
  - ConsoleCrypt contributors
'''


class InventoryModule(BaseInventoryPlugin):
    NAME = 'consolecrypt.inventory.snapshot'

    def verify_file(self, path):
        # No auto-plugin YAML configuration or plugin chaining. This suffix is
        # only dispatch; the shared strict reader provides actual validation.
        return isinstance(path, str) and path.endswith('.consolecrypt.json')

    def parse(self, inventory, loader, path, cache=True):
        try:
            data = read_snapshot(path)
        except SnapshotError as error:
            raise AnsibleParserError(str(error)) from None
        # Validate the entire bounded document before modifying inventory.
        super().parse(inventory, loader, path, cache=False)
        groups = [name for name in data if name not in ('_meta', 'all', 'ungrouped')]
        for name in groups:
            inventory.add_group(name)
        for name in sorted(data['_meta']['hostvars']):
            inventory.add_host(name)
            for key, value in data['_meta']['hostvars'][name].items():
                inventory.set_variable(name, key, value)
        for group in groups:
            inventory.add_child('all', group)
            for host in data[group]['hosts']:
                inventory.add_host(host, group=group)
