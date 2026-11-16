import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service_revocation.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_status.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation_controller.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../support/agent_collector_service_revocation_fixture.dart' as r;
import 'agent_collector_service_status_test.dart' show statusWire;

AgentCollectorServiceStatus target() => parseAgentCollectorServiceStatus(jsonEncode(statusWire()), f.scope, f.record);
RustAgentCollectorServiceWithRevocation port(
  r.RevokeNative native, {
  Future<AgentCollectorServiceRevocationNativeHandle> Function(String)? prepare,
}) {
  final base = f.Calls(f.NativeHandle()).port;
  return RustAgentCollectorServiceWithRevocation(
    original: base,
    status: RustAgentCollectorServiceWithStatus(original: base, readJson: (_) async => jsonEncode(statusWire())),
    prepareOriginal: prepare ?? (_) async => native,
  );
}

void main() {
  test(
    'targeted revoke exact original request retains same handle; localblock and receipt never observe runtime',
    () async {
      final t = target(), h = r.RevokeNative(t), requests = <Map<String, dynamic>>[];
      final p = port(
        h,
        prepare: (raw) async {
          requests.add(jsonDecode(raw) as Map<String, dynamic>);
          return h;
        },
      );
      expect(requests, isEmpty);
      final c = AgentCollectorServiceRevocationController(port: p, target: t, original: () => true);
      await c.prepare();
      await c.confirmRevocation();
      await c.confirmRevocation();
      expect(requests.single, {
        'version': 1,
        'profileId': f.scope.profileId,
        'notificationScopeId': f.scope.nonce,
        'associationId': f.record.id,
      });
      expect(c.localBlocked, isTrue);
      expect(c.remoteRevoked, isTrue);
      expect(c.cleanup!.state, 'closedObserved');
      expect(h.calls, ['details', 'confirmRevocation', 'joined', 'dispose']);
      await c.close();
      c.dispose();
    },
  );
  test('strict native revocation parser denies foreign target/owner, optimistic runtime, invalid state pairs and extra grants', () {
    final t = target(), valid = r.revocationWire(t);
    for (final value in <Map<String, dynamic>>[
      {...valid, 'version': 1.0},
      {...valid, 'serviceHash': 'a' * 64},
      {...valid, 'notificationScopeId': 'foreign'},
      {...valid, 'hostRevision': '99'},
      {...valid, 'runtimeState': 'stopped'},
      {...valid, 'grant': true},
      {...valid, 'localBlocked': null},
      {...valid, 'remoteRevoked': true},
      {...valid, 'phase': 'revoked', 'localBlocked': true, 'remoteRevoked': false},
      {...valid, 'remainingMs': 0},
      {
        ...valid,
        'owner': {...(valid['owner'] as Map<String, dynamic>), 'id': t.record.hostId},
      },
    ]) {
      expect(() => parseAgentCollectorServiceRevocation(jsonEncode(value), t), throwsFormatException);
    }
    expect(
      () => parseAgentCollectorServiceRevocationCleanup(r.cleanupWire(phase: 'unknown', local: true)),
      returnsNormally,
    );
    expect(
      () => parseAgentCollectorServiceRevocationCleanup(r.cleanupWire(phase: 'revoked', local: false, remote: true)),
      throwsFormatException,
    );
  });
  test(
    'cancel reaches original handle during pending details and Unknown join does not claim a remote revoke',
    () async {
      final t = target(),
          h = r.RevokeNative(t)
            ..pendingDetails = Completer<String>()
            ..pendingJoin = Completer<String>();
      final c = AgentCollectorServiceRevocationController(port: port(h), target: t, original: () => true),
          preparing = c.prepare();
      await Future<void>.delayed(Duration.zero);
      c.cancel();
      expect(h.calls, ['details', 'cancel', 'joined']);
      expect(c.awaitingCleanup, isTrue);
      h.pendingDetails!.complete(jsonEncode(r.revocationWire(t)));
      await preparing;
      expect(c.view, isNull);
      h.pendingJoin!.complete(r.cleanupWire(state: 'unknown'));
      await c.close();
      expect(c.cleanupUnknown, isTrue);
      expect(c.remoteRevoked, isFalse);
      expect(h.calls.last, 'dispose');
      c.dispose();
    },
  );
  test('late issuance after original retirement is cancelled/joined/disposed and never displayed', () async {
    final t = target(), h = r.RevokeNative(t), issued = Completer<AgentCollectorServiceRevocationNativeHandle>();
    var current = true, closed = false;
    final c = AgentCollectorServiceRevocationController(
      port: port(h, prepare: (_) => issued.future),
      target: t,
      original: () => current,
    );
    final preparing = c.prepare();
    current = false;
    c.retire();
    final closing = c.close().then((_) => closed = true);
    await Future<void>.delayed(Duration.zero);
    expect(closed, isFalse);
    issued.complete(h);
    await preparing;
    await closing;
    expect(h.calls, ['cancel', 'joined', 'dispose']);
    expect(c.view, isNull);
    c.dispose();
  });
  test(
    'original deadline cannot reset; no expired confirm, and dispatched revoke remains one-use on lost response',
    () async {
      var elapsed = Duration.zero;
      final t = target(),
          h = r.RevokeNative(t),
          c = AgentCollectorServiceRevocationController(
            port: port(h),
            target: t,
            original: () => true,
            elapsed: () => elapsed,
          );
      await c.prepare();
      elapsed = const Duration(seconds: 10);
      await c.refreshReview();
      expect(c.remainingMs, 50000);
      elapsed = const Duration(seconds: 61);
      await c.confirmRevocation();
      expect(h.calls, isNot(contains('confirmRevocation')));
      await c.close();
      c.dispose();
      elapsed = Duration.zero;
      final lost = r.RevokeNative(t)..confirmError = StateError('owned fixture lost response');
      final uncertain = AgentCollectorServiceRevocationController(
        port: port(lost),
        target: t,
        original: () => true,
        elapsed: () => elapsed,
      );
      await uncertain.prepare();
      await uncertain.confirmRevocation();
      await uncertain.confirmRevocation();
      expect(uncertain.unknown, isTrue);
      expect(uncertain.localBlocked, isTrue);
      expect(uncertain.remoteRevoked, isFalse);
      expect(lost.calls.where((v) => v == 'confirmRevocation').length, 1);
      uncertain.dispose();
      elapsed = Duration.zero;
      final active = r.RevokeNative(t)..pendingConfirm = Completer<String>();
      final dispatched = AgentCollectorServiceRevocationController(
        port: port(active),
        target: t,
        original: () => true,
        elapsed: () => elapsed,
      );
      await dispatched.prepare();
      final pending = dispatched.confirmRevocation();
      elapsed = const Duration(seconds: 61);
      expect(active.calls, ['details', 'confirmRevocation']);
      active.pendingConfirm!.complete(jsonEncode(r.revocationWire(t, phase: 'revoked', local: true, remote: true)));
      await pending;
      expect(dispatched.remoteRevoked, isTrue);
      expect(active.calls, isNot(contains('cancel')));
      dispatched.dispose();
    },
  );
}
