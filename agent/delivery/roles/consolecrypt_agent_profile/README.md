# ConsoleCrypt public profile role

This role uses the same `integration.profile_manifest.ProfileManifest` model as
the client-facing prototype. It validates a base/custom profile and exposes a
review plan plus separate runtime/collector configuration. It does not install
an Agent, start a collector, modify Prometheus, enroll identities or manage any
exporter process/package. Signed Agent installation is a separate explicit step.

The controller needs Python 3.11+ and the Agent source/distribution containing
this role, `delivery/render_profile.py`, `integration`, `runtime` and `collector`.
There is no Galaxy or package download. Set `ANSIBLE_ROLES_PATH` to this `roles`
directory and pass a public `cc_agent_profile_manifest` dictionary:

```yaml
- hosts: monitored_nodes
  gather_facts: false
  roles:
    - role: consolecrypt_agent_profile
      cc_agent_profile_manifest: '{{ reviewed_profile }}'
      cc_agent_profile_write: true
      cc_agent_profile_output_root: /tmp/my-reviewed-agent-config
```

By default the role only returns `cc_agent_profile_artifacts` as a host fact.
With explicit writing, artifacts are created on the controller under a directory
named by the canonical profile digest. A second identical run is idempotent;
a changed profile gets a new directory, so connection-only mode cannot reuse
an old collector config at the same artifact path. Existing artifact directories
are retained for separate review/cleanup; no exporter data/service is removed.

`linux_basic` selects one explicitly named node_exporter endpoint. Extra
application/cAdvisor targets are custom entries. `ownership: external` means
attach/read only: no duplicate installation, update, restart or deletion, even
if an identity later receives broader scopes. Existing exporters must not have
an installation artifact. Agent-managed exporters merely produce a blocked
review action until a separate lifecycle implementation and permit exist.

`existing_prometheus` requires its backend, `base_profile: none` and no enabled
exporters. It produces no runtime or collector file and starts no new scrape.
This is connection configuration; compatibility still needs a capability probe.
Remote-write plans remain blocked by the independent strict-buffer acceptance
gate; rendering a configuration does not authorize its execution.

Each actual installation must generate its own identity. Public IDs/fingerprints
here do not register or attest a machine. Multi-host plays must supply
`cc_agent_profile_fleet`, a complete list of reviewed profiles, to check duplicate
Host IDs, Agent IDs and fingerprints. The renderer also requires the current
profile to be present in that fleet. Single-host validation defaults to a fleet
containing that one profile. This does not prove SSH target-to-identity binding.

No vault credentials, general vault key or scope grant belongs in this manifest.
The role delegates only validation and public artifact writes to localhost,
with `become: false`; it contains no service/package/shell tasks or host changes.
