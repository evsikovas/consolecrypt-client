# Owned monitoring-prefix prototype

Disabled by default. The role applies the same MonitoringPlan as
`monitoring-node.yml` to an explicitly initialized empty owned prefix. It needs
already available Prometheus/promtool binaries and SHA-256 pins. It downloads
nothing, creates no account/service/mount/partition, starts no process and uses
`become: false`. Only owned paths and the unprivileged loopback port are adapted.

Pinned promtool checks config/rules before replacement. Unknown, modified or
linked paths are refused. Changed configuration gets bounded content-addressed
backups; metric storage is not removed. A second unchanged apply reports zero
changes. Check mode validates temporary drafts without persisting runtime files.
The generated command is a reviewable argv array, not proof of a running service.
Physical volume sizing, retention duration and systemd remain separate gates.

Actual owned Ubuntu checks applied receiver-off/on twice each, second-run
changed=0. Real Prometheus 3.16.0 loaded two supplied rules. Reception-off gave
HTTP 404; explicit reception-on ingested a real vmagent payload visible to query.
The collector used its explicitly test-only memory queue, so strict Agent buffer
acceptance is not claimed. Backups and temporary process cleanup were checked.
Evidence is `delivery/evidence/monitoring-prefix/MONITORING_PREFIX_REPORT.json`.

Explicit named configuration-only snapshot/restore is available separately via
[`monitoring_snapshot.py`](../../MONITORING_CONFIG_BACKUP.md). Restoration still
requires the reviewed MonitoringPlan and pinned binaries; receiver state is
never enabled from backup implicitly. Missing managed files require explicit
recovery with the existing valid marker/ledger. The role does not automatically
restore a snapshot, restart a service or back up TSDB/WAL. That separate native
workflow retained a query-visible sample/timestamp and data sentinel across
A/off → B/on → restore A/off. Ordinary-error rollback guards have separate
targeted evidence; single-writer and multi-file crash limitations still apply.

Direct initial apply/update now use the shared fixed-file transaction as well.
An existing ledger with a missing config denies by default; only explicit
snapshot recovery enables the internal missing-file option. Ordinary errors
restore previous bytes/absence or report unknown if rollback fails. Optional
data-directory creation occurs before live config writes; an empty directory
created before a later error remains, and existing metric data is never removed.
See `delivery/evidence/monitoring-restore/TRANSACTION_GUARDS_NATIVE_REPORT.json`.
