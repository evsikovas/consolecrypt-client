import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_association_wire.dart';

const profileId = '10000000-0000-4000-8000-000000000001',
    hostId = '20000000-0000-4000-8000-000000000002',
    nodeId = '30000000-0000-4000-8000-000000000003',
    identityId = '40000000-0000-4000-8000-000000000004',
    associationId = '50000000-0000-4000-8000-000000000005';
final scope = RuntimeNoticeScope.fromProfile({
  'id': profileId,
  'active': true,
  'vault_state': 'unlocked',
  'notification_scope_id': 'original opaque 🔐',
})!;
final selectedHost = AgentAssociationHost(
  id: hostId,
  revision: '9007199254740993',
  name: 'Saved owned SSH host',
);
Map<String, dynamic> operationWire({
  String phase = 'offerReview',
  int remaining = 60000,
  int maximum = 2592000,
}) => {
  'version': 1,
  'profileId': profileId,
  'notificationScopeId': scope.nonce,
  'phase': phase,
  'hostId': hostId,
  'hostRevision': selectedHost.revision,
  'remainingMs': remaining,
  'node': {
    'agentId': nodeId,
    'keyEpoch': '1',
    'publicKey': 'd' * 64,
    'keyFingerprint': 'a' * 64,
  },
  'nodeContext': {
    'kind': 'localProfile',
    'id': '60000000-0000-4000-8000-000000000006',
    'deploymentId': '70000000-0000-4000-8000-000000000007',
  },
  'owner': {
    'kind': 'localProfile',
    'id': profileId,
    'deviceId': '80000000-0000-4000-8000-000000000008',
    'signingKeySha256': 'e' * 64,
  },
  'ownerEpoch': '18446744073709551615',
  'checkpoint': phase == 'offerReview'
      ? '1'
      : phase == 'claimReview'
      ? '2'
      : '3',
  'actorRevision': phase == 'offerReview'
      ? '0'
      : phase == 'claimReview'
      ? '2'
      : '3',
  'clientIdentity': {'identityId': identityId, 'certificateSha256': 'b' * 64},
  'associationId': phase == 'offerReview' ? null : associationId,
  'claimDigest': phase == 'offerReview' ? null : 'c' * 64,
  'certificateDigest': phase == 'associated' ? 'd' * 64 : null,
  'maxCertificateLifetimeSeconds': phase == 'claimReview' ? maximum : 0,
  'certificateExpiresUtc': phase == 'associated'
      ? '2026-11-15T12:00:00Z'
      : null,
  'actorAdmission': false,
  'cleanupState': 'pending',
};
Map<String, dynamic> statusWire({String state = 'storedMetadata'}) => {
  'version': 1,
  'profileId': profileId,
  'notificationScopeId': scope.nonce,
  'hosts': [
    {
      'hostId': hostId,
      'hostRevision': selectedHost.revision,
      'name': selectedHost.name,
    },
  ],
  'clientIdentity': {
    'state': state,
    'identityId': state == 'missing' ? null : identityId,
    'certificateSha256': state == 'missing' ? null : 'b' * 64,
  },
  'associations': <Object>[],
};
Map<String, dynamic> identityWire({bool stored = false}) => {
  'version': 1,
  'profileId': profileId,
  'notificationScopeId': scope.nonce,
  'phase': stored ? 'stored' : 'createReview',
  'owner': {
    'kind': 'localProfile',
    'id': profileId,
    'deviceId': '80000000-0000-4000-8000-000000000008',
    'signingKeySha256': 'e' * 64,
  },
  'algorithm': 'ecdsaP256Sha256',
  'validityDays': 30,
  'remainingMs': 60000,
  'identityId': stored ? identityId : null,
  'certificateSha256': stored ? 'b' * 64 : null,
  'cleanupState': 'notStarted',
};
AgentAssociationReview review({
  bool claim = false,
  int remaining = 60000,
  int maximum = 2592000,
}) => parseAgentAssociationReview(
  jsonEncode(
    operationWire(
      phase: claim ? 'claimReview' : 'offerReview',
      remaining: remaining,
      maximum: maximum,
    ),
  ),
  scope,
  selectedHost,
  claim
      ? AgentAssociationReviewStage.certificate
      : AgentAssociationReviewStage.offer,
);
AgentAssociationResult result() => parseAgentAssociationResult(
  jsonEncode(operationWire(phase: 'associated')),
  scope,
  selectedHost,
);

final class Handle implements AgentAssociationHandle {
  int probes = 0, offers = 0, certificates = 0, cancels = 0, joins = 0;
  final retired = Completer<void>();
  Completer<AgentAssociationReview>? pendingProbe, pendingOffer;
  Completer<AgentAssociationResult>? pendingCertificate;
  bool failOffer = false;
  int remaining = 60000, maximum = 2592000;
  int? lastLifetime;
  @override
  Future<AgentAssociationReview> probe() async {
    probes++;
    return pendingProbe == null
        ? review(remaining: remaining)
        : await pendingProbe!.future;
  }

  @override
  Future<AgentAssociationReview> approveOffer() async {
    offers++;
    if (failOffer) throw StateError('owned unknown transport');
    return pendingOffer == null
        ? review(claim: true, remaining: remaining, maximum: maximum)
        : await pendingOffer!.future;
  }

  @override
  Future<AgentAssociationResult> approveCertificate({
    required int lifetimeSeconds,
  }) async {
    certificates++;
    lastLifetime = lifetimeSeconds;
    return pendingCertificate == null
        ? result()
        : await pendingCertificate!.future;
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<AgentAssociationCleanup> joined() {
    joins++;
    return retired.future.then(
      (_) => const AgentAssociationCleanup('unknown', 'closedObserved'),
    );
  }
}

final class Port implements AgentAssociationPort {
  final issued = Handle();
  int statuses = 0, registrations = 0;
  Completer<AgentAssociationHandle>? pendingRegistration;
  String identityState = 'storedMetadata';
  int identityPrepares = 0;
  late final identity = IdentityHandle(this);
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(
    RuntimeNoticeScope requested,
  ) async {
    identityPrepares++;
    return identity;
  }

  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope requested) async {
    statuses++;
    return parseAgentAssociationStatus(
      jsonEncode(statusWire(state: identityState)),
      scope,
    );
  }

  @override
  Future<AgentAssociationHandle> registerProbe(
    RuntimeNoticeScope requested,
    AgentAssociationHost host,
  ) async {
    registrations++;
    return pendingRegistration == null
        ? issued
        : await pendingRegistration!.future;
  }
}

final class IdentityHandle implements AgentClientIdentityHandle {
  IdentityHandle(this.port);
  final Port port;
  int detailsCalls = 0, confirms = 0, cancels = 0, joins = 0;
  final retired = Completer<void>();
  Completer<AgentClientIdentityResult>? pending;
  @override
  Future<AgentClientIdentityReview> details() async {
    detailsCalls++;
    return parseAgentClientIdentityReview(jsonEncode(identityWire()), scope);
  }

  @override
  Future<AgentClientIdentityResult> confirmCreation() async {
    confirms++;
    if (pending != null) return pending!.future;
    port.identityState = 'storedMetadata';
    return parseAgentClientIdentityResult(
      jsonEncode(identityWire(stored: true)),
      scope,
    );
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<AgentAssociationCleanup> joined() {
    joins++;
    return retired.future.then(
      (_) => const AgentAssociationCleanup('stored', 'closedObserved'),
    );
  }
}
