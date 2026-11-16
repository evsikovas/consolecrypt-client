import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';

import 'agent_collector_service_fixture.dart' as v;
import 'agent_collector_service_flow_fixture.dart' as f;

/// Synthetic actual service_revoke.rs shape. No Node receipt/signature/FFI proof.
Map<String, dynamic> revocationWire(
  AgentCollectorServiceStatus target, {
  String phase = 'serviceRevokeReview',
  bool local = false,
  bool remote = false,
  int remaining = 60000,
}) {
  final source = v.serviceWire();
  return {
    'version': 1,
    'profileId': target.scope.profileId,
    'notificationScopeId': target.scope.nonce,
    'kind': 'finiteCollectorServiceRevocation',
    'phase': phase,
    'associationId': target.record.id,
    'serviceId': target.serviceId,
    'serviceHash': target.serviceHash,
    'hostId': target.record.hostId,
    'hostRevision': target.record.hostRevision,
    'owner': source['owner'],
    'node': source['node'],
    'remainingMs': remaining,
    'localBlocked': local,
    'remoteRevoked': remote,
    'runtimeState': 'notObserved',
    'lossUnknown': true,
    'deliveryGuaranteed': false,
    'cleanupState': 'pending',
  };
}

String cleanupWire({
  String phase = 'cancelled',
  bool local = false,
  bool remote = false,
  String state = 'closedObserved',
}) => jsonEncode({
  'version': 1,
  'phase': phase,
  'localBlocked': local,
  'remoteRevoked': remote,
  'runtimeState': 'notObserved',
  'lossUnknown': true,
  'deliveryGuaranteed': false,
  'cleanupState': state,
});

final class RevokeNative implements AgentCollectorServiceRevocationNativeHandle {
  RevokeNative(this.target);
  final AgentCollectorServiceStatus target;
  final calls = <String>[];
  Completer<String>? pendingDetails, pendingConfirm, pendingJoin;
  Object? confirmError;
  String phase = 'serviceRevokeReview';
  bool local = false, remote = false;
  @override
  Future<String> details() async {
    calls.add('details');
    return pendingDetails?.future ?? jsonEncode(revocationWire(target));
  }

  @override
  Future<String> confirmRevocation() async {
    calls.add('confirmRevocation');
    phase = 'revoking';
    if (confirmError != null) {
      phase = 'unknown';
      local = true;
      f.throwFixtureError(confirmError!);
    }
    if (pendingConfirm != null) {
      final raw = await pendingConfirm!.future, m = jsonDecode(raw) as Map<String, dynamic>;
      phase = m['phase'] as String;
      local = m['localBlocked'] as bool;
      remote = m['remoteRevoked'] as bool;
      return raw;
    }
    local = true;
    remote = true;
    phase = 'revoked';
    return jsonEncode(revocationWire(target, phase: phase, local: local, remote: remote));
  }

  @override
  void cancel() {
    calls.add('cancel');
    if (phase == 'serviceRevokeReview') phase = 'cancelled';
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return pendingJoin?.future ??
        cleanupWire(
          phase: phase,
          local: local,
          remote: remote,
          state: phase == 'revoking' ? 'unknown' : 'closedObserved',
        );
  }

  @override
  void dispose() {
    calls.add('dispose');
  }
}
