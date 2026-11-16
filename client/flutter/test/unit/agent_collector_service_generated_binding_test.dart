import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_generated.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_controller.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:consolecrypt/src/rust/api/agent_collector_service.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_collector_service_credentials_fixture.dart' as c;
import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../support/agent_collector_service_lifetime_fixture.dart' as l;
import '../support/agent_collector_service_revocation_fixture.dart' as r;
import 'agent_collector_service_status_test.dart' show statusWire;

// Actual emitted FRB interfaces and dispatch functions; synthetic payloads and
// generated-handle doubles only. No FFI, signed Node ACK or service proof.
final class _Service extends Fake implements native.AgentCollectorServiceHandle {
  _Service(this.original);
  final AgentCollectorServiceNativeHandle original;
  @override
  Future<String> details() => original.details();
  @override
  Future<String> confirmConfiguration() => original.confirmConfiguration();
  @override
  void cancel() => original.cancel();
  @override
  Future<String> joined() => original.joined();
  @override
  void dispose() => original.dispose();
}

final class _Revoke extends Fake implements native.AgentCollectorServiceRevocationHandle {
  _Revoke(this.original);
  final r.RevokeNative original;
  @override
  Future<String> details() => original.details();
  @override
  Future<String> confirmRevocation() => original.confirmRevocation();
  @override
  void cancel() => original.cancel();
  @override
  Future<String> joined() => original.joined();
  @override
  void dispose() => original.dispose();
}

final class _Api extends Fake implements RustLibApi {
  final requests = <String, List<Map<String, dynamic>>>{};
  final singleInner = f.NativeHandle(), lifetimeInner = l.LifetimeNative();
  late _Service single = _Service(singleInner);
  late final lifetime = _Service(lifetimeInner);
  late final revokeInner = r.RevokeNative(
    parseAgentCollectorServiceStatus(jsonEncode(statusWire()), f.scope, f.record),
  );
  late final revoke = _Revoke(revokeInner);
  Completer<native.AgentCollectorServiceHandle>? pending;
  BridgeError? failure;
  void record(String operation, String raw) {
    requests.putIfAbsent(operation, () => []).add(jsonDecode(raw) as Map<String, dynamic>);
    if (failure case final error?) {
      throw error;
    }
  }

  @override
  Future<native.AgentCollectorServiceHandle> crateApiAgentCollectorServiceAgentCollectorServicePrepare({
    required String requestJson,
  }) async {
    record('prepare', requestJson);
    return pending?.future ?? single;
  }

  @override
  Future<String> crateApiAgentCollectorServiceAgentCollectorServiceCatalog({required String requestJson}) async {
    record('catalog', requestJson);
    return f.catalogReply();
  }

  @override
  Future<String> crateApiAgentCollectorServiceAgentCollectorServiceStatus({required String requestJson}) async {
    record('status', requestJson);
    return jsonEncode(statusWire());
  }

  @override
  Future<native.AgentCollectorServiceRevocationHandle> crateApiAgentCollectorServiceAgentCollectorServiceRevokePrepare({
    required String requestJson,
  }) async {
    record('revoke', requestJson);
    return revoke;
  }

  @override
  Future<String> crateApiAgentCollectorServiceAgentCollectorServiceCredentialChoices({
    required String requestJson,
  }) async {
    record('choices', requestJson);
    return jsonEncode(c.choicesWire());
  }

  @override
  Future<native.AgentCollectorServiceHandle> crateApiAgentCollectorServiceAgentCollectorServiceLifetimePrepare({
    required String requestJson,
  }) async {
    record('lifetime', requestJson);
    return lifetime;
  }
}

