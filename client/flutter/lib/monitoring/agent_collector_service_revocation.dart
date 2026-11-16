import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart' show AgentRotationNode;

/// A targeted service-policy revocation, separate from cancelling its caller
/// channel. Neither a local block nor a remote receipt observes a process stop.
enum AgentCollectorServiceRevocationPhase { serviceRevokeReview, revoking, revoked, unknown, cancelled }

abstract interface class AgentCollectorServiceRevocationPort {
  Future<AgentCollectorServiceRevocationHandle> prepareServiceRevocation(AgentCollectorServiceStatus target);
}

abstract interface class AgentCollectorServiceRevocationHandle {
  Future<AgentCollectorServiceRevocationView> details();
  Future<AgentCollectorServiceRevocationView> confirmRevocation();
  void cancel();
  Future<AgentCollectorServiceRevocationCleanup> joined();
  void dispose();
}

final class AgentCollectorServiceRevocationView {
  const AgentCollectorServiceRevocationView({
    required this.target,
    required this.phase,
    required this.node,
    required this.ownerKind,
    required this.ownerId,
    required this.ownerDeviceId,
    required this.ownerSigningKeySha256,
    required this.remainingMs,
    required this.localBlocked,
    required this.remoteRevoked,
    required this.cleanupState,
  });
  final AgentCollectorServiceStatus target;
  final AgentCollectorServiceRevocationPhase phase;
  final AgentRotationNode node;
  final String ownerKind, ownerId, ownerDeviceId, ownerSigningKeySha256, cleanupState;
  final int remainingMs;
  final bool localBlocked, remoteRevoked;
  bool sameOriginal(AgentCollectorServiceRevocationView other) =>
      identical(target, other.target) &&
      node.same(other.node) &&
      ownerKind == other.ownerKind &&
      ownerId == other.ownerId &&
      ownerDeviceId == other.ownerDeviceId &&
      ownerSigningKeySha256 == other.ownerSigningKeySha256;
}

final class AgentCollectorServiceRevocationCleanup {
  const AgentCollectorServiceRevocationCleanup(this.phase, this.localBlocked, this.remoteRevoked, this.state);
  final AgentCollectorServiceRevocationPhase phase;
  final bool localBlocked, remoteRevoked;
  final String state;
}

Never _bad() => throw const FormatException('Service revocation observation unavailable');
Map<String, dynamic> _keys(Object? value, Set<String> keys) {
  if (value is! Map<String, dynamic> || value.length != keys.length || !value.keys.every(keys.contains)) _bad();
  return value;
}

Map<String, dynamic> _decode(String raw, Set<String> keys) {
  if (raw.isEmpty || utf8.encode(raw).length > 16384) _bad();
  return _keys(jsonDecode(raw), keys);
}

AgentCollectorServiceRevocationPhase _phase(Map<String, dynamic> m) {
  if (m['version'] is! int ||
      m['version'] != 1 ||
      m['runtimeState'] != 'notObserved' ||
      m['lossUnknown'] != true ||
      m['deliveryGuaranteed'] != false ||
      m['localBlocked'] is! bool ||
      m['remoteRevoked'] is! bool) {
    _bad();
  }
  final phase = AgentCollectorServiceRevocationPhase.values.where((v) => v.name == m['phase']).firstOrNull;
  if (phase == null ||
      m['remoteRevoked'] == true &&
          (m['localBlocked'] != true || phase != AgentCollectorServiceRevocationPhase.revoked) ||
      phase == AgentCollectorServiceRevocationPhase.revoked && m['remoteRevoked'] != true ||
      const {
            AgentCollectorServiceRevocationPhase.serviceRevokeReview,
            AgentCollectorServiceRevocationPhase.cancelled,
          }.contains(phase) &&
          (m['localBlocked'] != false || m['remoteRevoked'] != false)) {
    _bad();
  }
  return phase;
}

AgentCollectorServiceRevocationView parseAgentCollectorServiceRevocation(
  String raw,
  AgentCollectorServiceStatus target,
) {
  final m = _decode(raw, {
        'version',
        'profileId',
        'notificationScopeId',
        'kind',
        'phase',
        'associationId',
        'serviceId',
        'serviceHash',
        'hostId',
        'hostRevision',
        'owner',
        'node',
        'remainingMs',
        'localBlocked',
        'remoteRevoked',
        'runtimeState',
        'lossUnknown',
        'deliveryGuaranteed',
        'cleanupState',
      }),
      phase = _phase(m),
      remaining = m['remainingMs'];
  if (m['kind'] != 'finiteCollectorServiceRevocation' ||
      m['profileId'] != target.scope.profileId ||
      m['notificationScopeId'] != target.scope.nonce ||
      m['associationId'] != target.record.id ||
      m['serviceId'] != target.serviceId ||
      m['serviceHash'] != target.serviceHash ||
      m['hostId'] != target.record.hostId ||
      m['hostRevision'] != target.record.hostRevision ||
      remaining is! int ||
      remaining < 1 ||
      remaining > 60000 ||
      !const {'notStarted', 'pending', 'closedObserved', 'unknown'}.contains(m['cleanupState'])) {
    _bad();
  }
  final owner = _keys(m['owner'], {'kind', 'id', 'deviceId', 'signingKeySha256'});
  // The target itself came from the original strict saved-service observation.
  // Validate owner shape here; the controller additionally freezes it across replies.
  String id(Object? value) => value is String ? checkedAssociationId(value) : _bad();
  final ownerId = id(owner['id']);
  if (!const {'localProfile', 'personalAccount'}.contains(owner['kind']) ||
      owner['kind'] == 'localProfile' && ownerId != target.scope.profileId) {
    _bad();
  }
  final node = AgentRotationNode.parse(m['node']);
  if (!node.same(target.node)) _bad();
  return AgentCollectorServiceRevocationView(
    target: target,
    phase: phase,
    node: node,
    ownerKind: owner['kind'] as String,
    ownerId: ownerId,
    ownerDeviceId: id(owner['deviceId']),
    ownerSigningKeySha256: owner['signingKeySha256'] is String
        ? associationHash(owner['signingKeySha256'] as String)
        : _bad(),
    remainingMs: remaining,
    localBlocked: m['localBlocked'] as bool,
    remoteRevoked: m['remoteRevoked'] as bool,
    cleanupState: m['cleanupState'] as String,
  );
}

AgentCollectorServiceRevocationCleanup parseAgentCollectorServiceRevocationCleanup(String raw) {
  final m = _decode(raw, {
        'version',
        'phase',
        'localBlocked',
        'remoteRevoked',
        'runtimeState',
        'lossUnknown',
        'deliveryGuaranteed',
        'cleanupState',
      }),
      phase = _phase(m);
  if (!const {'closedObserved', 'unknown'}.contains(m['cleanupState'])) _bad();
  return AgentCollectorServiceRevocationCleanup(
    phase,
    m['localBlocked'] as bool,
    m['remoteRevoked'] as bool,
    m['cleanupState'] as String,
  );
}
