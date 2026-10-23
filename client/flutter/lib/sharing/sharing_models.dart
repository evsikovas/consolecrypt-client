import 'dart:collection';
import 'dart:convert';

enum SharingKind { host, snippet, group, secret, kubeContext, dbConnection }

extension SharingKindWire on SharingKind {
  String get wireName => switch (this) {
    SharingKind.kubeContext => 'kube_context',
    SharingKind.dbConnection => 'db_connection',
    _ => name,
  };
}

enum SharingRole { reader, editor }

enum SharingTrust { unverified, verified, blocked, deleted }

T _enum<T extends Enum>(List<T> values, Object? value) =>
    values.firstWhere((v) => v.name.toLowerCase() == value.toString().replaceAll('_', '').toLowerCase());

/// This device's pin of a colleague device's keys (audit H3, TR-12).
enum PeerTrust {
  /// Never chosen on this device.
  none,

  /// Chosen; keys pinned on first use, not verified.
  chosen,

  /// Verified outside the server (safety number or QR code).
  verified,

  /// The server reported other keys than the pinned ones: no new access
  /// until verified again.
  keysChanged;

  static PeerTrust fromWire(Object? v) => switch (v) {
    'chosen' => PeerTrust.chosen,
    'verified' => PeerTrust.verified,
    'keys_changed' => PeerTrust.keysChanged,
    _ => PeerTrust.none,
  };
}

final class SharingIdentity {
  const SharingIdentity({
    required this.instance,
    required this.userId,
    required this.deviceId,
    required this.encryptionKey,
    required this.signingKey,
    required this.code,
    this.trust = PeerTrust.none,
    this.verificationId,
    this.verifiedAt,
  });

  /// [code] is the key fingerprint (details only) — never a comparison code.
  final String instance, userId, deviceId, encryptionKey, signingKey, code;
  final PeerTrust trust;

  /// The pin to pass when granting / accepting (checked in Rust).
  final String? verificationId;
  final DateTime? verifiedAt;
  bool sameKeys(SharingIdentity o) =>
      o.deviceId == deviceId && o.encryptionKey == encryptionKey && o.signingKey == signingKey;
  SharingIdentity withPin(PeerPin pin) => SharingIdentity(
    instance: instance,
    userId: userId,
    deviceId: deviceId,
    encryptionKey: encryptionKey,
    signingKey: signingKey,
    code: code,
    trust: pin.status,
    verificationId: pin.verificationId,
    verifiedAt: pin.verifiedAt,
  );
  factory SharingIdentity.fromJson(Map<String, dynamic> j) => SharingIdentity(
    instance: j['server_instance_id'] as String,
    userId: j['user_id'] as String,
    deviceId: j['device_id'] as String,
    encryptionKey: j['encryption_public_key'] as String,
    signingKey: j['signing_public_key'] as String,
    code: j['verification_code'] as String,
    trust: PeerTrust.fromWire(j['trust']),
    verificationId: j['verification_id'] as String?,
    verifiedAt: j['verified_at_ms'] == null
        ? null
        : DateTime.fromMillisecondsSinceEpoch((j['verified_at_ms'] as num).toInt()),
  );
  Map<String, Object?> toJson() => {
    'server_instance_id': instance,
    'user_id': userId,
    'device_id': deviceId,
    'encryption_public_key': encryptionKey,
    'signing_public_key': signingKey,
    'verification_code': code,
  };
}

final class SharingGrant {
  const SharingGrant(this.identity, this.role);
  final SharingIdentity identity;
  final SharingRole role;

  /// The pin of [identity]'s keys; Rust checks status and policy (TR-16).
  /// Existing members with unchanged keys need none.
  String? get verificationId => identity.verificationId;
  Map<String, Object?> toJson() => {
    'identity': identity.toJson(),
    'role': role == SharingRole.reader ? 'Reader' : 'Editor',
    'verification_id': verificationId,
  };
}

/// A pinned colleague device (`PeerPinDto`).
final class PeerPin {
  const PeerPin({
    required this.verificationId,
    required this.deviceId,
    required this.userId,
    required this.status,
    this.method,
    this.verifiedAt,
    this.email,
  });
  final String verificationId, deviceId, userId;
  final PeerTrust status;
  final String? method, email;
  final DateTime? verifiedAt;
  factory PeerPin.fromJson(Map<String, dynamic> j) => PeerPin(
    verificationId: j['verification_id'] as String,
    deviceId: j['device_id'] as String,
    userId: j['user_id'] as String,
    status: PeerTrust.fromWire(j['status']),
    method: j['method'] as String?,
    verifiedAt: j['verified_at_ms'] == null
        ? null
        : DateTime.fromMillisecondsSinceEpoch((j['verified_at_ms'] as num).toInt()),
    email: j['email'] as String?,
  );
}

