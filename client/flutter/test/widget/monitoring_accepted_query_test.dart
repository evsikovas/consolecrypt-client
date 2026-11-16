import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/monitoring_accepted_query_view.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
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

Future<ProviderContainer> _open(WidgetTester tester, _Backend api, {double width = 1600, bool russian = false}) async {
  final app = testBackend();
  addTearDown(app.dispose);
  await app.debugCreateUnlockedLocalProfile(name: 'Owned accepted-result profile');
  api.switchProfile = () async {
    await app.debugCreateUnlockedLocalProfile(name: 'Other accepted-result profile');
  };
  if (russian) {
    final settings = app.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    app,
    size: Size(width, 1100),
    locale: russian ? AppLocale.ru : AppLocale.en,
    captureBoundary: true,
    overrides: [monitoringBackendProvider.overrideWithValue(api)],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  // These regressions exercise explicit MANUAL refresh retention. The default
  // Auto owner and its typed/range/scope gates are covered separately in D2.
  await tapKey(tester, 'monitoring-auto-settings');
  final auto = find.byKey(const ValueKey('monitoring-auto-enabled'));
  await Scrollable.ensureVisible(tester.element(auto), alignment: .5);
  await settle(tester);
  await tester.tap(auto);
  await settle(tester);
  expect(tester.widget<CheckboxListTile>(auto).value, isFalse);
  final pin = container.read(monitoringPinnedObservationProvider);
  addTearDown(() => pin?.stop(MonitoringPinStop.manual));
  return container;
}

Future<void> _tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await Scrollable.ensureVisible(tester.element(finder), alignment: 0.5);
  await settle(tester);
  await tester.tap(finder);
  await settle(tester);
}

Future<void> _failRefresh(WidgetTester tester, MonitoringTestService service) async {
  service.pending = Completer<QueryOutcome>();
  await _tap(tester, 'monitoring-query');
  service.pending!.completeError(const MonitoringFailure.local(MonitoringErrorCode.timeout));
  await settle(tester);
  service.pending = null;
}

