import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_metrics_collector.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_controller.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_wire.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_metrics_disk_fixture.dart' as disk;
import '../support/agent_metrics_fixture.dart' as f;

AgentMetricsCollectorView parse(Map<String, dynamic> value) =>
    parseAgentMetricsCollectorView(jsonEncode(value), f.scope, AgentMetricsCollectorIntent(f.metricRecord(), 300));
void main() {
  test('disk series minimum follows Native256 while legacy RAM still permits1', () {
    for (final n in [255, 256]) {
      final v = disk.diskView();
      (((v['manifest'] as Map)['targets'] as List).single as Map)['series_limit'] = n;
      if (n == 255) {
        expect(() => parse(v), throwsFormatException);
      } else {
        expect(parse(v).collector.manifest.targets.single.seriesLimit, 256);
      }
    }
    final ram = f.view();
    (((ram['manifest'] as Map)['targets'] as List).single as Map)['series_limit'] = 1;
    expect(parse(ram).collector.manifest.targets.single.seriesLimit, 1);
  });

  test(
    'closed disk producer shape separates buffer residence from original permission and retains unknown delivery',
    () {
      final v = parse(disk.diskView());
      expect(v.persistentDisk, true);
      expect(v.crashReplay, isNull);
      expect(v.bufferMode, 'persistentNativeWal');
      expect(v.diskBudget!.maxChargedBytes, 536870912);
      expect(v.collector.manifest.maxBufferAgeSeconds, 7200);
      expect(v.collector.manifest.maxBufferBytes, 536870912);
      expect(v.originalExpiresUtc, DateTime.utc(2030, 1, 1, 0, 5));
      expect(v.reviewRemainingMs, 60000);
      expect(v.automaticRecovery, false);
      expect(v.requiresFreshGrantForReplay, true);
      expect(v.automaticRenewal, false);
      expect(v.lossUnknown, true);
      expect(v.deliveryGuaranteed, false);
      expect(v.actorAdmission, false);
      expect(parse(disk.diskView(phase: 'running')).leaseHash, 'c' * 64);
    },
  );
  test('disk exact budget and type plus mandatory no-recovery flags cannot be widened', () {
    final keys = (disk.diskView()['diskBudget'] as Map).keys.toList();
    for (final key in keys) {
      for (final invalid in [0, 536870912.0, null]) {
        final v = disk.diskView();
        (v['diskBudget'] as Map)[key] = invalid;
        expect(() => parse(v), throwsFormatException, reason: '$key/$invalid');
      }
    }
    for (final mutate in <void Function(Map<String, dynamic>)>[
      (v) => v['crashReplay'] = false,
      (v) => v.remove('automaticRecovery'),
      (v) => v['automaticRecovery'] = true,
      (v) => v['requiresFreshGrantForReplay'] = false,
      (v) => v['bufferMode'] = 'other',
      (v) => (v['diskBudget'] as Map)['extra'] = 1,
      (v) => v['automaticRenewal'] = true,
    ]) {
      final v = disk.diskView();
      mutate(v);
      expect(() => parse(v), throwsFormatException);
    }
  });
  test('RAM and disk manifest/view forms are disjoint and full manifest hash remains exact Native observation', () {
    final ram = parse(f.view());
    expect(ram.persistentDisk, false);
    expect(ram.crashReplay, false);
    expect(ram.diskBudget, isNull);
    expect(ram.collector.manifest.maxBufferBytes, 33554432);
    expect(ram.collector.manifest.maxBufferAgeSeconds, 300);
    expect(ram.collector.manifest.storageProfile, isNot(parse(disk.diskView()).collector.manifest.storageProfile));
    for (final mutate in <void Function(Map<String, dynamic>)>[
      (v) => (v['manifest'] as Map).remove('profile'),
      (v) => (v['manifest'] as Map)['bufferMode'] = 'volatile',
      (v) => (v['manifest'] as Map)['buffer'] = {'max_bytes': 33554432, 'max_age_seconds': 300},
      (v) => (v['manifest'] as Map)['callerBudget'] = 1,
      (v) => v['manifest'] = f.manifest(),
      (v) => v['profile'] = 'future-profile',
    ]) {
      final v = disk.diskView();
      mutate(v);
      expect(() => parse(v), throwsFormatException);
    }
    expect(parse(disk.diskView()).collector.manifestSha256, '1' * 64);
  });
  test('disk control uses unchanged request and cannot extend permission to buffer residence', () async {
    var elapsed = Duration.zero;
    final h = f.NativeHandle()..detailsReply = jsonEncode(disk.diskView());
    final calls = f.Calls(h);
    final observed = AgentMetricsCollectorController(
      port: RustAgentMetricsCollectorPort(calls: calls),
      scope: f.scope,
      record: f.metricRecord(),
      original: () => true,
      elapsed: () => elapsed,
      now: () => f.now,
    );
    expect(calls.requests, isEmpty);
    await observed.prepare();
    expect(calls.requests.single, {
      'version': 1,
      'profileId': f.scope.profileId,
      'notificationScopeId': f.scope.nonce,
      'associationId': f.metricRecord().id,
      'durationSeconds': 300,
    });
    expect(observed.sessionRemainingMs, 300000);
    elapsed = const Duration(seconds: 30);
    await observed.refresh();
    expect(observed.sessionRemainingMs, 270000);
    expect(observed.reviewRemainingMs, 30000);
    await observed.close();
    observed.dispose();
  });
  test('same original handle cannot change storage profile even with a reused metadata digest', () async {
    final h = f.NativeHandle()..detailsReply = jsonEncode(disk.diskView()), calls = f.Calls(f.NativeHandle());
    calls.current = h;
    final c = AgentMetricsCollectorController(
      port: RustAgentMetricsCollectorPort(calls: calls),
      scope: f.scope,
      record: f.metricRecord(),
      original: () => true,
      now: () => f.now,
    );
    await c.prepare();
    h.detailsReply = jsonEncode(f.view()..['manifestSha256'] = '1' * 64);
    await c.refresh();
    expect(c.unknown, true);
    expect(c.cancelled, true);
    expect(calls.requests.length, 1);
    expect(h.calls, contains('cancel'));
    await c.close();
    c.dispose();
  });
}
