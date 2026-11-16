# Linux delegated service

This package supplies the real `cc-service-node`, fixed private Unix transport,
and systemd unit for Ubuntu24.04/kernel6.8 with local EXT state. It does
not enroll an owner, authorize a job, install a user Vault, or start a schedule.
Windows/macOS service hosting and network filesystems are not supported here.

Build only as part of the final reviewed Linux release:

    python3 runner/deployment/bundle.py build --output /new/final-service-output

The existing dependency/license audit and public-source exporter must pass.
The output has separate `install/` and `corresponding-source/` trees. Distribute
the matching source tree and LICENSE/notices alongside the binary, not inside
the Ansible project. Rebuild the CLI from corresponding-source with
`cargo build --locked --release --manifest-path client/rust/Cargo.toml -p cc-automation-core --bin cc-service-node`.
The build produces no upload, release, ZIP or host install.
`bundle.json` is bounded versioned artifact data, never an execution approval.

Use `install/` as a separately reviewed automation project. Choose the saved SSH
target explicitly, then review the normal privileged Ansible job. No AI password
or passphrase is needed. These nonsecret parameters are the only choices:

| operation | parameters and behavior |
| --- | --- |
| install | canonical `service_identity_id` and reviewed `service_bundle_sha256`; requires absent instance/package; creates only this fresh identity |
| status | same identity; observes installed version/checksum and manager state only |
| update | new package hash plus `expected_instance_pointer_sha256` captured from status; verifies a new immutable software tree, stops/joins this instance, atomically selects its new version and leaves it stopped |
| start | reviewed package hash and expected instance-pointer hash; explicit existing-only start; can resume only the original still-valid consent |
| remove | expected instance-pointer hash; stops/joins and disables this Node; preserves software, keys, encrypted capsules, scheduler slots, JobStore and the inactive-only raw-retention timer; this is not full uninstall |

Run `service.yml` with those parameters through the existing normal review/job
flow. `status` needs no privilege change. Other operations explicitly use the
operator's already authorized privilege escalation; they do not create root
logins, modify SSH policy, add users/groups, download software or disable host
security. Each package has a separate root-owned readonly tree at
`/opt/consolecrypt/service-bundles/<manifest SHA256>`. Updates never overwrite
an existing tree, including an incomplete one. The atomic root-owned
`/etc/consolecrypt-service/<identity UUID>.json` selects software for only that
instance. Another instance starting during an update keeps its own complete
version. This software reference grants no access and does not change identity,
policy, quotas or slots. Fixed daemon, relay, verifier and systemd bootstrap
assets are installed once: an ordinary update refuses changes to them. Their
future migration requires a separate reviewed maintenance procedure.

Pointer writers hold the same root-owned per-instance flock through nofollow
expected-hash checking and atomic replacement. A competing writer gets busy or
stale denial; locks are never unlinked. This protects the software selection,
not the whole manager stop/start workflow: coordinate one maintenance operator
per instance. Non-cooperating root modifications and automatic rollback are
not supported. Before software changes it verifies Ubuntu24.04/x86-64,
the minimum GLIBC version extracted from the actual built ELF, systemd255 or
newer, and the exact private Python/helper/dependency fingerprints. Public helper
source is copied only to this operation's private temporary preflight directory;
no runtime, host feature, account or existing permission is repaired.

A failed read-only observation exits with code78 and a closed diagnostic
`service_host_or_runtime_unsupported:NN`. It never prints exception text,
private paths, source envelopes or dependency contents. Stages01–04 check
identity, input, privilege flags and the temporary namespace;05–08 check the
host/runtime and source-file metadata;09–10 verify/load the source courier and
run the original probe. Stages11–14 identify helper, dependency-file, lock and
combined-environment fingerprint mismatches;15–16 identify dependency readiness
and nonroot status;17–19 identify filesystem/PID/network sandbox refusal. Stage20
is result serialization;00 is an unclassified stage. A refusal still blocks all
persistent writes; the stage number never grants permission to skip a check.

The same-UID listener/systemd path has passed an installed two-boot fixture.
The versioned loader has separately passed real concurrent Node-start testing;
the full Ansible install/update/remove maintenance role still needs its actual
installed-host gate. The final release build and license/source bundle also
remain unrun here. These checks do not certify every Linux host or filesystem.

Before installation, the operator provisions the dedicated nonroot account and
primary group `consolecrypt-service`, without supplementary privileged groups,
and its mode0700 private `/var/lib/consolecrypt-service` home. The selected SSH
login must have this SAME effective UID; owning the socket pathname alone is
insufficient. Configure its SSH key/trust through the existing reviewed SSH
setup. Existing accounts, ownership, permissions and unknown states are never
silently repaired or adopted. `/opt/cc-runtime/bin/python` must already be the
private supported Python3.12 runtime with the exact locked dependencies and
working filesystem/PID/network task isolation. There is no ambient pip fallback.

After install, choose explicit start, then select the actual Service Runner in
ConsoleCrypt. The original-peer probe and namespace checks must pass before a
separate service review and explicit confirmation. Independent bounded-offline
access expires at its original deadline (at most24h); no restart renews it.
Signed owner revocation is separate from stopping/uninstalling the process.

Updates retain all original state. Changed/unsupported policy, helper, journal,
identity or package formats stop rather than re-enroll, reset quotas, retry an
unknown slot or reconstruct authority. A partial install/update is an uncertain
maintenance outcome: inspect status and private state deliberately. Do not
report it as rolled back, purge it, or automatically rerun installation. There
is no default private-state purge or shared-bundle deletion. Reinstalling the
same keys does not undo an owner revocation.

## Raw artifact lifetime

Every explicitly installed UUID has its own systemd retention timer and bounded
nonroot oneshot service. The hourly/Persistent trigger checks the original
30-day raw-retention threshold even if the Desktop never reopens the service.
It uses the same immutable package, signed original policy, identity/runtime
binding and original locks. It requires positively inactive/dead service and
absent canonical cgroup. It cannot attach, stop or start a Node, mint a job,
resolve a credential or renew a policy. Active, failed, ambiguous, not-yet-due
or unsupported state refuses; that refusal is not successful raw retirement.

On an available supported host, eligible raw is handled by the next hourly
check. Persistent timers catch up after boot; no deletion while powered off,
physical secure erasure, swap or external backup lifetime is promised. Raw
output/input/template/revision retirement preserves original structured unknown
results, nonces, replay/custody and admission audit. A pinned Native summary
does not extend raw lifetime.

The current remove operation disables the Node but preserves its private state,
software and this raw-retention timer. It is deliberately not full uninstall.
Removing the software/timer or opting out of future raw maintenance is a
separate explicit operation with a retained-data policy; history is not silently
deleted to produce a clean uninstall status. The manual fixed --retire-raw
operation remains an explicit operator action, with its existing ended-policy
and actual quiescence guards. The automatic --retire-raw-inactive path accepts
only the installed UUID, never supplied approval/PID/path/join JSON.
