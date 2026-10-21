# Protocol and encrypted model changes

## Unreleased

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
