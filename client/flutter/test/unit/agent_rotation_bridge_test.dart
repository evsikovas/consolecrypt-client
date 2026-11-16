import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_association.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';
import 'package:consolecrypt/monitoring/agent_rotation_controller.dart';
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_rotation_fixture.dart' as f;

// Actual generated endpoints/opaque type, synthetic metadata; no DLL/SSH.
class _Handle extends Fake implements native.AgentNodeRotationHandle {
  final calls = <String>[];
  bool reconcile = false;
  BridgeError? cancelFailure;
  Completer<String>? pendingDetails;
  @override
  Future<String> details() async {
    calls.add('details');
    return pendingDetails?.future ??
        jsonEncode(
          f.details(phase: reconcile ? 'rotationReconcileReview' : 'rotationPreparationReview', reconcile: reconcile),
        );
  }

  @override
  Future<String> prepareCandidateOnce() async {
    calls.add('candidate');
    return jsonEncode(f.details(phase: 'rotationCertificateReview'));
  }

  @override
  Future<String> confirmOnce() async {
    calls.add('confirm');
    return jsonEncode(f.details(phase: 'rotated', reconcile: reconcile));
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return '{"version":1,"phase":"unknown","selectionObserved":false,"cleanupState":"closedObserved"}';
  }

  @override
  void cancel() {
    calls.add('cancel');
    if (cancelFailure case final error?) throw error;
  }
}

class _Api extends Fake implements RustLibApi {
  final original = _Handle(), replacement = _Handle();
  _Handle? next;
  final actions = <String>[], requests = <Map<String, dynamic>>[];
  BridgeError? failure;
  @override
  Future<native.AgentNodeRotationHandle> crateApiAgentNodeAssociationAgentNodeAssociationRotationPrepare({
    required String requestJson,
  }) async {
    actions.add('rotate');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    if (failure case final error?) throw error;
    return next ?? original;
  }

  @override
  Future<native.AgentNodeRotationHandle> crateApiAgentNodeAssociationAgentNodeAssociationRotationReconcilePrepare({
    required String requestJson,
  }) async {
    actions.add('reconcile');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    original.reconcile = true;
    return original;
  }
}

AgentAssociationRotationPort production() =>
    (const RustMonitoringBackend().createService() as AgentAssociationCapability).agentAssociationPort!
        as AgentAssociationRotationPort;
void main() {
  late _Api api;
  setUp(() {
    api = _Api();
    RustLib.initMock(api: api);
    vaultKeyAlarm.value = null;
  });
  tearDown(() {
    RustLib.dispose();
    vaultKeyAlarm.value = null;
  });
  test(
    'actual service factory stays lazy, preserves Reader/revoke, and every rotation call uses original issued handle',
    () async {
      final service = const RustMonitoringBackend().createService(),
          port = (service as AgentAssociationCapability).agentAssociationPort!;
      expect(service, isA<AgentReaderCapability>());
      expect(port, isA<RustAgentAssociationPort>());
      expect(port, isA<AgentAssociationRevocationPort>());
      final p = port as AgentAssociationRotationPort;
      expect(api.actions, isEmpty);
      final s = f.scope(), r = f.record(), held = await p.prepareRotation(s, r);
      api.next = api.replacement;
      final initial = await held.details(),
          candidate = await held.prepareCandidateOnce(),
          result = await held.confirmOnce();
      held.cancel();
      await held.joined();
      expect(api.requests.single, {
        'version': 1,
        'profileId': s.profileId,
        'notificationScopeId': s.nonce,
        'associationId': r.id,
      });
      expect(api.actions, ['rotate']);
      expect(api.original.calls, ['details', 'candidate', 'confirm', 'cancel', 'joined']);
      expect(api.replacement.calls, isEmpty);
      expect(initial.phase, 'rotationPreparationReview');
      expect(candidate.phase, 'rotationCertificateReview');
      expect(result.selectionObserved, true);
    },
  );
  test('actual reconcile factory is separate and never invokes candidate endpoint', () async {
    final s = f.scope(), r = f.record(reconcile: true), port = production();
    final held = await port.prepareRotationReconcile(s, r);
    expect((await held.details()).phase, 'rotationReconcileReview');
    expect((await held.confirmOnce()).phase, 'rotated');
    expect(api.actions, ['reconcile']);
    expect(api.original.calls, ['details', 'confirm']);
    held.cancel();
    await held.joined();
  });
  test(
    'actual pending details cancel is immediate, stale metadata cannot publish, vault mismatch survives both lanes',
    () async {
      final s = f.scope(), r = f.record(), port = production();
      api.original.pendingDetails = Completer<String>();
      final c = AgentRotationController(
        port: port,
        scope: s,
        record: r,
        mode: AgentRotationMode.rotate,
        original: () => true,
      );
      addTearDown(c.dispose);
      final pending = c.prepare();
      await Future<void>.delayed(Duration.zero);
      c.cancel();
      expect(api.original.calls, contains('cancel'));
      api.original.pendingDetails!.complete(jsonEncode(f.details()));
      await pending;
      expect(c.details, isNull);
      expect(c.result, isNull);
      api.failure = const BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE', details: {});
      await expectLater(
        port.prepareRotation(s, r),
        throwsA(isA<AppException>().having((v) => v.code, 'code', AppErrorCode.vaultKeyMismatch)),
      );
      expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
      vaultKeyAlarm.value = null;
      api.failure = null;
      final held = await port.prepareRotation(s, r);
      api.original.cancelFailure = const BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE', details: {});
      expect(held.cancel, throwsA(isA<AppException>().having((v) => v.code, 'code', AppErrorCode.vaultKeyMismatch)));
      expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
    },
  );
}
