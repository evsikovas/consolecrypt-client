import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_outbound_bootstrap.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap_controller.dart';
import 'package:consolecrypt/src/rust/api/agent_node_association.dart' as native;
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_outbound_bootstrap_fixture.dart' as f;

/// Synthetic nominal generated opaque object: delegation proof, not FFI/runtime.
final class _GeneratedHandle extends Fake implements native.AgentTunnelControlHandle {
  final original = f.NativeHandle();
  @override
  Future<String> details() => original.details();
  @override
  Future<String> approveOnce() => original.approveOnce();
  @override
  void cancel() => original.cancel();
  @override
  Future<String> joined() => original.joined();
  @override
  void dispose() => original.dispose();
}

void main() {
  test(
    'actual Main bootstrap factory is lazy and exposes a distinct original service port without FFI initialization',
    () {
      final first = const RustMonitoringBackend().createService() as AgentOutboundBootstrapCapability;
      final second = const RustMonitoringBackend().createService() as AgentOutboundBootstrapCapability;
      expect(first.agentOutboundBootstrapPort, isA<RustAgentOutboundBootstrapPort>());
      expect(second.agentOutboundBootstrapPort, isA<RustAgentOutboundBootstrapPort>());
      expect(identical(first.agentOutboundBootstrapPort, second.agentOutboundBootstrapPort), isFalse);
      // Calling the default generated function here would require RustLib.init.
      // Successful construction itself proves no Native call was attempted.
    },
  );
  test('actual generated bootstrap signature forwards exact request and same issued opaque through review approval cleanup', () async {
    final first = _GeneratedHandle(), replacement = _GeneratedHandle();
    var current = first;
    final requests = <Map<String, dynamic>>[];
    final port = createGeneratedAgentOutboundBootstrapPort(
      prepare: ({required String requestJson}) async {
        final request = jsonDecode(requestJson) as Map<String, dynamic>;
        requests.add(request);
        current.original.ttl = request['ttlSeconds'] as int;
        return current;
      },
    );
    expect(requests, isEmpty);
    final intent = AgentOutboundBootstrapIntent(f.record);
    final issued = await port.prepare(f.scope, intent);
    expect(requests.single, intent.request(f.scope));
    current = replacement;
    expect((await issued.details()).phase, 'bootstrapReview');
    expect((await issued.approveOnce()).phase, 'configured');
    issued.cancel();
    expect((await issued.joined()).state, 'closedObserved');
    issued.dispose();
    issued.dispose();
    expect(requests, hasLength(1));
    expect(first.original.calls, ['details', 'approveOnce', 'cancel', 'joined', 'dispose']);
    expect(replacement.original.calls, isEmpty);
  });

  test('selected producer shape matches exact original frame/node and finite independent registration', () {
    final intent = AgentOutboundBootstrapIntent(f.record);
    final parsed = parseAgentOutboundBootstrapView(jsonEncode(f.wire()), f.scope, intent);
    expect(parsed.maximumLifetimeSeconds, 3600);
    expect(parsed.remainingMs, 60000);
    expect(parsed.registrationExpiresAt, isNull);
    expect(intent.request(f.scope).keys.toSet(), {
      'version',
      'profileId',
      'notificationScopeId',
      'associationId',
      'ttlSeconds',
    });
    final finalView = parseAgentOutboundBootstrapView(jsonEncode(f.wire(phase: 'configured')), f.scope, intent);
    expect(finalView.registrationExpiresAt!.isUtc, isTrue);
    expect(parsed.sameOriginal(finalView), isTrue);
  });
  test('wrong original identity, epoch, unknown keys, renewed mode and imprecise numeric types are denied', () {
    final intent = AgentOutboundBootstrapIntent(f.record);
    final corrupt = <Map<String, Object?>>[
      {...f.wire(), 'version': 1.0},
      {...f.wire(), 'notificationScopeId': 'other'},
      {...f.wire(), 'associationId': '80000000-0000-4000-8000-000000000008'},
      {...f.wire(), 'autoRenew': true},
      {...f.wire(), 'nodeAdmission': true},
      {...f.wire(), 'registrationGenerationCap': 65},
      {...f.wire(), 'remainingMs': 60001},
      {...f.wire(), 'maximumRegistrationLifetimeSeconds': 86400},
      {...f.wire(), 'requestedRegistrationLifetimeSeconds': 3600.0},
      {...f.wire(), 'endpoint': '127.0.0.1:22'},
      {
        ...f.wire(),
        'node': {...(f.wire()['node']! as Map<String, Object>), 'keyEpoch': '2'},
      },
      {...f.wire(phase: 'configured'), 'cleanupState': 'unknown'},
    ];
    for (final m in corrupt) {
      expect(() => parseAgentOutboundBootstrapView(jsonEncode(m), f.scope, intent), throwsFormatException);
    }
    expect(() => parseAgentOutboundBootstrapCleanup(f.cleanup('notStarted')), throwsFormatException);
  });
  test(
    'callback adapter retains same issued handle and forwards exact values without a generated/FFI fallback',
    () async {
      final h = f.NativeHandle();
      final selected = f.Calls(h),
          port = selected.port,
          intent = AgentOutboundBootstrapIntent(f.record, ttlSeconds: 86400);
      expect(selected.requests, isEmpty);
      final issued = await port.prepare(f.scope, intent);
      expect(selected.requests.single, intent.request(f.scope));
      expect((await issued.details()).maximumLifetimeSeconds, 86400);
      expect((await issued.approveOnce()).phase, 'configured');
      issued.cancel();
      expect((await issued.joined()).state, 'closedObserved');
      issued.dispose();
      expect(h.calls, ['details', 'approveOnce', 'cancel', 'joined', 'dispose']);
    },
  );
  test('async details cancellation immediately cancels original and late review never publishes', () async {
    final h = f.NativeHandle()
      ..pendingDetails = Completer<String>()
      ..pendingJoin = Completer<String>();
    final c = AgentOutboundBootstrapController(
      port: f.Calls(h).port,
      scope: f.scope,
      intent: AgentOutboundBootstrapIntent(f.record),
      original: () => true,
    );
    final waiting = c.prepare();
    await Future<void>.delayed(Duration.zero);
    c.cancel();
    expect(h.calls, ['details', 'cancel', 'joined']);
    expect(c.awaitingCleanup, isTrue);
    h.pendingDetails!.complete(jsonEncode(f.wire()));
    await waiting;
    expect(c.view, isNull);
    h.pendingJoin!.complete(f.cleanup());
    await c.close();
    expect(h.calls.last, 'dispose');
    c.dispose();
  });
  test('late preparation is cancelled/joined exactly and close waits for its actual cleanup', () async {
    final h = f.NativeHandle(), calls = f.Calls(h)..pendingPrepare = Completer<AgentOutboundBootstrapNativeHandle>();
    final c = AgentOutboundBootstrapController(
      port: calls.port,
      scope: f.scope,
      intent: AgentOutboundBootstrapIntent(f.record),
      original: () => true,
    );
    final pending = c.prepare();
    var finished = false;
    final closing = c.close().then((_) => finished = true);
    await Future<void>.delayed(Duration.zero);
    expect(finished, isFalse);
    calls.pendingPrepare!.complete(h);
    await pending;
    await closing;
    expect(h.calls, ['cancel', 'joined', 'dispose']);
    expect(c.view, isNull);
    c.dispose();
  });
  test('stale original scope after review denies approval and unknown join never claims closed', () async {
    var live = true;
    final h = f.NativeHandle()..joinState = 'unknown';
    final calls = f.Calls(h),
        c = AgentOutboundBootstrapController(
          port: calls.port,
          scope: f.scope,
          intent: AgentOutboundBootstrapIntent(f.record),
          original: () => live,
        );
    await c.prepare();
    live = false;
    c.retire();
    await c.close();
    await c.approveOnce();
    expect(h.calls, isNot(contains('approveOnce')));
    expect(c.unknown, isTrue);
    expect(c.cleanup!.state, 'unknown');
    expect(calls.requests.length, 1);
    c.dispose();
  });
  test(
    'original countdown is not renewed, no approval after expiry and dispatched approval is not retracted by timer',
    () async {
      var elapsed = Duration.zero;
      final h = f.NativeHandle();
      final c = AgentOutboundBootstrapController(
        port: f.Calls(h).port,
        scope: f.scope,
        intent: AgentOutboundBootstrapIntent(f.record),
        original: () => true,
        elapsed: () => elapsed,
      );
      await c.prepare();
      elapsed = const Duration(seconds: 60);
      await c.approveOnce();
      expect(h.calls, isNot(contains('approveOnce')));
      await c.close();
      c.dispose();
      elapsed = Duration.zero;
      final h2 = f.NativeHandle()..pendingApproval = Completer<String>();
      final c2 = AgentOutboundBootstrapController(
        port: f.Calls(h2).port,
        scope: f.scope,
        intent: AgentOutboundBootstrapIntent(f.record),
        original: () => true,
        elapsed: () => elapsed,
      );
      await c2.prepare();
      final pending = c2.approveOnce();
      elapsed = const Duration(minutes: 2);
      expect(h2.calls, ['details', 'approveOnce']);
      h2.pendingApproval!.complete(jsonEncode(f.wire(phase: 'configured')));
      await pending;
      expect(c2.view!.phase, 'configured');
      await c2.approveOnce();
      expect(h2.calls.where((v) => v == 'approveOnce').length, 1);
      c2.dispose();
    },
  );
}
