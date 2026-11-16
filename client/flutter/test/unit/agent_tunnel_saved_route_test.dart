import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_tunnel_route.dart' show originalRustTunnelRouteHandle;
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_saved_route.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_controller.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_wire.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_saved_route_port.dart';
import 'package:consolecrypt/src/rust/api/agent_tunnel_route.dart' as native;
import 'package:flutter_test/flutter_test.dart';

import 'agent_tunnel_route_test.dart' as f;

AgentTunnelRouteIntent savedIntent({bool opted = true}) => AgentTunnelRouteIntent(
  record: f.intent().record,
  listenIp: '127.0.0.1',
  listenPort: 0,
  destinationIp: '127.0.0.1',
  destinationPort: 9090,
  ttlSeconds: 60,
  idleSeconds: 30,
  resumeOriginal: opted,
);
Map<String, dynamic> savedView({String phase = 'listening', bool opted = true, int remaining = 60000}) => {
  ...f.view(phase: phase, remaining: remaining),
  'destination': {'protocol': 'tcp', 'ip': '127.0.0.1', 'port': 9090},
  'resumeOriginalSignedOptIn': opted,
  'resumeTransportImplemented': true,
};
Map<String, dynamic> detachWire({int remaining = 60000}) => {
  'version': 1,
  'profileId': f.scope.profileId,
  'notificationScopeId': f.scope.nonce,
  'phase': 'detached',
  'associationId': f.association,
  'destination': {'protocol': 'tcp', 'ip': '127.0.0.1', 'port': 9090},
  'boundAddress': '127.0.0.1:32123',
  'grantExpiresUtc': '2030-01-01T00:01:00Z',
  'remainingMs': remaining,
  'originalOperationRetained': true,
  'cleanupState': 'pending',
  'remoteDrainObserved': false,
  'countersExact': false,
  'nativeAccountingOnly': true,
};

final class _Preferences implements AgentTunnelResumePreferencePort {
  @override
  Future<AgentTunnelResumePreference> readResumePreference(
    RuntimeNoticeScope scope,
    AgentTunnelResumeQuery query,
  ) async => AgentTunnelResumePreference(query, true);
}

final class _ReleaseNative extends Fake implements native.AgentTunnelRouteHandle {
  _ReleaseNative(this.original, {this.failFirst = false});
  final f.NativeHandle original;
  final bool failFirst;
  void Function()? onDispose;
  int attempts = 0;
  @override
  Future<String> details() => original.details();
  @override
  Future<String> approveOnce() => original.approveOnce();
  @override
  Future<String> stop() => original.stop();
  @override
  void cancel() => original.cancel();
  @override
  Future<String> joined() => original.joined();
  @override
  void dispose() {
    attempts++;
    original.calls.add('release-attempt');
    onDispose?.call();
    if (failFirst && attempts == 1) throw StateError('synthetic raw release failed before completion');
    original.dispose();
  }
}

/// SYNTHETIC actual Native21f019/48b433 shape, existing generated opaque nominal type.
/// No new generated method, FFI, registry or actual route acceptance is used.
final class SavedCalls implements AgentTunnelSavedRouteNativeCalls {
  SavedCalls({this.found = true, this.opted = true}) {
    raw.detailsReply = jsonEncode(savedView(phase: found ? 'listening' : 'routeReview', opted: opted));
    raw.approvalReply = jsonEncode(savedView(opted: opted));
    raw.stopReply = jsonEncode(savedView(phase: 'stopRequested', opted: opted));
  }
  final bool found, opted;
  f.NativeHandle raw = f.NativeHandle();
  native.AgentTunnelRouteHandle? issued;
  final requests = <String, List<Map<String, dynamic>>>{};
  Completer<native.AgentTunnelRouteHandle?>? pendingAttach;
  Completer<String>? pendingDetach;
  Object? attachError;
  Map<String, dynamic>? receiptOverride;
  late final port = RustAgentTunnelRouteWithSavedOriginal(calls: this, preferences: _Preferences());
  void record(String kind, String request) =>
      requests.putIfAbsent(kind, () => []).add(jsonDecode(request) as Map<String, dynamic>);
  @override
  Future<native.AgentTunnelRouteHandle> prepare(String request) async {
    record('prepare', request);
    return issued ?? raw;
  }

  @override
  Future<native.AgentTunnelRouteHandle?> attachSaved(String request) async {
    record('attach', request);
    if (attachError case final error?) {
      if (error is Exception) throw error;
      if (error is Error) throw error;
      throw StateError('Unsupported synthetic failure');
    }
    return pendingAttach?.future ?? (found ? issued ?? raw : null);
  }

