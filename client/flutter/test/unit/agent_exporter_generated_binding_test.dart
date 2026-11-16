import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_exporter_generated.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_exporter.dart';
import 'package:consolecrypt/src/rust/api/agent_exporter.dart' as native;
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as association;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_exporter_fixture.dart' as f;

// Actual emitted interfaces with SYNTHETIC payloads. No real FFI or Agent proof.
final class _Handle extends Fake implements native.AgentExporterHandle {
  final f.Raw raw = f.Raw();
  @override
  Future<String> details() => raw.details();
  @override
  Future<String> approveOnce() => raw.approveOnce();
  @override
  Future<String> stop() => raw.stop();
  @override
  void cancel() => raw.cancel();
  @override
  Future<String> joined() => raw.joined();
  @override
  void dispose() => raw.dispose();
}

final class _Association extends Fake implements association.AgentNodeAssociationHandle {
  final List<String> calls = [];
  @override
  void cancel() {
    calls.add('cancel');
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return jsonEncode({'version': 1, 'phase': 'cancelled', 'cleanupState': 'closedObserved'});
  }
}

final class _Revocation extends Fake implements association.AgentNodeRevocationHandle {
  final List<String> calls = [];
  @override
  void cancel() {
    calls.add('cancel');
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return jsonEncode({
      'version': 1,
      'phase': 'cancelled',
      'remoteRevocationObserved': false,
      'cleanupState': 'closedObserved',
    });
  }
}

final class _Api extends Fake implements RustLibApi {
  final List<String> calls = [];
  final List<Map<String, dynamic>> requests = [];
  final _Handle handle = _Handle();
  final _Association enrollment = _Association();
  final _Revocation revocation = _Revocation();
  BridgeError? failure;
  void record(String method, String requestJson) {
    calls.add(method);
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    if (failure case final error?) {
      throw error;
    }
  }

  @override
  Future<String> crateApiAgentExporterAgentExporterCatalog({required String requestJson}) async {
    record('catalog', requestJson);
    return jsonEncode(f.catalogWire());
  }

  @override
  Future<native.AgentExporterHandle> crateApiAgentExporterAgentExporterPrepare({required String requestJson}) async {
    record('prepare', requestJson);
    return handle;
  }

  @override
  Future<association.AgentNodeAssociationHandle> crateApiAgentNodeAssociationAgentExporterNodeAssociationRegister({
    required String requestJson,
  }) async {
    record('register-exporter', requestJson);
    return enrollment;
  }

  @override
  Future<association.AgentNodeRevocationHandle> crateApiAgentNodeAssociationAgentExporterNodeAssociationRevokePrepare({
    required String requestJson,
  }) async {
    record('revoke-exporter', requestJson);
    return revocation;
  }
}

void main() {
  test('actual Main exporter capability constructs lazily without Native initialization', () {
    final proposed = createGeneratedAgentExporterPort();
    expect(proposed, isA<AgentExporterPort>());
    expect(proposed, isA<AgentExporterEnrollmentPort>());
    final current = const RustMonitoringBackend().createService() as AgentExporterCapability;
    expect(current.agentExporterPort, isNotNull);
  });
  group('actual FRB233 generated exporter dispatch', () {
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
    test('catalog and finite prepare reach actual generated entries; issued calls retain same opaque object', () async {
      final port = createGeneratedAgentExporterPort();
      final catalog = await port.catalog(f.scope, f.record());
      final handle = await port.prepare(f.scope, f.record(), catalog, 120);
      expect(api.calls, ['catalog', 'prepare']);
      expect(api.requests[0], {
        'version': 1,
        'profileId': f.profile,
        'notificationScopeId': f.scope.nonce,
        'associationId': f.association,
      });
      expect(api.requests[1], {...api.requests[0], 'manifestSha256': catalog.manifestSha256, 'durationSeconds': 120});
      expect((await handle.details()).runtimeObserved, isFalse);
      expect((await handle.approveOnce()).runningVersion, isNull);
      await handle.stop();
      handle.cancel();
      await handle.joined();
      handle.dispose();
      expect(api.handle.raw.calls, ['details', 'approve', 'stop', 'cancel', 'joined', 'dispose']);
      expect(api.calls, ['catalog', 'prepare']);
    });
    test('ExporterOnly enrollment and revocation use distinct actual entries without scope or secret inputs', () async {
      final port = createGeneratedAgentExporterPort();
      final enrolled = await port.registerExporterProbe(
        f.scope,
        AgentAssociationHost(id: f.host, revision: f.record().hostRevision!, name: 'Synthetic Host'),
      );
      final revoked = await port.prepareExporterRevocation(f.scope, f.record());
      expect(api.requests, [
        {
          'version': 1,
          'profileId': f.profile,
          'notificationScopeId': f.scope.nonce,
          'hostId': f.host,
          'expectedHostRevision': f.record().hostRevision,
        },
        {'version': 1, 'profileId': f.profile, 'notificationScopeId': f.scope.nonce, 'associationId': f.association},
      ]);
      enrolled.cancel();
      await enrolled.joined();
      revoked.cancel();
      await revoked.joined();
      expect(api.enrollment.calls, ['cancel', 'joined']);
      expect(api.revocation.calls, ['cancel', 'joined']);
      expect(api.calls, ['register-exporter', 'revoke-exporter']);
      expect(api.handle.raw.calls, isEmpty);
    });
    test('actual generated error is mapped and never retries preparation or another authority endpoint', () async {
      final port = createGeneratedAgentExporterPort();
      api.failure = const BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE', details: {});
      await expectLater(
        port.catalog(f.scope, f.record()),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)),
      );
      expect(api.calls, ['catalog']);
      expect(api.handle.raw.calls, isEmpty);
    });
  });
}
