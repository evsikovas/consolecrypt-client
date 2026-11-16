# Install and operate a separately provisioned Agent

The maintained Ansible role delivers signed software to a Linux host selected
by your inventory. It does not create a Node identity, enroll a machine or grant
telemetry/collector/tunnel permission. Installed-service acceptance is still
pending; these are operator instructions for the reviewed source, not a claim
that a production release or container image has been published.

Run the examples from a Linux Ansible controller with Python3.11+ and the
maintained Ansible2.20.11 environment. The target's service runtime and reviewed
release must match their own fixed prerequisites; the controller's Python is
not evidence about the target.

The installed volatile collector contract currently targets Ubuntu24.04 amd64,
systemd255 and cgroupv2 with the explicitly reviewed namespace/delegation policy.
ARM64 currently has a separate native compile/test artifact, not a full signed
installed collector package. The Prometheus-overlay hard-mode implementation
is a separate contract; upstream vmagent installation does not prove that mode.

## Native systemd installation

Before running the role, the administrator supplies the following existing
inputs. The role refuses missing/foreign input rather than repairing it:

- A dedicated nonroot `consolecrypt-agent` account and primary group, with
  disabled login and a physical private state parent owned by that account.
  Volatile collector opt-in additionally requires home
  `/var/lib/consolecrypt-agent` and shell `/usr/sbin/nologin` exactly.
- The independently authenticated release verifier, public release-key pin and
  `trust.json` at the fixed paths in [RELEASE_SIGNING.md](RELEASE_SIGNING.md).
  A key shipped beside a candidate is not its own trust bootstrap.
- The actual signed ZIP and its matching signed release manifest on the Ansible
  controller. The role copies these public artifacts to the target; it does not
  download a release, invent a version or accept unsigned packages.
- Reviewed `/etc/consolecrypt-agent/manifest.json` and `identity-config.json`,
  with all referenced original identity/custody/checkpoint/owner-pin inputs
  available to the service UID. Configuration files are not authorization.
  [The profile role](roles/consolecrypt_agent_profile/README.md) renders public
  configuration; [the identity role](roles/consolecrypt_agent_identity/README.md)
  generates a unique node-local key only when explicitly requested. Complete
  the separate claim/owner-approval flow before expecting admitted requests.

Use the same account and original state on restart. Do not distribute a common
private key, initialize an existing empty identity directory as a workaround,
or change ownership of unknown existing paths to make a check pass.

Create an operator-owned `agent-software.yml` on the controller:

```yaml
---
- name: Explicit signed Agent software delivery
  hosts: consolecrypt_agent_nodes
  gather_facts: false
  become: true
  roles:
    - consolecrypt_agent_install
```

Put the following in `agent-delivery-vars.json`, replacing the two absolute
controller paths with your already verified release inputs:

```json
{
  "cc_agent_install_operation": "install",
  "cc_agent_install_approved": true,
  "cc_agent_install_start": false,
  "cc_agent_install_metrics_runtime": false,
  "cc_agent_release_artifact": "/srv/reviewed-agent/candidate.zip",
  "cc_agent_release_manifest": "/srv/reviewed-agent/release-manifest.json"
}
```

From the existing client repository, select only the intended hosts in your
reviewed SSH inventory, then run:

```sh
ANSIBLE_ROLES_PATH="$PWD/agent/delivery/roles" \
  ansible-playbook -i inventory.yml agent-software.yml \
  --extra-vars @agent-delivery-vars.json
```

`install` is for an absent software selection. With `start: false`, this selects
verified software and loads the fixed unit without starting it. After the
separate configuration/enrollment is ready, the administrator may explicitly
enable/start that same unit using `sudo systemctl enable --now
consolecrypt-agent.service`. Alternatively, choose `cc_agent_install_start:
true` on the original reviewed installation. Do not repeat `install` as a
generic repair/restart operation on an existing selection.

The maintained paths are:

| Purpose | Path |
| --- | --- |
| Root-controlled software/staging prefix | `/opt/consolecrypt-agent-install` |
| Stable launcher | `/opt/consolecrypt-agent-install/opt/consolecrypt-agent/launch.py` |
| Fixed systemd unit | `/etc/systemd/system/consolecrypt-agent.service` |
| Public runtime configuration | `/etc/consolecrypt-agent` |
| Original service-owned identity/history/checkpoints | `/var/lib/consolecrypt-agent` |
| Independent release high-water record | `/opt/consolecrypt-agent-install/var/lib/consolecrypt-agent/release-anchor.json` |

Do not copy the old prototype launcher to `/opt/consolecrypt-agent` or install
an unrelated unit over an existing one. Foreign units, drop-ins and modified
software require an explicit migration, not a force overwrite.

## Update, known rollback, status and software removal

For `update`, put the new matching signed ZIP/manifest paths in the vars file,
change the operation to `update`, and run the same playbook. Choose `start: true`
only when you intend to run the selected release. The role verifies before
stopping the original service and requires its actual joined process/cgroup.

For `rollback`, select operation `rollback` and the intended `start` boolean.
This selects only the recorded verified previous release. It does not accept
an arbitrary old artifact or lower the independent floor for future updates.
The backup covers software selection, not live identity/database snapshots.

For a read-only selection observation, run the same playbook with this final
override; no mutation approval is needed:

```sh
ANSIBLE_ROLES_PATH="$PWD/agent/delivery/roles" \
  ansible-playbook -i inventory.yml agent-software.yml \
  --extra-vars @agent-delivery-vars.json \
  --extra-vars '{"cc_agent_install_operation":"status","cc_agent_install_approved":false}'
```