void main() {
  test(
    'actual Main collector-service capability constructs lazily without Native initialization',
    () {
      final port = createGeneratedAgentCollectorServicePort();
      expect(port, isNotNull);
      final service = const RustMonitoringBackend().createService() as AgentCollectorServiceCapability;
      expect(service.agentCollectorServicePort, isNotNull);
    },
  );
  group('actual generated private ABI dispatch', () {
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
      'explicit readonly catalog status and credential choices use three actual functions with exact frame',
      () async {
        final port = createGeneratedAgentCollectorServicePort();
        expect(api.requests, isEmpty);
        await port.readCatalog(f.scope, f.record);
        final status = await port.readSavedStatus(f.scope, f.record);
        await port.readCredentialChoices(f.scope);
        final frame = {'version': 1, 'profileId': f.scope.profileId, 'notificationScopeId': f.scope.nonce};
        expect(api.requests['catalog']!.single, {...frame, 'associationId': f.record.id});
        expect(api.requests['status']!.single, {...frame, 'associationId': f.record.id});
        expect(api.requests['choices']!.single, frame);
        expect(api.requests.keys, unorderedEquals(['catalog', 'status', 'choices']));
        expect(status.runtimeObserved, isFalse);
        expect(api.singleInner.calls, isEmpty);
      },
    );
    test('all issued configuration methods retain the original generated handle without another free call', () async {
      final port = createGeneratedAgentCollectorServicePort(), intent = f.intent();
      final issued = await port.prepare(f.scope, intent);
      final replacement = f.NativeHandle();
      api.single = _Service(replacement);
      expect(api.requests['prepare']!.single, intent.request(f.scope));
      await issued.details();
      await issued.confirmConfiguration();
      issued.cancel();
      await issued.joined();
      issued.dispose();
      expect(api.singleInner.calls, ['details', 'confirmConfiguration', 'cancel', 'joined', 'dispose']);
      expect(replacement.calls, isEmpty);
      expect(api.requests.keys, ['prepare']);
    });
    test('lifetime uses only its actual distinct prepare and same no-argument generated confirmation', () async {
      final port = createGeneratedAgentCollectorServicePort(), intent = l.lifetimeIntent();
      expect(() => port.prepare(f.scope, intent), throwsFormatException);
      expect(api.requests, isEmpty);
      final issued = await port.prepareLifetime(f.scope, intent);
      expect(api.requests['lifetime']!.single, intent.request(f.scope));
      final details = await issued.details();
      expect(details.lifetimeCredentials!.purpose, l.purpose);
      final configured = await issued.confirmConfiguration();
      expect(configured.runtimeObserved, isFalse);
      await issued.joined();
      issued.dispose();
      expect(api.lifetimeInner.calls, ['details', 'confirmConfiguration', 'joined', 'dispose']);
      expect(api.singleInner.calls, isEmpty);
      expect(api.requests.keys, ['lifetime']);
    });
    test('targeted revoke uses actual separate opaque type and never configuration or runtime inference', () async {
      final port = createGeneratedAgentCollectorServicePort();
      final target = await port.readSavedStatus(f.scope, f.record);
      final issued = await port.prepareServiceRevocation(target);
      await issued.details();
      final result = await issued.confirmRevocation();
      issued.cancel();
      final cleanup = await issued.joined();
      issued.dispose();
      expect(result.remoteRevoked, isTrue); // Synthetic closed receipt metadata.
      expect(cleanup.remoteRevoked, isTrue);
      expect(api.revokeInner.calls, ['details', 'confirmRevocation', 'cancel', 'joined', 'dispose']);
      expect(api.requests.keys, unorderedEquals(['status', 'revoke']));
      expect(api.singleInner.calls, isEmpty);
    });
    test('late actual generated issuance after cancellation joins only original and never confirms', () async {
      api.pending = Completer<native.AgentCollectorServiceHandle>();
      final owner = AgentCollectorServiceController(
        port: createGeneratedAgentCollectorServicePort(),
        scope: f.scope,
        intent: f.intent(),
        original: () => true,
      );
      final preparing = owner.prepare();
      owner.cancel();
      final closing = owner.close();
      api.pending!.complete(api.single);
      await preparing;
      await closing;
      expect(api.singleInner.calls, ['cancel', 'joined', 'dispose']);
      expect(owner.view, isNull);
      expect(api.requests.keys, ['prepare']);
      owner.dispose();
    });
    test('generated error is guarded and cannot retry or fall back to another preparation mode', () async {
      api.failure = const BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE', details: {});
      final port = createGeneratedAgentCollectorServicePort();
      await expectLater(
        port.prepareLifetime(f.scope, l.lifetimeIntent()),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)),
      );
      expect(vaultKeyAlarm.value, isNotNull);
      expect(api.requests.keys, ['lifetime']);
      expect(api.singleInner.calls, isEmpty);
      expect(api.lifetimeInner.calls, isEmpty);
    });
  });
}
