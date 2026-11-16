import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_view.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_alert_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';

Future<(ProviderContainer, MockBackend)> _open(
  WidgetTester tester,
  MonitoringAlertTestService service, {
  MonitoringTestStore? store,
  Size size = const Size(1500, 1100),
  AppLocale locale = AppLocale.en,
  bool dark = false,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned alert UI profile');
  if (dark) {
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    backend,
    size: size,
    locale: locale,
    captureBoundary: true,
    overrides: [monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(service: service, store: store))],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  return (container, backend);
}

void main() {
  testWidgets(
    'explicit read preserves partial warning and unknown value without inventing healthy zero or following annotation URL',
    (tester) async {
      final service = MonitoringAlertTestService();
      await _open(tester, service);
      expect(service.reads, isEmpty);
      await tapKey(tester, 'monitoring-read-alerts');
      expect(service.reads.single.$3, MonitoringAlertKind.alerts);
      final l = tester.element(find.byType(MonitoringAlertsView)).l10n;
      expect(tester.getSize(find.byKey(const ValueKey('monitoring-active-alerts'))).height, lessThan(100));
      expect(find.byKey(const ValueKey('monitoring-alerts-partial')), findsOneWidget);
      expect(find.text(l.monitoringWarning('Fixture partial backend warning')), findsOneWidget);
      expect(find.text(l.monitoringAlertFiring), findsOneWidget);
      await tester.ensureVisible(find.text('FixtureCPU'));
      await tester.tap(find.text('FixtureCPU'));
      await settle(tester);
      expect(find.text('NaN'), findsOneWidget);
      await tester.ensureVisible(find.text(l.monitoringAlertAnnotations));
      await tester.tap(find.text(l.monitoringAlertAnnotations));
      await settle(tester);
      expect(find.textContaining('https://untrusted.example.invalid/do-not-open'), findsOneWidget);
      expect(service.reads.length, 1);
      expect(service.queries, 0);
      expect(tester.takeException(), isNull);
    },
  );
  for (final kind in MonitoringAlertKind.values) {
    final key = kind == MonitoringAlertKind.alerts ? 'monitoring-read-alerts' : 'monitoring-read-alert-rules';
    testWidgets('cancel $kind uses existing workspace owner and suppresses late completion', (tester) async {
      final service = MonitoringAlertTestService()..pendingAlerts = Completer<MonitoringAlertSnapshot>();
      await _open(tester, service);
      await tapKey(tester, key);
      await tapKey(tester, 'monitoring-alerts-cancel');
      service.pendingAlerts!.complete(MonitoringAlertSnapshot.fromJson(kind, alertEnvelope(kind: kind)));
      await settle(tester);
      expect(service.cancellations, greaterThan(0));
      expect(find.byKey(const ValueKey('monitoring-alerts-status')), findsNothing);
      expect(service.reads.length, 1);
      expect(tester.takeException(), isNull);
    });
    testWidgets('navigation away during $kind discards response and returning never automatically rereads', (
      tester,
    ) async {
      final service = MonitoringAlertTestService()..pendingAlerts = Completer<MonitoringAlertSnapshot>();
      final (container, _) = await _open(tester, service);
      await tapKey(tester, key);
      container.read(routerProvider).go(AppRoutes.terminal);
      await settle(tester);
      service.pendingAlerts!.complete(MonitoringAlertSnapshot.fromJson(kind, alertEnvelope(kind: kind)));
      await settle(tester);
      container.read(routerProvider).go(AppRoutes.monitoring);
      await settle(tester);
      expect(service.reads.length, 1);
      expect(find.byKey(const ValueKey('monitoring-alerts-status')), findsNothing);
      expect(service.cancellations, greaterThan(0));
      expect(tester.takeException(), isNull);
    });
    testWidgets('actual widget profile lock cancels $kind and cannot render its late records', (tester) async {
      final service = MonitoringAlertTestService()..pendingAlerts = Completer<MonitoringAlertSnapshot>();
      final (container, _) = await _open(tester, service);
      await tapKey(tester, key);
      await container.read(vaultServiceProvider).lock();
      await settle(tester);
      service.pendingAlerts!.complete(MonitoringAlertSnapshot.fromJson(kind, alertEnvelope(kind: kind)));
      await settle(tester);
      expect(find.byType(MonitoringWorkspace), findsNothing);
      expect(find.text('FixtureCPU'), findsNothing);
      expect(service.cancellations, greaterThan(0));
      expect(tester.takeException(), isNull);
    });
  }
  testWidgets('replacing the profile service disposes old owner and does not replay an alert read', (tester) async {
    final old = MonitoringAlertTestService()..pendingAlerts = Completer<MonitoringAlertSnapshot>();
    final (container, backend) = await _open(tester, old);
    await tapKey(tester, 'monitoring-read-alerts');
    final fresh = MonitoringAlertTestService();
    container.updateOverrides([
      appServicesProvider.overrideWithValue(backend.services),
      monitoringBackendProvider.overrideWithValue(MonitoringTestBackend(service: fresh)),
    ]);
    await settle(tester);
    old.pendingAlerts!.complete(MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, alertEnvelope()));
    await settle(tester);
    expect(old.cancellations, greaterThan(0));
    expect(fresh.reads, isEmpty);
    expect(find.byKey(const ValueKey('monitoring-alerts-status')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('changing selected saved source never accepts old source alert result', (tester) async {
    final second = monitoringFixture.withRecord(id: '22222222-2222-4222-8222-222222222222', revision: 4);
    final store = MonitoringTestStore(initial: [monitoringFixture, second]);
    final service = MonitoringAlertTestService()..pendingAlerts = Completer<MonitoringAlertSnapshot>();
    await _open(tester, service, store: store);
    await tapKey(tester, 'monitoring-read-alerts');
    final selector = tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).first);
    selector.onChanged!(second.id);
    await settle(tester);
    service.pendingAlerts!.complete(MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, alertEnvelope()));
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-alerts-status')), findsNothing);
    expect(service.reads.single.$1, monitoringFixture.id);
    expect(service.reads.length, 1);
    expect(tester.takeException(), isNull);
  });
  testWidgets('saving source revision during a pending status read invalidates prior generation', (tester) async {
    final service = MonitoringAlertTestService()..pendingAlerts = Completer<MonitoringAlertSnapshot>();
    final store = MonitoringTestStore();
    await _open(tester, service, store: store);
    await tapKey(tester, 'monitoring-read-alerts');
    final l = tester.element(find.byType(MonitoringWorkspace)).l10n;
    await tester.ensureVisible(find.text(l.monitoringEditSource));
    await tester.tap(find.text(l.monitoringEditSource));
    await settle(tester);
    await tapKey(tester, 'monitoring-save-source');
    expect(store.sources.single.revision, 2);
    service.pendingAlerts!.complete(MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, alertEnvelope()));
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-alerts-status')), findsNothing);
    expect(service.reads.single.$2, 1);
    expect(tester.takeException(), isNull);
  });
  testWidgets('actual profile switch disposes source-status owner and discards old response', (tester) async {
    final service = MonitoringAlertTestService()..pendingAlerts = Completer<MonitoringAlertSnapshot>();
    final (_, backend) = await _open(tester, service);
    await tapKey(tester, 'monitoring-read-alert-rules');
    await backend.debugCreateUnlockedLocalProfile(name: 'Other owned alert profile');
    await settle(tester);
    service.pendingAlerts!.complete(
      MonitoringAlertSnapshot.fromJson(
        MonitoringAlertKind.alertRules,
        alertEnvelope(kind: MonitoringAlertKind.alertRules),
      ),
    );
    await settle(tester);
    expect(find.text('Fixture group'), findsNothing);
    expect(service.reads.length, 1);
    expect(service.cancellations, greaterThan(0));
    expect(tester.takeException(), isNull);
  });
  testWidgets('partial backend rules show unknown state health failure opaque file and expression as passive text', (
    tester,
  ) async {
    final service = MonitoringAlertTestService()
      ..response = alertEnvelope(kind: MonitoringAlertKind.alertRules, status: 'error');
    await _open(tester, service);
    await tapKey(tester, 'monitoring-read-alert-rules');
    final l = tester.element(find.byType(MonitoringAlertsView)).l10n;
    expect(find.byKey(const ValueKey('monitoring-alerts-partial')), findsOneWidget);
    expect(find.text(l.monitoringAlertContinuation), findsOneWidget);
    await tester.ensureVisible(find.text('Fixture group'));
    await tester.tap(find.text('Fixture group'));
    await settle(tester);
    expect(find.text('/backend/opaque.yml'), findsOneWidget);
    await tester.ensureVisible(find.text('Fixture CPU rule'));
    await tester.tap(find.text('Fixture CPU rule'));
    await settle(tester);
    expect(find.text(l.monitoringAlertUnknown), findsOneWidget);
    expect(find.text(l.monitoringRuleHealthError), findsOneWidget);
    await tester.ensureVisible(find.text(l.monitoringRuleExpression));
    await tester.tap(find.text(l.monitoringRuleExpression));
    await settle(tester);
    expect(find.text('rate(example_total{job="quoted"}[5m]) > 1'), findsOneWidget);
    expect(service.reads.length, 1);
    expect(service.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Native transport diagnostic stays localized instead of displaying raw implementation error', (
    tester,
  ) async {
    final service = MonitoringAlertTestService()
      ..alertError = const MonitoringFailure('transport', 'Native-only raw diagnostic must not be shown');
    await _open(tester, service);
    await tapKey(tester, 'monitoring-read-alerts');
    final l = tester.element(find.byType(MonitoringWorkspace)).l10n;
    expect(find.text(l.monitoringErrorTransport), findsOneWidget);
    expect(find.textContaining('Native-only raw'), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-alerts-status')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian current alerts and rules remain bounded at width $width', (tester) async {
      await loadAutomationProofFonts(tester);
      final rules = width > 1000, service = MonitoringAlertTestService();
      await _open(tester, service, size: Size(width, 1100), locale: AppLocale.ru, dark: true);
      await tapKey(tester, rules ? 'monitoring-read-alert-rules' : 'monitoring-read-alerts');
      final l = tester.element(find.byType(MonitoringAlertsView)).l10n;
      expect(
        tester
            .getSize(find.byKey(ValueKey(rules ? 'monitoring-alert-rule-groups' : 'monitoring-active-alerts')))
            .height,
        lessThan(100),
      );
      if (rules) {
        await tester.ensureVisible(find.text('Fixture group'));
        await tester.tap(find.text('Fixture group'));
        await settle(tester);
        await tester.ensureVisible(find.text('Fixture CPU rule'));
        await tester.tap(find.text('Fixture CPU rule'));
        await settle(tester);
      } else {
        await tester.ensureVisible(find.text('FixtureCPU'));
        await tester.tap(find.text('FixtureCPU'));
        await settle(tester);
      }
      await tester.ensureVisible(find.text(l.monitoringAlertsTitle));
      await settle(tester);
      expect(find.text(l.monitoringAlertsTitle), findsOneWidget);
      expect(find.byKey(const ValueKey('monitoring-alerts-partial')), findsOneWidget);
      await captureAutomationWorkspace(tester, 'monitoring-alerts-${rules ? 'rules-wide' : 'current-narrow'}-ru');
      expect(service.reads.length, 1);
      expect(service.queries, 0);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
