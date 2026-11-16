import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_chart_model.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show settle, tapKey;

MonitoringSample p(double time, double value) => MonitoringSample(time, SampleKind.finite, value);
MonitoringSeries s(String name, List<MonitoringSample> samples) => MonitoringSeries({'instance': name}, samples);
QueryOutcome outcome(List<MonitoringSeries> series) => QueryOutcome(
  status: 'success',
  httpStatus: 200,
  data: QueryData('matrix', series: series),
  partial: true,
  warnings: const ['fixture chart warning'],
  effectiveStepSeconds: 10,
);
final window = MonitoringTimeWindow.fromEpoch(0, 1000, 10);
Future<void> open(
  WidgetTester tester,
  QueryOutcome result, {
  double width = 900,
  MonitoringUnit? unit,
  Locale locale = const Locale('en'),
  String scope = 'same',
}) async {
  tester.view.physicalSize = Size(width, 1100);
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
          body: SingleChildScrollView(
            child: MonitoringResultView(key: ValueKey(scope), result: result, requestedWindow: window, unit: unit),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> point(WidgetTester tester, double fraction, {bool hover = false}) async {
  final chart = find.byKey(const ValueKey('monitoring-chart'));
  await tester.ensureVisible(chart);
  await tester.pump();
  final rect = tester.getRect(chart), pos = Offset(rect.left + 8 + (rect.width - 16) * fraction, rect.center.dy);
  if (hover) {
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: Offset.zero);
    await mouse.moveTo(pos);
    addTearDown(mouse.removePointer);
  } else {
    await tester.tapAt(pos);
  }
  await tester.pump();
}

Finder toggle(MonitoringChartSeries row) => find.byKey(ValueKey('monitoring-chart-toggle-${row.id}'));
Finder inspected(MonitoringChartSeries row) => find.byKey(ValueKey('monitoring-chart-tooltip-${row.id}'));

void main() {
  testWidgets('one native chart includes every row, bounded legend and original diagnostic/sample information', (
    tester,
  ) async {
    final result = outcome([
      s('a', [p(0, 1), const MonitoringSample(10, SampleKind.nan)]),
      s('b', [p(0, 20)]),
      s('c', [p(0, 30)]),
    ]);
    await open(tester, result);
    expect(find.byKey(const ValueKey('monitoring-chart-paint')), findsOneWidget);
    expect(find.byType(Checkbox), findsNWidgets(3));
    expect(tester.widgetList<Checkbox>(find.byType(Checkbox)).every((c) => c.value!), isTrue);
    final l = tester.element(find.byType(MonitoringResultView)).l10n;
    expect(find.text(l.monitoringWarning('fixture chart warning')), findsOneWidget);
    expect(find.text(l.monitoringPartialResult), findsOneWidget);
    expect(find.text('NaN'), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-samples')), findsOneWidget);
    final table = tester.widget<GlassSelect<int>>(find.byKey(const ValueKey('monitoring-table-series')));
    table.onChanged!(1);
    await tester.pump();
    expect(find.text('20.0'), findsOneWidget);
  });
  testWidgets('legend switches each row locally, all hidden is explicit and Show all restores the chart', (
    tester,
  ) async {
    final result = outcome([
          s('a', [p(0, 1)]),
          s('b', [p(0, 2)]),
        ]),
        rows = monitoringChartSeries(result.data!.series);
    await open(tester, result);
    await tester.tap(toggle(rows.first));
    await tester.pump();
    expect(tester.widget<Checkbox>(toggle(rows.first)).value, isFalse);
    expect(tester.widget<Checkbox>(toggle(rows.last)).value, isTrue);
    await tester.tap(toggle(rows.last));
    await tester.pump();
    expect(find.byKey(const ValueKey('monitoring-chart-all-hidden')), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-samples')), findsOneWidget);
    await tapKey(tester, 'monitoring-chart-show-all');
    expect(find.byKey(const ValueKey('monitoring-chart-all-hidden')), findsNothing);
    expect(tester.widgetList<Checkbox>(find.byType(Checkbox)).every((c) => c.value!), isTrue);
  });
  testWidgets('hover has visible exact UTC and does not carry values across another row or long gap', (tester) async {
    final result = outcome([
          s('a', [p(0, 1), p(100, 2)]),
          s('b', [p(20, 3), p(90, 4)]),
        ]),
        rows = monitoringChartSeries(result.data!.series);
    await open(tester, result, unit: MonitoringUnit.bytes);
    await point(tester, .101, hover: true);
    final l = tester.element(find.byType(MonitoringResultView)).l10n;
    expect(find.text(l.monitoringChartInspectionUtc('1970-01-01T00:01:40.000Z')), findsOneWidget);
    expect(tester.widget<Text>(inspected(rows.first)).data, '2.0 ${l.monitoringUnitBytes}');
    expect(tester.widget<Text>(inspected(rows.last)).data, l.monitoringChartMissingValue);
    await point(tester, .05);
    expect(find.text(l.monitoringChartNoSampleNearCursor), findsOneWidget);
    expect(tester.widget<Text>(inspected(rows.first)).data, l.monitoringChartMissingValue);
    expect(tester.widget<Text>(inspected(rows.last)).data, l.monitoringChartMissingValue);
  });
  testWidgets('touch and keyboard inspect real samples including NaN and Escape clears the inspector', (tester) async {
    final result = outcome([
          s('a', [
            p(0, 1),
            const MonitoringSample(10, SampleKind.nan),
            const MonitoringSample(20, SampleKind.positiveInfinity),
          ]),
        ]),
        row = monitoringChartSeries(result.data!.series).single;
    await open(tester, result);
    await point(tester, 0);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await tester.pump();
    expect(tester.widget<Text>(inspected(row)).data, startsWith('NaN '));
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await tester.pump();
    expect(tester.widget<Text>(inspected(row)).data, startsWith('+Inf '));
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(find.byKey(const ValueKey('monitoring-chart-tooltip-time')), findsNothing);
    await point(tester, .02);
    await tapKey(tester, 'monitoring-chart-clear-inspection');
    expect(find.byKey(const ValueKey('monitoring-chart-tooltip-time')), findsNothing);
  });
  testWidgets('known enum units are explicit and arbitrary metric label text never asserts a unit', (tester) async {
    final result = outcome([
      MonitoringSeries({'__name__': 'memory_bytes'}, [p(0, 2048)]),
    ]);
    await open(tester, result);
    var l = tester.element(find.byType(MonitoringResultView)).l10n;
    expect(find.text(l.monitoringChartUnit(l.monitoringChartUnknownUnit)), findsOneWidget);
    await open(tester, result, unit: MonitoringUnit.bytes);
    l = tester.element(find.byType(MonitoringResultView)).l10n;
    expect(find.text(l.monitoringChartUnit(l.monitoringUnitBytes)), findsOneWidget);
    await point(tester, 0);
    expect(find.text('2048.0 ${l.monitoringUnitBytes}'), findsOneWidget); // Exact native value, no guessed scaling.
  });
  testWidgets('result reorder preserves unique legend/table identity but clears the previous inspection', (
    tester,
  ) async {
    final a = s('a', [p(0, 1)]), b = s('b', [p(0, 2)]), result = outcome([a, b]), rows = monitoringChartSeries([a, b]);
    await open(tester, result);
    tester.widget<GlassSelect<int>>(find.byKey(const ValueKey('monitoring-table-series'))).onChanged!(1);
    await tester.pump();
    await tester.tap(toggle(rows.first));
    await tester.pump();
    await point(tester, 0);
    await open(tester, outcome([b, a]));
    expect(tester.widget<Checkbox>(toggle(rows.first)).value, isFalse);
    expect(tester.widget<Checkbox>(toggle(rows.last)).value, isTrue);
    expect(tester.widget<GlassSelect<int>>(find.byKey(const ValueKey('monitoring-table-series'))).value, 0);
    expect(find.byKey(const ValueKey('monitoring-chart-tooltip-time')), findsNothing);
  });
  testWidgets('duplicate labels retain separate rows and reset ambiguous visibility on refresh', (tester) async {
    final a = s('same', [p(0, 1)]), b = s('same', [p(0, 2)]), rows = monitoringChartSeries([a, b]);
    await open(tester, outcome([a, b]));
    expect(find.byType(Checkbox), findsNWidgets(2));
    await tester.tap(toggle(rows.first));
    await tester.pump();
    await open(tester, outcome([b, a]));
    expect(tester.widgetList<Checkbox>(find.byType(Checkbox)).every((c) => c.value!), isTrue);
    expect(
      tester
          .widget<GlassSelect<int>>(find.byKey(const ValueKey('monitoring-table-series')))
          .items
          .map((i) => i.label)
          .toSet()
          .length,
      2,
    );
  });
  testWidgets('duplicate sample timestamps are reported explicitly rather than selecting one value', (tester) async {
    final result = outcome([
          s('a', [p(100, 2), p(100, 5)]),
        ]),
        row = monitoringChartSeries(result.data!.series).single;
    await open(tester, result);
    await point(tester, .1);
    final l = tester.element(find.byType(MonitoringResultView)).l10n;
    expect(tester.widget<Text>(inspected(row)).data, l.monitoringChartDuplicateSamples('2'));
    expect(find.text('2.0'), findsOneWidget);
    expect(find.text('5.0'), findsOneWidget);
  });
  testWidgets('scope teardown removes the inspection and local visibility choices', (tester) async {
    final result = outcome([
          s('a', [p(0, 1)]),
          s('b', [p(0, 2)]),
        ]),
        rows = monitoringChartSeries(result.data!.series);
    await open(tester, result);
    await tester.tap(toggle(rows.first));
    await tester.pump();
    await point(tester, 0);
    expect(find.byKey(const ValueKey('monitoring-chart-tooltip-time')), findsOneWidget);
    await open(tester, result, scope: 'new-profile');
    expect(find.byKey(const ValueKey('monitoring-chart-tooltip-time')), findsNothing);
    expect(tester.widgetList<Checkbox>(find.byType(Checkbox)).every((c) => c.value!), isTrue);
  });
  testWidgets('actual saved-binding panel passes validated Native metric units into this same result renderer', (
    tester,
  ) async {
    final reader = MonitoringBindingReadTestService(), time = MonitoringTimeWindowController(initial: window);
    addTearDown(time.dispose);
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(
          body: SingleChildScrollView(
            child: MonitoringBindingReaderView(
              reader: reader,
              source: monitoringFixture,
              binding: bindingFixture(),
              timeWindow: time,
            ),
          ),
        ),
      ),
    );
    await settle(tester);
    await tapKey(tester, 'monitoring-binding-load-panel');
    final cpu = find.descendant(
      of: find.byKey(const ValueKey('monitoring-panel-query-cpuBusy')),
      matching: find.byType(MonitoringResultView),
    );
    expect(tester.widget<MonitoringResultView>(cpu).unit, MonitoringUnit.percent);
    expect(reader.panels.length, 1);
    final checkbox = find.descendant(of: cpu, matching: find.byType(Checkbox));
    await tester.ensureVisible(checkbox.first);
    await tester.tap(checkbox.first);
    await tester.pump();
    expect(reader.panels.length, 1); // Presentation interaction issues no read.
    expect(reader.queries, 0);
  });
  for (final width in [360.0, 1000.0]) {
    testWidgets('Russian multi-series legend and inspector remain bounded at width $width', (tester) async {
      final result = outcome([
        for (var i = 0; i < 30; i++) s('long-explicit-label-$i.example.test', [p(0, i + 1.0), p(100, i + 2.0)]),
      ]);
      await open(tester, result, width: width, unit: MonitoringUnit.requestsPerSecond, locale: const Locale('ru'));
      await point(tester, .1);
      await tester.ensureVisible(find.byKey(const ValueKey('monitoring-chart-tooltip-time')));
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-chart-interaction-${width < 500 ? 'narrow' : 'wide'}-ru');
      expect(find.byType(Checkbox), findsNWidgets(30));
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
