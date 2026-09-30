import 'dart:convert';

enum SharingKind { host, snippet, group, secret }

enum SharingRole { reader, editor }

enum SharingTrust { unverified, verified, blocked, deleted }

T _enum<T extends Enum>(List<T> values, Object? value) =>
    values.firstWhere((v) => v.name.toLowerCase() == value.toString().toLowerCase());

final class SharingIdentity {
  const SharingIdentity({
    required this.instance,
    required this.userId,
    required this.deviceId,
    required this.encryptionKey,
    required this.signingKey,
    required this.code,
  });
  final String instance, userId, deviceId, encryptionKey, signingKey, code;
  factory SharingIdentity.fromJson(Map<String, dynamic> j) => SharingIdentity(
    instance: j['server_instance_id'] as String,
    userId: j['user_id'] as String,
    deviceId: j['device_id'] as String,
    encryptionKey: j['encryption_public_key'] as String,
    signingKey: j['signing_public_key'] as String,
    code: j['verification_code'] as String,
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
  const SharingGrant(this.identity, this.role, this.confirmedCode);
  final SharingIdentity identity;
  final SharingRole role;
  final String confirmedCode;
  Map<String, Object?> toJson() => {
    'identity': identity.toJson(),
    'role': role == SharingRole.reader ? 'Reader' : 'Editor',
    'confirmed_code': confirmedCode,
  };
}

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
  });
  final bool enabled, locked, supportsGroups, supportsSecrets, supportsOwnerOnlineEnrollment;
  final String? instance;
  final SharingIdentity? identity;
  final int pending, blocked;
  factory SharingStatus.fromJson(Map<String, dynamic> j) => SharingStatus(
    enabled: j['enabled'] == true,
    supportsGroups: j['supports_groups'] == true,
    supportsSecrets: j['supports_secrets'] == true,
    supportsOwnerOnlineEnrollment: j['supports_owner_online_enrollment_v1'] == true,
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
  });
  final String id, itemId, ownerUserId, ownerDeviceId;
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
          return SharingGrant(identity, _enum(SharingRole.values, m['role']), identity.code);
        })
        .toList(growable: false),
  );
  @override
  String toString() => 'SharingItem(<redacted>)';
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
