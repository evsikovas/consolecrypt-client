import 'package:consolecrypt/sharing/sharing_models.dart';

enum EnrollmentMode { manual, automatic }

enum EnrollmentGrantState { active, revoked }

enum EnrollmentRequestState { pending, challenged, responded, accepted, denied, expired, blocked }

SharingRole _role(Object? value) => SharingRole.values.firstWhere((v) => v.name == value.toString().toLowerCase());

final class EnrollmentGrant {
  const EnrollmentGrant({
    required this.shareId,
    required this.id,
    required this.anchor,
    required this.roleCeiling,
    required this.mode,
    required this.state,
    required this.expires,
    required this.maxAdmissions,
    required this.admitted,
    this.frozen = false,
    this.blockedReason,
  });
  final String shareId, id;
  final SharingIdentity anchor;
  final SharingRole roleCeiling;
  final EnrollmentMode mode;
  final EnrollmentGrantState state;
  final DateTime expires;
  final int maxAdmissions, admitted;
  final bool frozen;
  final String? blockedReason;
  bool get active =>
      state == EnrollmentGrantState.active &&
      !frozen &&
      blockedReason == null &&
      admitted < maxAdmissions &&
      expires.isAfter(DateTime.now());
  factory EnrollmentGrant.fromJson(Map<String, dynamic> j) => EnrollmentGrant(
    shareId: j['share_id'] as String,
    id: j['grant_id'] as String,
    anchor: SharingIdentity.fromJson(j['anchor'] as Map<String, dynamic>),
    roleCeiling: _role(j['role_ceiling']),
    mode: EnrollmentMode.values.firstWhere((m) => m.name == j['mode'].toString().toLowerCase()),
    state: EnrollmentGrantState.values.firstWhere((s) => s.name == j['status'].toString().toLowerCase()),
    expires: DateTime.fromMillisecondsSinceEpoch((j['expires_at'] as num).toInt() * 1000, isUtc: true),
    maxAdmissions: (j['max_admissions'] as num).toInt(),
    admitted: (j['admitted_count'] as num).toInt(),
    frozen: j['frozen'] == true,
    blockedReason: j['blocked_reason'] as String?,
  );
}

final class EnrollmentRequest {
  const EnrollmentRequest({
    required this.shareId,
    required this.grantId,
    required this.id,
    required this.target,
    required this.role,
    required this.code,
    required this.state,
    required this.expires,
    this.blockedReason,
  });
  final String shareId, grantId, id, code;
  final SharingIdentity target;
  final SharingRole role;
  final EnrollmentRequestState state;
  final DateTime expires;
  final String? blockedReason;
  factory EnrollmentRequest.fromJson(Map<String, dynamic> j) => EnrollmentRequest(
    shareId: j['share_id'] as String,
    grantId: j['grant_id'] as String,
    id: j['request_id'] as String,
    target: SharingIdentity.fromJson(j['target'] as Map<String, dynamic>),
    role: _role(j['requested_role']),
    code: j['comparison_code'] as String,
    state: EnrollmentRequestState.values.firstWhere((s) => s.name == j['status'].toString().toLowerCase()),
    expires: DateTime.fromMillisecondsSinceEpoch((j['expires_at'] as num).toInt() * 1000, isUtc: true),
    blockedReason: j['blocked_reason'] as String?,
  );
}

/// Public signed pairing transcript. Import alone never approves a device.
final class EnrollmentPairing {
  const EnrollmentPairing({
    required this.shareId,
    required this.grantId,
    required this.bundle,
    required this.expires,
    this.requestId,
    this.code,
    this.target,
    this.requestedRole,
  });
  final String shareId, grantId, bundle;
  final String? requestId, code;
  final SharingIdentity? target;
  final SharingRole? requestedRole;
  final DateTime expires;
  factory EnrollmentPairing.fromJson(Map<String, dynamic> j) => EnrollmentPairing(
    shareId: j['share_id'] as String,
    grantId: j['grant_id'] as String,
    bundle: j['public_bundle_json'] as String,
    requestId: j['request_id'] as String?,
    code: j['comparison_code'] as String?,
    target: j['target'] == null ? null : SharingIdentity.fromJson(j['target'] as Map<String, dynamic>),
    requestedRole: j['requested_role'] == null ? null : _role(j['requested_role']),
    expires: DateTime.fromMillisecondsSinceEpoch((j['expires_at'] as num).toInt() * 1000, isUtc: true),
  );
}

final class EnrollmentGrantCreate {
  const EnrollmentGrantCreate({
    required this.anchor,
    required this.roleCeiling,
    required this.mode,
    required this.expires,
    required this.maxAdmissions,
  });
  final SharingIdentity anchor;
  final SharingRole roleCeiling;
  final EnrollmentMode mode;
  final DateTime expires;
  final int maxAdmissions;
  Map<String, Object?> toJson() => {
    'anchor': anchor.toJson(),
    'confirmed_identity_code': anchor.code,
    'role_ceiling': roleCeiling == SharingRole.reader ? 'Reader' : 'Editor',
    'mode': mode == EnrollmentMode.manual ? 'Manual' : 'Automatic',
    'expires_at': expires.toUtc().millisecondsSinceEpoch ~/ 1000,
    'max_admissions': maxAdmissions,
  };
}
