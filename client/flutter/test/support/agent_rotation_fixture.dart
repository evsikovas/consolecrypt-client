import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';

// Synthetic UI protocol cases only. Never Native/Actor/SSH acceptance evidence.
const profileId = '10000000-0000-4000-8000-000000000001',
    associationId = '20000000-0000-4000-8000-000000000001',
    hostId = '30000000-0000-4000-8000-000000000001',
    agentId = '40000000-0000-4000-8000-000000000001',
    deviceId = '50000000-0000-4000-8000-000000000001',
    deploymentId = '60000000-0000-4000-8000-000000000001';
const nonce = 'original opaque rotation ñ';
RuntimeNoticeScope scope() => RuntimeNoticeScope.fromProfile({
  'id': profileId,
  'active': true,
  'vault_state': 'unlocked',
  'notification_scope_id': nonce,
})!;
AgentAssociationRecord record({bool reconcile = false}) => AgentAssociationRecord(
  associationId: associationId,
  hostId: hostId,
  agentId: agentId,
  phase: reconcile ? AgentAssociationPhase.blocked : AgentAssociationPhase.associated,
  hostRevision: '9007199254740993',
  nodeKeyFingerprint: 'b' * 64,
  nodeKeyEpoch: '1',
  rotationState: reconcile ? 'unknown' : 'notRequested',
  revocationState: 'notRequested',
  remoteRevocationObserved: false,
  certificateDigest: 'c' * 64,
  certificateExpiresUtc: DateTime.utc(2030),
);
Map<String, dynamic> node({bool proposed = false}) => {
  'agentId': agentId,
  'keyEpoch': proposed ? '2' : '1',
  'publicKey': (proposed ? 'd' : 'a') * 64,
  'keyFingerprint': (proposed ? 'e' : 'b') * 64,
};
Map<String, dynamic> details({
  String phase = 'rotationPreparationReview',
  int remaining = 60000,
  bool reconcile = false,
}) {
  final candidate = phase != 'rotationPreparationReview', selected = phase == 'rotated';
  return {
    'version': 1,
    'profileId': profileId,
    'notificationScopeId': nonce,
    'remainingMs': remaining,
    'phase': phase,
    'associationId': associationId,
    'hostId': hostId,
    'hostRevision': '9007199254740993',
    'owner': {'kind': 'localProfile', 'id': profileId, 'deviceId': deviceId, 'signingKeySha256': 'f' * 64},
    'nodeContext': {'kind': 'localProfile', 'id': hostId, 'deploymentId': deploymentId},
    'previousNode': node(),
    'node': selected
        ? node(proposed: true)
        : reconcile
        ? null
        : node(),
    'proposedNode': candidate ? node(proposed: true) : null,
    'rotationId': candidate ? 'a' * 32 : null,
    'actorRevision': selected || reconcile ? '9007199254740994' : '9007199254740993',
    'checkpoint': selected || reconcile ? '9007199254740994' : '9007199254740993',
    'certificateDigest': selected || reconcile ? 'd' * 64 : 'c' * 64,
    'certificateExpiresUtc': '2030-01-01T00:00:00Z',
    'scope': 'tunnel.open',
    'capabilitiesUnchanged': true,
    'expiryExtension': false,
    'selectionObserved': selected,
    'localBlocked': selected ? false : candidate,
    'actorAdmission': false,
    'cleanupState': selected ? 'closedObserved' : 'pending',
  };
}

AgentRotationDetails parsed(
  RuntimeNoticeScope scope,
  AgentAssociationRecord record, {
  String phase = 'rotationPreparationReview',
  int remaining = 60000,
  bool reconcile = false,
}) => parseAgentRotationDetails(
  jsonEncode(details(phase: phase, remaining: remaining, reconcile: reconcile)),
  scope,
  record,
);