/// What to compare with a colleague (`SafetyNumberDto`).
final class SafetyNumber {
  const SafetyNumber({required this.groups, required this.ownQrPayload, this.trust = PeerTrust.none});
  final List<String> groups;
  final String ownQrPayload;
  final PeerTrust trust;
  factory SafetyNumber.fromJson(Map<String, dynamic> j) => SafetyNumber(
    groups: (j['safety_number'] as String).split(' '),
    ownQrPayload: j['own_qr_payload'] as String,
    trust: PeerTrust.fromWire(j['trust']),
  );
}

/// "Only verified recipients" (`SharingPolicyDto`, TR-14).
final class SharingPolicy {
  const SharingPolicy({this.sensitiveVerifiedOnly = true, this.otherVerifiedOnly = false});
  final bool sensitiveVerifiedOnly, otherVerifiedOnly;
  factory SharingPolicy.fromJson(Map<String, dynamic> j) => SharingPolicy(
    sensitiveVerifiedOnly: j['sensitive_verified_only'] != false,
    otherVerifiedOnly: j['other_verified_only'] == true,
  );
  Map<String, Object?> toJson() => {
    'sensitive_verified_only': sensitiveVerifiedOnly,
    'other_verified_only': otherVerifiedOnly,
  };
}

/// Secrets, Kubernetes contexts and database connections (TR-14).
bool sharingKindSensitive(SharingKind kind) =>
    kind == SharingKind.secret || kind == SharingKind.kubeContext || kind == SharingKind.dbConnection;

final class SharingStatus {
  const SharingStatus({
    this.enabled = false,
    this.locked = false,
    this.instance,
    this.identity,
    this.pending = 0,
    this.blocked = 0,
    this.supportsGroups = false,
    this.supportsSecrets = false,
    this.supportsOwnerOnlineEnrollment = false,
    this.supportsKubeContexts = false,
    this.supportsDbConnections = false,
    this.supportsLeave = false,
    this.supportsRestore = false,
  });
  final bool enabled,
      locked,
      supportsGroups,
      supportsSecrets,
      supportsOwnerOnlineEnrollment,
      supportsKubeContexts,
      supportsDbConnections;

  /// Server protocol 1.8: recipients may leave; deleted items are restorable.
  final bool supportsLeave, supportsRestore;
  final String? instance;
  final SharingIdentity? identity;
  final int pending, blocked;
  factory SharingStatus.fromJson(Map<String, dynamic> j) => SharingStatus(
    enabled: j['enabled'] == true,
    supportsGroups: j['supports_groups'] == true,
    supportsSecrets: j['supports_secrets'] == true,
    supportsKubeContexts: j['supports_kube_contexts'] == true,
    supportsDbConnections: j['supports_db_connections'] == true,
    supportsOwnerOnlineEnrollment: j['supports_owner_online_enrollment_v1'] == true,
    supportsLeave: j['supports_leave'] == true,
    supportsRestore: j['supports_restore'] == true,
    locked: j['locked'] == true,
    instance: j['server_instance_id'] as String?,
    identity: j['identity'] == null ? null : SharingIdentity.fromJson(j['identity'] as Map<String, dynamic>),
    pending: (j['pending'] as num?)?.toInt() ?? 0,
    blocked: (j['blocked'] as num?)?.toInt() ?? 0,
  );
}

final class SharingItem {
  const SharingItem({
    required this.id,
    required this.itemId,
    required this.kind,
    required this.ownerUserId,
    required this.ownerDeviceId,
    required this.revision,
    required this.epoch,
    required this.owned,
    required this.trust,
    this.role,
    this.previewJson,
    this.blockedReason,
    this.members = const [],
    this.memberRoles = const [],
    this.departedUserIds = const [],
    this.ownerTrust = PeerTrust.none,
  });
  final String id, itemId, ownerUserId, ownerDeviceId;

  /// Received items: the pin of the owner's keys (chosen = not verified).
  final PeerTrust ownerTrust;
  final SharingKind kind;
  final int revision, epoch;
  final bool owned;
  final SharingTrust trust;
  final SharingRole? role;

  /// Transient verified projection; never written by the Dart side to disk.
  final String? previewJson;
  final String? blockedReason;
  final List<SharingIdentity> members;
  final List<SharingGrant> memberRoles;

