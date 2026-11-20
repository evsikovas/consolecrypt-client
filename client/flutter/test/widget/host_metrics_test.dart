import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/terminal/terminal_server_panel.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _HostMetrics implements HostMetricsReader {
  final samples = <StreamController<HostMetricsSample>>[];
  final watches = <({Host host, TerminalSessionId tab, RuntimeNoticeScope scope, String id})>[];
  final cancelled = <String>[];

  @override
  Stream<HostMetricsSample> watchHost(
    Host host,
    TerminalSessionId sessionId,
    RuntimeNoticeScope scope,
    String operationId,
  ) {
    watches.add((host: host, tab: sessionId, scope: scope, id: operationId));
    final controller = StreamController<HostMetricsSample>();
    samples.add(controller);
    return controller.stream;
  }

  @override
  Future<void> cancelHostWatch(String operationId) async => cancelled.add(operationId);
}

class _NoticeScope implements RuntimeNoticeEvents {
  _NoticeScope(this.currentScope);

  @override
  RuntimeNoticeFrame currentScope;
  // Closed by the widget test teardown.
  // ignore: close_sinks
  final changes = StreamController<RuntimeNoticeFrame>.broadcast();

  @override
  Stream<RuntimeNoticeFrame> watchRuntimeNotices() => changes.stream;
}

void main() {
  testWidgets('server details follow the active SSH tab and stop when the dock closes', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    final host = await backend.inventory.saveHost(Host.create(name: 'Linux fixture', address: '192.0.2.8'));
    final other = await backend.inventory.saveHost(Host.create(name: 'Second fixture', address: '192.0.2.9'));
    final scope = RuntimeNoticeScope.fromProfile({
      'active': true,
      'vault_state': 'unlocked',
      'id': backend.profiles.currentProfiles.active!.id.value,
      'notification_scope_id': 'host-metrics-fixture',
    })!;
    final events = _NoticeScope(RuntimeNoticeFrame(epoch: 1, scope: scope)), metrics = _HostMetrics();
    addTearDown(() {
      unawaited(events.changes.close());
      for (final sample in metrics.samples) {
        unawaited(sample.close());
      }
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
    final serverTool = find.byKey(const ValueKey('nav-server'));
    expect(serverTool, findsNothing);
    expect(find.byKey(ValueKey('host-metrics-${host.id.value}')), findsNothing);
    await tapKey(tester, 'connect-Linux fixture');
    await tapKey(tester, 'accept-host-key');
    await enterKey(tester, 'terminal-password', 'fixture password');
    await tapKey(tester, 'terminal-password-submit');
    expect(router.state.uri.path, AppRoutes.terminal);
    expect(serverTool, findsOneWidget);
    expect(metrics.watches, isEmpty);
    await tester.tap(serverTool);
    await settle(tester);
    final tabs = container.read(terminalTabsProvider.notifier);
    final first = container.read(terminalTabsProvider).active!;
    expect(metrics.watches.single.host.id, host.id);
    expect(metrics.watches.single.scope.same(scope), isTrue);
    expect(metrics.watches.single.tab, first.sessionId);
    metrics.samples.single.add(
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
        'os': 'Test OS',
        'kernel': '6.12.0',
        'cpuModel': 'Example CPU',
        'architecture': 'aarch64',
        'logicalCpus': 4,
        'loadAverage': '0.10 / 0.08 / 0.04',
      }),
    );
    await settle(tester);
    expect(find.text('21.5 %'), findsOneWidget);
    expect(find.text('Test OS'), findsOneWidget);
    expect(find.text('Example CPU · aarch64 · 4 logical CPUs'), findsOneWidget);
    await tester.tap(find.text('Values'));
    await settle(tester);
    expect(find.textContaining('21.5 %', findRichText: true), findsOneWidget);
    final opening = tabs.open(other);
    await settle(tester);
    final second = await opening;
    await tapKey(tester, 'accept-host-key');
    await enterKey(tester, 'terminal-password', 'fixture password');
    await tapKey(tester, 'terminal-password-submit');
    expect(metrics.cancelled, contains(metrics.watches.first.id));
    expect(metrics.watches.last.tab, second.sessionId);
    expect(find.text('Test OS'), findsNothing);

    await tapKey(tester, 'workspace-tools-close');
    expect(metrics.cancelled, metrics.watches.map((watch) => watch.id));
    expect(router.state.uri.path, AppRoutes.terminal);
    expect(container.read(terminalTabsProvider).tabs, hasLength(2));

    await tester.tap(serverTool);
    await settle(tester);
    final closing = tabs.closeTab(second);
    await settle(tester);
    await closing;
    expect(metrics.watches.last.tab, first.sessionId);
    router.go(AppRoutes.hosts);
    await settle(tester);
    expect(metrics.cancelled, metrics.watches.map((watch) => watch.id));
    expect(find.byKey(const ValueKey('workspace-tools-panel')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
