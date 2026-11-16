# Owned-prefix monitoring configuration snapshots

`monitoring_snapshot.py` backs up configuration only. It starts/reloads/stops no
process or service, provisions no account/mount/disk and does not read, copy,
delete or recreate metric data, TSDB blocks or WAL. It uses the existing
MonitoringPlan model and pinned promtool validation.

The fixed mapping contains Prometheus config, rules, command JSON, reviewed plan
JSON and `.monitoring-managed.json`. Named snapshots are under the initialized
prefix's `config-snapshots/`. Names are bounded lowercase ASCII; four snapshots
are allowed. Each file is at most 64 KiB, the ledger 16 KiB and each payload at
most 128 KiB including its manifest. The small store marker and filesystem/
atomic-write overhead are separate; this is not a hard physical disk cap.
Snapshot names bind immutable
content; repeated identical snapshot creation is unchanged. Existing corrupt or
different content is preserved and rejected.

The store and manifest bind the canonical prefix and its marker digest. Every
fixed payload has a digest, and extra/missing mapping entries are rejected.
These are local integrity records, not signatures or update permissions.
Snapshot files are written mode 0600 in a private directory.

```python
snapshot(prefix, "before-change")
restore(prefix, "before-change", reviewed_plan.variables(),
        prometheus_path, reviewed_prometheus_sha256,
        promtool_path, reviewed_promtool_sha256, port)
```

Restore requires the explicit reviewed plan, exact binary path/digest, port and
current packaged rules. Its rendered bytes must equal the selected snapshot.
It cannot silently enable a receiver from old content. Both rules and a temporary
path-adjusted config are checked by the pinned promtool before live writes.
Existing managed bytes must match the existing fixed-name ledger. Foreign,
unowned, symlinked/reparse-linked, hardlinked, corrupt, oversized, changed or
missing inputs deny by default.

For a **missing managed configuration file only**, the explicit
`recover_missing=True` / `--recover-missing` option permits restoration when the
valid marker and existing canonical ledger still prove that fixed owned name.
Missing marker/ledger, altered existing bytes or an unknown filename remain
denied. The restored plan still must be explicitly reviewed and validated.
Recovery never regenerates data or an absent metric-data directory.

The CLI has `snapshot` and `restore` operations with `--prefix` and `--name`.
Restore additionally requires `--prometheus-binary`, `--prometheus-sha256`,
`--promtool`, `--promtool-sha256` and `--port`; the reviewed MonitoringPlan variables
JSON is read from stdin. The snapshot itself is never treated as approval.

The native fixture applied A with receiver off, B with receiver on, then restored
A and repeated restoration unchanged. Real Prometheus 3.16.0 loaded both rules
and became ready at every stage; receiver responses were 404/400/404. The
original query-visible sample and exact sample timestamp survived all restarts,
and an owned data sentinel remained unchanged. Explicit missing-rule recovery,
modified-file refusal and symlink refusal were also exercised. See
`evidence/monitoring-restore/MONITORING_CONFIG_RESTORE_REPORT.json`.
Eight guard tests passed on Windows and native Ubuntu, including
hardlink refusal without changing the outside sentinel and the manifest-inclusive
snapshot budget. Injected second-live-write failures restore the prevalidated
bytes, canonical ledger and original missing-file absence, after which retry
works. A failure after ledger replacement restores the old ledger last.
Rollback first validates that every managed file still contains either the
prevalidated old bytes or intended target bytes; it never overwrites newly
introduced foreign content. Failed rollback raises `RestoreStateUnknown` with
`state='unknown'` and reports no success. Content-addressed prechange backups
may remain within their separate existing budget. Their native result is
`evidence/monitoring-restore/SNAPSHOT_GUARDS_REPORT.json`.
Independent post-fix review passed three additional mocked fault probes,
including an originally missing command file, with no further actionable finding
in this documented scope. Its source-bound receipt is
`../collector/evidence/monitoring-snapshot-review/REVIEW_REPORT.json`.
That receipt covers the earlier standalone restore helper. The subsequent shared
fixed-file transaction also protects direct initial apply/update, while restore
delegates without a nested rollback. Its 15 Windows/native guard tests and exact
source hashes are in `TRANSACTION_GUARDS_NATIVE_REPORT.json` in the same evidence
directory; shared-helper review is tracked separately in the checkpoint.
An additional targeted Windows/native guard covers a read failure during the
snapshot's final post-commit check: it reports unknown, retains the committed
target and does not invoke a second rollback. Its receipt is
`POST_COMMIT_GUARD_NATIVE_REPORT.json` in that evidence directory.

This is a single-writer prefix configuration workflow. No common lock coordinates
apply/snapshot/restore; the caller must serialize them. Ordinary caught write or
verification exceptions inside the shared commit attempt bounded in-process
rollback. A later post-commit read failure reports unknown without nested
rollback. Per-file atomic writes
do not make a multi-file crash-atomic transaction. It does not certify full-node
or off-host backup, encrypted
backup transport, cross-version rules/Prometheus migration, disk recovery or
retention endurance. Metric backup/recovery requires a separate maintained
Prometheus workflow and review; none is implemented here.
