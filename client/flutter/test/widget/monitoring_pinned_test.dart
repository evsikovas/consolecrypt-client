import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';

class _Backend implements MonitoringBackend {
  final store = MonitoringTestStore();
  late Future<void> Function() switchProfile;
  final services = <MonitoringTestService>[];
  @override
  MonitoringSourceStore createSourceStore() => store;
  @override
  MonitoringService createService() {
    final service = MonitoringTestService();
    services.add(service);
    return service;
  }
}

Future<ProviderContainer> _open(WidgetTester tester, _Backend api, {double width = 1400, bool russian = false}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned pinned profile');
  api.switchProfile = () async {
    await backend.debugCreateUnlockedLocalProfile(name: 'Other owned pinned profile');
  };
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
    overrides: [monitoringBackendProvider.overrideWithValue(api)],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  await tapKey(tester, 'monitoring-auto-settings');
  final owner = container.read(monitoringPinnedObservationProvider);
  addTearDown(() => owner?.stop(MonitoringPinStop.manual));
  return container;
}

Future<void> _pinAccepted(WidgetTester tester) async {
  await tapKey(tester, 'monitoring-query');
  await tapKey(tester, 'monitoring-observation-pin');
}

void main() {
  testWidgets('accepted manual read seeds one dedicated service; explicit pin retains global Stop in hidden terminal', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    expect(api.services, hasLength(1));
    await tester.enterText(find.byKey(const ValueKey('monitoring-expression')), 'up{job="explicit"}');
    await settle(tester);
    expect(api.services.single.queries, 0);
    await _pinAccepted(tester);
    expect(api.services, hasLength(2));
    final pinned = api.services.last;
    expect(container.read(monitoringPinnedObservationProvider)!.state.plan!.expression, 'up{job="explicit"}');
    expect(pinned.last, isNull);
    expect(pinned.queries, 0);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(api.services.first.cancellations, greaterThan(0));
    expect(pinned.cancellations, 0);
    expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsOneWidget);
    await tester.pump(const Duration(seconds: 60));
    await settle(tester);
    expect(pinned.queries, 4);
    expect(pinned.last!.expression, 'up{job="explicit"}');
    expect(pinned.last!.range, isFalse);
    await tapKey(tester, 'monitoring-pin-global-stop');
    expect(pinned.cancellations, 1);
    expect(container.read(monitoringPinnedObservationProvider)!.state.outcome, isNull);
    await tester.pump(const Duration(minutes: 5));
    expect(pinned.queries, 4);
    expect(tester.takeException(), isNull);
  });
  for (final change in ['removed', 'revision']) {
    testWidgets('$change source stops hidden pin before next Native query', (tester) async {
      final api = _Backend();
      final container = await _open(tester, api);
      await _pinAccepted(tester);
      container.read(routerProvider).go(AppRoutes.terminal);
      await settle(tester);
      api.store.sources = change == 'removed' ? [] : [monitoringFixture.withRecord(revision: 2)];
      await tester.pump(const Duration(seconds: 60));
      await settle(tester);
      expect(api.services.last.queries, 0);
      final value = container.read(monitoringPinnedObservationProvider)!.state;
      expect(value.stopReason, MonitoringPinStop.sourceChanged);
      expect(value.outcome, isNull);
      expect(tester.takeException(), isNull);
    });
  }
  testWidgets('profile lock cancels dedicated pending read and clears a late response', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _pinAccepted(tester);
    final owner = container.read(monitoringPinnedObservationProvider)!;
    final service = api.services.last..pending = Completer<QueryOutcome>();
    owner.refresh();
    await settle(tester);
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pending!.complete(service.result);
    await settle(tester);
    expect(owner.state.plan, isNull);
    expect(owner.state.outcome, isNull);
    expect(service.cancellations, 1);
    expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('acknowledged source save stops pin immediately before its next interval', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _pinAccepted(tester);
    final owner = container.read(monitoringPinnedObservationProvider)!;
    final l = tester.element(find.byKey(const ValueKey('monitoring-observation-pin'))).l10n;
    final edit = find.text(l.monitoringEditSource);
    await Scrollable.ensureVisible(tester.element(edit), alignment: 0.5);
    await settle(tester);
    await tester.tap(edit);
    await settle(tester);
    final save = find.byKey(const ValueKey('monitoring-save-source'));
    await Scrollable.ensureVisible(tester.element(save), alignment: 0.5);
    await settle(tester);
    await tester.tap(save);
    await settle(tester);
    expect(api.store.sources.single.revision, 2);
    expect(owner.state.stopReason, MonitoringPinStop.sourceChanged);
    expect(owner.state.outcome, isNull);
    expect(api.services.last.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets(
    'failed refresh uses boxed danger banner while accepted snapshot and last success stay explicitly stale',
    (tester) async {
      final api = _Backend();
      final container = await _open(tester, api);
      await _pinAccepted(tester);
      final owner = container.read(monitoringPinnedObservationProvider)!;
      final accepted = owner.state.outcome, success = owner.state.lastSuccessUtc;
      api.services.last.result = const QueryOutcome(
        status: 'error',
        httpStatus: 503,
        error: MonitoringFailure.local(MonitoringErrorCode.timeout),
      );
      owner.refresh();
      await settle(tester);
      expect(owner.state.outcome, same(accepted));
      expect(owner.state.lastSuccessUtc, success);
      expect(owner.state.phase, MonitoringPinPhase.backoff);
      final banner = tester.widget<InfoBanner>(find.byKey(const ValueKey('monitoring-pin-error')));
      expect(banner.tone, BannerTone.danger);
      final l = tester.element(find.byKey(const ValueKey('monitoring-pin-status'))).l10n;
      expect(find.text(l.monitoringPinStale), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('range draft cannot replace an already accepted pinned query', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _pinAccepted(tester);
    await tester.enterText(find.byKey(const ValueKey('monitoring-expression')), 'changed_draft');
    tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-range'))).onChanged!(true);
    await settle(tester);
    final button = tester.widget<GlassButton>(find.byKey(const ValueKey('monitoring-observation-pin')));
    expect(button.onPressed, isNull);
    expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-range'))).value, isTrue);
    expect(container.read(monitoringPinnedObservationProvider)!.state.plan!.expression, 'up');
    expect(api.services.last.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('switching profile clears old owner and never automatically starts another pin', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _pinAccepted(tester);
    final owner = container.read(monitoringPinnedObservationProvider)!;
    final pinned = api.services.last;
    await api.switchProfile();
    await settle(tester);
    expect(owner.state.plan, isNull);
    expect(owner.state.outcome, isNull);
    expect(pinned.cancellations, 1);
    expect(identical(container.read(monitoringPinnedObservationProvider), owner), isFalse);
    await tester.pump(const Duration(minutes: 5));
    expect(pinned.queries, 0);
    expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('desktop inactive keeps pin; paused clears and resume does not restart', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _pinAccepted(tester);
    final owner = container.read(monitoringPinnedObservationProvider)!;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    await tester.pump(const Duration(seconds: 60));
    await settle(tester);
    expect(owner.state.active, isTrue);
    expect(api.services.last.queries, 4);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await settle(tester);
    expect(owner.state.outcome, isNull);
    expect(owner.state.plan, isNull);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump(const Duration(minutes: 5));
    await settle(tester);
    expect(api.services.last.queries, 4);
    expect(owner.state.active, isFalse);
    expect(tester.takeException(), isNull);
  });
  testWidgets('mobile hidden branch retains visible global Stop', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api, width: 430);
    await _pinAccepted(tester);
    final pinned = api.services.last;
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsOneWidget);
    expect(pinned.cancellations, 0);
    await tapKey(tester, 'monitoring-pin-global-stop');
    expect(pinned.cancellations, 1);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android}));
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian current values pin readable at width $width', (tester) async {
      await loadAutomationProofFonts(tester);
      final api = _Backend();
      await _open(tester, api, width: width, russian: true);
      await _pinAccepted(tester);
      await Scrollable.ensureVisible(
        tester.element(find.byKey(const ValueKey('monitoring-pin-view'))),
        alignment: 0.15,
      );
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-pin-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsOneWidget);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
