import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_route.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_wire.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:flutter_test/flutter_test.dart';

import 'agent_tunnel_route_test.dart' as f;

AgentTunnelResumeQuery query({int port = 9090}) => AgentTunnelResumeQuery(f.intent().record, '::1', port);
String reply(AgentTunnelResumeQuery q, {bool enabled = false}) => jsonEncode({
  'version': 1,
  'profileId': f.scope.profileId,
  'notificationScopeId': f.scope.nonce,
  'associationId': q.record.id,
  'destination': {'protocol': 'tcp', 'ip': q.ip, 'port': q.port},
  'resumeOriginal': enabled,
});

class Reader implements AgentTunnelResumePreferencePort {
  int calls = 0;
  Completer<AgentTunnelResumePreference>? pending;
  @override
  Future<AgentTunnelResumePreference> readResumePreference(RuntimeNoticeScope scope, AgentTunnelResumeQuery q) async {
    calls++;
    return pending?.future ?? AgentTunnelResumePreference(q, true);
  }
}

AgentTunnelRouteIntent choice(bool enabled) {
  final x = f.intent();
  return AgentTunnelRouteIntent(
    record: x.record,
    listenIp: x.listenIp,
    listenPort: x.listenPort,
    destinationIp: x.destinationIp,
    destinationPort: x.destinationPort,
    ttlSeconds: x.ttlSeconds,
    idleSeconds: x.idleSeconds,
    resumeOriginal: enabled,
  );
}

void main() {
  test('closed metadata read matches original opaque frame, selected association and canonical destination', () {
    final q = query();
    expect(q.request(f.scope), {
      'version': 1,
      'profileId': f.scope.profileId,
      'notificationScopeId': f.scope.nonce,
      'associationId': f.association,
      'destinationIp': '::1',
      'destinationPort': 9090,
    });
    expect(parseAgentTunnelResumePreference(reply(q), f.scope, q).resumeOriginal, false);
    for (final mutate in <void Function(Map<String, dynamic>)>[
      (v) => v['resumeOriginal'] = null,
      (v) => v['resumeOriginal'] = 1,
      (v) => v['version'] = 1.0,
      (v) => v['notificationScopeId'] = 'other',
      (v) => v['associationId'] = f.device,
      (v) => (v['destination'] as Map)['port'] = 9091,
      (v) => (v['destination'] as Map)['protocol'] = 'udp',
      (v) => v['approved'] = true,
    ]) {
      final bad = jsonDecode(reply(q)) as Map<String, dynamic>;
      mutate(bad);
      expect(() => parseAgentTunnelResumePreference(jsonEncode(bad), f.scope, q), throwsFormatException);
    }
  });
  test('route preparation defaults off and signed view must equal the exact selected bool in both directions', () {
    expect(f.intent().request(f.scope)['resumeOriginal'], false);
    for (final enabled in [false, true]) {
      final raw = f.view()..['resumeOriginalSignedOptIn'] = enabled;
      expect(parseAgentTunnelRouteView(jsonEncode(raw), f.scope, choice(enabled)).resumeOriginalSignedOptIn, enabled);
      expect(() => parseAgentTunnelRouteView(jsonEncode(raw), f.scope, choice(!enabled)), throwsFormatException);
    }
    expect(
      () =>
          parseAgentTunnelRouteView(jsonEncode(f.view()..['resumeOriginalSignedOptIn'] = null), f.scope, choice(false)),
      throwsFormatException,
    );
  });
  test('Native35 transport capability is typed independently of exact signed consent', () {
    for (final capability in [false, true]) {
      for (final selected in [false, true]) {
        final raw = f.view()
          ..['resumeTransportImplemented'] = capability
          ..['resumeOriginalSignedOptIn'] = selected;
        final actual = parseAgentTunnelRouteView(jsonEncode(raw), f.scope, choice(selected));
        expect(actual.resumeTransportImplemented, capability);
        expect(actual.resumeOriginalSignedOptIn, selected);
        expect(actual.remoteDrainObserved, false);
        expect(() => parseAgentTunnelRouteView(jsonEncode(raw), f.scope, choice(!selected)), throwsFormatException);
      }
    }
    for (final invalid in [null, 1, 'true']) {
      final raw = f.view()..['resumeTransportImplemented'] = invalid;
      expect(() => parseAgentTunnelRouteView(jsonEncode(raw), f.scope, choice(false)), throwsFormatException);
    }
  });
  test('preference controller is inert and explicit read returns data without starting a route', () async {
    final port = Reader();
    final owner = AgentTunnelResumePreferenceController(port: port, scope: f.scope, original: () => true);
    final q = query();
    owner.select(q);
    expect(port.calls, 0);
    await owner.read();
    expect(port.calls, 1);
    expect(owner.value!.resumeOriginal, true);
    owner.dispose();
  });
  test('destination switch discards pending result and prevents another read overtaking it', () async {
    final port = Reader()..pending = Completer();
    final owner = AgentTunnelResumePreferenceController(port: port, scope: f.scope, original: () => true),
        old = query(),
        next = query(port: 9091);
    owner.select(old);
    final pending = owner.read();
    owner.select(next);
    await owner.read();
    expect(port.calls, 1);
    port.pending!.complete(AgentTunnelResumePreference(old, true));
    await pending;
    expect(owner.value, isNull);
    port.pending = null;
    await owner.read();
    expect(port.calls, 2);
    expect(owner.value!.query.same(next), true);
    owner.dispose();
  });
  test('original retirement hides a late read and never automatically retries', () async {
    var current = true;
    final port = Reader()..pending = Completer();
    final owner = AgentTunnelResumePreferenceController(port: port, scope: f.scope, original: () => current),
        q = query();
    owner.select(q);
    final pending = owner.read();
    current = false;
    owner.retire();
    current = true;
    port.pending!.complete(AgentTunnelResumePreference(q, true));
    await pending;
    expect(owner.value, isNull);
    await owner.read();
    expect(port.calls, 1);
    owner.dispose();
  });
  test(
    'optional callback adapter delegates original route port and preserves the vault alarm on metadata read failure',
    () async {
      final calls = f.Calls(f.NativeHandle()), q = query();
      var reads = 0;
      final inner = RustAgentTunnelRoutePort(calls: calls);
      final port = RustAgentTunnelRouteWithResumePreference(
        original: inner,
        readJson: (raw) async {
          reads++;
          expect(jsonDecode(raw), q.request(f.scope));
          throw const BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE', details: {});
        },
      );
      expect(reads, 0);
      vaultKeyAlarm.value = null;
      await expectLater(
        port.readResumePreference(f.scope, q),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)),
      );
      expect(reads, 1);
      expect(calls.requests, isEmpty);
      expect(vaultKeyAlarm.value, isNotNull);
      vaultKeyAlarm.value = null;
    },
  );
}
