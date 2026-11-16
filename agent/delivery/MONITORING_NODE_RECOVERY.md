# Installed monitoring configuration recovery

The existing `monitoring-node.yml` defaults to the explicit `install` action.
It retains the original Ubuntu 24.04 x86_64, capacity, dedicated-volume,
independent trust-boundary, exact Prometheus 3.16.0 archive and loopback guards.
The role does not partition, format, mount or expose a public listening port.

After installing or updating configuration, it completes the real restart and
observes `http://127.0.0.1:9090/-/ready` before saving the fixed configuration
backup and integrity manifest on the already provisioned separate volume.
No failed-to-load configuration is reported as a verified backup.

Recovery is an explicit separate invocation with the original reviewed plan:

```text
ansible-playbook -i inventory monitoring-node.yml -e @plan.json \
  -e cc_monitoring_operation=restore -e cc_monitoring_restore_approved=true
```

The backup is not approval. Before replacing any live configuration, the fixed
preflight checks all three bounded files and the manifest, requires the exact
current reviewed config, packaged rules and rendered service unit, and runs the
installed root-owned promtool against the backup config and backup rules. A
missing/corrupt manifest, changed payload, symlink/hardlink, different receiver
intent or invalid rules refuses recovery. No old snapshot can silently enable
remote_write. Explicit receiver selection remains in the reviewed plan/unit.

The restore mapping contains only `prometheus.yml`, `rules.yml` and the
Prometheus systemd unit. It then performs an actual restart and readiness check.
It does not install binaries/accounts, download anything, initialize a missing
metric data directory, back up/restore TSDB blocks or WAL, delete metrics, or
certify disk/off-host recovery. An absent original metric data directory is
refused. The helper itself only reads configuration; it never controls a service.

The operator must serialize install/restore and other root configuration edits.
Ansible's per-file atomic replacement is not a multi-file crash transaction. A
write/restart/readiness failure remains an explicit failed operation, possibly
with partial configuration changes; it does not report successful recovery or
invent metric rollback. The existing config backup is retained for review.
The backup contains configuration secrets if the operator deliberately places
them in Prometheus config. Its directory is root0700 and files root0600; it does
not inherit vault E2EE.

`alertmanager_enabled` retains its documented meaning: use an **already installed
loopback Alertmanager**, not install one. The recipe now checks its real readiness
before mutation when selected. Automatic Alertmanager installation and full
installed-node VM acceptance remain separate open scope; the staged component
tests are not an AGT-08 platform acceptance claim.
