import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';

/// Two fixed Native operations. Reconciliation cannot create a new proposal.
abstract interface class AgentAssociationRotationPort {
  Future<AgentRotationHandle> prepareRotation(RuntimeNoticeScope scope, AgentAssociationRecord record);
  Future<AgentRotationHandle> prepareRotationReconcile(RuntimeNoticeScope scope, AgentAssociationRecord record);
}

abstract interface class AgentRotationHandle {
  Future<AgentRotationDetails> details();
  Future<AgentRotationDetails> prepareCandidateOnce();
  Future<AgentRotationDetails> confirmOnce();
  void cancel();
  Future<AgentRotationCleanup> joined();
}

enum AgentRotationMode { rotate, reconcile }

Never _bad() => throw const FormatException('Agent rotation response invalid');
typedef _Json = Map<String, dynamic>;
_Json _keys(Object? value, Set<String> fields) {
  if (value is! _Json || value.length != fields.length || !value.keys.every(fields.contains)) _bad();
  return value;
}

_Json _decode(String raw, int cap) {
  if (utf8.encode(raw).length > cap) _bad();
  final v = jsonDecode(raw);
  if (v is! _Json) _bad();
  return v;
}

String _id(Object? value) => value is String ? checkedAssociationId(value) : _bad();
String _hash(Object? value) => value is String ? associationHash(value) : _bad();
String _number(Object? value, {bool positive = false, bool signed = false}) =>
    value is String && value.length <= 20 ? associationNumber(value, positive: positive, signed: signed) : _bad();
