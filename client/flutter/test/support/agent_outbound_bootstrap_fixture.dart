import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_outbound_bootstrap.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap.dart';

import 'agent_association_fixture.dart' as association;

final scope = association.scope;
const associationId = '90000000-0000-4000-8000-000000000009';
final record = AgentAssociationRecord(
  associationId: associationId,
  hostId: association.selectedHost.id,
  agentId: '20000000-0000-4000-8000-000000000002',
  phase: AgentAssociationPhase.associated,
  hostRevision: association.selectedHost.revision,
  nodeKeyFingerprint: '11' * 32,
  certificateDigest: '22' * 32,
  nodeKeyEpoch: '1',
  purpose: AgentAssociationPurpose.tunnel,
);

/// Synthetic shape taken from selected Native bbc65762 view producer and Context.publish.
/// Not an actual Agent/SSH response, carrier activation, Actor admission or FFI capture.
Map<String, Object?> wire({int ttl = 3600, String phase = 'bootstrapReview'}) => {
  'version': 1,
  'profileId': scope.profileId,
  'notificationScopeId': scope.nonce,
  'phase': phase,
  'associationId': record.id,
  'node': {'agentId': record.agentId, 'keyEpoch': '1', 'publicKey': '33' * 32, 'keyFingerprint': '11' * 32},
  'ownerAccountId': '30000000-0000-4000-8000-000000000003',
  'requestedRegistrationLifetimeSeconds': ttl,
  'maximumRegistrationLifetimeSeconds': ttl,
  'serverSessionMayShorten': true,
  'reviewLifetimeSeconds': 300,
  'controlAttachmentMaximumSeconds': 300,
  'registrationMaximumSeconds': 86400,
  'registrationGenerationCap': 64,
  'registrationCiphertextAllocationBytes': 536870912,
  'carrierOnly': true,
  'nodeAdmission': false,
  'autoRenew': false,
  'controlId': phase == 'configured' ? '40000000-0000-4000-8000-000000000004' : null,
  'registrationExpiresAt': phase == 'configured' ? 1893459600 : null,
  'remainingMs': 60000,
  'cleanupState': phase == 'configured' ? 'closedObserved' : 'notStarted',
};
String cleanup([String state = 'closedObserved']) => jsonEncode({
  'version': 1,
  'phase': 'bootstrapReview',
  'carrierOnly': true,
  'nodeAdmission': false,
  'cleanupState': state,
});

final class NativeHandle implements AgentOutboundBootstrapNativeHandle {
  final calls = <String>[];
  int ttl = 3600;
  Completer<String>? pendingDetails, pendingApproval, pendingJoin;
  String joinState = 'closedObserved';
  @override
  Future<String> details() async {
    calls.add('details');
    return pendingDetails?.future ?? jsonEncode(wire(ttl: ttl));
  }

  @override
  Future<String> approveOnce() async {
    calls.add('approveOnce');
    return pendingApproval?.future ?? jsonEncode(wire(ttl: ttl, phase: 'configured'));
  }

  @override
  void cancel() {
    calls.add('cancel');
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return pendingJoin?.future ?? cleanup(joinState);
  }

  @override
  void dispose() {
    calls.add('dispose');
  }
}

final class Calls {
  Calls(this.handle);
  final NativeHandle handle;
  final requests = <Map<String, dynamic>>[];
  Completer<AgentOutboundBootstrapNativeHandle>? pendingPrepare;
  Future<AgentOutboundBootstrapNativeHandle> prepare(String request) async {
    final m = jsonDecode(request) as Map<String, dynamic>;
    requests.add(m);
    handle.ttl = m['ttlSeconds'] as int;
    return pendingPrepare?.future ?? handle;
  }

  AgentOutboundBootstrapPort get port => RustAgentOutboundBootstrapPort(prepareOriginal: prepare);
}