`status` describes selected software, not authenticated running version. To
inspect the real manager separately, use `systemctl show
consolecrypt-agent.service -p ActiveState -p SubState -p MainPID` and
`journalctl -u consolecrypt-agent.service`. An active PID is not a Node grant or
a trusted runtime heartbeat; `actualRuntimeVersion` remains null in delivery
output.

For `remove`, set operation `remove`, `approved: true` and `start: false`. The
role joins/disables the service and removes only unchanged recorded software.
It retains configuration, node keys, history, checkpoints, raw data and the
release floor. Modified files are retained and reported as partial removal.
There is no data-purge switch in this operation. Do not follow it with a broad
recursive deletion or recreate the identity to recover space.

An UNKNOWN/partial transaction is not permission to retry or roll back blindly.
Preserve its output and inspect the original unit and unchanged selected files;
an interrupted stage may already have advanced the release floor.

## Explicit volatile collector opt-in

Leave `cc_agent_install_metrics_runtime: false` for ordinary software delivery.
To request its separately reviewed infrastructure, use approved `install` or
`update`, `cc_agent_install_start: true`,
`cc_agent_install_metrics_runtime: true`, and the actual existing
nonzero numeric `cc_agent_install_service_uid`/`cc_agent_install_service_gid`.
The account, catalog and pinned Python/receiver policy must already match.
The role provisions only its fixed user-manager/tmpfs subset; it does not mint
collector permission or enroll the Node. See [the role contract](roles/consolecrypt_agent_install/README.md)
for exact prerequisites and retained infrastructure on removal.

Native egress remains loopback-only by default. The installed policy accepts
only explicitly reviewed numeric Relay/resolver addresses in the fixed
`/etc/consolecrypt-agent/egress.json`; it does not grant Node/Relay authority.
The [noninstalled TUN example](../OPERATOR_TUN.md) is not an override to paste
over this installed unit. Collector infrastructure, TUN and remote access are
separate explicit configurations.

## Alternative: isolated unprivileged container

No production image is supplied here. Use only an operator-reviewed,
signature/provenance-verified image with an immutable digest. It must contain
the required Linux/Python runtime and the exact executables referenced by your
existing identity configuration. [container_preflight.py](container_preflight.py)
checks only digest-reference syntax; it does not pull or authenticate an image.
The current full software prefix is amd64-only here too; a container does not
turn the ARM64 compile artifact into an installed collector package.

Prepare these three physical host directories separately; do not mount a live
native service's state while that service or another container owns it:

| Host input | Container path | Ownership and access |
| --- | --- | --- |
| Verified complete installed prefix, including selection/floor | `/opt/consolecrypt-agent-install` | Root-owned readable0755 code/parents, public files0644/executables0755, no group/world write; bind read-only |
| Reviewed configuration | `/etc/consolecrypt-agent` | Root-owned, readable by the selected service GID, no group/world write; directory0750/files0640; bind read-only |
| Original private Agent state | `/var/lib/consolecrypt-agent` | Exact selected nonzero service UID/GID, private0700 directories/0600 private files on supported storage; bind read-write |

All absolute paths in the existing configuration must retain their meaning
inside the container. Never copy a private key into an image layer. Select the
actual provisioned UID/GID rather than using root or assuming another machine's
numeric IDs. For example, after setting `AGENT_IMAGE` to your verified
`registry/repository@sha256:...` reference and the three absolute host paths:

```sh
AGENT_UID="$(id -u consolecrypt-agent)"
AGENT_GID="$(id -g consolecrypt-agent)"
test "$AGENT_UID" -gt 0 && test "$AGENT_GID" -gt 0
python3 -I -B agent/delivery/container_preflight.py --image "$AGENT_IMAGE"
docker run --name consolecrypt-agent --pull=never \
  --user "$AGENT_UID:$AGENT_GID" --read-only --network none \
  --cap-drop ALL --security-opt no-new-privileges=true \
  --memory 256m --memory-swap 256m --cpus .25 --pids-limit 16 \
  --tmpfs /tmp:rw,noexec,nosuid,nodev,size=32m,mode=1777 \
  --mount "type=bind,source=$AGENT_SOFTWARE,target=/opt/consolecrypt-agent-install,readonly" \
  --mount "type=bind,source=$AGENT_CONFIG,target=/etc/consolecrypt-agent,readonly" \
  --mount "type=bind,source=$AGENT_STATE,target=/var/lib/consolecrypt-agent" \
  --entrypoint /usr/bin/python3 "$AGENT_IMAGE" -I -B \
  /opt/consolecrypt-agent-install/opt/consolecrypt-agent/launch.py \
  --manifest /etc/consolecrypt-agent/manifest.json \
  --identity-config /etc/consolecrypt-agent/identity-config.json \
  --state-dir /var/lib/consolecrypt-agent/history
```

This base template runs only the loopback backend in its own network namespace;
it publishes no host port and permits no network egress. A normal Docker port
mapping does not expose a listener bound to container127.0.0.1. Remote transport
needs its own reviewed deployment; do not switch to host networking or change
the manifest to an unsupported public bind as a shortcut. Requests still need
the original signed Node admission even from the same namespace.

The container receives no host root filesystem, host Docker/containerd socket,
privileged mode, host devices or user-manager bus. Its metrics describe its own
view; host exporters require explicit endpoints/configuration. The native
volatile collector's delegated systemd user-manager contract is not provided by
this base container. Do not add privileges, a Docker socket, host mounts or
arbitrary capabilities to pretend it is enabled.

Stop/join the original container before replacing its verified image/software;
restart with the same original configuration and state. `docker rm` removes the
stopped container, not the three bind-mounted directories. It is not Agent
data erasure or release-floor reset. A second independent Agent needs its own
identity/state, not a concurrent writer to this one's directory.
