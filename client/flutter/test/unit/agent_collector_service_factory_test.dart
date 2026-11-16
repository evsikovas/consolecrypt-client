import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_factory.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_controller.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_collector_service_credentials_fixture.dart' as c;
import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../support/agent_collector_service_lifetime_fixture.dart' as l;
import '../support/agent_collector_service_revocation_fixture.dart' as r;
import 'agent_collector_service_status_test.dart' show statusWire;

// Synthetic callback composition only. No generated/FFI, Node or service proof.
final class _Calls implements AgentCollectorServiceNativeCalls {
  final requests = <String, List<Map<String, dynamic>>>{};
  final single = f.NativeHandle(), lifetime = l.LifetimeNative();
  late final revoke = r.RevokeNative(
    parseAgentCollectorServiceStatus(
      jsonEncode(statusWire()),
      f.scope,
      f.record,
    ),
  );
  Completer<AgentCollectorServiceNativeHandle>? pending;
  void record(String operation, String raw) => requests
      .putIfAbsent(operation, () => [])
      .add(jsonDecode(raw) as Map<String, dynamic>);
  @override
  Future<AgentCollectorServiceNativeHandle> prepare(String raw) async {
    record('prepare', raw);
    return pending?.future ?? single;
  }

  @override
  Future<AgentCollectorServiceNativeHandle> lifetimePrepare(String raw) async {
    record('lifetime', raw);
    return lifetime;
  }

  @override
  Future<String> catalog(String raw) async {
    record('catalog', raw);
    return f.catalogReply();
  }

  @override
  Future<String> status(String raw) async {
    record('status', raw);
    return jsonEncode(statusWire());
  }

  @override
  Future<String> credentialChoices(String raw) async {
    record('choices', raw);
    return jsonEncode(c.choicesWire());
  }

  @override
  Future<AgentCollectorServiceRevocationNativeHandle> revokePrepare(
    String raw,
  ) async {
    record('revoke', raw);
    return revoke;
  }
}

void main() {
  test('composition is lazy and three explicit reads use their original exact frames', () async {
    final calls = _Calls(),
        port = createRustAgentCollectorServicePort(calls: calls);
    expect(calls.requests, isEmpty);
    final catalog = await port.readCatalog(f.scope, f.record);
    final status = await port.readSavedStatus(f.scope, f.record);
    await port.readCredentialChoices(f.scope);
    final frame = {
      'version': 1,
      'profileId': f.scope.profileId,
      'notificationScopeId': f.scope.nonce,
    };
    expect(calls.requests['catalog']!.single, {
      ...frame,
      'associationId': f.record.id,
    });
    expect(calls.requests['status']!.single, {
      ...frame,
      'associationId': f.record.id,
    });
    expect(calls.requests['choices']!.single, frame);
    expect(
      calls.requests.keys,
      unorderedEquals(['catalog', 'status', 'choices']),
    );
    expect(catalog.cleanupState, 'closedObserved');
    expect(status.runtimeObserved, isFalse);
  });
  test('distinct lifetime entry cannot fall back to SingleStart or replace issued ownership', () async {
    final calls = _Calls(),
        port = createRustAgentCollectorServicePort(calls: calls);
    expect(
      () => port.prepare(f.scope, l.lifetimeIntent()),
      throwsFormatException,
    );
    await expectLater(
      port.prepareLifetime(f.scope, f.intent()),
      throwsFormatException,
    );
    expect(calls.requests, isEmpty);
    final single = await port.prepare(f.scope, f.intent());
    final lifetime = await port.prepareLifetime(f.scope, l.lifetimeIntent());
    expect(calls.requests['prepare']!.single, f.intent().request(f.scope));
    expect(
      calls.requests['lifetime']!.single,
      l.lifetimeIntent().request(f.scope),
    );
    await single.details();
    await lifetime.details();
    await lifetime.confirmConfiguration();
    lifetime.cancel();
    await lifetime.joined();
    lifetime.dispose();
    expect(calls.lifetime.calls, [
      'details',
      'confirmConfiguration',
      'cancel',
      'joined',
      'dispose',
    ]);
    expect(calls.single.calls, ['details']);
    single.cancel();
    await single.joined();
    single.dispose();
  });
  test(
    'retirement during prepare joins only the late original and never confirms',
    () async {
      final calls = _Calls()
        ..pending = Completer<AgentCollectorServiceNativeHandle>();
      final owner = AgentCollectorServiceController(
        port: createRustAgentCollectorServicePort(calls: calls),
        scope: f.scope,
        intent: f.intent(),
        original: () => true,
      );
      final preparing = owner.prepare();
      owner.cancel();
      final closing = owner.close();
      calls.pending!.complete(calls.single);
      await preparing;
      await closing;
      expect(calls.single.calls, ['cancel', 'joined', 'dispose']);
      expect(calls.requests.keys, ['prepare']);
      expect(owner.view, isNull);
      owner.dispose();
    },
  );
  test('saved-status revoke stays separate from configuration and preserves uncertain original cleanup', () async {
    final calls = _Calls(),
        port = createRustAgentCollectorServicePort(calls: calls);
    final target = await port.readSavedStatus(f.scope, f.record);
    final issued = await port.prepareServiceRevocation(target);
    calls.revoke.pendingJoin = Completer<String>()
      ..complete(r.cleanupWire(phase: 'unknown', state: 'unknown', local: true));
    await issued.details();
    issued.cancel();
    final cleanup = await issued.joined();
    issued.dispose();
    expect(cleanup.state, 'unknown');
    expect(cleanup.remoteRevoked, isFalse);
    expect(calls.requests.keys, unorderedEquals(['status', 'revoke']));
    expect(calls.revoke.calls, ['details', 'cancel', 'joined', 'dispose']);
    expect(calls.single.calls, isEmpty);
    expect(calls.lifetime.calls, isEmpty);
  });
}
