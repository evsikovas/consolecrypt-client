import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_browser.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_open.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/test_app.dart';

const address = 'http://10.10.10.11:3000/d/owned/metrics?orgId=1';

class _Browser {
  final calls = <Uri>[];
  Completer<bool>? pending;
  bool result = true, fail = false;
  Future<bool> launch(Uri uri) async {
    calls.add(uri);
    if (fail) throw StateError('private platform diagnostics');
    return pending?.future ?? result;
  }
}

Future<(ProviderContainer, MockBackend, SavedPanelTestService, SavedPanelTestStore)> _open(
  WidgetTester tester,
  _Browser browser, {
  double width = 1500,
  bool russian = false,
}) async {
  final backend = testBackend(), service = SavedPanelTestService(), store = SavedPanelTestStore();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned browser profile');
  if (russian) {
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    backend,
    size: Size(width, 1100),
    locale: russian ? AppLocale.ru : AppLocale.en,
    captureBoundary: true,
    overrides: [
      monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(service: service, store: store)),
      monitoringGrafanaBrowserLauncherProvider.overrideWithValue(browser.launch),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  await tapKey(tester, 'saved-panel-open-grafana');
  return (container, backend, service, store);
}

Future<void> review(WidgetTester tester, [String value = address]) async {
  await enterKey(tester, 'grafana-open-address', value);
  await tapKey(tester, 'grafana-open-review');
}

void main() {
  testWidgets('browser destination starts empty and is never inferred from the selected source', (tester) async {
    final browser = _Browser();
    final (_, _, service, store) = await _open(tester, browser);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('grafana-open-address'))).controller!.text, isEmpty);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-open-confirm'))).onPressed, isNull);
    expect(browser.calls, isEmpty);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('review shows exact HTTP destination; only separate Open launches it once without metric or save calls', (
    tester,
  ) async {
    final browser = _Browser();
    final (_, _, service, store) = await _open(tester, browser);
    await review(tester);
    expect(browser.calls, isEmpty);
    expect(tester.widget<SelectableText>(find.byKey(const ValueKey('grafana-open-exact-uri'))).data, address);
    await tapKey(tester, 'grafana-open-confirm');
    expect(browser.calls.map((uri) => uri.toString()).toList(), [address]);
    expect(find.byType(MonitoringGrafanaOpen), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(service.reads, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('editing reviewed address revokes Open until the new exact address is reviewed', (tester) async {
    final browser = _Browser();
    await _open(tester, browser);
    await review(tester);
    const changed = 'https://grafana.example/d/other';
    await enterKey(tester, 'grafana-open-address', changed);
    expect(find.byKey(const ValueKey('grafana-open-exact-uri')), findsNothing);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-open-confirm'))).onPressed, isNull);
    await tapKey(tester, 'grafana-open-review');
    await tapKey(tester, 'grafana-open-confirm');
    expect(browser.calls.single.toString(), changed);
    expect(tester.takeException(), isNull);
  });
  testWidgets('invalid schemes and embedded credentials render a fixed framed error without launching', (tester) async {
    final browser = _Browser();
    await _open(tester, browser);
    for (final value in ['javascript:owned', 'file:///owned', 'https://user:private@grafana.example/']) {
      await review(tester, value);
      expect(find.byKey(const ValueKey('grafana-open-error')), findsOneWidget);
      final l = tester.element(find.byType(MonitoringGrafanaOpen)).l10n;
      expect(find.text(l.grafanaOpenInvalid), findsOneWidget);
      expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-open-confirm'))).onPressed, isNull);
    }
    expect(browser.calls, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('platform failure is framed and contains no platform diagnostics', (tester) async {
    final browser = _Browser()..fail = true;
    await _open(tester, browser);
    await review(tester);
    await tapKey(tester, 'grafana-open-confirm');
    expect(browser.calls, hasLength(1));
    expect(find.byKey(const ValueKey('grafana-open-error')), findsOneWidget);
    expect(find.textContaining('private platform'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('lock after Review denies the captured old Open callback', (tester) async {
    final browser = _Browser();
    final (container, _, service, store) = await _open(tester, browser);
    await review(tester);
    final callback = tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-open-confirm'))).onPressed!;
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    callback();
    await settle(tester);
    expect(browser.calls, isEmpty);
    expect(find.byType(MonitoringGrafanaOpen), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('cancel/profile switch denies late launcher completion without reopening or relaunching', (tester) async {
    final browser = _Browser()..pending = Completer<bool>();
    final (_, backend, service, store) = await _open(tester, browser);
    await review(tester);
    await tapKey(tester, 'grafana-open-confirm');
    final l = tester.element(find.byType(MonitoringGrafanaOpen)).l10n;
    await tester.tap(find.descendant(of: find.byType(MonitoringGrafanaOpen), matching: find.text(l.commonCancel)));
    await settle(tester);
    await backend.debugCreateUnlockedLocalProfile(name: 'Next owned browser profile');
    await settle(tester);
    browser.pending!.complete(true);
    await settle(tester);
    expect(browser.calls, hasLength(1));
    expect(find.byType(MonitoringGrafanaOpen), findsNothing);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('mobile uses the same explicit external-launch seam without Native query or desktop process', (
    tester,
  ) async {
    final browser = _Browser();
    final (_, _, service, store) = await _open(tester, browser, width: 430);
    await review(tester);
    await tapKey(tester, 'grafana-open-confirm');
    expect(browser.calls.single.toString(), address);
    expect(store.saves, isEmpty);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android}));
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian reviewed Grafana address at width $width', (tester) async {
      await loadAutomationProofFonts(tester);
      final browser = _Browser();
      await _open(tester, browser, width: width, russian: true);
      await review(tester);
      await captureAutomationWorkspace(tester, 'grafana-open-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(browser.calls, isEmpty);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
