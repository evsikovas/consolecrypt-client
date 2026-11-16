# Explicit node-local identity provisioning

This role is disabled by default. An operator must set
`cc_agent_identity_provision: true`, an already signed/digest-verified installed
Linux binary and its expected SHA-256, a private target state path and a unique
public Agent UUID. The role downloads nothing, chooses no root account, creates
no service/user/group and invokes no registration/approval/claim API.

Run as the intended service account. The parent of `cc_agent_identity_state_dir`
must already be a physical private directory with mode 0700. Do not create the
leaf identity directory in advance. `provision-node` creates it and a mode-0600
private key from OS randomness on that node. A valid existing matching key is
reused; missing, partial, corrupt, linked or mismatched existing state fails
closed. The role never deletes or silently regenerates that state.

Supply `cc_agent_identity_fleet_ids`, a mapping from inventory names to unique
Agent UUIDs, for multi-host plays. The current host ID must match its mapping;
duplicate IDs are rejected before generation. Each node generates its own key.
There is no controller seed/private-key template, no private-key fetch/slurp and
no shared identity file distributed to hosts.

The only returned fact is `cc_agent_node_public` with format, Agent ID, key epoch,
public key and fingerprint. These can be used to build the separately reviewed
`cc-agent-profile/v1` input. Public hello is neither enrollment nor an accepted
Host ID binding, and grants no telemetry/collector/job/tunnel/update scope.
Administrator pairing, TTL invitation, exact-key confirmation and later
authorization remain separate steps.

Owned Ubuntu 24.04 fixture checks completed with Ansible core 2.20.11: two
node-local paths produced distinct public keys; repeated apply had changed=0;
check mode created no keys; existing empty state was refused without repair;
and prefix uninstall preserved the actual key files. The role ran as UID 10001
against an already signed/digest-verified staged Linux binary. No system account,
systemd service, production host or registration endpoint was changed or used.
