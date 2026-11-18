import 'dart:async';
import 'dart:ui' show PointerDeviceKind;

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/hosts/host_metrics_screen.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _HostMetrics implements HostMetricsReader {
  final samples = StreamController<HostMetricsSample>();
  final watches = <({Host host, RuntimeNoticeScope scope, String id})>[];
  final cancelled = <String>[];

  @override
  Stream<HostMetricsSample> watchHost(Host host, RuntimeNoticeScope scope, String operationId) {
    watches.add((host: host, scope: scope, id: operationId));
    return samples.stream;
  }

  @override
  Future<void> cancelHostWatch(String operationId) async => cancelled.add(operationId);
}

class _NoticeScope implements RuntimeNoticeEvents {
  _NoticeScope(this.currentScope);

  @override
  RuntimeNoticeFrame currentScope;
  final changes = StreamController<RuntimeNoticeFrame>.broadcast();

  @override
  Stream<RuntimeNoticeFrame> watchRuntimeNotices() => changes.stream;
}

void main() {
  testWidgets('a host card opens graphs and values in Hosts and cancels when leaving', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    final host = await backend.inventory.saveHost(Host.create(name: 'Linux fixture', address: '192.0.2.8'));
    final scope = RuntimeNoticeScope.fromProfile({
      'active': true,
      'vault_state': 'unlocked',
      'id': backend.profiles.currentProfiles.active!.id.value,
      'notification_scope_id': 'host-metrics-fixture',
    })!;
    final events = _NoticeScope(RuntimeNoticeFrame(epoch: 1, scope: scope)), metrics = _HostMetrics();
    addTearDown(() {
      unawaited(events.changes.close());
      unawaited(metrics.samples.close());
    });
    await pumpApp(
      tester,
      backend,
      overrides: [
        hostMetricsReaderProvider.overrideWithValue(metrics),
        criticalRuntimeEventsProvider.overrideWithValue(events),
      ],
    );

    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final router = container.read(routerProvider);
    final button = find.byKey(ValueKey('host-metrics-${host.id.value}'));
    expect(find.descendant(of: button, matching: find.byIcon(Icons.show_chart_rounded)), findsOneWidget);
    expect(find.text('Host metrics'), findsNothing);
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: tester.getCenter(button));
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    await tester.pumpAndSettle();
    expect(find.text('Host metrics'), findsOneWidget);
    await mouse.removePointer();
    await tester.pumpAndSettle();
    expect(metrics.watches, isEmpty);
    await tapKey(tester, 'host-metrics-${host.id.value}');
    expect(router.state.uri.path, AppRoutes.hostMetrics(host.id));
    expect(find.text('Host metrics · Linux fixture'), findsOneWidget);
    expect(metrics.watches.single.host.id, host.id);
    expect(metrics.watches.single.scope.same(scope), isTrue);
    expect(container.read(terminalTabsProvider).tabs, isEmpty);
    metrics.samples.add(
      HostMetricsSample.fromJson({
        'timestampMs': DateTime.utc(2026, 11, 18).millisecondsSinceEpoch,
        'cpuPercent': 21.5,
        'uptimeSeconds': 551,
        'memoryUsed': 2048,
        'memoryTotal': 4096,
        'diskUsed': 1024,
        'diskTotal': 4096,
        'receivedPerSecond': 55,
        'sentPerSecond': 12,
      }),
    );
    await settle(tester);
    expect(find.text('21.5 %'), findsOneWidget);
    await tester.tap(find.text('Values'));
    await settle(tester);
    expect(find.textContaining('21.5 %', findRichText: true), findsOneWidget);
    await tapKey(tester, 'host-metrics-back');
    expect(router.state.uri.path, AppRoutes.hosts);
    expect(metrics.cancelled, [metrics.watches.single.id]);
    expect(tester.takeException(), isNull);
  });
}
