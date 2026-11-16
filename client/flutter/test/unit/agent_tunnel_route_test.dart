import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_tunnel_route.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_controller.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_wire.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/src/rust/api/agent_tunnel_route.dart' as native;
import 'package:flutter_test/flutter_test.dart';

const profile = '10000000-0000-4000-8000-000000000001',
    host = '20000000-0000-4000-8000-000000000002',
    agent = '30000000-0000-4000-8000-000000000003',
    association = '40000000-0000-4000-8000-000000000004',
    device = '50000000-0000-4000-8000-000000000005',
    grant = '60000000-0000-4000-8000-000000000006';
final scope = RuntimeNoticeScope.fromProfile({
  'id': profile,
  'active': true,
  'vault_state': 'unlocked',
  'notification_scope_id': 'original opaque СЂСџвЂќС’',
})!;
AgentTunnelRouteIntent intent({int listen = 0, int ttl = 60}) => AgentTunnelRouteIntent(
  record: AgentAssociationRecord(
    associationId: association,
    hostId: host,
    agentId: agent,
    phase: AgentAssociationPhase.associated,
    hostRevision: '9007199254740993',
    nodeKeyFingerprint: 'a' * 64,
    certificateDigest: 'b' * 64,
    nodeKeyEpoch: '1',
  ),
  listenIp: '127.0.0.1',
  listenPort: listen,
  destinationIp: '::1',
  destinationPort: 9090,
  ttlSeconds: ttl,
  idleSeconds: ttl < 30 ? ttl : 30,
);
Map<String, dynamic> view({String phase = 'routeReview', int remaining = 60000}) => {
  'version': 1,
  'profileId': profile,
  'notificationScopeId': scope.nonce,
  'phase': phase,
  'associationId': association,
  'hostId': host,
  'hostRevision': '9007199254740993',
  'owner': {'kind': 'localProfile', 'id': profile, 'deviceId': device, 'signingKeySha256': 'c' * 64},
  'node': {'agentId': agent, 'keyEpoch': '1', 'publicKey': 'd' * 64, 'keyFingerprint': 'a' * 64},
  'certificateDigest': 'b' * 64,
  'destination': {'protocol': 'tcp', 'ip': '::1', 'port': 9090},
  'clientBind': {'ip': '127.0.0.1', 'port': 0},
  'limits': {
    'maxTtlSeconds': 60,
    'idleTimeoutSeconds': 30,
    'maxChannels': 1,
    'maxFrameBytes': 8192,
    'maxPendingFramesPerChannel': 1,
    'maxPendingFramesTotal': 2,
    'maxQueuedBytesPerChannel': 8192,
    'maxQueuedBytesTotal': 16384,
    'maxBytesPerChannel': 4194304,
    'maxBytesTotal': 4194304,
  },
  'resumeOriginalSignedOptIn': false,
  'resumeTransportImplemented': false,
  'grantExpiresUtc': '2030-01-01T00:01:00Z',
  'remainingMs': remaining,
  'boundAddress': phase == 'routeReview' || phase == 'starting' ? null : '127.0.0.1:32123',
  'actorAdmission': phase == 'connected',
  'remoteDrainObserved': false,
  'cleanupState': 'notStarted',
  'sentBytes': '0',
  'receivedBytes': '0',
  'countersExact': false,
  'nativeAccountingOnly': true,
};
String cleanup({String state = 'closedObserved'}) => jsonEncode({
  'version': 1,
  'phase': 'unknown',
  'remoteDrainObserved': false,
  'countersExact': false,
  'nativeAccountingOnly': true,
  'sentBytes': '7',
  'receivedBytes': '8',
  'actorAdmission': true,
  'cleanupState': state,
});
Map<String, dynamic> auditWire() => {
  'version': 1,
  'profileId': profile,
  'notificationScopeId': scope.nonce,
  'boundedLocalObservations': true,
  'immutableLog': false,
  'remoteDeliveryGuarantee': false,
  'rows': [
    {
      'sequence': 1,
      'profileId': profile,
      'deviceId': device,
      'personalUserId': null,
      'record': {
        'operationId': grant,
        'associationId': association,
        'hostId': host,
        'hostRevision': 1,
        'grantId': grant,
        'grantDigest': 'e' * 64,
        'policyDigest': 'f' * 64,
        'agentId': agent,
        'keyEpoch': 1,
        'protocol': 'tcp',
        'destinationIp': '::1',
        'destinationPort': 9090,
        'expiresAt': 1893456060,
        'observedAt': 1893456000,
        'event': 'unknown',
        'sent': 7,
        'received': 8,
        'countersExact': false,
        'nativeControlFinished': true,
      },
    },
  ],
};

