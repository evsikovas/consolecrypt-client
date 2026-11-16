import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_observation_result.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_view.dart' show monitoringPinIntervalLabel;
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/test_app.dart';

class _Store extends SavedPanelTestStore implements MonitoringBindingStore {
  @override
  List<MonitoringBinding> bindings = [bindingFixture()];
  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('No catalog mutation in this read fixture');
}

class _Service extends MonitoringBindingReadTestService implements MonitoringSavedPanelReader {
  final savedReads = <(String, int, SavedPanelRange?)>[];
  @override
  Future<SavedPanelRead> readSavedPanel(
    MonitoringSource source,
    SavedMonitoringPanel panel, {
    SavedPanelRange? window,
  }) async {
    savedReads.add((panel.id, panel.revision, window));
    return SavedPanelRead(result, panel.definition.unitHint);
  }
}

class _Backend implements MonitoringBackend {
  final store = _Store();
  final services = <_Service>[];
  @override
  MonitoringSourceStore createSourceStore() => store;
  @override
  MonitoringService createService() {
    final service = _Service();
    services.add(service);
    return service;
  }
}

Future<ProviderContainer> _open(WidgetTester tester, _Backend api, {double width = 1500, double height = 1100, bool russian = false, bool expandSettings = true}) async {
  await loadAutomationProofFonts(tester);
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned observation profile');
  final settings = backend.services.settings;
  await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  await pumpApp(
    tester,
    backend,
    size: Size(width, height),
    locale: russian ? AppLocale.ru : AppLocale.en,
    captureBoundary: true,
    overrides: [monitoringBackendProvider.overrideWithValue(api)],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  if (expandSettings) await tapKey(tester, 'monitoring-auto-settings');
  final owner = container.read(monitoringPinnedObservationProvider);
  addTearDown(() => owner?.stop(MonitoringPinStop.manual));
  return container;
}

Future<void> _range(WidgetTester tester) async {
  tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-range'))).onChanged!(true);
  await settle(tester);
}

void main() {
  testWidgets('summary follows actual15s plan through edit30 and foreground unpin until stop', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _range(tester);
    await tapKey(tester, 'monitoring-query');
    await tapKey(tester, 'monitoring-observation-pin');
    final owner = container.read(monitoringPinnedObservationProvider)!;
    final original = owner.state.plan!;
    expect(original.interval, MonitoringPinInterval.seconds15);
    final requests = api.services.fold<int>(0, (n, service) => n + service.queries);
    tester.widget<GlassSelect<MonitoringPinInterval>>(find.byKey(const ValueKey('monitoring-auto-interval')))
      .onChanged!(MonitoringPinInterval.seconds30);
    await settle(tester);
    final summary = find.byKey(const ValueKey('monitoring-auto-summary'));
    final l = tester.element(summary).l10n;
    String text() => tester.widget<Text>(summary).data!;
    expect(text(), contains(monitoringPinIntervalLabel(l, MonitoringPinInterval.seconds15)));
    expect(text(), isNot(contains(monitoringPinIntervalLabel(l, MonitoringPinInterval.seconds30))));
    expect(owner.state.plan, same(original));
    await tapKey(tester, 'monitoring-observation-unpin');
    expect(owner.state.active, true);
    expect(owner.state.visibility, MonitoringObservationVisibility.foregroundOnly);
    expect(text(), contains(monitoringPinIntervalLabel(l, MonitoringPinInterval.seconds15)));
    await tapKey(tester, 'monitoring-pin-stop');
    expect(owner.state.active, false);
    expect(text(), contains(monitoringPinIntervalLabel(l, MonitoringPinInterval.seconds30)));
    expect(api.services.fold<int>(0, (n, service) => n + service.queries), requests);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('narrow observation summary is compact and opening or editing settings starts zero queries',(tester)async{
    final api=_Backend();final container=await _open(tester,api,width:800,height:600,russian:true,expandSettings:false);
    final card=find.byKey(const ValueKey('monitoring-auto-controls'));
    expect(tester.getSize(card).height,lessThanOrEqualTo(132));
    expect(find.byKey(const ValueKey('monitoring-auto-summary')),findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-auto-enabled')),findsNothing);
    expect(api.services.length,1);expect(api.services.single.queries,0);
    await tapKey(tester,'monitoring-auto-settings');
    expect(find.byKey(const ValueKey('monitoring-auto-enabled')),findsOneWidget);
    tester.widget<GlassSelect<MonitoringPinInterval>>(find.byKey(const ValueKey('monitoring-auto-interval'))).onChanged!(MonitoringPinInterval.seconds30);
    await settle(tester);expect(api.services.length,1);expect(api.services.single.queries,0);
    await tapKey(tester,'monitoring-auto-settings');await tester.pump(const Duration(seconds:60));
    expect(api.services.single.queries,0);expect(tester.takeException(),isNull);
    container.read(monitoringPinnedObservationProvider)!.stop(MonitoringPinStop.manual);await settle(tester);
    final summary=find.byKey(const ValueKey('monitoring-auto-summary'));
    expect(tester.widget<Text>(summary).data,contains(tester.element(summary).l10n.monitoringPinStopped));
  },variant:const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('visible Auto default15 before manual Read; opening and editing do not allocate or query', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-auto-enabled'))).value, isTrue);
    final cadence = tester.widget<GlassSelect<MonitoringPinInterval>>(
      find.byKey(const ValueKey('monitoring-auto-interval')),
    );
    expect(cadence.value, MonitoringPinInterval.seconds15);
    await enterKey(tester, 'monitoring-expression', 'rate(owned_total[5m])');
    await _range(tester);
    tester.widget<GlassSelect<MonitoringTimePreset>>(find.byKey(const ValueKey('monitoring-time-preset'))).onChanged!(
      MonitoringTimePreset.hours6,
    );
    await settle(tester);
    await tapKey(tester, 'monitoring-time-apply-window');
    expect(api.services, hasLength(1));
    expect(api.services.single.queries, 0);
    expect(container.read(monitoringPinnedObservationProvider)!.state.active, isFalse);
    await tester.pump(const Duration(seconds: 60));
    expect(api.services.single.queries, 0);
  });
  testWidgets('manual success seeds foreground with no duplicate; hidden route cancels foreground only', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await tapKey(tester, 'monitoring-query');
    expect(api.services, hasLength(2));
    expect(api.services.first.queries, 1);
    expect(api.services.last.queries, 0);
    final owner = container.read(monitoringPinnedObservationProvider)!;
    expect(owner.state.visibility, MonitoringObservationVisibility.foregroundOnly);
    expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsNothing);
    await tester.pump(const Duration(seconds: 15));
    await settle(tester);
    expect(api.services.last.queries, 1);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(owner.state.active, isFalse);
    expect(api.services.last.cancellations, 1);
    await tester.pump(const Duration(seconds: 60));
    expect(api.services.last.queries, 1);
  });
  testWidgets('pin promotes same range request and service; Terminal has global Stop without instant fallback', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _range(tester);
    await tapKey(tester, 'monitoring-query');
    final owner = container.read(monitoringPinnedObservationProvider)!,
        plan = owner.state.plan!,
        seed = owner.state.value!;
    expect(plan.target, isA<MonitoringCustomRangeTarget>());
    await tapKey(tester, 'monitoring-observation-pin');
    expect(api.services, hasLength(2));
    expect(api.services.last.queries, 0);
    expect(owner.state.value, same(seed));
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsOneWidget);
    await tester.pump(const Duration(seconds: 15));
    await settle(tester);
    expect(api.services.last.last!.range, isTrue);
    expect(api.services.last.last!.start, seed.window!.startSeconds);
    expect(api.services.last.last!.end, seed.window!.endSeconds);
    await tapKey(tester, 'monitoring-pin-global-stop');
    expect(owner.state.hasAcceptedResult, isFalse);
    await tester.pump(const Duration(minutes: 1));
    expect(api.services.last.queries, 1);
  });
  testWidgets('Auto off still allows explicit accepted pin with no duplicate first request', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-auto-enabled'))).onChanged!(false);
    await settle(tester);
    await tapKey(tester, 'monitoring-query');
    expect(api.services, hasLength(1));
    await tapKey(tester, 'monitoring-observation-pin');
    expect(api.services, hasLength(2));
    expect(api.services.last.queries, 0);
    expect(
      container.read(monitoringPinnedObservationProvider)!.state.visibility,
      MonitoringObservationVisibility.pinned,
    );
  });
  testWidgets('settings edit during pending manual read cannot arm from late success', (tester) async {
    final api = _Backend();
    await _open(tester, api);
    final normal = api.services.first..pending = Completer<QueryOutcome>();
    await tapKey(tester, 'monitoring-query');
    // Normal request is busy, so settings are disabled; capture its old callback
    // before route invalidation to exercise the actual lifetime fence instead.
    final auto = tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-auto-enabled')));
    expect(auto.onChanged, isNull);
    final context = tester.element(find.byType(Navigator).first), container = ProviderScope.containerOf(context);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    normal.pending!.complete(normal.result);
    await settle(tester);
    expect(api.services, hasLength(1));
    expect(container.read(monitoringPinnedObservationProvider)!.state.hasAcceptedResult, isFalse);
  });
  testWidgets('new manual read while pinned stays manual and visibly cannot replace the pin', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await tapKey(tester, 'monitoring-query');
    await tapKey(tester, 'monitoring-observation-pin');
    final owner = container.read(monitoringPinnedObservationProvider)!, original = owner.state.plan;
    await enterKey(tester, 'monitoring-expression', 'different_manual_literal');
    await tapKey(tester, 'monitoring-query');
    expect(owner.state.plan, same(original));
    expect(api.services, hasLength(2));
    final pin = tester.widget<GlassButton>(find.byKey(const ValueKey('monitoring-observation-pin')));
    expect(pin.onPressed, isNull);
    final l = tester.element(find.byKey(const ValueKey('monitoring-auto-controls'))).l10n;
    expect(find.text(l.monitoringObservationManualLimit), findsOneWidget);
  });
  testWidgets('saved range refresh keeps Native panel ID/revision and never falls back to generic expression', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await tapKey(tester, 'saved-panel-range-$savedPanelId');
    final owner = container.read(monitoringPinnedObservationProvider)!;
    expect(owner.state.plan!.target, isA<MonitoringSavedPanelTarget>());
    final seed = owner.state.value!;
    await tapKey(tester, 'monitoring-observation-pin');
    await tester.pump(const Duration(seconds: 15));
    await settle(tester);
    expect(api.services.last.savedReads, hasLength(1));
    expect(api.services.last.queries, 0);
    final read = api.services.last.savedReads.single;
    expect(read.$1, savedPanelId);
    expect(read.$2, 1);
    expect(read.$3!.start, seed.window!.startSeconds);
    expect(read.$3!.end, seed.window!.endSeconds);
  });
  testWidgets('lock clears a pending generalized pin and rejects its late result', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _range(tester);
    await tapKey(tester, 'monitoring-query');
    await tapKey(tester, 'monitoring-observation-pin');
    final owner = container.read(monitoringPinnedObservationProvider)!,
        private = api.services.last..pending = Completer<QueryOutcome>();
    owner.refresh();
    await settle(tester);
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    private.pending!.complete(private.result);
    await settle(tester);
    expect(owner.state.hasAcceptedResult, isFalse);
    expect(find.byKey(const ValueKey('monitoring-pin-global-stop')), findsNothing);
    expect(private.cancellations, 1);
  });
  testWidgets('binding panel promotion keeps Native ID/revision and performs no generic query or extra discovery', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await tapKey(tester, 'monitoring-binding-inspect-${bindingFixture().id}');
    await tapKey(tester, 'monitoring-binding-load-panel');
    final owner = container.read(monitoringPinnedObservationProvider)!;
    expect(owner.state.plan!.target, isA<MonitoringBindingPanelTarget>());
    expect(api.services.first.panels, hasLength(1));
    expect(api.services.first.discoveries, isEmpty);
    final accepted = owner.state.value!;
    await tapKey(tester, 'monitoring-observation-pin');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    await tester.pump(const Duration(seconds: 15));
    await settle(tester);
    final service = api.services.last;
    expect(service.panels, hasLength(1));
    expect(service.discoveries, isEmpty);
    expect(service.queries, 0);
    expect(service.panels.single.$1, bindingFixture().id);
    expect(service.panels.single.$2, 7);
    expect(service.panels.single.$4, accepted.window!.startSeconds);
    expect(service.panels.single.$5, accepted.window!.endSeconds);
  });
  for (final kind in ['binding', 'saved']) {
    testWidgets('removed Native $kind record stops hidden observation before any new read', (tester) async {
      final api = _Backend();
      final container = await _open(tester, api);
      if (kind == 'binding') {
        await tapKey(tester, 'monitoring-binding-inspect-${bindingFixture().id}');
        await tapKey(tester, 'monitoring-binding-load-panel');
      } else {
        await tapKey(tester, 'saved-panel-range-$savedPanelId');
      }
      await tapKey(tester, 'monitoring-observation-pin');
      container.read(routerProvider).go(AppRoutes.terminal);
      await settle(tester);
      if (kind == 'binding') {
        api.store.bindings = [];
      } else {
        api.store.panels = [];
      }
      await tester.pump(const Duration(seconds: 15));
      await settle(tester);
      final owner = container.read(monitoringPinnedObservationProvider)!, service = api.services.last;
      expect(owner.state.stopReason, MonitoringPinStop.sourceChanged);
      expect(owner.state.hasAcceptedResult, isFalse);
      expect(service.queries, 0);
      expect(service.panels, isEmpty);
      expect(service.savedReads, isEmpty);
    });
  }
  testWidgets('Auto OFF preserves latest readonly UTC snapshot without a timer or hidden re-arm', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _range(tester);
    await tapKey(tester, 'monitoring-query');
    await tester.pump(const Duration(seconds: 15));
    await settle(tester);
    final owner = container.read(monitoringPinnedObservationProvider)!, accepted = owner.state.value!;
    tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-auto-enabled'))).onChanged!(false);
    await settle(tester);
    expect(owner.state.active, isFalse);
    expect(owner.state.value, same(accepted));
    expect(owner.state.stopReason, MonitoringPinStop.autoDisabled);
    expect(owner.refresh(), isFalse);
    final l = tester.element(find.byKey(const ValueKey('monitoring-auto-controls'))).l10n;
    expect(find.text(l.monitoringObservationAutoStopped), findsOneWidget);
    await tester.pump(const Duration(seconds: 60));
    expect(api.services.last.queries, 1);
    tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-auto-enabled'))).onChanged!(true);
    await settle(tester);
    await tester.pump(const Duration(seconds: 60));
    expect(api.services.last.queries, 1);
    expect(owner.state.value!.window, accepted.window);
  });
  testWidgets(
    'binding option changes during manual pending read deny late Auto seed without losing the manual result',
    (tester) async {
      final api = _Backend();
      await _open(tester, api);
      await tapKey(tester, 'monitoring-binding-inspect-${bindingFixture().id}');
      api.services.first.pendingBinding = Completer<BindingReadResult>();
      await tapKey(tester, 'monitoring-binding-load-panel');
      tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-auto-enabled'))).onChanged!(false);
      await settle(tester);
      api.services.first.pendingBinding!.complete(
        BindingReadResult.fromJson(bindingPanelJson(panel: MonitoringPanel.hostOverview)),
      );
      await settle(tester);
      expect(api.services, hasLength(1));
      expect(find.byKey(const ValueKey('monitoring-binding-read-state')), findsOneWidget);
    },
  );
  testWidgets('permanent schema failure clears observed data without resurrecting the original manual snapshot', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await tapKey(tester, 'monitoring-query');
    expect(find.byType(MonitoringResultView), findsOneWidget);
    final owner = container.read(monitoringPinnedObservationProvider)!;
    api.services.last.result = const QueryOutcome(
      status: 'error',
      httpStatus: 500,
      error: MonitoringFailure.local(MonitoringErrorCode.invalidResponse, httpStatus: 500),
    );
    owner.refresh();
    await settle(tester);
    expect(owner.state.hasAcceptedResult, isFalse);
    expect(find.byType(MonitoringResultView), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-pin-error')), findsOneWidget);
  });
  testWidgets('accepted binding metric charts share one passive UTC viewport; presentation sends no query', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await tapKey(tester, 'monitoring-binding-inspect-${bindingFixture().id}');
    tester.widget<GlassSelect<MonitoringPanel>>(find.byKey(const ValueKey('monitoring-binding-panel'))).onChanged!(
      MonitoringPanel.memory,
    );
    await settle(tester);
    final reader = tester.widget<MonitoringBindingReaderView>(find.byType(MonitoringBindingReaderView));
    reader.timeWindow!.restore(MonitoringTimeWindow.fromEpoch(900, 1100, 15));
    await settle(tester);
    await tapKey(tester, 'monitoring-binding-load-panel');
    final views = tester
        .widgetList<MonitoringResultView>(
          find.descendant(
            of: find.byKey(const ValueKey('monitoring-pin-view')),
            matching: find.byType(MonitoringResultView),
          ),
        )
        .toList();
    expect(views, hasLength(2));
    expect(views[0].timeWindow, same(views[1].timeWindow));
    final owner = container.read(monitoringPinnedObservationProvider)!, requested = owner.state.value!.window;
    expect(views.every((view) => view.requestedWindow == requested), isTrue);
    expect(views.first.timeWindow!.zoom(.5), isTrue);
    await settle(tester);
    expect(views[0].timeWindow!.viewport.value, views[1].timeWindow!.viewport.value);
    expect(api.services.first.panels, hasLength(1));
    expect(api.services.last.panels, isEmpty);
    expect(api.services.every((service) => service.queries == 0), isTrue);
  });
  testWidgets('inactive window preserves pin; mobile/desktop paused stops and resumed does not restart', (
    tester,
  ) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await tapKey(tester, 'monitoring-query');
    await tapKey(tester, 'monitoring-observation-pin');
    final owner = container.read(monitoringPinnedObservationProvider)!;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    await tester.pump(const Duration(seconds: 15));
    await settle(tester);
    expect(api.services.last.queries, 1);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await settle(tester);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await settle(tester);
    await tester.pump(const Duration(seconds: 60));
    expect(api.services.last.queries, 1);
    expect(owner.state.active, isFalse);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.android}));
  testWidgets('hidden pinned chart cannot consume Terminal keyboard while its one owner continues', (tester) async {
    final api = _Backend();
    final container = await _open(tester, api);
    await _range(tester);
    tester
        .widget<MonitoringTimeWindowControls>(find.byType(MonitoringTimeWindowControls).first)
        .controller
        .restore(MonitoringTimeWindow.fromEpoch(900, 1100, 15));
    await settle(tester);
    await tapKey(tester, 'monitoring-query');
    await tapKey(tester, 'monitoring-observation-pin');
    final result = tester.widget<MonitoringResultView>(find.byType(MonitoringResultView)), time = result.timeWindow!;
    final chart = find.byKey(const ValueKey('monitoring-chart'));
    await Scrollable.ensureVisible(tester.element(chart), alignment: .5);
    await settle(tester);
    await tester.tap(chart);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.equal);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(time.viewport.value, isNot(time.applied));
    final zoom = time.viewport.value;
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(
      tester
          .widget<MonitoringObservationResultView>(find.byType(MonitoringObservationResultView, skipOffstage: false))
          .active,
      isFalse,
    );
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.equal);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(time.viewport.value, zoom);
    expect(container.read(monitoringPinnedObservationProvider)!.state.active, isTrue);
    expect(api.services, hasLength(2));
    expect(api.services.last.queries, 0);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('narrow enlarged text keeps Auto choices and explicit read accessible without arming on layout', (
    tester,
  ) async {
    tester.platformDispatcher.textScaleFactorTestValue = 1.6;
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    final api = _Backend();
    await _open(tester, api, width: 360);
    expect(api.services, hasLength(1));
    expect(api.services.single.queries, 0);
    await tapKey(tester, 'monitoring-query');
    expect(api.services, hasLength(2));
    await tapKey(tester, 'monitoring-observation-pin');
    expect(api.services.last.queries, 0);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android}));
  for (final width in [580.0, 1500.0]) {
    testWidgets('Russian automatic range observation at width $width', (tester) async {
      final api = _Backend();
      await _open(tester, api, width: width, russian: true);
      await _range(tester);
      final end = DateTime.utc(2026, 11, 12, 12).millisecondsSinceEpoch / 1000;
      tester
          .widget<MonitoringTimeWindowControls>(find.byType(MonitoringTimeWindowControls).first)
          .controller
          .restore(MonitoringTimeWindow.fromEpoch(end - 120, end, 15));
      await settle(tester);
      api.services.first.result = QueryOutcome(
        status: 'success',
        httpStatus: 200,
        partial: true,
        warnings: const ['Owned synthetic partial fixture'],
        effectiveStepSeconds: 15,
        data: QueryData(
          'matrix',
          series: [
            for (var series = 0; series < 2; series++)
              MonitoringSeries(
                {'job': 'owned-proof', 'series': 'sample-$series'},
                [
                  for (var i = 0; i < 9; i++)
                    MonitoringSample(
                      end - 120 + i * 15,
                      i == 4 ? SampleKind.nan : SampleKind.finite,
                      i == 4 ? null : (series * 3 + i % 4).toDouble(),
                    ),
                ],
              ),
          ],
        ),
      );
      await tapKey(tester, 'monitoring-query');
      await tapKey(tester, 'monitoring-observation-pin');
      api.services.last.result = api.services.first.result;
      await tester.pump(const Duration(seconds: 15));
      await settle(tester);
      final controls = find.byKey(const ValueKey('monitoring-auto-controls'));
      await Scrollable.ensureVisible(tester.element(controls), alignment: 0);
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-observation-${width < 800 ? 'narrow' : 'wide'}-ru');
      await Scrollable.ensureVisible(tester.element(find.byKey(const ValueKey('monitoring-pin-view'))), alignment: 0);
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-observation-result-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
