import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_controller.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_collector_service_fixture.dart' as v;
import '../support/agent_collector_service_flow_fixture.dart' as f;

AgentCollectorServiceController owner(f.Calls calls, {bool Function()? current, Duration Function()? elapsed}) =>
    AgentCollectorServiceController(
      port: calls.port,
      scope: f.scope,
      intent: f.intent(),
      original: current ?? () => true,
      elapsed: elapsed,
    );
void main() {
  test('actual readonly query and prepare have exact distinct requests; no callerTTL/path/authority and same opaque object', () async {
    final h = f.NativeHandle(), calls = f.Calls(h), port = calls.port;
    expect(calls.catalogRequests, isEmpty);
    expect(calls.prepareRequests, isEmpty);
    final catalog = await port.readCatalog(f.scope, f.record);
    expect(calls.catalogRequests.single, {
      'version': 1,
      'profileId': f.scope.profileId,
      'notificationScopeId': f.scope.nonce,
      'associationId': f.record.id,
    });
    expect(catalog.cleanupState, 'closedObserved');
    final intent = AgentCollectorServiceIntent(f.record, catalog.catalog), issued = await port.prepare(f.scope, intent);
    expect(calls.prepareRequests.single, intent.request(f.scope));
    expect(calls.prepareRequests.single.keys.toSet(), {
      'version',
      'profileId',
      'notificationScopeId',
      'associationId',
      'collectorId',
      'manifestSha256',
    });
    await issued.details();
    final configured = await issued.confirmConfiguration();
    expect(configured.configurationRecorded, isTrue);
    expect(configured.runtimeObserved, isFalse);
    issued.cancel();
    expect((await issued.joined()).serviceMayRemainActive, isTrue);
    issued.dispose();
    expect(h.calls, ['details', 'confirmConfiguration', 'cancel', 'joined', 'dispose']);
    final foreign = jsonDecode(f.catalogReply()) as Map<String, dynamic>;
    foreign['notificationScopeId'] = 'other';
    expect(() => parseAgentCollectorServiceCatalogView(jsonEncode(foreign), f.scope, f.record), throwsFormatException);
    final wrong = jsonDecode(f.joinedReply()) as Map<String, dynamic>;
    wrong['runtimeState'] = 'stopped';
    expect(() => parseAgentCollectorServiceCleanup(jsonEncode(wrong)), throwsFormatException);
  });
  test(
    'original countdown survives explicit details refresh; configuration ACK is one-use and not processRunning',
    () async {
      var elapsed = Duration.zero;
      final h = f.NativeHandle(), calls = f.Calls(h), c = owner(calls, elapsed: () => elapsed);
      expect(calls.prepareRequests, isEmpty);
      await c.prepare();
      elapsed = const Duration(seconds: 10);
      await c.refreshReview();
      expect(c.remainingMs, 50000);
      await c.confirmConfiguration();
      await c.confirmConfiguration();
      expect(c.view!.configurationRecorded, isTrue);
      expect(c.view!.runtimeObserved, isFalse);
      expect(c.cleanup!.state, 'closedObserved');
      expect(c.serviceMayRemainActive, isTrue);
      expect(h.calls, ['details', 'details', 'confirmConfiguration', 'joined', 'dispose']);
      await c.close();
      expect(c.unknown, isFalse);
      c.dispose();
    },
  );
  test('cancel during async details immediately hits original handle; late details cannot publish and unknown cleanup is not stop', () async {
    final h = f.NativeHandle()
      ..pendingDetails = Completer<String>()
      ..pendingJoin = Completer<String>();
    final c = owner(f.Calls(h)), preparing = c.prepare();
    await Future<void>.delayed(Duration.zero);
    c.cancel();
    expect(h.calls, ['details', 'cancel', 'joined']);
    expect(c.awaitingCleanup, isTrue);
    h.pendingDetails!.complete(jsonEncode(v.serviceWire()));
    await preparing;
    expect(c.view, isNull);
    h.pendingJoin!.complete(f.joinedReply(state: 'unknown'));
    await c.close();
    expect(c.cleanupUnknown, isTrue);
    expect(c.cleanup!.state, 'unknown');
    expect(h.calls.last, 'dispose');
    c.dispose();
  });
  test('late prepare is owned and close waits for exact late native cleanup', () async {
    final h = f.NativeHandle(), calls = f.Calls(h)..pendingPrepare = Completer<AgentCollectorServiceNativeHandle>();
    final c = owner(calls), pending = c.prepare();
    var closed = false;
    final closing = c.close().then((_) => closed = true);
    await Future<void>.delayed(Duration.zero);
    expect(closed, isFalse);
    calls.pendingPrepare!.complete(h);
    await pending;
    await closing;
    expect(h.calls, ['cancel', 'joined', 'dispose']);
    expect(c.view, isNull);
    c.dispose();
  });
  test('uncertain confirm is spent; original retirement suppresses late ACK while service may remain active', () async {
    final lost = f.NativeHandle()..confirmError = StateError('owned fixture lost reply');
    final c = owner(f.Calls(lost));
    await c.prepare();
    await c.confirmConfiguration();
    await c.confirmConfiguration();
    expect(c.unknown, isTrue);
    expect(c.serviceMayRemainActive, isTrue);
    expect(lost.calls.where((x) => x == 'confirmConfiguration').length, 1);
    c.dispose();
    var current = true;
    final h = f.NativeHandle()..pendingConfirm = Completer<String>();
    final stale = owner(f.Calls(h), current: () => current);
    await stale.prepare();
    final confirming = stale.confirmConfiguration();
    current = false;
    stale.retire();
    h.pendingConfirm!.complete(jsonEncode(v.serviceWire(phase: 'configurationRecorded')));
    await confirming;
    await stale.close();
    expect(stale.view!.configurationRecorded, isFalse);
    expect(stale.unknown, isTrue);
    expect(stale.serviceMayRemainActive, isTrue);
    stale.dispose();
  });
  test('expiry blocks new confirm but local timer cannot retract an already dispatched confirmation', () async {
    var elapsed = Duration.zero;
    final h = f.NativeHandle(), c = owner(f.Calls(h), elapsed: () => elapsed);
    await c.prepare();
    elapsed = const Duration(seconds: 61);
    await c.confirmConfiguration();
    expect(h.calls, isNot(contains('confirmConfiguration')));
    await c.close();
    c.dispose();
    elapsed = Duration.zero;
    final active = f.NativeHandle()..pendingConfirm = Completer<String>();
    final issued = owner(f.Calls(active), elapsed: () => elapsed);
    await issued.prepare();
    final pending = issued.confirmConfiguration();
    elapsed = const Duration(seconds: 61);
    expect(active.calls, ['details', 'confirmConfiguration']);
    active.pendingConfirm!.complete(jsonEncode(v.serviceWire(phase: 'configurationRecorded')));
    await pending;
    expect(issued.view!.configurationRecorded, isTrue);
    expect(issued.view!.runtimeObserved, isFalse);
    issued.dispose();
  });
}
