import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/src/rust/api/app.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/global_event_fence_fakes.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late GlobalEventNativeFixture api;
  late RustBackend backend;
  late MonitoringScope original;
  setUp(() async {
    api = GlobalEventNativeFixture();
    RustLib.initMock(api: api);
    backend = RustBackend(
      const CoreInfo(
        dataDir: '/unused-owned-fixture',
        platform: 'windows',
        coreVersion: 'fixture',
        alreadyInitialized: false,
      ),
    );
    await backend.start();
    await flushGlobalEvents();
    final epoch = backend.runtimeNoticeEvents.epoch;
    original = MonitoringScope(
      const ObjectId(globalEventProfile),
      true,
      isCurrent: () => backend.runtimeNoticeEvents.currentScope.epoch == epoch,
    );
  });
  tearDown(() async {
    await backend.dispose();
    await api.events.close();
    await api.prompts.close();
    RustLib.dispose();
  });

  test('global lag retires both observers synchronously before pending profile refresh completes', () async {
    final runtimeEpoch = backend.runtimeNoticeEvents.epoch, jobEpoch = backend.jobNoticeEvents.epoch;
    final pending = api.pendingProfiles = Completer<String>();
    addTearDown(() {
      if (!pending.isCompleted) pending.complete(api.current);
    });
    api.events.add(jsonEncode({'type': 'lagged'}));
    expect(original.current, isFalse); // No pump, flush or refresh completion.
    expect(backend.runtimeNoticeEvents.epoch, greaterThan(runtimeEpoch));
    expect(backend.jobNoticeEvents.epoch, greaterThan(jobEpoch));
    expect(backend.runtimeNoticeEvents.currentScope.scope, isNull);
    expect(backend.runtimeNoticeEvents.currentScope.incomplete, isTrue);
    expect(backend.jobNoticeEvents.currentScope.refreshNeeded, isTrue);
    pending.complete(globalEventProfiles(globalEventProfile, 'unlocked', globalEventScopeB));
    await flushGlobalEvents();
    expect(backend.runtimeNoticeEvents.currentScope.scope!.nonce, globalEventScopeB);
    expect(original.current, isFalse);
    expect(api.eventSubscriptions, 1);
  });
  test('global feed error retires before refresh and keeps existing no-auto-reconnect policy', () async {
    final reads = api.profileReads;
    api.events.addError(StateError('synthetic-private-error-must-not-be-retained'));
    expect(original.current, isFalse);
    expect(backend.runtimeNoticeEvents.currentScope.scope, isNull);
    expect(backend.jobNoticeEvents.currentScope.refreshNeeded, isTrue);
    await flushGlobalEvents();
    expect(api.profileReads, reads);
    expect(api.eventSubscriptions, 1);
  });
  test('global feed done retires original scope and preserves unavailable status through manual refresh', () async {
    await api.events.close();
    expect(original.current, isFalse);
    expect(backend.runtimeNoticeEvents.currentScope.streamUnavailable, isTrue);
    expect(backend.jobNoticeEvents.currentScope.streamUnavailable, isTrue);
    api.current = globalEventProfiles(globalEventProfile, 'unlocked', globalEventScopeB);
    await backend.refreshState();
    await flushGlobalEvents();
    expect(original.current, isFalse);
    expect(backend.runtimeNoticeEvents.currentScope.streamUnavailable, isTrue);
    expect(api.eventSubscriptions, 1);
  });
  for (final wire in ['{broken', '[]', '{}', '{"type":0}', '{"type":""}']) {
    test('malformed global envelope retires original without type errors or implicit refresh: $wire', () async {
      final reads = api.profileReads;
      api.events.add(wire);
      expect(original.current, isFalse);
      expect(backend.runtimeNoticeEvents.currentScope.incomplete, isTrue);
      expect(backend.jobNoticeEvents.currentScope.refreshNeeded, isTrue);
      await flushGlobalEvents();
      expect(api.profileReads, reads);
      expect(api.eventSubscriptions, 1);
    });
  }
  test('scoped domain gap preserves unrelated Monitoring lifetime and issues no refresh', () async {
    final epoch = backend.runtimeNoticeEvents.epoch, jobEpoch = backend.jobNoticeEvents.epoch, reads = api.profileReads;
    api.events.add(
      jsonEncode({'type': 'critical_notice_gap', 'profile_id': globalEventProfile, 'scope_id': globalEventScopeA}),
    );
    expect(original.current, isTrue);
    expect(backend.runtimeNoticeEvents.epoch, epoch);
    expect(backend.jobNoticeEvents.epoch, jobEpoch);
    expect(backend.runtimeNoticeEvents.currentScope.incomplete, isTrue);
    expect(backend.runtimeNoticeEvents.currentScope.scope!.nonce, globalEventScopeA);
    await flushGlobalEvents();
    expect(api.profileReads, reads);
    expect(api.eventSubscriptions, 1);
  });
  test('malformed scoped terminal payload is a domain gap rather than a global unlock boundary', () async {
    final epoch = backend.runtimeNoticeEvents.epoch;
    api.events.add(
      jsonEncode({
        'type': 'critical_state_changed',
        'profile_id': globalEventProfile,
        'scope_id': globalEventScopeA,
        'record_id': globalEventJob,
        'revision': 1,
        'change': {'domain': 'terminal', 'state': 'failed'},
        'raw_error': 'never-retained',
      }),
    );
    expect(original.current, isTrue);
    expect(backend.runtimeNoticeEvents.epoch, epoch);
    expect(backend.runtimeNoticeEvents.currentScope.incomplete, isTrue);
  });
  test('unknown future typed event does not invent a global loss or extra subscription', () async {
    api.events.add(jsonEncode({'type': 'future_owned_event'}));
    expect(original.current, isTrue);
    expect(api.eventSubscriptions, 1);
  });
  test('pre-loss pending profile read cannot re-establish its old original scope', () async {
    final pending = api.pendingProfiles = Completer<String>();
    addTearDown(() {
      if (!pending.isCompleted) pending.complete(api.current);
    });
    final before = backend.refreshState();
    api.current = globalEventProfiles(globalEventProfile, 'unlocked', globalEventScopeB);
    api.events.add(jsonEncode({'type': 'lagged'}));
    expect(original.current, isFalse);
    pending.complete(globalEventProfiles(globalEventProfile, 'unlocked', globalEventScopeA));
    await before;
    await flushGlobalEvents();
    expect(original.current, isFalse);
    expect(backend.runtimeNoticeEvents.currentScope.scope!.nonce, globalEventScopeB);
    expect(api.eventSubscriptions, 1);
  });
}
