import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/monitoring_preferences.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/monitoring/monitoring_card.dart';
import 'package:consolecrypt/monitoring/monitoring_inventory.dart';
import 'package:consolecrypt/monitoring/monitoring_poller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';

class _SlowReader implements DashboardMetricsReader {
  final requests = <String, Completer<HostMetricsSample>>{};
  final cancelled = <String>[];
  @override
  Future<HostMetricsSample> sampleHost(Host host, String operationId) =>
      (requests[operationId] = Completer<HostMetricsSample>()).future;
  @override
  Future<void> cancelHostWatch(String operationId) async {
    cancelled.add(operationId);
  }
}

HostMetricsSample _sample() => HostMetricsSample.fromJson({'timestampMs': 0, 'uptimeSeconds': 123});

void main() {
  testWidgets('numeric thresholds match in both views at 40, 90 and 91 percent', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final ctx = tester.element(find.byType(Navigator).first);
    final tokens = GlassTokens.of(ctx);
    final host = backend.inventory.currentHosts.first;
    final sample = HostMetricsSample.fromJson({
      'timestampMs': 0,
      'uptimeSeconds': 0,
      'cpuPercent': 40,
      'memoryTotal': 100,
      'memoryUsed': 90,
      'diskTotal': 100,
      'diskUsed': 91,
    });
    final overlay = Overlay.of(tester.element(find.byType(Scaffold).last));
    for (final minimal in [false, true]) {
      final entry = OverlayEntry(
        builder: (context) => Align(
          alignment: Alignment.center,
          child: Material(
            child: SizedBox(
              width: 310,
              child: MonitoringCard(
                host: host,
                reading: (sample: sample, error: null),
                percent: true,
                minimal: minimal,
              ),
            ),
          ),
        ),
      );
      overlay.insert(entry);
      await tester.pump();
      final rows = tester.widgetList<Text>(
        find.descendant(of: find.byType(MonitoringCard), matching: find.byType(Text)),
      );
      for (final (value, color) in [
        ('40%', tokens.palette.success),
        ('90%', tokens.palette.warning),
        ('91%', tokens.palette.danger),
      ]) {
        if (minimal) {
          final valueSpan = rows
              .map((r) => r.textSpan)
              .whereType<TextSpan>()
              .expand((s) => s.children ?? const <InlineSpan>[])
              .whereType<TextSpan>()
              .singleWhere((s) => s.text == value);
          expect(valueSpan.style!.color, color);
        } else {
          expect(rows.singleWhere((r) => r.data == value).style!.color, color);
        }
      }
      entry.remove();
      entry.dispose();
      await tester.pump();
    }
  });
  testWidgets('Russian dashboard shows latest sample and restores saved hosts', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final selected = backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1');
    final second = await backend.inventory.saveHost(
      Host.create(name: 'prod-web-2', address: '192.0.2.8').withGroup(selected.groupId),
    );
    final settings = backend.services.settings;
    final active = backend.services.profiles.currentProfiles.active!;
    await settings.updateLocal(
      settings.currentLocal.copyWith(
        themeMode: AppThemeMode.dark,
        monitoringByProfile: {
          active.id.value: MonitoringPreferences(groups: {selected.groupId!.value}, seconds: 30),
        },
      ),
    );
    await loadAutomationProofFonts(tester);
    await pumpApp(tester, backend, size: const Size(1360, 800), locale: AppLocale.ru, captureBoundary: true);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    expect(find.byKey(ValueKey(second.id)), findsOneWidget);
    await captureAutomationWorkspace(tester, 'monitoring-hosts-ru');
    final mode = find.byKey(const ValueKey('monitor-minimal'));
    expect(find.descendant(of: mode, matching: find.byType(Text)), findsNothing);
    expect(find.byTooltip('Включить минимальный вид'), findsOneWidget);
    final detailedHeight = tester.getSize(find.byKey(ValueKey(second.id))).height;
    await tapKey(tester, 'monitor-minimal');
    expect(settings.currentLocal.monitoringByProfile[active.id.value]!.minimal, isTrue);
    expect(find.byTooltip('Включить подробный вид'), findsOneWidget);
    expect(tester.getSize(find.byKey(ValueKey(second.id))).height, lessThan(detailedHeight));
    await captureAutomationWorkspace(tester, 'monitoring-minimal-ru');
    container.read(routerProvider).go(AppRoutes.hosts);
    await settle(tester);
  });
  test('a saved folder follows descendants, deduplicates explicit hosts and excludes RDP', () {
    final root = Group.create('root'), child = Group.create('child', parentId: root.id);
    final one = Host.create(name: 'one', address: 'localhost').withGroup(child.id);
    final two = Host.create(name: 'two', address: 'localhost').withGroup(root.id);
    final rdp = Host(
      id: ObjectId.generate(),
      name: 'desktop',
      address: 'localhost',
      createdAt: one.createdAt,
      updatedAt: one.updatedAt,
      protocol: HostProtocol.rdp,
      groupId: root.id,
    );
    final selection = MonitoringPreferences(
      hosts: {one.id.value},
      groups: {root.id.value},
      seconds: 60,
      percent: true,
      minimal: true,
    );
    final restored = localSettingsFromJson(
      localSettingsToJson(LocalSettings(monitoringByProfile: {'profile-a': selection})),
    );
    final prefs = restored.monitoringByProfile['profile-a']!;
    expect(restored.monitoringByProfile['profile-b'], isNull);
    expect((prefs.seconds, prefs.percent, prefs.minimal), (60, true, true));
    expect(MonitoringInventory([one, rdp], [child, root]).resolve(prefs), [one]);
    expect(MonitoringInventory([one, two, rdp], [root, child]).resolve(prefs), [one, two]);
  });

  testWidgets('a failed host does not block other hosts; leaving cancels the outstanding probes', (tester) async {
    final reader = _SlowReader(), poller = MonitoringPoller(reader);
    final hosts = List.generate(6, (i) => Host.create(name: 'host-$i', address: 'localhost'));
    poller.configure(hosts, 15);
    expect(reader.requests.length, 4);
    reader.requests.values.first.completeError(StateError('offline'));
    await tester.pump();
    expect(poller.readings[hosts.first.id]?.error, isA<StateError>());
    expect(reader.requests.length, 5);
    reader.requests.values.elementAt(1).complete(_sample());
    await tester.pump();
    expect(poller.readings[hosts[1].id]?.sample, isNotNull);
    expect(reader.requests.length, 6);
    poller.configure([], 15);
    expect(reader.cancelled.length, 4);
    for (final pending in reader.requests.values.where((p) => !p.isCompleted)) {
      pending.complete(_sample());
    }
    await tester.pump(const Duration(minutes: 1));
    expect(poller.readings, isEmpty);
    expect(reader.requests.length, 6);
    poller.dispose();
  });

  testWidgets('switching to manual stops automatic refresh and an error preserves the last sample', (tester) async {
    final reader = _SlowReader(), poller = MonitoringPoller(reader);
    final host = Host.create(name: 'host', address: 'localhost');
    poller.configure([host], 15);
    reader.requests.values.first.complete(_sample());
    await tester.pump();
    await tester.pump(const Duration(seconds: 15));
    expect(reader.requests.length, 2);
    reader.requests.values.last.completeError(StateError('unavailable'));
    await tester.pump();
    expect(poller.readings[host.id]?.sample, isNotNull);
    expect(poller.readings[host.id]?.error, isA<StateError>());
    poller.configure([host], 0);
    await tester.pump(const Duration(minutes: 3));
    expect(reader.requests.length, 2);
    final manual = poller.refresh();
    reader.requests.values.last.complete(_sample());
    await manual;
    expect(poller.readings[host.id]?.error, isNull);
    poller.dispose();
  });

  for (final size in [const Size(390, 844), const Size(800, 900), const Size(1440, 900)]) {
    testWidgets('select a folder and keep cards inside the monitoring workspace at ${size.width}', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      final one = backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1');
      final two = await backend.inventory.saveHost(
        Host.create(name: 'prod-web-2', address: '192.0.2.8').withGroup(one.groupId),
      );
      await pumpApp(tester, backend, size: size);
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      container.read(routerProvider).go(AppRoutes.monitoring);
      await settle(tester);
      await tapKey(tester, 'monitor-hosts');
      await tester.tap(find.descendant(of: findDialog(), matching: find.text('Groups')));
      await settle(tester);
      await tester.tap(find.text('Production / Web'));
      await tester.tap(find.descendant(of: findDialog(), matching: find.text('Save')));
      await settle(tester);
      final first = find.byKey(ValueKey(one.id)), second = find.byKey(ValueKey(two.id));
      final a = tester.getRect(first), b = tester.getRect(second);
      expect(a.left, greaterThanOrEqualTo(0));
      expect(b.right, lessThanOrEqualTo(size.width));
      expect(find.descendant(of: first, matching: find.text('17%')), findsOneWidget);
      if (size.width == 390) {
        expect(b.top, greaterThan(a.bottom));
      }
      if (size.width == 1440) {
        expect(b.left, greaterThan(a.right));
        expect(a.top, b.top);
      }
      await tester.tap(find.byTooltip('Show memory and disk usage as percentages'));
      await settle(tester);
      expect(find.descendant(of: first, matching: find.text('38%')), findsOneWidget);
      await tapKey(tester, 'monitor-minimal');
      final compact = tester.getRect(first);
      expect(compact.height, lessThan(a.height));
      expect(compact.left, greaterThanOrEqualTo(0));
      expect(compact.right, lessThanOrEqualTo(size.width));
      expect(find.descendant(of: first, matching: find.byType(LinearProgressIndicator)), findsNothing);
      await tapKey(tester, 'monitor-minimal');
      expect(find.descendant(of: first, matching: find.text('38%')), findsOneWidget);
      container.read(routerProvider).go(AppRoutes.hosts);
      await settle(tester);
      container.read(routerProvider).go(AppRoutes.monitoring);
      await settle(tester);
      expect(find.descendant(of: first, matching: find.text('38%')), findsOneWidget);
      container.read(routerProvider).go(AppRoutes.hosts);
      await settle(tester);
    });
  }
}
