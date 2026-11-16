# Explicit signed Linux Agent software delivery

This role is administrator-operated Ansible delivery. It adds no Native RPC,
generic executor, signing key, enrollment, vault access or permission from JSON.
The existing profile role renders reviewed public config and the identity role
explicitly provisions a unique node-local key. Those remain separate ceremonies.

The static `consolecrypt-agent` account/group must already exist, with nonzero
UID/GID and disabled login. Provisioning an account or changing ownership of
unknown paths is not implicit. The profile/runtime and identity config must be
separately installed at their existing fixed paths before `start` can work.
No common vault key, controller private identity, seed or automatic enrollment is
distributed. Missing configuration causes real service activation to fail rather
than creating an identity or fabricating enrollment.

Before installation, the administrator must provision the existing Rust release
verifier at `/usr/local/libexec/consolecrypt-agent-release/cc-agent-identity`, its
public signer pin at `/etc/consolecrypt-agent-release/public-key.json` and a
root-owned `trust.json` in the same directory:

```json
{"version":1,"verifierSha256":"<64 lowercase hex>","publicKeySha256":"<64 lowercase hex>","platform":"linux-x86_64"}
```

There is no production release key in this implementation and no unsigned
fallback. An absent/changed verifier or trusted key refuses the operation. The
caller provides only the already signed artifact and signed release manifest;
the existing `ReleaseVerifier` delegates real Ed25519/digest/platform/protocol/
expiry/high-water checks to the existing Rust policy. The required complete
package file set comes from `prototype_package.SOURCES`; the pinned WSS wheel
comes from `package_wheel.py`. No pip, ambient package installation or import of
unverified candidate code occurs in the installer. A signed incomplete package
is refused before stopping the original service.

Explicit operations are `install`, `update`, `rollback`, `remove` and `status`.
Mutating actions require `cc_agent_install_approved: true`. Set
`cc_agent_install_start: true` only after original configuration/enrollment are
ready; it explicitly enables and starts the fixed systemd unit. Default is
software selection only, not a running-service or enrollment claim.

The fixed root-controlled staging prefix is `/opt/consolecrypt-agent-install`.
The real stable launcher runs from its `opt/consolecrypt-agent/launch.py`; its
existing release-floor file remains under the root-controlled prefix, separate
from the Agent-owned `/var/lib/consolecrypt-agent` identity/history/checkpoints.
No symlink alias or modification of signed package bytes changes that layout.
Legacy, unknown, linked or modified service units/parents are refused; their
migration is a separate reviewed action, not an overwrite/permission repair.

One root-owned flock serializes these installer invocations. The verifier is run
before stopping the original service. Live replacement requires actual original
PID-fd exit and kernel cgroup-empty/removal observation. Known inactive units
also require no live cgroup. Explicit rollback uses the existing verified prior
release records and preserves the high-water floor. If staging fails part way,
the floor may already have advanced: the operation reports UNKNOWN and remains
stopped, without blind rollback or retry. After a completed update stage, a real
start failure can stop/join that new process and select/start only the known
previous release. A rollback-start failure is UNKNOWN as well.

The software selection backup is public version/artifact metadata plus existing
immutable prior release files; it never copies live identity DBs/private keys.
At most eight retained release directories are admitted (each already bounded
by the staging package limits). A full/unknown software area refuses a new
release before process stop instead of deleting unknown files. The operator can
explicitly remove recorded software and reinstall a current signed release;
the independent replay floor remains.

`remove` joins and disables the service and calls the existing unchanged-only
software uninstall. Modified files are preserved and reported as partial removal.
Configuration, node keys, history, checkpoints, raw data and the release floor
remain. This is not data erasure. Purging identity/data would require separate
explicit consent and a reviewed authority/recovery policy; no purge capability
or hidden recursive deletion is provided here.

Outputs separate selected/verified package metadata from process observations.
`actualRuntimeVersion` is always null here: systemd active/PID and a selected
manifest are not a fresh authenticated runtime heartbeat. The existing
`update_status.py` genuine observation model is not bypassed. A real runtime-
version confirmation can be added only through that existing trusted observation
path, not by copying the desired package version into status.

The current tests use the real maintained Rust verifier and existing test-only
release signer on disposable directories. Their dummy ELF is inert and their
supervisor is explicitly synthetic. They prove delivery/refusal/rollback data
flow, not actual Linux ownership, systemd activation, worker join, production
release distribution or AGT-01/AGT-10 platform acceptance. Current Linux x86_64
installed-service acceptance remains pending; arm64 is not certified by these
synthetic tests. The original loopback-only/no-capability service boundary is
retained; protected remote transport is a separate explicit deployment choice.


## Explicit volatile-collector infrastructure subset

Default `cc_agent_install_metrics_runtime: false` preserves ordinary `start` and
`stage-only`. The new opt-in is permitted only for approved install/update with
`cc_agent_install_start: true` and explicit matching nonzero service UID/GID.
The existing account must be dedicated `consolecrypt-agent`, home exactly
`/var/lib/consolecrypt-agent`, shell `/usr/sbin/nologin`; no account, password,
Node key, owner pin, grant or enrollment is generated by this role.