  /// Owner view: accounts that left; already omitted from [memberRoles].
  final List<String> departedUserIds;
  Map<String, dynamic>? get projection => previewJson == null ? null : jsonDecode(previewJson!) as Map<String, dynamic>;
  Map<String, dynamic>? get data => projection?['data'] as Map<String, dynamic>?;
  String? get name => data?['name'] as String?;
  bool get canEdit => trust == SharingTrust.verified && role == SharingRole.editor;
  factory SharingItem.fromJson(Map<String, dynamic> j) => SharingItem(
    id: j['share_id'] as String,
    itemId: j['item_id'] as String,
    kind: _enum(SharingKind.values, j['kind']),
    ownerUserId: j['owner_user_id'] as String,
    ownerDeviceId: j['owner_device_id'] as String,
    revision: (j['revision'] as num).toInt(),
    epoch: (j['access_epoch'] as num).toInt(),
    owned: j['owned'] == true,
    trust: _enum(SharingTrust.values, j['trust']),
    role: j['role'] == null ? null : _enum(SharingRole.values, j['role']),
    previewJson: j['preview_json'] as String?,
    blockedReason: j['blocked_reason'] as String?,
    members: (j['members'] as List)
        .map((v) => SharingIdentity.fromJson(v as Map<String, dynamic>))
        .toList(growable: false),
    memberRoles: ((j['member_roles'] as List?) ?? const [])
        .map((v) {
          final m = v as Map<String, dynamic>;
          final identity = SharingIdentity.fromJson(m['identity'] as Map<String, dynamic>);
          return SharingGrant(identity, _enum(SharingRole.values, m['role']));
        })
        .toList(growable: false),
    departedUserIds: ((j['departed_user_ids'] as List?) ?? const []).cast<String>().toList(growable: false),
    ownerTrust: PeerTrust.fromWire(j['owner_trust']),
  );
  @override
  String toString() => 'SharingItem(<redacted>)';
}

/// An owner-deleted shared item that the server still keeps restorable.
final class SharingDeletedItem {
  const SharingDeletedItem({
    required this.id,
    required this.kind,
    required this.restorableUntil,
    this.previewJson,
    this.recipients = 0,
  });
  final String id;
  final SharingKind kind;
  final DateTime restorableUntil;

  /// Masked metadata only (same as [SharingItem.previewJson]).
  final String? previewJson;
  final int recipients;
  String? get name {
    final preview = previewJson == null ? null : jsonDecode(previewJson!);
    final data = preview is Map<String, dynamic> ? preview['data'] : null;
    return data is Map<String, dynamic> ? data['name'] as String? : null;
  }

  factory SharingDeletedItem.fromJson(Map<String, dynamic> j) => SharingDeletedItem(
    id: j['share_id'] as String,
    kind: _enum(SharingKind.values, j['kind']),
    restorableUntil: DateTime.fromMillisecondsSinceEpoch((j['restorable_until_ms'] as num).toInt()),
    previewJson: j['preview_json'] as String?,
    recipients: (j['recipients'] as num?)?.toInt() ?? 0,
  );
  @override
  String toString() => 'SharingDeletedItem(<redacted>)';
}

final class SharingInvitation {
  const SharingInvitation(this.item, this.owner);
  final SharingItem item;
  final SharingIdentity owner;
  factory SharingInvitation.fromJson(Map<String, dynamic> j) => SharingInvitation(
    SharingItem.fromJson(j['item'] as Map<String, dynamic>),
    SharingIdentity.fromJson(j['owner'] as Map<String, dynamic>),
  );
}

final class SharingOutboxEntry {
  const SharingOutboxEntry(this.mutationId, this.shareId, this.blocked, this.reason);
  final String mutationId, shareId;
  final bool blocked;
  final String? reason;
  factory SharingOutboxEntry.fromJson(Map<String, dynamic> j) => SharingOutboxEntry(
    j['mutation_id'] as String,
    j['share_id'] as String,
    j['state'].toString().toLowerCase() == 'blocked',
    j['reason'] as String?,
  );
}

/// A shared list plus how many entries the core skipped because they could
/// not be read (malformed, or failing signature/trust/ciphertext checks).
final class SharingItemList extends ListBase<SharingItem> {
  SharingItemList(this._items, {this.unreadable = 0});
  final List<SharingItem> _items;
  final int unreadable;

  /// The core's `{items, unreadable}` reply; a bare list from older cores.
  factory SharingItemList.fromJson(Object? json) {
    final map = json is Map<String, dynamic> ? json : null;
    final items = (map == null ? json : map['items']) as List;
    return SharingItemList(
      items.map((v) => SharingItem.fromJson(v as Map<String, dynamic>)).toList(),
      unreadable: (map?['unreadable'] as num?)?.toInt() ?? 0,
    );
  }

  @override
  int get length => _items.length;
  @override
  set length(int value) => _items.length = value;
  @override
  SharingItem operator [](int index) => _items[index];
  @override
  void operator []=(int index, SharingItem value) => _items[index] = value;
  @override
  void add(SharingItem element) => _items.add(element);
}

/// Entries of [items] the core could not read (0 for plain lists).
int sharingUnreadable(List<SharingItem>? items) => items is SharingItemList ? items.unreadable : 0;
