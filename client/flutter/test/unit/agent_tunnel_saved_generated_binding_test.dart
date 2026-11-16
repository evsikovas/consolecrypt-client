import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_tunnel_saved_route_generated.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_saved_route_port.dart';
import 'package:consolecrypt/src/rust/api/agent_tunnel_route.dart' as native;
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import 'agent_tunnel_route_test.dart' as f;
import 'agent_tunnel_saved_route_test.dart' as s;

// Actual new generated API/interface with synthetic payloads; no real FFI,
// Native ticket claim, signed grant or successful route retention is proved.
final class _Handle extends Fake implements native.AgentTunnelRouteHandle {
  final original = f.NativeHandle()..detailsReply = jsonEncode(s.savedView());
  @override Future<String> details() => original.details();
  @override Future<String> approveOnce() => original.approveOnce();
  @override Future<String> stop() => original.stop();
  @override void cancel() => original.cancel();
  @override Future<String> joined() => original.joined();
  @override void dispose() => original.dispose();
  @override Future<String> detachAutoOpen() async {
    original.calls.add('detach'); return jsonEncode(s.detachWire());
  }
}
final class _Api extends Fake implements RustLibApi {
  final calls = <String>[], requests = <Map<String, dynamic>>[];
  final handle = _Handle();
  bool found = true;
  BridgeError? failure;
  @override
  Future<native.AgentTunnelRouteHandle?> crateApiAgentTunnelRouteAgentTunnelRouteAttachSaved({required String requestJson}) async {
    calls.add('attach'); requests.add(jsonDecode(requestJson) as Map<String, dynamic>);
    if (failure case final error?) {
      throw error;
    }
    return found ? handle : null;
  }
  @override
  Future<native.AgentTunnelRouteHandle> crateApiAgentTunnelRouteAgentTunnelRoutePrepare({required String requestJson}) async {
    calls.add('prepare'); requests.add(jsonDecode(requestJson) as Map<String, dynamic>); return handle;
  }
}
void main() {
  test('actual Main Saved factory is lazy and keeps an empty distinct cache per service', () {
    final future = createGeneratedSavedTunnelRoutePort();
    expect(future, isA<AgentTunnelSavedRoutePort>());
    expect(future.savedIntents.restore(f.scope, s.savedIntent().record), isNull);
    final service = const RustMonitoringBackend().createService() as AgentTunnelRouteCapability;
    final selected = service.agentTunnelRoutePort as AgentTunnelSavedRoutePort;
    final other = const RustMonitoringBackend().createService() as AgentTunnelRouteCapability;
    final second = other.agentTunnelRoutePort as AgentTunnelSavedRoutePort;
    expect(selected.savedIntents, isNot(same(second.savedIntents)));
    expect(selected.savedIntents.restore(f.scope, s.savedIntent().record), isNull);
    expect(second.savedIntents.restore(f.scope, s.savedIntent().record), isNull);
  });
  group('actual private FRB208 Saved dispatch', () {
    late _Api api;
    setUp(() { api = _Api(); RustLib.initMock(api: api); vaultKeyAlarm.value = null; });
    tearDown(() { RustLib.dispose(); vaultKeyAlarm.value = null; });
    test('new attach function and new opaque detach method preserve same original without prepare or approve', () async {
      final port = createGeneratedSavedTunnelRoutePort(), intent = s.savedIntent();
      final handle = await port.attachSaved(f.scope, intent);
      expect(api.requests.single, intent.request(f.scope)); expect(api.calls, ['attach']);
      final view = await handle!.details();
      final receipt = await (handle as AgentTunnelDetachableRouteHandle).detachAutoOpen(view);
      expect(receipt.cleanupState.name, 'pending'); expect(receipt.remoteDrainObserved, isFalse);
      handle.dispose();
      expect(api.handle.original.calls, ['details', 'detach', 'dispose']);
      expect(api.calls, ['attach']);
    });
    test('new nullable None and guarded error never dispatch the existing prepare as fallback', () async {
      final port = createGeneratedSavedTunnelRoutePort(); api.found = false;
      expect(await port.attachSaved(f.scope, s.savedIntent()), isNull);
      expect(api.calls, ['attach']);
      api.failure = const BridgeError(code: 'vault_key_mismatch', message: 'PRIVATE', details: {});
      await expectLater(port.attachSaved(f.scope, s.savedIntent()),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.vaultKeyMismatch)));
      expect(api.calls, ['attach', 'attach']); expect(api.handle.original.calls, isEmpty);
    });
  });
}