The role publishes an absent exact root0600 `collector-runtime.json` with only
format/uid/gid and selects the closed `start-collector` transaction. Existing
foreign/changed profiles are refused, not repaired. Merely finding this config
never activates infrastructure: only the explicit mode, after signed package
verification, original service join and selected immutable release, calls the
fixed root `collector_provision.py --provision` before backend start. Normal
start/stage-only do not invoke it. Presence of a profile is not consent, Permit,
Node enrollment or collector-running evidence. The unit remains unchanged by
this role; the installer publishes exactly one reviewed runtime drop-in.

Prerequisites are Ubuntu24.04 x86_64/systemd255, existing unprivileged user and
mount namespace support including the distribution's AppArmor policy, cgroupv2
cpu/memory/pids delegation, and an actual administrator-reviewed root catalog.
The catalog contains the explicit `/usr/bin/python3.12` SHA; no measured binary
is silently accepted as new trust, downloaded, followed through a symlink or
substituted. The immutable signed selected package supplies the exact three
runtime scripts plus maintained vmagent at the installed prefix. The actual
metrics catalog must use their updated hashes, full canonical manifest SHA and
derived collector ResourceId. Root/owner approval and nonredirecting receiver
review remain separate; provisioning never creates or changes that catalog.

The helper owns only a dedicated `user@<NSSuid>.service` drop-in, root-only
per-user-manager config and a fixed32MiB volatile tmpfs mount (noswap,nodev,
nosuid,noexec,nr_inodes4096, service0700). It resets that manager's PAM and
EnvironmentFile/PassEnvironment inputs, fixes the reviewed nine-variable
baseline and disables environment generators using the exact `/dev/null`
setting. Per-manager `DefaultEnvironment=PATH=/usr/bin:/bin` accounts for the
v255 default user-PATH replacement. Optional DBUS address must name only the
sameUID bus. Unknown environment/config/state denies; no global configuration,
ambient-user variable unsetting, sysctl weakening, Polkit grant, capability,
linger change or unknown-file overwrite is performed. The manager is bounded
640MiB/no swap/CPU100%/Tasks96 and the individual collector retains its original
512MiB/CPU100%/Tasks64 caps. The manager's system-service egress policy is
loopback-only; Relay IPs are exclusively the backend's policy. Actual BPF
attachment/egress enforcement still needs a kernel gate, not a property claim.

The single installer runtime drop-in requires the exact tmpfs and user manager,
preserves the RuntimeDirectory parent, and uses `ProtectHome=tmpfs` plus a
read-only bind of `/run/user/<same NSSuid>` so the backend can use its own bus
without exposing other home directories. The helper checks actual sameUID
runtime directory/socket, delegation/mount/kernel-visible prerequisites and
runs one fixed no-payload namespace probe before reporting factual readback.
The probe creates no collector, authority or connection. Unsupported namespace
policy denies; there is no fallback to a root collector or sandbox weakening.
All of this source remains unexecuted until a separate actual installation gate.

Partial provisioning remains an explicit unknown/manual-inspection condition.
No failure deletes a pre-existing file, unmounts storage, stops a guessed unit,
purges data or retries automatically. The root ownership record only binds this
local managed configuration and is never portable authorization. Software remove
continues to retain identity/configuration/ownership data; deprovisioning this
infrastructure is a separate reviewed administration action, not an implicit
side effect of status/remove or a user request. Existing package/service facts
must never be labeled as authenticated collector heartbeats.

The supported Ubuntu vendor closure also includes only its existing fixed
`user@.service.d/10-login-barrier.conf` and `timeout.conf` under
`/usr/lib/systemd/system`. Their root regular bytes/hashes are retained with the
vendor unit in the same ownership descriptor. Effective DropInPaths must be the
exact duplicate-free set of those present vendor files plus our one perUID60
file; initial cold state has only that vendor set. Foreign vendor/admin extras,
changed/missing/new vendor bytes or unowned equal assets refuse. No vendor file
is deleted or rewritten, and no order-sensitive path comparison is invented.

The separate explicit `cc_agent_install_metrics_buffer_mode: persistentNativeWal`
selects only the fixed maintained Prometheus profile (512 MiB charged root,
7200-second data residence policy). The default `volatile` profile is unchanged.
Both modes still require a fresh owner-approved MetricsCollect session lasting
30–300 seconds; disk bytes, a layout marker or an earlier session never authorize
replay. There is no automatic recovery or lease renewal. A later explicit grant
is required to reopen a retained WAL under the identical owner/node/certificate
and complete manifest binding. Profile changes and unknown pre-existing roots
are refused; no format, mount, adoption or deletion of existing persistent data
is performed. Software removal retains these data and provisioning records.

Disk installation requires an explicitly supplied normal `cmd/prometheus` ELF,
its completed build receipt and full package notices. A Go test executable is
not an accepted substitute. Existing signed-package size/count bounds still
apply; source wiring alone does not establish artifact availability or readiness.
