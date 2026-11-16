// Causal UI fixture derived from the staged closed Native source shape.
// No actual Agent/FFI acceptance is claimed by these fake ports.
import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_association_wire.dart';

import 'agent_association_fixture.dart' as f;
export 'agent_association_fixture.dart' show scope;

Map<String, dynamic> rowWire() => {
  'associationId': f.associationId,
  'hostId': f.hostId,
  'hostRevision': f.selectedHost.revision,
  'agentId': f.nodeId,
  'nodeKeyFingerprint': 'a' * 64,
  'certificateDigest': 'c' * 64,
  'certificateExpiresUtc': '2026-11-15T12:00:00Z',
  'state': 'associated',
  'remoteRevocationObserved': false,
  'revocationState': 'notRequested',
};
AgentAssociationStatus capturedStatus() => parseAgentAssociationStatus(
  jsonEncode({
    ...f.statusWire(),
    'associations': [rowWire()],
  }),
  f.scope,
);
AgentAssociationRecord record() => capturedStatus().records.single;
Map<String, dynamic> wire({
  String phase = 'revocationReview',
  int remaining = 60000,
}) {
  final old = f.operationWire();
  return {
    'version': 1,
    'profileId': f.scope.profileId,
    'notificationScopeId': f.scope.nonce,
    'remainingMs': remaining,
    'phase': phase,
    'associationId': f.associationId,
    'hostId': f.hostId,
    'hostRevision': f.selectedHost.revision,
    'node': old['node'],
    'owner': old['owner'],
    'nodeContext': old['nodeContext'],
    'certificateDigest': 'c' * 64,
    'actorRevision': phase == 'revoked' ? '5' : '4',
    'checkpoint': phase == 'revoked' ? '8' : '7',
    'localBlocked': phase == 'revoked' ? true : false,
    'remoteRevocationObserved': phase == 'revoked',
    'actorAdmission': false,
    'cleanupState': 'pending',
  };
}

final class Handle implements AgentRevocationHandle {
  late AgentAssociationRecord original;
  int detailsCalls = 0, confirms = 0, cancels = 0, joins = 0, remaining = 60000;
  bool failConfirm = false;
  Exception? confirmFailure;
  Completer<AgentRevocationDetails>? pendingDetails, pendingConfirm;
  final cleanup = Completer<AgentRevocationCleanup>();
  AgentRevocationDetails value({String phase = 'revocationReview'}) =>
      parseAgentRevocationDetails(
        jsonEncode(wire(phase: phase, remaining: remaining)),
        f.scope,
        original,
      );
  @override
  Future<AgentRevocationDetails> details() async {
    detailsCalls++;
    return pendingDetails == null ? value() : await pendingDetails!.future;
  }

  @override
  Future<AgentRevocationDetails> confirmOnce() async {
    confirms++;
    if (confirmFailure != null) {
      throw confirmFailure!;
    }
    if (failConfirm) {
      throw StateError('owned lost outcome');
    }
    return pendingConfirm == null
        ? value(phase: 'revoked')
        : await pendingConfirm!.future;
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<AgentRevocationCleanup> joined() {
    joins++;
    return cleanup.future;
  }

  void finishCleanup({bool observed = false}) {
    if (!cleanup.isCompleted) {
      cleanup.complete(
        AgentRevocationCleanup(
          observed ? 'revoked' : 'unknown',
          'closedObserved',
          observed,
        ),
      );
    }
  }
}

class Port implements AgentAssociationPort, AgentAssociationRevocationPort {
  final base = f.Port();
  final issued = Handle();
  int statuses = 0, prepares = 0;
  Completer<AgentRevocationHandle>? pendingPrepare;
  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope s) async {
    statuses++;
    return statusValue;
  }

  AgentAssociationStatus get statusValue => capturedStatus();
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(
    RuntimeNoticeScope s,
  ) => base.prepareClientIdentity(s);
  @override
  Future<AgentAssociationHandle> registerProbe(
    RuntimeNoticeScope s,
    AgentAssociationHost host,
  ) => base.registerProbe(s, host);
  @override
  Future<AgentRevocationHandle> prepareRevocation(
    RuntimeNoticeScope s,
    AgentAssociationRecord record,
  ) async {
    prepares++;
    issued.original = record;
    return pendingPrepare == null ? issued : await pendingPrepare!.future;
  }
}

class Legacy implements AgentAssociationPort {
  final port = Port();
  @override
  Future<AgentAssociationStatus> status(RuntimeNoticeScope s) => port.status(s);
  @override
  Future<AgentClientIdentityHandle> prepareClientIdentity(
    RuntimeNoticeScope s,
  ) => port.prepareClientIdentity(s);
  @override
  Future<AgentAssociationHandle> registerProbe(
    RuntimeNoticeScope s,
    AgentAssociationHost host,
  ) => port.registerProbe(s, host);
}
