# Public candidate-only target role

Disabled by default. Requires explicit reviewed bootstrap/bundle SHA pins, target
Python3.11+, pre-existing physical owned0700 config root and separate transport
parent. Protected code/node/provider/anchor/history/audit paths are explicit.
Controller fleet validation precedes actual target temporary-copy/stat/command
tasks. Target Python runs isolated with site disabled and imports only the pinned
extracted validation closure. No package install or dependency discovery occurs.

`cc_config_candidate_profile`, nullable `cc_config_candidate_agent_id`, and
`cc_config_candidate_host_id` are configured public bindings, not attestation or
authority. `cc_config_candidate_initialize=true` performs initialization before
staging; without it missing marker denies. Check mode on an explicitly initialized
empty root returns would-init/create with null instance/path/candidate digest.

Result `cc_config_candidate_result` contains changed, would_init, would_create,
root_instance, candidate_digest, candidate_path, profile_digest, runtime_digest.
Use candidate_path/profile.json only in a separately authorized process launch.
The role never writes a live selector, changes a running process, provisions a
key, grants scope, touches external exporters, changes code version or enables
remote-write. Public model/bundle/manifest metadata are not independent trust.

An existing exact candidate is unchanged. Foreign/linked/modified candidates and
unknown inventory deny. Published-candidate/ledger failure is unknown/quarantine;
the helper never deletes a published candidate for automatic repair. Single-
writer serialization is external. No multi-file crash-atomicity claim.

Transport is temporary housekeeping, excluded from changed counters. Cleanup
requires the same measured directory identity and known immediate transport
names; a foreign replacement/extra object refuses recursive removal. This is a
controlled operator workflow, not protection against a privileged concurrent
attacker changing paths after checks.

## Fixed operator failure outcome (new reporting source, awaiting acceptance)

Success preserves the original eight result keys. A separately pinned bootstrap
now emits compact sorted ensure_ascii JSON plus one LF, after extraction cleanup.
Producer rc2 means fixed rejected/changed=false; rc3 means fixed unknown/changed=null.
Error reason codes are closed; stderr, exceptions, profile bodies and URLs are not
operator diagnostics. Existing malformed metadata and post-mutation errors are
unknown, not inferred healthy/unchanged from exception class.
Known rejection uses explicit exception type/code tables; future codes and generic
ValueError/TypeError/KeyError remain unknown. Invocation facts/ownership registers
are cleared before option checks, including invalid opt-in on a repeated role use.
Disabled false performs no target staging. Raw failure events stay no_log until
the final safe object is shown; cleanup never borrows an earlier invocation's pin.

The role rechecks the independent bootstrap SHA before isolated result decoding,
preserves LF explicitly, and exposes `cc_config_candidate_failure` before its final
safe failure after cleanup. Unknown uses conservative Ansible changed=true for
possible mutation; the fact's changed=null is authoritative and does not confirm
application. Cleanup uncertainty never triggers repair or lowers unknown to reject.
The decoder uses no model imports or candidate writes. This source delta requires
its own bootstrap pin/validation bundle and does not rewrite accepted v2 evidence.

# Reviewed cleanup boundary

The role snapshots regular single-link mode0600 transport files after successful
copy/input tasks and compares device/inode, owner and SHA before deleting the
temporary directory. An incomplete snapshot, extra entry, changed bytes, links
or replacement preserves that directory as unknown; cleanup is not retried by
adopting new objects. External single-writer serialization remains required.