  @override
  Future<String> detachAutoOpen(native.AgentTunnelRouteHandle original) async {
    expect(identical(original, issued ?? raw), isTrue);
    raw.calls.add('detach');
    return pendingDetach?.future ?? jsonEncode(receiptOverride ?? detachWire());
  }

  @override
  Future<String> audit(String request) async {
    record('audit', request);
    return jsonEncode(f.auditWire());
  }
}

AgentTunnelRouteController savedOwner(SavedCalls calls, {Duration Function()? elapsed, bool Function()? original}) =>
    AgentTunnelRouteController(
      port: calls.port,
      scope: f.scope,
      intent: savedIntent(opted: calls.opted),
      original: original ?? () => true,
      elapsed: elapsed,
      now: () => DateTime.utc(2030).add(elapsed?.call() ?? Duration.zero),
    );

void main() {
  test('closed fourteen-key detach receipt binds exact original and never claims Closed or delivery', () async {
    final c = SavedCalls(), owner = savedOwner(c);
    await owner.attachSaved();
    final original = owner.view!;
    final receipt = parseAgentTunnelDetachReceipt(jsonEncode(detachWire()), original);
    expect(receipt.originalOperationRetained, isTrue);
    expect(receipt.cleanupState, AgentTunnelCleanupState.pending);
    expect(receipt.remoteDrainObserved, isFalse);
    for (final delta in <Map<String, dynamic>>[
      {'version': 1.0},
      {'phase': 'closed'},
      {'originalOperationRetained': false},
      {'cleanupState': 'closedObserved'},
      {'profileId': f.agent},
      {'notificationScopeId': 'foreign'},
      {'associationId': f.agent},
      {'boundAddress': '127.0.0.1:32124'},
      {'grantExpiresUtc': '2030-01-01T00:01:01Z'},
      {'remainingMs': 60001},
      {'remainingMs': 0},
      {'remainingMs': 1.0},
      {'remoteDrainObserved': true},
      {
        'destination': {'protocol': 'tcp', 'ip': '::1', 'port': 9090},
      },
      {'newAuthority': true},
    ]) {
      expect(
        () => parseAgentTunnelDetachReceipt(jsonEncode({...detachWire(), ...delta}), original),
        throwsFormatException,
      );
    }
    expect(
      () => parseAgentTunnelDetachReceipt(jsonEncode(detachWire()..remove('cleanupState')), original),
      throwsFormatException,
    );
    await owner.close();
    owner.dispose();
  });
  test('explicit lookup None and error never prepare or sign automatically', () async {
    final none = SavedCalls(found: false), owner = savedOwner(none);
    expect(none.requests, isEmpty);
    await owner.attachSaved();
    expect(owner.noSavedOriginal, isTrue);
    expect(owner.canPrepare, isTrue);
    expect(none.requests.keys, ['attach']);
    expect(none.raw.calls, isEmpty);
    await owner.prepare(); // A separate explicit normal review action.
    expect(none.requests.keys, ['attach', 'prepare']);
    expect(none.raw.calls, ['details']);
    await owner.close();
    owner.dispose();
    final error = SavedCalls()..attachError = StateError('synthetic unavailable');
    final failed = savedOwner(error);
    await failed.attachSaved();
    expect(failed.unknown, isTrue);
    expect(failed.canPrepare, isFalse);
    expect(error.requests.keys, ['attach']);
    expect(error.raw.calls, isEmpty);
    await failed.close();
    failed.dispose();
  });
  test('lookup Some retains exact already-approved object without another prepare or approval', () async {
    final c = SavedCalls(), owner = savedOwner(c);
    await owner.attachSaved();
    await owner.approveOnce();
    expect(c.requests['attach']!.single, owner.intent.request(f.scope));
    expect(c.requests.keys, ['attach']);
    expect(c.raw.calls, ['details']);
    expect(owner.canApprove, isFalse);
    expect(owner.canStop, isTrue);
    await owner.stop();
    await owner.close();
    owner.dispose();
    expect(c.raw.calls, ['details', 'stop', 'cancel', 'joined', 'dispose']);
  });
  test('only explicit validated detach disposes proxy without cancel or join and cannot extend budget', () async {
    var elapsed = Duration.zero;
    final c = SavedCalls(), owner = savedOwner(c, elapsed: () => elapsed);
    await owner.attachSaved();
    expect(c.port.savedIntents.restore(f.scope, savedIntent().record), isNull);
    elapsed = const Duration(seconds: 10);
    await owner.detachAutoOpen();
    await owner.detachAutoOpen();
    expect(owner.detached, isTrue);
    expect(owner.remainingMs, 50000);
    expect(owner.cleanup, isNull);
    expect(owner.cancelRequested, isFalse);
    expect(owner.canStop, isFalse);
    final restored = c.port.savedIntents.restore(f.scope, savedIntent().record)!;
    expect(restored.request(f.scope), owner.intent.request(f.scope));
    expect(
      SavedCalls().port.savedIntents.restore(f.scope, restored.record),
      isNull,
    ); // Another service/process has no metadata.
    final foreign = RuntimeNoticeScope.fromProfile({
      'id': f.profile,
      'active': true,
      'vault_state': 'unlocked',
      'notification_scope_id': 'another original frame',
    })!;
    expect(c.port.savedIntents.restore(foreign, restored.record), isNull);
    expect(await owner.close(), isNull);
    owner.dispose();
    expect(c.raw.calls, ['details', 'detach', 'dispose']);
  });
  test('intent metadata is bounded to32 and rejects changed Host certificate scope or expired original', () {
    final cache = AgentTunnelSavedIntentCache(now: () => DateTime.utc(2030));
    final intents = <AgentTunnelRouteIntent>[];
    for (var index = 0; index < 33; index++) {
      final id = '40000000-0000-4000-8000-${(index + 1).toString().padLeft(12, '0')}';
      final record = AgentAssociationRecord(
        associationId: id,
        hostId: f.host,
        agentId: f.agent,
        phase: AgentAssociationPhase.associated,
        hostRevision: '9007199254740993',
        certificateDigest: 'b' * 64,
        nodeKeyEpoch: '1',
        nodeKeyFingerprint: 'a' * 64,
      );
      final intent = AgentTunnelRouteIntent(
        record: record,
        listenIp: '127.0.0.1',
        listenPort: 0,
        destinationIp: '127.0.0.1',
        destinationPort: 9090,
        ttlSeconds: 60,
        idleSeconds: 30,
        resumeOriginal: true,
      );
      final view = parseAgentTunnelRouteView(jsonEncode({...savedView(), 'associationId': id}), f.scope, intent);
      final receipt = parseAgentTunnelDetachReceipt(jsonEncode({...detachWire(), 'associationId': id}), view);
      cache.retainValidated(f.scope, intent, receipt, 60000);
      intents.add(intent);
    }
    expect(cache.restore(f.scope, intents.first.record), isNull);
    expect(cache.restore(f.scope, intents[1].record), isNotNull);
    final last = intents.last, original = last.record;
    final changed = AgentAssociationRecord(
      associationId: original.id,
      hostId: original.hostId,
      agentId: original.agentId,
      phase: original.phase,
      hostRevision: '2',
      certificateDigest: original.certificateDigest,
      nodeKeyEpoch: original.nodeKeyEpoch,
      nodeKeyFingerprint: original.nodeKeyFingerprint,
    );
    expect(cache.restore(f.scope, changed), isNull);
    final changedCertificate = AgentAssociationRecord(
      associationId: original.id,
      hostId: original.hostId,
      agentId: original.agentId,
      phase: original.phase,
      hostRevision: original.hostRevision,
      certificateDigest: 'c' * 64,
      nodeKeyEpoch: original.nodeKeyEpoch,
      nodeKeyFingerprint: original.nodeKeyFingerprint,
    );
    expect(cache.restore(f.scope, changedCertificate), isNull);
    final view = parseAgentTunnelRouteView(jsonEncode({...savedView(), 'associationId': original.id}), f.scope, last);
    final receipt = parseAgentTunnelDetachReceipt(jsonEncode({...detachWire(), 'associationId': original.id}), view);
    final oldMetadataVersion = cache.metadataVersion(f.scope, last);
    cache.retainValidated(f.scope, last, receipt, 60000);
    expect(cache.metadataVersion(f.scope, last), isNot(oldMetadataVersion));
    cache.forget(f.scope, last, expectedVersion: oldMetadataVersion);
    expect(cache.restore(f.scope, original), isNotNull); // Old UI cleanup cannot erase a newer metadata handoff.
    final expired = AgentTunnelSavedIntentCache(now: () => DateTime.utc(2031));
    expired.retainValidated(f.scope, last, receipt, 60000);
    expect(expired.restore(f.scope, original), isNull);
  });
  test('ordinary close remains cancel join; failed or reentrant release never bypasses original cleanup', () async {
    final c = SavedCalls(), owner = savedOwner(c);
    await owner.attachSaved();
    await owner.close();
    owner.dispose();
    expect(owner.detached, isFalse);
    expect(c.raw.calls, ['details', 'cancel', 'joined', 'dispose']);
    final plain = SavedCalls(found: false, opted: false), manual = savedOwner(plain);
    await manual.prepare();
    await manual.approveOnce();
    expect(manual.canDetach, isFalse);
    await manual.close();
    manual.dispose();
    expect(plain.raw.calls, isNot(contains('detach')));
    final failing = SavedCalls();
    final failingRaw = _ReleaseNative(failing.raw, failFirst: true);
    failing.issued = failingRaw;
    final failed = savedOwner(failing);
    await failed.attachSaved();
    await failed.detachAutoOpen();
    await failed.close();
    expect(failed.detached, isFalse);
    expect(failed.unknown, isTrue);
    expect(failing.port.savedIntents.restore(f.scope, savedIntent().record), isNull);
    expect(failing.raw.calls, [
      'details',
      'detach',
      'release-attempt',
      'cancel',
      'joined',
      'release-attempt',
      'dispose',
    ]);
    failed.dispose();
    final reentrant = _ReleaseNative(f.NativeHandle());
    final proxy = originalRustTunnelRouteHandle(reentrant, f.scope, savedIntent());
    reentrant.onDispose = proxy.dispose;
    proxy.dispose();
    proxy.dispose();
    expect(reentrant.attempts, 1);
    expect(reentrant.original.calls, ['release-attempt', 'dispose']);
    final savedReentrantCalls = SavedCalls();
    final savedReentrant = _ReleaseNative(savedReentrantCalls.raw);
    savedReentrantCalls.issued = savedReentrant;
    final savedProxy = (await savedReentrantCalls.port.attachSaved(f.scope, savedIntent()))!;
    savedReentrant.onDispose = savedProxy.dispose;
    savedProxy.dispose();
    savedProxy.dispose();
    expect(savedReentrant.attempts, 1);
    expect(savedReentrant.original.calls, ['release-attempt', 'dispose']);
  });
  test('cancelled pending lookup owns and joins only the late original handle', () async {
    final c = SavedCalls()..pendingAttach = Completer<native.AgentTunnelRouteHandle?>();
    final owner = savedOwner(c), lookup = owner.attachSaved();
    owner.cancel();
    final closing = owner.close();
    c.pendingAttach!.complete(c.raw);
    await lookup;
    await closing;
    expect(owner.view, isNull);
    expect(c.raw.calls, ['cancel', 'joined', 'dispose']);
    expect(c.requests.keys, ['attach']);
    owner.dispose();
  });
  test('pending detach then cancellation never accepts late receipt and waits original Unknown join', () async {
    final c = SavedCalls()..pendingDetach = Completer<String>();
    c.raw.pendingJoin = Completer<String>();
    final owner = savedOwner(c);
    await owner.attachSaved();
    final pending = owner.detachAutoOpen();
    owner.cancel();
    var closed = false;
    final closing = owner.close().then((_) => closed = true);
    await Future<void>.delayed(Duration.zero);
    expect(closed, isFalse);
    c.pendingDetach!.complete(jsonEncode(detachWire()));
    c.raw.pendingJoin!.complete(f.cleanup(state: 'unknown'));
    await pending;
    await closing;
    expect(owner.detached, isFalse);
    expect(owner.unknown, isTrue);
    expect(c.raw.calls, ['details', 'detach', 'cancel', 'joined', 'dispose']);
    owner.dispose();
  });
  test('foreign expired or claimed lost detach stays Unknown with cleanup attempts and no grant fallback', () async {
    for (final mode in ['foreign', 'expired', 'claimed-lost']) {
      var elapsed = Duration.zero;
      final c = SavedCalls()..pendingDetach = Completer<String>();
      final owner = savedOwner(c, elapsed: () => elapsed);
      await owner.attachSaved();
      final pending = owner.detachAutoOpen();
      if (mode == 'expired') elapsed = const Duration(seconds: 61);
      if (mode == 'claimed-lost') {
        c.raw.pendingJoin = Completer<String>();
        c.pendingDetach!.completeError(StateError('synthetic lost handoff reply'));
        await Future<void>.delayed(Duration.zero);
        c.raw.pendingJoin!.completeError(StateError('synthetic already-claimed ownership'));
      } else {
        c.pendingDetach!.complete(
          jsonEncode({...detachWire(), if (mode == 'foreign') 'notificationScopeId': 'foreign'}),
        );
      }
      await pending;
      await owner.close();
      expect(owner.detached, isFalse);
      expect(owner.unknown, isTrue);
      if (mode == 'claimed-lost') {
        expect(owner.cleanup, isNull);
        expect(owner.cleanupError, isNotNull);
      }
      expect(c.raw.calls, ['details', 'detach', 'cancel', 'joined', 'dispose']);
      expect(c.requests.keys, ['attach']);
      owner.dispose();
    }
  });
}
