# Protocol and encrypted model changes

## Unreleased

- Protocol 1.6 (2026-10-21, server 0.1.15, additive): selective sharing of
  database connections. New `SharedItemKind::DbConnection` (`db_connection`,
  signed domain byte 6), advertised as `supports_db_connections` and listed
  only with an explicit `include_db_connections=true`; 1.5 clients never
  receive it and the client never requests or publishes it unless advertised.
  The encrypted projection (`{"kind":"db_connection","data":{…}}`) carries
  name, engine, host, port, database, username, TLS, auth mechanism, public
  client certificate, production flag, limits, typed driver options, tags and
  colour, plus a route shape without personal IDs:
  `{"kind":"direct"}`, `{"kind":"relay"}` (SSH host, jump profile, proxy or
  tunnel of the owner), `{"kind":"kubernetes","namespace":…,"target_kind":…,
  "target_name":…}` or `{"kind":"docker","container":…}`. The receiver saves
  a personal copy with their own compatible route; a non-direct route never
  becomes direct. `authentication` (`password`, `client_key`) is present only
  after explicit consent and additionally requires `supports_secrets` both at
  publication and when importing. Groups, history, session permissions and AI
  consents are never part of it. Collections may reference `db_connection`
  children (as for `kube_context`, a pre-1.6 client cannot decode such a
  collection). Personal vault objects, encryption format and the `DbRoute`
  model are unchanged.
- Client (2026-10-21, protocol 1.7 consumer, server 0.1.16): `cc-protocol`
  pinned to the server commit with new-device protection. Gated on
  `/v1/meta` `capabilities.device_protection`: `device_registered` events
  become a "new device signed in" banner on trusted devices ("It's me" /
  "Revoke…"), the device list shows "Not confirmed" and "New, restricted
  until <date>", `device_in_quarantine` / `envelope_rate_limited` get their
  own messages with the date or time to retry, and "Sign out on all devices"
  reports trusted devices kept by the server's quarantine
  (`200 LogoutResponse`). Without the capability the UI is unchanged; servers
  before 1.7 omit `DeviceInfo.trusted`, so trust is derived from
  `trusted_vaults`. No encrypted payload changes.

- Client Dev (2026-10-21, client-only encrypted payloads): add three
  restorable variants to the `db_connection.route` tagged union:
  `{"kind":"kubernetes","context_id":…,"namespace":…,"target_kind":
  "pod"|"service"|"deployment"|"stateful_set","target_name":…}`,
  `{"kind":"tunnel","tunnel_id":…}` and
  `{"kind":"docker","host_id":…|null,"container":…}` (`host_id` null or
  absent = this computer's engine; the container is referenced by name so it
  survives re-creation). For Kubernetes and Docker the connection `port` is
  the container port and `host` is loopback; for a tunnel `host`/`port` keep
  the tunnel target (TLS identity) and the socket dials the tunnel's local
  listener. The forward itself is device-local state, restored (or a running
  one reused) on connect and never synced. References are validated on save
  (`kube_context`; a `tunnel` of kind `local`; a `host`), and a referenced
  context, tunnel or host cannot be deleted. Existing route shapes are
  byte-for-byte unchanged. `DbRoute` keeps `deny_unknown_fields`: older
  clients cannot decode a connection with a new route, so they skip it and
  retain its ciphertext (reported like any undecodable object) instead of
  silently treating it as a direct connection to a dead loopback port. Server
  DTOs, encryption format and wire version are unchanged.

- Client Dev (2026-10-03, client-only encrypted payloads): add the independent
  `rdp_host` object kind under the Inventory KEK, with host identity/group/tags,
  an explicit password-credential reference, domain and desktop dimensions.
  It is deliberately not a new optional flag on `host`: older clients skip
  unknown encrypted object kinds and retain their ciphertext, instead of
  treating an RDP endpoint as SSH or dropping its settings when re-saving.
  Existing SSH bytes, server DTOs, encryption format and wire version are
  unchanged. Clipboard/folder permissions and certificate decisions are local
  session state and never part of the synced object. RDP sharing is not enabled
  by this model addition.
