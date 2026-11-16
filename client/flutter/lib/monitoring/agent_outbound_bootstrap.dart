import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart' show AgentRotationNode;

/// Optional, explicit carrier registration. This never grants Node access.
abstract interface class AgentOutboundBootstrapCapability {
  AgentOutboundBootstrapPort? get agentOutboundBootstrapPort;
}

abstract interface class AgentOutboundBootstrapPort {
  Future<AgentOutboundBootstrapHandle> prepare(RuntimeNoticeScope scope, AgentOutboundBootstrapIntent intent);
}

abstract interface class AgentOutboundBootstrapHandle {
  Future<AgentOutboundBootstrapView> details();
  Future<AgentOutboundBootstrapView> approveOnce();
  void cancel();
  Future<AgentOutboundBootstrapCleanup> joined();
  void dispose();
}

final class AgentOutboundBootstrapIntent {
  AgentOutboundBootstrapIntent(this.record, {this.ttlSeconds = 3600}) {
    if (!record.canRequestTunnel || ttlSeconds < 30 || ttlSeconds > 86400) _bad();
  }
  final AgentAssociationRecord record;
  final int ttlSeconds;
  Map<String, Object> request(RuntimeNoticeScope scope) => {
    'version': 1,
    'profileId': scope.profileId,
    'notificationScopeId': scope.nonce,
    'associationId': record.id,
    'ttlSeconds': ttlSeconds,
  };
}

Never _bad() => throw const FormatException('Outbound carrier metadata unavailable');
Map<String, dynamic> _keys(Object? v, Set<String> keys) {
  if (v is! Map<String, dynamic> || v.length != keys.length || !v.keys.every(keys.contains)) _bad();
  return v;
}

Map<String, dynamic> _decode(String raw) {
  if (utf8.encode(raw).length > 16384) _bad();
  final v = jsonDecode(raw);
  if (v is! Map<String, dynamic>) _bad();
  return v;
}

int _int(Object? v, int min, int max) {
  if (v is! int || v < min || v > max) _bad();
  return v;
}

String _id(Object? v) => v is String ? checkedAssociationId(v) : _bad();
String _choice(Object? v, Set<String> choices) => v is String && choices.contains(v) ? v : _bad();
void _fixed(Object? actual, Object expected) {
  if (actual.runtimeType != expected.runtimeType || actual != expected) _bad();
}

final class AgentOutboundBootstrapView {
  const AgentOutboundBootstrapView({
    required this.scope,
    required this.intent,
    required this.node,
    required this.ownerAccountId,
    required this.phase,
    required this.maximumLifetimeSeconds,
    required this.remainingMs,
    required this.cleanupState,
    this.controlId,
    this.registrationExpiresAt,
  });
  final RuntimeNoticeScope scope;
  final AgentOutboundBootstrapIntent intent;
  final AgentRotationNode node;
  final String ownerAccountId, phase, cleanupState;
  final int maximumLifetimeSeconds, remainingMs;
  final String? controlId;
  final DateTime? registrationExpiresAt;
  bool sameOriginal(AgentOutboundBootstrapView other) =>
      scope.same(other.scope) &&
      identical(intent, other.intent) &&
      node.same(other.node) &&
      ownerAccountId == other.ownerAccountId &&
      maximumLifetimeSeconds == other.maximumLifetimeSeconds;
}

AgentOutboundBootstrapView parseAgentOutboundBootstrapView(
  String raw,
  RuntimeNoticeScope scope,
  AgentOutboundBootstrapIntent intent,
) {
  final m = _keys(_decode(raw), {
    'version',
    'profileId',
    'notificationScopeId',
    'phase',
    'associationId',
    'node',
    'ownerAccountId',
    'requestedRegistrationLifetimeSeconds',
    'maximumRegistrationLifetimeSeconds',
    'serverSessionMayShorten',
    'reviewLifetimeSeconds',
    'controlAttachmentMaximumSeconds',
    'registrationMaximumSeconds',
    'registrationGenerationCap',
    'registrationCiphertextAllocationBytes',
    'carrierOnly',
    'nodeAdmission',
    'autoRenew',
    'controlId',
    'registrationExpiresAt',
    'remainingMs',
    'cleanupState',
  });
  _fixed(m['version'], 1);
  if (m['profileId'] != scope.profileId ||
      m['notificationScopeId'] != scope.nonce ||
      _id(m['associationId']) != intent.record.id) {
    _bad();
  }
  final node = AgentRotationNode.parse(m['node']);
  if (node.agentId != intent.record.agentId ||
      intent.record.nodeKeyEpoch != null && node.keyEpoch != intent.record.nodeKeyEpoch ||
      intent.record.nodeKeyFingerprint != null && node.fingerprint != intent.record.nodeKeyFingerprint) {
    _bad();
  }
  _fixed(m['requestedRegistrationLifetimeSeconds'], intent.ttlSeconds);
  final maximum = _int(m['maximumRegistrationLifetimeSeconds'], 30, intent.ttlSeconds);
  for (final entry in <String, Object>{
    'serverSessionMayShorten': true,
    'reviewLifetimeSeconds': 300,
    'controlAttachmentMaximumSeconds': 300,
    'registrationMaximumSeconds': 86400,
    'registrationGenerationCap': 64,
    'registrationCiphertextAllocationBytes': 536870912,
    'carrierOnly': true,
    'nodeAdmission': false,
    'autoRenew': false,
  }.entries) {
    _fixed(m[entry.key], entry.value);
  }
  final phase = _choice(m['phase'], {'bootstrapReview', 'bootstrapping', 'configured', 'unknown'}),
      cleanup = _choice(m['cleanupState'], {'notStarted', 'closedObserved', 'unknown'});
  final String? control = m['controlId'] == null ? null : _id(m['controlId']);
  final int? expires = m['registrationExpiresAt'] == null ? null : _int(m['registrationExpiresAt'], 1, 253402300799);
  // An uncertain attempt can retain its known control ID before an expiry is received.
  if (expires != null && control == null ||
      phase == 'bootstrapReview' && (control != null || expires != null) ||
      phase == 'configured' && (control == null || expires == null || cleanup != 'closedObserved')) {
    _bad();
  }
  return AgentOutboundBootstrapView(
    scope: scope,
    intent: intent,
    node: node,
    ownerAccountId: _id(m['ownerAccountId']),
    phase: phase,
    maximumLifetimeSeconds: maximum,
    remainingMs: _int(m['remainingMs'], 1, 60000),
    cleanupState: cleanup,
    controlId: control,
    registrationExpiresAt: expires == null ? null : DateTime.fromMillisecondsSinceEpoch(expires * 1000, isUtc: true),
  );
}

final class AgentOutboundBootstrapCleanup {
  const AgentOutboundBootstrapCleanup(this.phase, this.state);
  final String phase, state;
}

AgentOutboundBootstrapCleanup parseAgentOutboundBootstrapCleanup(String raw) {
  final m = _keys(_decode(raw), {'version', 'phase', 'carrierOnly', 'nodeAdmission', 'cleanupState'});
  _fixed(m['version'], 1);
  _fixed(m['carrierOnly'], true);
  _fixed(m['nodeAdmission'], false);
  return AgentOutboundBootstrapCleanup(
    _choice(m['phase'], {'bootstrapReview', 'bootstrapping', 'configured', 'unknown'}),
    _choice(m['cleanupState'], {'closedObserved', 'unknown'}),
  );
}
