# ConsoleCrypt inventory collection

INV-09 source integration (not included in the immutable R10 preview). The collection name is `consolecrypt.inventory`; the
inventory plugin's full name is `consolecrypt.inventory.snapshot`, following
Ansible's `namespace.collection.plugin` naming rule.

This collection reads an explicitly issued **static data file**. It never opens
ConsoleCrypt's vault, contacts a live API, obtains credentials, executes the
snapshot, starts a target command, or grants native ConsoleCrypt authority.
Origin fields are informational: an ordinary file can be edited. Exported data
has no access lease or live revocation promise. Users supply their own external
Ansible credentials and SSH host-key trust; this collection never disables it.

Version 1 exports only explicitly selected saved hosts with direct SSH routes.
Endpoint fields are `ansible_host`, `ansible_port`, and `ansible_user`.
Only explicitly selected project Ansible groups appear, including empty groups.
Group membership cannot expand the selected hosts. No arbitrary variables,
credential references, passwords, key paths, SSH arguments, proxy or tunnel
settings, templates, display text, notes or tags are accepted.

Snapshots must be UTF-8 JSON regular files of at most 256 KiB, with the exact
versioned schema. The reader rejects symlinks in every path component, special
files, duplicate/unknown properties, future versions, inconsistent membership,
over-limit data, template expressions, control characters and unsupported
endpoints. The current descriptor reader supports POSIX control nodes with
`O_NOFOLLOW` and `dir_fd`; unsupported platforms fail explicitly.

## Offline CLI

After placing this collection under an explicit Ansible collection path:

```console
python /explicit/path/ansible_collections/consolecrypt/inventory/bin/consolecrypt-inventory --snapshot /explicit/path/issued.consolecrypt.json --list
python /explicit/path/ansible_collections/consolecrypt/inventory/bin/consolecrypt-inventory --snapshot /explicit/path/issued.consolecrypt.json --host cc_11111111111141118111111111111111
```

`--list` prints standard inventory JSON; `--host` prints one host's three
variables, or `{}` for an unknown alias. No file is written. Diagnostics are
static bounded codes, not input JSON or file content. The reader/CLI requires
only Python's standard library, not the ConsoleCrypt Runner.

## Inventory plugin

No plugin is activated automatically. In a separately selected external Ansible
environment, explicitly enable `consolecrypt.inventory.snapshot`, configure the
collection path, and pass the issued `.consolecrypt.json` file as inventory.
There is no YAML plugin configuration, arbitrary snapshot include, constructed
group/variable composition, persistent cache or live refresh endpoint.

The API target for initial verification is pinned `ansible-core==2.20.11`.
Other versions are not claimed compatible until actually tested. CLI and plugin
call the same strict snapshot reader; the entire document is validated before
the plugin modifies Ansible's in-memory inventory.

Actual pinned Ansible/plugin/CLI proof: 12 tests passed in an owned offline
non-root fixture, including full `ansible-inventory --list` plugin loading.
Ansible's JSON display preserves its unsafe markers around external strings;
the plugin does not mark external values trusted. The standalone CLI emits the
ordinary allowlisted standard JSON. Native issuance now binds the selected project revision, host/group selection and current unlocked profile. The project menu offers explicit selection and a file destination; it issues fresh bytes only after choosing the destination. Actual AppCore-issued bytes were read by this CLI and the pinned Ansible plugin in an owned offline fixture. Native tests cover SQL changes, profile locking and stale revisions; Flutter tests cover issuance and scope changes. Source integration is verified; a new packaged preview and full platform acceptance remain pending.

Primary API reference:
https://docs.ansible.com/projects/ansible/latest/dev_guide/developing_inventory.html