String _choice(Object? value, Set<String> choices) => value is String && choices.contains(value) ? value : _bad();
const rotationPhases = {
  'rotationPreparationReview',
  'preparingCandidate',
  'rotationCertificateReview',
  'rotationReconcileReview',
  'rotating',
  'rotated',
  'unknown',
  'cancelled',
  'expired',
};
DateTime _utc(Object? value) {
  if (value is! String || value.length > 64) _bad();
  final m = RegExp(r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$').firstMatch(value),
      d = DateTime.tryParse(value);
  if (m == null ||
      d == null ||
      !d.isUtc ||
      d.year != int.parse(m[1]!) ||
      d.month != int.parse(m[2]!) ||
      d.day != int.parse(m[3]!) ||
      d.hour != int.parse(m[4]!) ||
      d.minute != int.parse(m[5]!) ||
      d.second != int.parse(m[6]!)) {
    _bad();
  }
  return d;
}

final class AgentRotationNode {
  const AgentRotationNode(this.agentId, this.keyEpoch, this.publicKey, this.fingerprint);
  final String agentId, keyEpoch, publicKey, fingerprint;
  factory AgentRotationNode.parse(Object? value) {
    final v = _keys(value, {'agentId', 'keyEpoch', 'publicKey', 'keyFingerprint'});
    return AgentRotationNode(
      _id(v['agentId']),
      _number(v['keyEpoch'], positive: true),
      _hash(v['publicKey']),
      _hash(v['keyFingerprint']),
    );
  }
  bool same(AgentRotationNode? v) =>
      v != null &&
      agentId == v.agentId &&
      keyEpoch == v.keyEpoch &&
      publicKey == v.publicKey &&
      fingerprint == v.fingerprint;
}

final class AgentRotationDetails {
  const AgentRotationDetails({
    required this.scope,
    required this.record,
    required this.remainingMs,
    required this.phase,
    required this.ownerKind,
    required this.ownerId,
    required this.ownerDeviceId,
    required this.ownerSigningKeySha256,
    required this.nodeContextKind,
    required this.nodeContextId,
    required this.deploymentId,
    required this.previousNode,
    required this.node,
    required this.proposedNode,
    required this.rotationId,
    required this.actorRevision,
    required this.checkpoint,
    required this.certificateDigest,
    required this.certificateExpiresUtc,
    required this.localBlocked,
    required this.selectionObserved,
    required this.cleanupState,
  });
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final int remainingMs;
  final String phase,
      ownerKind,
      ownerId,
      ownerDeviceId,
      ownerSigningKeySha256,
      nodeContextKind,
      nodeContextId,
      deploymentId,
      actorRevision,
      checkpoint,
      certificateDigest,
      cleanupState;
  final DateTime certificateExpiresUtc;
  final AgentRotationNode previousNode;
  final AgentRotationNode? node, proposedNode;
  final String? rotationId;
  final bool? localBlocked;
  final bool selectionObserved;
  bool sameOwnerAndOriginal(AgentRotationDetails v) =>
      scope.same(v.scope) &&
      identical(record, v.record) &&
      previousNode.same(v.previousNode) &&
      ownerKind == v.ownerKind &&
      ownerId == v.ownerId &&
      ownerDeviceId == v.ownerDeviceId &&
      ownerSigningKeySha256 == v.ownerSigningKeySha256 &&
      nodeContextKind == v.nodeContextKind &&
      nodeContextId == v.nodeContextId &&
      deploymentId == v.deploymentId;
}

AgentRotationDetails parseAgentRotationDetails(String raw, RuntimeNoticeScope scope, AgentAssociationRecord record) {
  final v = _keys(_decode(raw, 16384), {
    'version',
    'profileId',
    'notificationScopeId',
    'remainingMs',
    'phase',
    'associationId',
    'hostId',
    'hostRevision',
    'owner',
    'nodeContext',
    'previousNode',
    'node',
    'proposedNode',
    'rotationId',
    'actorRevision',
    'checkpoint',
    'certificateDigest',
    'certificateExpiresUtc',
    'scope',
    'capabilitiesUnchanged',
    'expiryExtension',
    'selectionObserved',
    'localBlocked',
    'actorAdmission',
    'cleanupState',
  });
  final owner = _keys(v['owner'], {'kind', 'id', 'deviceId', 'signingKeySha256'}),
      context = _keys(v['nodeContext'], {'kind', 'id', 'deploymentId'});
  final ownerKind = _choice(owner['kind'], {'localProfile', 'personalAccount'}), ownerId = _id(owner['id']);
  final phase = _choice(v['phase'], rotationPhases),
      previous = AgentRotationNode.parse(v['previousNode']),
      node = v['node'] == null ? null : AgentRotationNode.parse(v['node']),
      proposed = v['proposedNode'] == null ? null : AgentRotationNode.parse(v['proposedNode']);
  final rotationId = v['rotationId'];
  if (v['version'] is! int ||
      v['version'] != 1 ||
      v['profileId'] != scope.profileId ||
      v['notificationScopeId'] != scope.nonce ||
      _id(v['associationId']) != record.id ||
      _id(v['hostId']) != record.hostId ||
      _number(v['hostRevision'], positive: true, signed: true) != record.hostRevision ||
      v['remainingMs'] is! int ||
      (v['remainingMs'] as int) < 1 ||
      (v['remainingMs'] as int) > 60000 ||
      v['scope'] != 'tunnel.open' ||
      v['capabilitiesUnchanged'] != true ||
      v['expiryExtension'] != false ||
      v['actorAdmission'] != false ||
      v['selectionObserved'] is! bool ||
      v['localBlocked'] != null && v['localBlocked'] is! bool ||
      previous.agentId != record.agentId ||
      previous.keyEpoch != '1' ||
      previous.fingerprint != record.nodeKeyFingerprint ||
      record.nodeKeyEpoch != null && record.nodeKeyEpoch != '1' ||
      ownerKind == 'localProfile' && ownerId != scope.profileId ||
      (proposed == null) != (rotationId == null) ||
      rotationId != null && (rotationId is! String || !RegExp(r'^[0-9a-f]{32}$').hasMatch(rotationId)) ||
      proposed != null &&
          (proposed.agentId != previous.agentId ||
              proposed.keyEpoch != '2' ||
              proposed.publicKey == previous.publicKey ||
              proposed.fingerprint == previous.fingerprint) ||
      node != null && !node.same(previous) && !node.same(proposed) ||
      phase == 'rotationPreparationReview' &&
          (proposed != null || v['localBlocked'] != false || node == null || !node.same(previous)) ||
      {'rotationCertificateReview', 'rotationReconcileReview', 'rotated'}.contains(phase) && proposed == null ||
      {'rotationCertificateReview', 'rotationReconcileReview'}.contains(phase) && v['localBlocked'] != true ||
      (v['selectionObserved'] == true) != (phase == 'rotated') ||
      phase == 'rotated' && (v['localBlocked'] != false || node == null || !node.same(proposed))) {
    _bad();
  }
  final cert = _hash(v['certificateDigest']), expiry = _utc(v['certificateExpiresUtc']);
  if (phase == 'rotationPreparationReview' &&
      (cert != record.certificateDigest ||
          record.certificateExpiresUtc != null && expiry != record.certificateExpiresUtc)) {
    _bad();
  }
  return AgentRotationDetails(
    scope: scope,
    record: record,
    remainingMs: v['remainingMs'] as int,
    phase: phase,
    ownerKind: ownerKind,
    ownerId: ownerId,
    ownerDeviceId: _id(owner['deviceId']),
    ownerSigningKeySha256: _hash(owner['signingKeySha256']),
    nodeContextKind: _choice(context['kind'], {'localProfile', 'personalAccount'}),
    nodeContextId: _id(context['id']),
    deploymentId: _id(context['deploymentId']),
    previousNode: previous,
    node: node,
    proposedNode: proposed,
    rotationId: rotationId as String?,
    actorRevision: _number(v['actorRevision']),
    checkpoint: _number(v['checkpoint']),
    certificateDigest: cert,
    certificateExpiresUtc: expiry,
    localBlocked: v['localBlocked'] as bool?,
    selectionObserved: v['selectionObserved'] as bool,
    cleanupState: _choice(v['cleanupState'], {'notStarted', 'pending', 'closedObserved', 'unknown'}),
  );
}

final class AgentRotationCleanup {
  const AgentRotationCleanup(this.phase, this.cleanupState, this.selectionObserved);
  final String phase, cleanupState;
  final bool selectionObserved;
}

AgentRotationCleanup parseAgentRotationCleanup(String raw) {
  final v = _keys(_decode(raw, 4096), {'version', 'phase', 'cleanupState', 'selectionObserved'}),
      phase = _choice(v['phase'], rotationPhases);
  if (v['version'] is! int ||
      v['version'] != 1 ||
      v['selectionObserved'] is! bool ||
      (v['selectionObserved'] == true) != (phase == 'rotated')) {
    _bad();
  }
  return AgentRotationCleanup(
    phase,
    _choice(v['cleanupState'], {'closedObserved', 'unknown'}),
    v['selectionObserved'] as bool,
  );
}