// Actual generated opaque/callback types; synthetic metadata only, no Native
// executable, TLS, Relay, remote ACK or actor admission is exercised here.
final class NativeHandle extends Fake implements native.AgentTunnelRouteHandle {
  final calls = <String>[];
  String detailsReply = jsonEncode(view()),
      approvalReply = jsonEncode(view(phase: 'listening')),
      stopReply = jsonEncode(view(phase: 'connected')),
      joinedReply = cleanup();
  Completer<String>? pendingDetails, pendingApproval, pendingJoin;
  @override
  Future<String> details() {
    calls.add('details');
    return pendingDetails?.future ?? Future.value(detailsReply);
  }

  @override
  Future<String> approveOnce() {
    calls.add('approve');
    return pendingApproval?.future ?? Future.value(approvalReply);
  }

  @override
  Future<String> stop() {
    calls.add('stop');
    return Future.value(stopReply);
  }

  @override
  void cancel() {
    calls.add('cancel');
  }

  @override
  Future<String> joined() {
    calls.add('joined');
    return pendingJoin?.future ?? Future.value(joinedReply);
  }

  @override
  void dispose() {
    calls.add('dispose');
  }
}

final class Calls implements AgentTunnelRouteNativeCalls {
  Calls(this.current);
  NativeHandle current;
  final requests = <Map<String, dynamic>>[];
  int auditCalls = 0;
  Completer<native.AgentTunnelRouteHandle>? pending;
  @override
  Future<native.AgentTunnelRouteHandle> prepare(String raw) {
    requests.add(jsonDecode(raw) as Map<String, dynamic>);
    return pending?.future ?? Future.value(current);
  }

  @override
  Future<String> audit(String raw) {
    auditCalls++;
    requests.add(jsonDecode(raw) as Map<String, dynamic>);
    return Future.value(jsonEncode(auditWire()));
  }
}

AgentTunnelRouteController controller(Calls calls, {bool Function()? original, Duration Function()? elapsed}) =>
    AgentTunnelRouteController(
      port: RustAgentTunnelRoutePort(calls: calls),
      scope: scope,
      intent: intent(),
      original: original ?? () => true,
      now: () => DateTime.utc(2030),
      elapsed: elapsed,
    );
Future<void> turn() => Future<void>.delayed(Duration.zero);

