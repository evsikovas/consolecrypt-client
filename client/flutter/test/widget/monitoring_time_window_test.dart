import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/test_app.dart' show enterKey, tapKey, settle;

final fixedNow = DateTime.utc(2026, 11, 11, 12);
MonitoringTimeWindowController controller() =>
    MonitoringTimeWindowController(initial: MonitoringTimeWindow.fromEpoch(0, 1000, 15), now: () => fixedNow);

Future<void> open(
  WidgetTester tester,
  MonitoringBindingReadTestService service,
  MonitoringBindingTestStore store,
  MonitoringTimeWindowController time, {
  bool active = true,
  Size size = const Size(1400, 1200),
  Locale locale = const Locale('en'),
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    RepaintBoundary(
      key: const ValueKey('automation-proof-root'),
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: locale,
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: MonitoringWorkspace(
            service: service,
            store: store,
            timeWindow: time,
            active: active,
            hosts: const [bindingHost],
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> rangeMode(WidgetTester tester) async {
  final field = tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-range')));
  field.onChanged!(true);
  await settle(tester);
}

Future<void> inspect(WidgetTester tester) => tapKey(tester, 'monitoring-binding-inspect-${bindingFixture().id}');
bool enabled(WidgetTester tester, String key) {
  final widget = tester.widget(find.byKey(ValueKey(key)));
  return switch (widget) {
    GlassButton(:final onPressed) => onPressed != null,
    ButtonStyleButton(:final onPressed) => onPressed != null,
    _ => throw StateError('Fixture key must refer to a button'),
  };
}

Future<void> close(WidgetTester tester) async {
  await tester.pumpWidget(const SizedBox());
  await tester.pump();
}

class _PendingSourceStore extends MonitoringBindingTestStore {
  final saved = Completer<void>();
  @override
  Future<MonitoringSource> upsert(MonitoringSource value, {bool create = false}) async {
    await saved.future;
    return super.upsert(value, create: create);
  }
}

void main() {
  testWidgets('real explorer and binding controls share one period; typing and applying never query', (tester) async {
    final time = controller(), service = MonitoringBindingReadTestService();
    addTearDown(time.dispose);
    final store = MonitoringBindingTestStore(rows: [bindingFixture()]);
    await open(tester, service, store, time);
    await rangeMode(tester);
    await inspect(tester);
    final controls = tester
        .widgetList<MonitoringTimeWindowControls>(find.byType(MonitoringTimeWindowControls))
        .toList();
    expect(controls, hasLength(2));
    expect(controls.every((view) => identical(view.controller, time)), isTrue);
    await enterKey(tester, 'monitoring-time-step', '30');
    expect(service.queries, 0);
    expect(service.panels, isEmpty);
    expect(service.discoveries, isEmpty);
    expect(enabled(tester, 'monitoring-query'), isFalse);
    expect(enabled(tester, 'monitoring-binding-load-panel'), isFalse);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('monitoring-binding-step'))).controller!.text, '30');
    await tapKey(tester, 'monitoring-time-apply-window');
    expect(service.queries, 0);
    expect(service.panels, isEmpty);
    await tapKey(tester, 'monitoring-query');
    expect((service.last!.start, service.last!.end, service.last!.stepSeconds), (0.0, 1000.0, 30.0));
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect((service.panels.single.$4, service.panels.single.$5, service.panels.single.$6), (0.0, 1000.0, 30.0));
    final charts = tester
        .widgetList<MonitoringResultView>(find.byType(MonitoringResultView))
        .where((view) => view.result.data?.kind == 'matrix')
        .toList();
    expect(charts.length, greaterThan(1));
    expect(charts.every((view) => view.requestedWindow == time.requested), isTrue);
    expect(service.discoveries, isEmpty); // Panel Native operation handles its own identity.
    await close(tester);
  });

  testWidgets('empty interval invalid UTC dates and reversed bounds block all data requests', (tester) async {
    final time = controller(), service = MonitoringBindingReadTestService();
    addTearDown(time.dispose);
    final original = time.applied;
    await open(tester, service, MonitoringBindingTestStore(rows: [bindingFixture()]), time);
    await rangeMode(tester);
    await inspect(tester);
    for (final invalid in [
      (field: 'start', value: ''),
      (field: 'start', value: '2026-02-30T00:00:00Z'),
      (field: 'end', value: ''),
      (field: 'end', value: '1970-01-01T00:16:40+00:00'),
      (field: 'step', value: ''),
      (field: 'step', value: 'NaN'),
      (field: 'step', value: '0'),
      (field: 'start', value: '1970-01-02T00:00:00Z'),
    ]) {
      time.restore(original);
      await settle(tester);
      await enterKey(tester, 'monitoring-time-${invalid.field}', invalid.value);
      await tapKey(tester, 'monitoring-time-apply-window');
      expect(find.byKey(const ValueKey('monitoring-time-error')), findsOneWidget);
      expect(time.applied, original);
      expect(enabled(tester, 'monitoring-query'), isFalse);
      expect(enabled(tester, 'monitoring-binding-load-panel'), isFalse);
      expect(service.queries, 0);
      expect(service.panels, isEmpty);
      expect(service.discoveries, isEmpty);
    }
    await close(tester);
  });

  testWidgets('all five explicit presets apply the same selected end without automatic reads', (tester) async {
    final time = controller(), service = MonitoringBindingReadTestService();
    addTearDown(time.dispose);
    await open(tester, service, MonitoringBindingTestStore(), time);
    await rangeMode(tester);
    for (final preset in MonitoringTimePreset.values.where((p) => p != MonitoringTimePreset.custom)) {
      tester.widget<GlassSelect<MonitoringTimePreset>>(find.byKey(const ValueKey('monitoring-time-preset'))).onChanged!(
        preset,
      );
      await settle(tester);
      await tapKey(tester, 'monitoring-time-apply-window');
      expect(time.requested.endUtc, fixedNow);
      expect(time.requested.endUtc.difference(time.requested.startUtc), preset.duration);
      expect(service.queries, 0);
      expect(service.panels, isEmpty);
    }
    await tapKey(tester, 'monitoring-query');
    expect(service.queries, 1);
    expect(service.last!.end! - service.last!.start!, const Duration(days: 7).inSeconds.toDouble());
    await close(tester);
  });

  testWidgets('explicit preset refresh moves its fixed end but does not apply or issue queries', (tester) async {
    var now = fixedNow;
    final time = MonitoringTimeWindowController(now: () => now), service = MonitoringBindingReadTestService();
    addTearDown(time.dispose);
    await open(tester, service, MonitoringBindingTestStore(), time);
    await rangeMode(tester);
    now = now.add(const Duration(hours: 1));
    await tapKey(tester, 'monitoring-time-now');
    expect(time.applied.endUtc, fixedNow);
    expect(time.draftEnd, now.toIso8601String());
    expect(enabled(tester, 'monitoring-query'), isFalse);
    expect(service.queries, 0);
    expect(service.panels, isEmpty);
    await tapKey(tester, 'monitoring-time-apply-window');
    expect(time.requested.endUtc, now);
    expect(service.queries, 0);
    expect(service.panels, isEmpty);
    await close(tester);
  });

  testWidgets('time draft cancels a pending explorer query and suppresses its late response', (tester) async {
    final time = controller(), service = MonitoringBindingReadTestService()..pending = Completer<QueryOutcome>();
    addTearDown(time.dispose);
    await open(tester, service, MonitoringBindingTestStore(), time);
    await rangeMode(tester);
    await tapKey(tester, 'monitoring-query');
    expect(service.queries, 1);
    final before = service.cancellations;
    await enterKey(tester, 'monitoring-time-step', '');
    expect(service.cancellations, greaterThan(before));
    service.pending!.complete(service.result);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-result-status')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-requested-axis')), findsNothing);
    expect(service.queries, 1);
    await close(tester);
  });

  testWidgets('shared apply cancels a pending binding panel even if applied values return to the same window', (
    tester,
  ) async {
    final time = controller(),
        service = MonitoringBindingReadTestService()..pendingBinding = Completer<BindingReadResult>();
    addTearDown(time.dispose);
    await open(tester, service, MonitoringBindingTestStore(rows: [bindingFixture()]), time);
    await inspect(tester);
    await tapKey(tester, 'monitoring-binding-load-panel');
    final before = service.bindingCancellations;
    time.edit(step: '15');
    time.apply();
    await settle(tester);
    expect(service.bindingCancellations, greaterThan(before));
    service.pendingBinding!.complete(BindingReadResult.fromJson(bindingPanelJson(panel: MonitoringPanel.hostOverview)));
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-binding-read-state')), findsNothing);
    expect(service.panels, hasLength(1));
    expect(service.queries, 0);
    await close(tester);
  });

  testWidgets('history restores its original absolute window without querying or rebasing to now', (tester) async {
    final time = controller(), service = MonitoringBindingReadTestService();
    addTearDown(time.dispose);
    final original = time.applied;
    await open(tester, service, MonitoringBindingTestStore(), time);
    await rangeMode(tester);
    await tapKey(tester, 'monitoring-query');
    time.select(MonitoringTimePreset.days7);
    time.apply();
    await settle(tester);
    expect(time.requested, isNot(original));
    expect(service.queries, 1);
    final entry = find
        .descendant(of: find.byKey(const ValueKey('monitoring-history')), matching: find.byType(TextButton))
        .first;
    await tester.ensureVisible(entry);
    await tester.tap(entry);
    await settle(tester);
    expect(time.requested, original);
    expect(service.queries, 1);
    await tapKey(tester, 'monitoring-query');
    expect((service.last!.start, service.last!.end), (original.startSeconds, original.endSeconds));
    await close(tester);
  });

  testWidgets('hidden workspace cancels and does not auto-query when shown again', (tester) async {
    final time = controller(), service = MonitoringBindingReadTestService()..pending = Completer<QueryOutcome>();
    addTearDown(time.dispose);
    final store = MonitoringBindingTestStore();
    await open(tester, service, store, time);
    await rangeMode(tester);
    await tapKey(tester, 'monitoring-query');
    await open(tester, service, store, time, active: false);
    service.pending!.complete(service.result);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-result-status')), findsNothing);
    await open(tester, service, store, time);
    expect(service.queries, 1);
    expect(service.panels, isEmpty);
    await close(tester);
  });

  testWidgets('time edits cancel reads without invalidating an independent Native source catalog save', (tester) async {
    final time = controller(), service = MonitoringBindingReadTestService(), store = _PendingSourceStore();
    addTearDown(time.dispose);
    await open(tester, service, store, time);
    await rangeMode(tester);
    await tester.ensureVisible(find.text('Edit source'));
    await tester.tap(find.text('Edit source'));
    await settle(tester);
    await enterKey(tester, 'monitoring-source-name', 'Renamed source');
    await tapKey(tester, 'monitoring-save-source');
    time.edit(step: '30');
    await settle(tester);
    store.saved.complete();
    await settle(tester);
    expect(store.sources.single.name, 'Renamed source');
    expect(find.byKey(const ValueKey('monitoring-save-source')), findsNothing);
    expect(find.text('Renamed source'), findsOneWidget);
    expect(service.queries, 0);
    expect(service.panels, isEmpty);
    await close(tester);
  });

  for (final width in [520.0, 1400.0]) {
    testWidgets('shared UTC controls and requested axes render at width $width', (tester) async {
      final time = controller(), service = MonitoringBindingReadTestService();
      addTearDown(time.dispose);
      await open(
        tester,
        service,
        MonitoringBindingTestStore(rows: [bindingFixture()]),
        time,
        size: Size(width, 1200),
        locale: const Locale('ru'),
      );
      await rangeMode(tester);
      await inspect(tester);
      await tapKey(tester, 'monitoring-binding-load-panel');
      expect(tester.takeException(), isNull);
      await tester.ensureVisible(find.byKey(const ValueKey('monitoring-binding-reader')));
      await captureAutomationWorkspace(
        tester,
        width > 700 ? 'monitoring-shared-time-wide-ru' : 'monitoring-shared-time-narrow-ru',
      );
      await tapKey(tester, 'monitoring-binding-preset');
      await captureAutomationWorkspace(
        tester,
        width > 700 ? 'monitoring-presets-wide-ru' : 'monitoring-presets-narrow-ru',
      );
      final label = monitoringTimePresetLabel(
        tester.element(find.byKey(const ValueKey('monitoring-binding-reader'))).l10n,
        MonitoringTimePreset.hour1,
      );
      await tester.tap(find.text(label).last);
      await settle(tester);
      expect(time.dirty, isTrue);
      expect(find.byKey(const ValueKey('monitoring-binding-now')), findsOneWidget);
      expect(service.panels, hasLength(1));
      expect(service.queries, 0);
      await captureAutomationWorkspace(
        tester,
        width > 700 ? 'monitoring-preset-draft-wide-ru' : 'monitoring-preset-draft-narrow-ru',
      );
      await close(tester);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