void main() {
  testWidgets(
    'explicit same-query failed refresh retains accepted partial NaN and last-success with framed stale error',
    (tester) async {
      final api = _Backend();
      await _open(tester, api);
      await _tap(tester, 'monitoring-query');
      final result = api.services.single.result;
      final last = tester.widget<Text>(find.byKey(const ValueKey('monitoring-read-last-success'))).data;
      api.services.single.pending = Completer<QueryOutcome>();
      await _tap(tester, 'monitoring-query');
      expect(find.byKey(const ValueKey('monitoring-previous-refreshing')), findsOneWidget);
      expect(tester.widget<MonitoringResultView>(find.byType(MonitoringResultView)).result, same(result));
      api.services.single.pending!.completeError(const MonitoringFailure.local(MonitoringErrorCode.timeout));
      await settle(tester);
      expect(find.byKey(const ValueKey('monitoring-previous-stale')), findsOneWidget);
      expect(find.byKey(const ValueKey('monitoring-refresh-error')), findsOneWidget);
      expect(tester.widget<Text>(find.byKey(const ValueKey('monitoring-read-last-success'))).data, last);
      expect(tester.widget<MonitoringResultView>(find.byType(MonitoringResultView)).result, same(result));
      expect(api.services.single.queries, 2);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('successful empty response replaces prior nonempty data and removes stale error', (tester) async {
    final api = _Backend();
    await _open(tester, api);
    await _tap(tester, 'monitoring-query');
    await _failRefresh(tester, api.services.single);
    api.services.single.result = const QueryOutcome(
      status: 'success',
      httpStatus: 200,
      data: QueryData('vector', series: []),
    );
    await _tap(tester, 'monitoring-query');
    expect(tester.widget<MonitoringResultView>(find.byType(MonitoringResultView)).result.data!.series, isEmpty);
    expect(find.byKey(const ValueKey('monitoring-previous-stale')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-refresh-error')), findsNothing);
    expect(api.services.single.queries, 3);
    expect(tester.takeException(), isNull);
  });
  testWidgets('failed Native outcome remains explicitly revealable beside accepted previous values', (tester) async {
    final api = _Backend();
    await _open(tester, api);
    await _tap(tester, 'monitoring-query');
    api.services.single.result = const QueryOutcome(
      status: 'error',
      httpStatus: 503,
      partial: true,
      error: MonitoringFailure.local(MonitoringErrorCode.timeout),
      infos: ['Bounded failed attempt info'],
    );
    await _tap(tester, 'monitoring-query');
    expect(find.byKey(const ValueKey('monitoring-failed-attempt')), findsOneWidget);
    await _tap(tester, 'monitoring-failed-attempt');
    expect(find.textContaining('Bounded failed attempt info'), findsOneWidget);
    expect(find.byType(MonitoringResultView), findsNWidgets(2));
    expect(tester.takeException(), isNull);
  });
  testWidgets('expression edit resets accepted state without issuing another request', (tester) async {
    final api = _Backend();
    await _open(tester, api);
    await _tap(tester, 'monitoring-query');
    await tester.enterText(find.byKey(const ValueKey('monitoring-expression')), 'other');
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-read-last-success')), findsNothing);
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(api.services.single.queries, 1);
    expect(tester.takeException(), isNull);
  });
  testWidgets('programmatic metadata-style expression edit cancels pending custom read and rejects late output', (
    tester,
  ) async {
    final api = _Backend();
    await _open(tester, api);
    final service = api.services.single..pending = Completer<QueryOutcome>();
    await _tap(tester, 'monitoring-query');
    tester.widget<TextField>(find.byKey(const ValueKey('monitoring-expression'))).controller!.text =
        'metadata_inserted_metric';
    await settle(tester);
    service.pending!.complete(service.result);
    await settle(tester);
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-read-last-success')), findsNothing);
    expect(service.queries, 1);
    expect(service.cancellations, greaterThan(0));
    expect(tester.takeException(), isNull);
  });
  for (final change in ['source', 'hide', 'lock', 'profile']) {
    testWidgets('$change resets accepted state and ignores pending late response', (tester) async {
      final api = _Backend();
      if (change == 'source') {
        api.store.sources = [monitoringFixture, monitoringFixture.withRecord(id: 'other', revision: 1)];
      }
      final container = await _open(tester, api);
      await _tap(tester, 'monitoring-query');
      final service = api.services.single..pending = Completer<QueryOutcome>();
      await _tap(tester, 'monitoring-query');
      switch (change) {
        case 'source':
          tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).first).onChanged!('other');
        case 'hide':
          container.read(routerProvider).go(AppRoutes.terminal);
        case 'lock':
          await container.read(vaultServiceProvider).lock();
        case 'profile':
          await api.switchProfile();
      }
      await settle(tester);
      service.pending!.complete(service.result);
      await settle(tester);
      expect(find.byKey(const ValueKey('monitoring-read-last-success')), findsNothing);
      expect(find.byType(MonitoringAcceptedQueryView), findsNothing);
      expect(service.queries, 2);
      expect(tester.takeException(), isNull);
    });
  }
  testWidgets(
    'presentation viewport leaves accepted snapshot and read count unchanged; period edit resets without querying',
    (tester) async {
      final api = _Backend();
      await _open(tester, api);
      tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-range'))).onChanged!(true);
      await settle(tester);
      await _tap(tester, 'monitoring-query');
      final view = tester.widget<MonitoringResultView>(find.byType(MonitoringResultView));
      final controller = view.timeWindow!, before = controller.applied;
      final last = tester.widget<Text>(find.byKey(const ValueKey('monitoring-read-last-success'))).data;
      controller.zoom(.5);
      controller.pan(-.2);
      await settle(tester);
      expect(controller.applied, before);
      expect(controller.viewport.value, isNot(before));
      expect(tester.widget<Text>(find.byKey(const ValueKey('monitoring-read-last-success'))).data, last);
      expect(api.services.single.queries, 1);
      controller.edit(step: '30');
      await settle(tester);
      expect(find.byType(MonitoringAcceptedQueryView), findsNothing);
      expect(find.byKey(const ValueKey('monitoring-read-last-success')), findsNothing);
      expect(api.services.single.queries, 1);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('saved source revision change clears accepted measurements before any fresh manual read', (tester) async {
    final api = _Backend();
    await _open(tester, api);
    await _tap(tester, 'monitoring-query');
    final l = tester.element(find.byType(MonitoringAcceptedQueryView)).l10n;
    final edit = find.text(l.monitoringEditSource);
    await Scrollable.ensureVisible(tester.element(edit), alignment: .5);
    await settle(tester);
    await tester.tap(edit);
    await settle(tester);
    await _tap(tester, 'monitoring-save-source');
    expect(api.store.sources.single.revision, 2);
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-read-last-success')), findsNothing);
    expect(api.services.single.queries, 1);
    expect(tester.takeException(), isNull);
  });
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian stale and last-success context readable at width $width', (tester) async {
      await loadAutomationProofFonts(tester);
      final api = _Backend();
      await _open(tester, api, width: width, russian: true);
      await _tap(tester, 'monitoring-query');
      await _failRefresh(tester, api.services.single);
      await Scrollable.ensureVisible(tester.element(find.byType(MonitoringAcceptedQueryView)), alignment: .15);
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-stale-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(find.byKey(const ValueKey('monitoring-previous-stale')), findsOneWidget);
      expect(find.byKey(const ValueKey('monitoring-read-last-success')), findsOneWidget);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
  testWidgets('normal custom-result cancellation leaves separate instant pin and global Stop working', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _tap(tester, 'monitoring-query');
    expect(api.services, hasLength(1));
    await _tap(tester, 'monitoring-observation-pin');
    expect(api.services, hasLength(2));
    expect(api.services.last.queries, 0);
    final owner = container.read(monitoringPinnedObservationProvider)!;
    await _tap(tester, 'monitoring-query');
    api.services.first.pending = Completer<QueryOutcome>();
    await _tap(tester, 'monitoring-query');
    await _tap(tester, 'monitoring-cancel');
    api.services.first.pending!.complete(api.services.first.result);
    await settle(tester);
    expect(owner.state.active, isTrue);
    expect(api.services, hasLength(2));
    expect(api.services.last.queries, 0);
    expect(api.services.last.cancellations, 0);
    expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-read-last-success')), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
