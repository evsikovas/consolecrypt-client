import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_route.dart';
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_saved_route.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/src/rust/api/agent_tunnel_route.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import 'agent_tunnel_route_test.dart' as f;

// Actual generated signatures and real production factory; synthetic metadata.
// No Native, TLS, Relay, Actor acceptance, or successful recovery is exercised.
final class _Api extends Fake implements RustLibApi {
  final calls = <String>[];
  final requests = <Map<String, dynamic>>[];
  final handle = f.NativeHandle();
  BridgeError? failure;
  bool foreignReply = false;
  @override
  Future<String> crateApiAgentTunnelRouteAgentTunnelResumePreference({required String requestJson}) async {
    calls.add('preference');
    final request = jsonDecode(requestJson) as Map<String, dynamic>;
    requests.add(request);
    if (failure case final error?) throw error;
    return jsonEncode({
      'version': 1,
      'profileId': foreignReply ? f.agent : f.scope.profileId,
      'notificationScopeId': f.scope.nonce,
      'associationId': f.association,
      'destination': {'protocol': 'tcp', 'ip': request['destinationIp'], 'port': request['destinationPort']},
      'resumeOriginal': true,
    });
  }

  @override
  Future<native.AgentTunnelRouteHandle> crateApiAgentTunnelRouteAgentTunnelRoutePrepare({
    required String requestJson,
  }) async {
    calls.add('prepare');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    handle.detailsReply = jsonEncode(f.view()..['resumeTransportImplemented'] = true);
    return handle;
  }

  @override
  Future<String> crateApiAgentTunnelRouteAgentTunnelAccessAudit({required String requestJson}) async {
    calls.add('audit');
    requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    return jsonEncode(f.auditWire());
  }
}

RustAgentTunnelRouteWithSavedOriginal productionPort() {
  final service = const RustMonitoringBackend().createService();
  return (service as AgentTunnelRouteCapability).agentTunnelRoutePort! as RustAgentTunnelRouteWithSavedOriginal;
}

void main() {
  test('actual production factory lazily exposes saved-preference capability without Native initialization', () {
    // No initMock: any hidden Native call would fail construction here.
    final port = productionPort();
    expect(port, isA<AgentTunnelResumePreferencePort>());
    expect(port.preferences, isA<RustAgentTunnelRouteWithResumePreference>());
    expect((port.preferences as RustAgentTunnelRouteWithResumePreference).original, isA<RustAgentTunnelRoutePort>());
  });
  group('actual generated production binding', () {
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
    test('explicit read encodes exact original frame and destination once without preparing a route', () async {
      final port = productionPort(), query = AgentTunnelResumeQuery(f.intent().record, '::1', 9090);
      expect(api.calls, isEmpty);
      final value = await port.readResumePreference(f.scope, query);
      expect(api.calls, ['preference']);
      expect(api.requests.single, query.request(f.scope));
      expect(value.resumeOriginal, true);
      expect(api.handle.calls, isEmpty);
      api.foreignReply = true;
      await expectLater(port.readResumePreference(f.scope, query), throwsFormatException);
      expect(api.calls, ['preference', 'preference']);
    });
    test('prepare audit and all issued methods retain original generated route and opaque handle', () async {
      final port = productionPort(), intent = f.intent();
      final handle = await port.prepare(f.scope, intent);
      expect(api.requests.single, intent.request(f.scope));
      final details = await handle.details();
      expect(details.resumeTransportImplemented, true);
      expect(details.resumeOriginalSignedOptIn, false);
      await handle.approveOnce();
      await handle.stop();
      handle.cancel();
      await handle.joined();
      handle.dispose();
      expect(api.handle.calls, ['details', 'approve', 'stop', 'cancel', 'joined', 'dispose']);
      final audit = await port.audit(f.scope);
      expect(audit.rows.length, 1);
      expect(api.calls, ['prepare', 'audit']);
      expect(api.requests.last, {'version': 1, 'profileId': f.scope.profileId, 'notificationScopeId': f.scope.nonce});
    });
    test('generated read failure keeps vault-key alarm and never starts or retries a route', () async {
      api.failure = const BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE', details: {});
      await expectLater(
        productionPort().readResumePreference(f.scope, AgentTunnelResumeQuery(f.intent().record, '127.0.0.1', 9090)),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)),
      );
      expect(vaultKeyAlarm.value, isNotNull);
      expect(api.calls, ['preference']);
      expect(api.handle.calls, isEmpty);
    });
  });
}
