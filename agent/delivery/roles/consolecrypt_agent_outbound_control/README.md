# Explicit finite outbound SSH carrier

This role is OFF by default. It prepares one fixed system service for an already
installed, signed Agent release containing the three outbound runtime files.
It does not install sshd, create a Node, enroll an owner, issue a grant, change
sudo policy, mint a runtime fingerprint or open a target supplied by a Relay.

The first supported subset is Ubuntu 24.04/systemd 255 with the root-installed
regular `/usr/bin/python3.12` from the reviewed OS supply. This is an OS runtime
prerequisite, not an independently pinned Python distribution. The transport
worker independently requires the selected release's explicit root-system interpreter pin. On Ubuntu 24.04 this must identify the actual `/usr/bin/python3.12` build; the closed transport also accepts `/usr/bin/python3.11` for the separately qualified Debian 12 subset. It does not discover or fall back to another interpreter.

Before unit publication the fixed `enrollment-role-check --tunnel-runtime-check` verifies the complete selected installed release and its packaged `python/supervisor-runtime.json`, then checks the actual physical root-owned 0755 Host interpreter and exact SHA. The original current pointer/record/pin are rechecked afterwards. The public pin must have been observed and reviewed for this target OS supply, passed explicitly to release packaging, and signed with that package. This role never synthesizes a fingerprint, writes a substitute pin or repairs the Host interpreter. Missing pin, different build SHA, changed inode/permissions, unsupported path or package mutation denies activation. This check grants no Node permission and observes no running daemon.

An operator must explicitly set `cc_agent_outbound_control_enabled: true` and
`cc_agent_outbound_control_approved: true`, the actual exclusive service UID/GID,
and a canonical IPv4 `cc_agent_outbound_control_relay_ip` equal to the existing
root `tunnel-transport.json` dial IP. `start` defaults to false. Preparing the
private directory and the unit does not create or start a registration.

The reviewed enrollment role must first be installed and pass its actual
`enrollment-role-check --live-check`. Its SSH login account is
`cc-agent-enroll`; the fixed sudo command runs the verified installed Rust
worker as `consolecrypt-agent`. This nologin service account, its private Node,
SQLite/anchor directories and this poller must have the SAME UID/GID. Fixture
accounts such as `ccenroll` are not production substitutes. The role refuses
missing or foreign Node state and never repairs its ownership.

An original authenticated Native review then creates a server registration
and uses the already pinned SSH session to write the fixed create-only
`/var/lib/consolecrypt-agent/tunnel/control/bootstrap.json` (service 0600 under
0700). Only AFTER that explicit bootstrap may the operator invoke the role with
`cc_agent_outbound_control_start: true`. Existing or partial credentials cannot
be overwritten; expiration/rebootstrap requires a separate explicit retirement
workflow. This role does not implement that workflow.

The service only polls the fixed trusted Relay origin and carries opaque bytes
to `127.0.0.1:22`. The existing SSH host key, client credential, fixed subsystem
and Actor checks remain authoritative. Registration defaults to one hour and
is capped at one day and the original server session/owner/certificate;
individual attachments remain at most 300 seconds. There is no automatic
renewal, reconnect of an offer, remote command or alternate local destination.

There is at most one 8192-byte application buffer in each direction; actual
read bytes count before forwarding. The poller allocates at most 64 fresh
offers and 512 MiB of signed carrier limits per registration. A response or
bridge completes before the interruptible two-second cadence begins. HTTP
work has an original five-second watchdog and no proxy, redirect or DNS
fallback. Uncertain HTTP or cleanup stops the service. Generic network EOF is
not admission, graceful SSH completion, remote drain or resume authority.

The unit is bounded to 128 MiB RAM, zero swap, 25% CPU and 16 tasks; it has no
capabilities, restart loop or Docker socket. Its system-service BPF properties
allow only localhost and the reviewed Relay IP. Actual BPF enforcement, sshd
port 22/subsystem behavior, signal/reader cleanup, signed package launch,
cross-UID refusal and end-to-end NAT are REQUIRED future gates, not results of
this source packet. Role publication preserves ambiguous partial state and
refuses foreign unit overrides or an equal unit without its original receipt.
The fixed reviewed publisher source under `files/outbound_control_publish.py`
runs only as approved root-role code via isolated Python `-c`, with public
UID/GID/IP and a ten-second task cap. It has no path/unit/body/command inputs.
The helper is not a preexisting installed executable to be adopted. Both fixed
unit and receipt use O_EXCL held FDs; the v2 receipt includes original unit
dev/inode and SHA. A competing equal unit cannot acquire our receipt. Failure
of the second create retains the first partial file without cleanup or retry.
Later calls accept only the complete original inode-bound owned pair.

The service emits only closed public counters/status. It never logs the bearer,
SSH bytes, endpoint response bodies, arbitrary exception text or credentials.
Stop uses the exact systemd unit; removal and credential retirement remain
explicit administrator actions after observing actual process retirement.
