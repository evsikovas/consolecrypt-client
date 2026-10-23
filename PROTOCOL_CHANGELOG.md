# Protocol and encrypted model changes

## Unreleased

- Client 0.9.2 (2026-10-23, protocol 1.11 consumer — vault key trust,
  audit H2; `cc-protocol` pinned to server commit b4bfd4a). Full wire
  description in the server repository's changelog ("Protocol 1.11").
  - Every profile pins its key commitment `C = HKDF-SHA256(VRK, salt =
    vault_id, info = "cc-vrk-commit-v1")` in `vault-keys.json`
    (`vault_key: { commitment, unconfirmed, published }`) and in the profile
    database (`app_core.vault_key`), and checks every VRK against it before
    the vault is installed: passphrase (also the retry with a fresh server
    envelope), Recovery Key, device unlock, approval, backup restore, enable
    sync. A mismatch is `vault_key_mismatch` (args `vault_id`, `reason`,
    `pinned`, `received`), journalled in `security-journal.jsonl` of the
    profile; nothing is decrypted, attested or synced.
  - Approval: on a 1.11 server the approver sends an approval intent and
    both devices show the pairing code v2; the new device pins the
    approver's keys only after the user confirmed the code there, then
    verifies `VaultRecoveryMaterial.device_approval` (signature, vault id
    from the signed list, envelope hash, commitment). Older servers or
    approvers: v1, key pinned "unconfirmed" until the next passphrase /
    Recovery Key unlock.
  - Own envelopes are cached: nothing from an `attest` response; on vault
    creation, passphrase change, kit regeneration and enable sync the
    device's own password / recovery envelopes (a server copy only when
    byte-identical).
  - Signed key-commitment records are published and compared after unlock;
    another `C` from a device of the account raises `vault_key_conflict`.
- Client 0.9.2 (2026-10-23, sharing key verification, audit H3; no wire
  change): `SharingGrantDto.confirmed_code` is replaced by
  `verification_id` (a pin), `sharing_accept` / `sharing_reconcile` /
  `enrollment_reconcile` take a `verification_id`,
  `EnrollmentGrantCreateDto.confirmed_identity_code` is removed, and the
  enrollment pairing code is 6 groups of 5 digits typed by the user. New
  plaintext model `cc_models::peer_pin::PeerPin` (pinned colleague keys:
  `chosen` / `verified` / `keys_changed`), kept in the encrypted profile
  database in 0.9.2 (not yet a synced vault object: clients up to 0.9.1
  would report it as an integrity warning). `VaultSettings.sharing_policy`
  (`sensitive_verified_only`, default on; `other_verified_only`, default
  off) is a new optional field; older clients that rewrite the settings
  drop it and the defaults apply. Safety number:
  `SHA-512("consolecrypt/v1/sharing-safety-number\0" ‖ instance ‖ the two
  parties sorted, each user ‖ device ‖ X25519 ‖ Ed25519)` → 12 groups of 5
  digits; QR payload `consolecrypt-verify:v1:<base64url(instance ‖ user ‖
  device ‖ X25519 ‖ Ed25519)>`.
- Protocol 1.7 (server 0.1.16): see the server changelog; the client entry
  "Client (2026-10-21, protocol 1.7 consumer …)" below covers what the
  client relies on.

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
- Client (2026-10-22, protocol 1.8 consumer, server 0.1.17): sharing
  recipient discovery sends the e-mail only in the JSON body of
  `POST /v1/shares/recipients/lookup` when `/v1/meta` advertises
  `capabilities.recipient_lookup_post`; older servers keep the deprecated
  `GET /v1/shares/recipients?email=`. `cc-protocol` pinned to server commit
  66d1f67. No encrypted payload changes.
- Client (2026-10-22, protocol 1.8 consumer, server 0.1.17): `cc-protocol`
  pinned to the server commit with owner-only delete, leave and restore.
  Gated on `/v1/meta` `capabilities.share_leave` / `share_restore`:
  recipients get "Leave share" (all their devices lose server access; the
  local cached record, its OS checkpoint mark and queued changes for that
  item are removed, personal copies stay); owners see departed accounts
  (`ShareListPage.departures`) hidden from the member list so the next access
  change drops them, and "Recently deleted" lists `GET /v1/shares/deleted`.
  A restore verifies the retained revision against the accepted tombstone's
  `previous_revision_hash`, opens it on the owner device and republishes it
  as a new item (`CreateShareRequest.restored_from`) with the previous
  recipients except departed ones; a tombstone is never resurrected and
  recipients accept the new item again. The outbox entry gains an optional
  `restored_from` (omitted when absent). No encrypted payload changes.

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