void main() {
  test('existing monitoring service exposes optional lazy route capability', () {
    var calls = 0;
    final port = RustAgentTunnelRoutePort(calls: Calls(NativeHandle()));
    final legacy = AppCoreMonitoringService(
      read: (_) async {
        calls++;
        return {};
      },
      cancelOperation: (_) async {},
    );
    final enabled = AppCoreMonitoringService(
      read: (_) async {
        calls++;
        return {};
      },
      cancelOperation: (_) async {},
      agentTunnelRoutePort: port,
    );
    expect(legacy.agentTunnelRoutePort, isNull);
    expect(identical(enabled.agentTunnelRoutePort, port), isTrue);
    expect(calls, 0);
  });
  test('construction does not prepare, open, poll or read audit', () {
    final calls = Calls(NativeHandle());
    final c = controller(calls);
    expect(calls.requests, isEmpty);
    expect(c.view, isNull);
    c.dispose();
  });
  test('generated callback receives only fixed intent; issued instance survives callback replacement', () async {
    final first = NativeHandle(),
        second = NativeHandle(),
        calls = Calls(first),
        port = RustAgentTunnelRoutePort(calls: calls);
    final h = await port.prepare(scope, intent());
    expect(calls.requests.single, intent().request(scope));
    calls.current = second;
    final review = await h.details();
    expect(review.phase, AgentTunnelRoutePhase.routeReview);
    await h.approveOnce();
    await h.stop();
    h.cancel();
    await h.joined();
    h.dispose();
    h.dispose();
    expect(first.calls, ['details', 'approve', 'stop', 'cancel', 'joined', 'dispose']);
    expect(second.calls, isEmpty);
    expect(calls.requests, hasLength(1));
  });
  test('listening is not connected or remote drain; positive connected requires Native admission', () {
    final a = parseAgentTunnelRouteView(jsonEncode(view(phase: 'listening')), scope, intent());
    expect(a.actorAdmission, isFalse);
    expect(a.remoteDrainObserved, isFalse);
    expect(a.countersExact, isFalse);
    expect(a.resumeTransportImplemented, isFalse);
    final bad = view(phase: 'connected')..['actorAdmission'] = false;
    expect(() => parseAgentTunnelRouteView(jsonEncode(bad), scope, intent()), throwsFormatException);
  });
  test('closed parser rejects foreign frame, secret extension and forged drain', () {
    for (final field in ['notificationScopeId', 'privateKey', 'remoteDrainObserved', 'resumeTransportImplemented']) {
      final m = view();
      m[field] = field == 'notificationScopeId'
          ? 'replacement'
          : field == 'resumeTransportImplemented'
          ? 'true'
          : true;
      expect(() => parseAgentTunnelRouteView(jsonEncode(m), scope, intent()), throwsFormatException);
    }
  });
  test('selected exact revision/certificate and loopback bind are immutable response fences', () {
    final badRevision = view()..['hostRevision'] = '9007199254740992';
    expect(() => parseAgentTunnelRouteView(jsonEncode(badRevision), scope, intent()), throwsFormatException);
    final badBind = view(phase: 'listening')..['boundAddress'] = '0.0.0.0:32123';
    expect(() => parseAgentTunnelRouteView(jsonEncode(badBind), scope, intent()), throwsFormatException);
    final noAddress = view(phase: 'listening')..['boundAddress'] = null;
    expect(() => parseAgentTunnelRouteView(jsonEncode(noAddress), scope, intent()), throwsFormatException);
  });
  test('joined unknown remains unknown and native lower-bound counters', () {
    final x = parseAgentTunnelRouteCleanup(cleanup(state: 'unknown'));
    expect(x.state, AgentTunnelCleanupState.unknown);
    expect(x.sentBytes, '7');
    expect(x.remoteDrainObserved, isFalse);
    expect(x.countersExact, isFalse);
  });
  test('explicit audit callback is local-only and preserves original unknown', () async {
    final calls = Calls(NativeHandle());
    final audit = await RustAgentTunnelRoutePort(calls: calls).audit(scope);
    expect(calls.requests.single, {'version': 1, 'profileId': profile, 'notificationScopeId': scope.nonce});
    expect(calls.current.calls, isEmpty);
    expect(audit.rows.single.record.event, AgentTunnelAccessEvent.unknown);
    expect(audit.immutableLog, isFalse);
    expect(audit.remoteDeliveryGuarantee, isFalse);
  });
  test('audit rejects lossy numeric revision and foreign provenance', () {
    final a = auditWire();
    ((a['rows'] as List).single as Map)['sequence'] = 9007199254740992;
    expect(() => parseAgentTunnelAccessAudit(jsonEncode(a), scope), throwsFormatException);
    final b = auditWire();
    ((b['rows'] as List).single as Map)['profileId'] = host;
    expect(() => parseAgentTunnelAccessAudit(jsonEncode(b), scope), throwsFormatException);
  });
  test('one-use approval cannot replay while original approval is pending', () async {
    final raw = NativeHandle()..pendingApproval = Completer<String>(), calls = Calls(raw), c = controller(calls);
    await c.prepare();
    final first = c.approveOnce();
    await c.approveOnce();
    expect(raw.calls.where((x) => x == 'approve'), hasLength(1));
    raw.pendingApproval!.complete(jsonEncode(view(phase: 'listening')));
    await first;
    expect(c.view!.phase, AgentTunnelRoutePhase.listening);
    expect(c.canApprove, isFalse);
    await c.close();
    c.dispose();
  });
  test('Stop is an explicit request and does not silently cancel or declare drained', () async {
    final raw = NativeHandle(), c = controller(Calls(raw));
    await c.prepare();
    await c.approveOnce();
    await c.stop();
    await c.stop();
    expect(raw.calls.where((x) => x == 'stop'), hasLength(1));
    expect(raw.calls, isNot(contains('cancel')));
    expect(c.view!.remoteDrainObserved, isFalse);
    expect(c.cleanup, isNull);
    await c.close();
    c.dispose();
  });
  test('cancel awaits actual original join before proxy dispose, without owner Stop', () async {
    final raw = NativeHandle()..pendingJoin = Completer<String>(), c = controller(Calls(raw));
    await c.prepare();
    await c.approveOnce();
    final done = c.close();
    expect(raw.calls, contains('cancel'));
    expect(raw.calls, isNot(contains('stop')));
    expect(raw.calls, isNot(contains('dispose')));
    expect(c.awaitingCleanup, isTrue);
    raw.pendingJoin!.complete(cleanup(state: 'unknown'));
    final observed = await done;
    expect(observed!.state, AgentTunnelCleanupState.unknown);
    expect(raw.calls.where((x) => x == 'joined'), hasLength(1));
    expect(raw.calls.where((x) => x == 'dispose'), hasLength(1));
    c.dispose();
  });
  test('late prepare ACK after disposal is cancelled/joined/disposed before close completes', () async {
    final raw = NativeHandle()..pendingJoin = Completer<String>(),
        calls = Calls(raw)..pending = Completer<native.AgentTunnelRouteHandle>(),
        c = controller(calls);
    final prepare = c.prepare();
    c.dispose();
    var completed = false;
    final close = c.close().then((v) {
      completed = true;
      return v;
    });
    await turn();
    expect(completed, isFalse);
    calls.pending!.complete(raw);
    await turn();
    expect(raw.calls, ['cancel', 'joined']);
    expect(completed, isFalse);
    expect(raw.calls, isNot(contains('details')));
    raw.pendingJoin!.complete(cleanup());
    await prepare;
    await close;
    expect(raw.calls, ['cancel', 'joined', 'dispose']);
  });
  test('original owner retirement during details denies publication and never selects replacement', () async {
    final raw = NativeHandle()..pendingDetails = Completer<String>(), other = NativeHandle(), calls = Calls(raw);
    var live = true;
    final c = controller(calls, original: () => live);
    final pending = c.prepare();
    await turn();
    live = false;
    calls.current = other;
    raw.pendingDetails!.complete(jsonEncode(view()));
    await pending;
    await c.close();
    expect(c.view, isNull);
    expect(raw.calls, ['details', 'cancel', 'joined', 'dispose']);
    expect(other.calls, isEmpty);
    c.dispose();
  });
  test('local countdown cannot be renewed by fresh details and expiry aborts without Stop', () async {
    var elapsed = Duration.zero;
    final raw = NativeHandle(), c = controller(Calls(raw), elapsed: () => elapsed);
    await c.prepare();
    expect(c.remainingMs, 60000);
    elapsed = const Duration(seconds: 40);
    await c.refresh();
    expect(c.remainingMs, 20000);
    elapsed = const Duration(seconds: 61);
    expect(c.remainingMs, 0);
    expect(c.canApprove, isFalse);
    await c.close();
    expect(raw.calls, isNot(contains('stop')));
    c.dispose();
  });
  test('reentrant retirement listener prevents a stale explicit action dispatch', () async {
    final raw = NativeHandle(), c = controller(Calls(raw));
    await c.prepare();
    c.addListener(() {
      if (c.busy) c.cancel();
    });
    await c.approveOnce();
    await c.close();
    expect(raw.calls, isNot(contains('approve')));
    expect(raw.calls.where((x) => x == 'joined'), hasLength(1));
    c.dispose();
  });
}
