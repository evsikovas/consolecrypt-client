# ConsoleCrypt Agent

This is the canonical Agent feature folder in the existing client repository. The initial source copy came from the outside-Git monitoring workspace. `BASE_COPY_PROVENANCE.json` records that copy only; subsequent reviewed changes are ordinary repository source changes.

The Agent is an independently provisioned node. Account login, a configured Host ID, Relay reachability and this folder do not grant Node or Reader permission. Carrier and native collector startup require explicit configuration and trusted provisioning. They remain OFF by default. See `OPERATOR_TUN.md` for the noninstalled service override example.

See `DEVICE_BUILD_MANIFEST.json`, `BUILDING.md` and `THIRD_PARTY.md`. This tree contains source and the pinned pure-Python WebSocket dependency; it contains no node state, user credentials, example operator identities, cached build target or runtime test executable.

Old sealed `0.1.0` packages, catalogs and notice inventories are historical artifacts and are not shipping proof for this source tree. No release, service activation or deployment is performed by adding these files.

## Install and operate

Use [the operator guide](delivery/INSTALLING.md) for explicit native systemd
install/update/known rollback/software removal, retained data, and the separate
unprivileged container layout. The installed prefix is
`/opt/consolecrypt-agent-install`; it is not the old `/opt/consolecrypt-agent`
prototype prefix. Software delivery does not enroll the Node or grant scopes.

The full signed collector candidate currently targets Linux amd64 only. ARM64
has a separate compile/test artifact, not an installed metrics package. No
published production image/key or completed installed-lifecycle VM acceptance
is implied by these source instructions.
