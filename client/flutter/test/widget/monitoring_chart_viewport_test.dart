import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/test_app.dart' show settle, tapKey;

MonitoringTimeWindow get window => MonitoringTimeWindow.fromEpoch(0, 1000, 10);
QueryOutcome get result => const QueryOutcome(
  status: 'success',
  httpStatus: 200,
  effectiveStepSeconds: 250,
  data: QueryData(
    'matrix',
    series: [
      MonitoringSeries(
        {'instance': 'a'},
        [
          MonitoringSample(0, SampleKind.finite, 1),
          MonitoringSample(250, SampleKind.finite, 2),
          MonitoringSample(500, SampleKind.nan),
          MonitoringSample(750, SampleKind.finite, 4),
          MonitoringSample(1000, SampleKind.finite, 5),
        ],
      ),
      MonitoringSeries(
        {'instance': 'b'},
        [
          MonitoringSample(0, SampleKind.finite, 5),
          MonitoringSample(250, SampleKind.finite, 4),
          MonitoringSample(500, SampleKind.finite, 3),
          MonitoringSample(750, SampleKind.finite, 2),
          MonitoringSample(1000, SampleKind.finite, 1),
        ],
      ),
    ],
  ),
);

class _PaletteIntent extends Intent {
  const _PaletteIntent();
}

Future<void> frame(
  WidgetTester tester,
  Widget child, {
  double width = 1000,
  Locale locale = const Locale('en'),
  VoidCallback? palette,
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
          body: Shortcuts(
            shortcuts: const {SingleActivator(LogicalKeyboardKey.keyK, control: true): _PaletteIntent()},
            child: Actions(
              actions: {
                _PaletteIntent: CallbackAction<_PaletteIntent>(
                  onInvoke: (_) {
                    palette?.call();
                    return null;
                  },
                ),
              },
              child: child,
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> charts(
  WidgetTester tester,
  MonitoringTimeWindowController? controller, {
  ScrollController? page,
  double width = 1000,
  Locale locale = const Locale('en'),
  MonitoringTimeWindow? requested,
  VoidCallback? palette,
  String scope = 'same',
  bool active = true,
}) => frame(
  tester,
  SingleChildScrollView(
    controller: page,
    child: Column(
      children: [
        const TextField(key: ValueKey('outside-chart-field')),
        for (final id in ['a', 'b'])
          MonitoringResultView(
            key: ValueKey('$scope-chart-$id'),
            result: result,
            requestedWindow: requested ?? window,
            timeWindow: controller,
            active: active,
          ),
      ],
    ),
  ),
  width: width,
  locale: locale,
  palette: palette,
);
Finder inside(String chart, String control) =>
    find.descendant(of: find.byKey(ValueKey(chart)), matching: find.byKey(ValueKey(control)));
Future<void> press(WidgetTester tester, String control, {String chart = 'same-chart-a'}) async {
  final target = inside(chart, control);
  await tester.ensureVisible(target);
  await tester.tap(target);
  await settle(tester);
}

Future<void> focusChart(WidgetTester tester) async {
  final chart = inside('same-chart-a', 'monitoring-chart');
  await tester.ensureVisible(chart);
  await tester.tap(chart);
  await tester.pump();
}

void main() {
  testWidgets('touch controls synchronize both panels without mutating the full query period', (tester) async {
    final controller = MonitoringTimeWindowController(initial: window);
    addTearDown(controller.dispose);
    await charts(tester, controller);
    await press(tester, 'monitoring-chart-zoom-in');
    expect(controller.viewport.value, MonitoringTimeWindow.fromEpoch(250, 750, 10));
    final l = tester.element(find.byType(MonitoringResultView).first).l10n;
    expect(
      find.text(l.monitoringChartViewport(controller.viewport.value.startText, controller.viewport.value.endText)),
      findsNWidgets(2),
    );
    await press(tester, 'monitoring-chart-pan-earlier');
    expect(controller.viewport.value, MonitoringTimeWindow.fromEpoch(125, 625, 10));
    await press(tester, 'monitoring-chart-reset-zoom', chart: 'same-chart-b');
    expect(controller.viewport.value, window);
    expect(controller.requested, window);
    expect(controller.revision, 0);
  });
  testWidgets('ordinary wheel scrolls the page; only Ctrl wheel registers chart zoom and leaves page offset fixed', (
    tester,
  ) async {
    final controller = MonitoringTimeWindowController(initial: window), page = ScrollController();
    addTearDown(controller.dispose);
    addTearDown(page.dispose);
    await charts(tester, controller, page: page);
    final plot = inside('same-chart-a', 'monitoring-chart');
    await tester.ensureVisible(plot);
    await tester.pump();
    final before = page.offset;
    await tester.sendEventToBinding(
      PointerScrollEvent(position: tester.getCenter(plot), scrollDelta: const Offset(0, 80)),
    );
    await settle(tester);
    expect(page.offset, greaterThan(before));
    expect(controller.viewport.value, window);
    await tester.ensureVisible(plot);
    await tester.pump();
    final offset = page.offset;
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendEventToBinding(
      PointerScrollEvent(position: tester.getCenter(plot), scrollDelta: const Offset(0, -120)),
    );
    await settle(tester);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(page.offset, offset);
    expect(controller.viewport.value.endSeconds - controller.viewport.value.startSeconds, lessThan(1000));
    expect(controller.requested, window);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets(
    'zoom keyboard shortcuts require chart focus and do not consume global Ctrl K or plain inspection arrows',
    (tester) async {
      final controller = MonitoringTimeWindowController(initial: window);
      addTearDown(controller.dispose);
      var palette = 0;
      await charts(tester, controller, palette: () => palette++);
      await focusChart(tester);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.equal);
      await tester.pump();
      expect(controller.viewport.value, MonitoringTimeWindow.fromEpoch(250, 750, 10));
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
      await tester.pump();
      expect(controller.viewport.value, MonitoringTimeWindow.fromEpoch(375, 875, 10));
      await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
      await tester.pump();
      expect(palette, 1);
      await tester.sendKeyEvent(LogicalKeyboardKey.digit0);
      await tester.pump();
      expect(controller.viewport.value, window);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
      await tester.pump();
      expect(controller.viewport.value, window);
      final field = find.byKey(const ValueKey('outside-chart-field'));
      await tester.ensureVisible(field);
      await tester.tap(field);
      await tester.pump();
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.equal);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pump();
      expect(controller.viewport.value, window);
    },
  );
  testWidgets(
    'standalone results keep their original behavior and mismatched interval results cannot inherit another viewport',
    (tester) async {
      await charts(tester, null);
      expect(find.byKey(const ValueKey('monitoring-chart-paint')), findsNWidgets(2));
      expect(find.byKey(const ValueKey('monitoring-chart-zoom-in')), findsNothing);
      final controller = MonitoringTimeWindowController(initial: window);
      addTearDown(controller.dispose);
      controller.zoom(.5);
      await charts(tester, controller, requested: MonitoringTimeWindow.fromEpoch(0, 1000, 20));
      expect(find.byKey(const ValueKey('monitoring-chart-zoom-in')), findsNothing);
      expect(controller.viewport.value, MonitoringTimeWindow.fromEpoch(250, 750, 10));
      await charts(tester, controller);
      expect(find.byKey(const ValueKey('monitoring-chart-zoom-in')), findsNWidgets(2));
    },
  );
  testWidgets('new scoped controller starts full and late old presentation callbacks cannot update the new panels', (
    tester,
  ) async {
    final old = MonitoringTimeWindowController(initial: window);
    await charts(tester, old);
    old.zoom(.5);
    await tester.pump();
    final current = MonitoringTimeWindowController(initial: window);
    addTearDown(current.dispose);
    await charts(tester, current, scope: 'new-profile');
    old.dispose();
    expect(old.pan(.25), isFalse);
    await tester.pump();
    expect(current.viewport.value, current.requested);
    final l = tester.element(find.byType(MonitoringResultView).first).l10n;
    expect(find.text(l.monitoringChartViewport(window.startText, window.endText)), findsNWidgets(2));
  });
  testWidgets(
    'hidden chart does not handle zoom shortcuts or display controls and retains its local same-window viewport on return',
    (tester) async {
      final controller = MonitoringTimeWindowController(initial: window);
      addTearDown(controller.dispose);
      await charts(tester, controller);
      await focusChart(tester);
      controller.zoom(.5);
      await tester.pump();
      final visible = controller.viewport.value;
      await charts(tester, controller, active: false);
      expect(find.byKey(const ValueKey('monitoring-chart-zoom-in')), findsNothing);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.equal);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pump();
      expect(controller.viewport.value, visible);
      await charts(tester, controller);
      expect(find.byKey(const ValueKey('monitoring-chart-zoom-in')), findsNWidgets(2));
      expect(controller.viewport.value, visible);
    },
  );
  testWidgets(
    'real explorer and binding panels share zoom without read/cancel calls and a pending same-window response retains it',
    (tester) async {
      final controller = MonitoringTimeWindowController(initial: window),
          service = MonitoringBindingReadTestService(),
          store = MonitoringBindingTestStore(rows: [bindingFixture()]);
      addTearDown(controller.dispose);
      service.result = result;
      await frame(
        tester,
        MonitoringWorkspace(service: service, store: store, timeWindow: controller, hosts: const [bindingHost]),
      );
      tester.widget<CheckboxListTile>(find.byKey(const ValueKey('monitoring-range'))).onChanged!(true);
      await settle(tester);
      await tapKey(tester, 'monitoring-query');
      await tapKey(tester, 'monitoring-binding-inspect-${bindingFixture().id}');
      await tapKey(tester, 'monitoring-binding-load-panel');
      final reads = (
        service.queries,
        service.panels.length,
        service.discoveries.length,
        service.cancellations,
        service.bindingCancellations,
      );
      var zoom = find.byKey(const ValueKey('monitoring-chart-zoom-in')).first;
      await tester.ensureVisible(zoom);
      await tester.tap(zoom);
      await settle(tester);
      expect((
        service.queries,
        service.panels.length,
        service.discoveries.length,
        service.cancellations,
        service.bindingCancellations,
      ), reads);
      final views = tester
          .widgetList<MonitoringResultView>(find.byType(MonitoringResultView))
          .where((view) => view.requestedWindow != null)
          .toList();
      expect(views.length, greaterThanOrEqualTo(2));
      expect(views.every((view) => identical(view.timeWindow, controller)), isTrue);
      final pending = Completer<QueryOutcome>();
      service.pending = pending;
      await tapKey(tester, 'monitoring-query');
      final pendingCalls = (service.queries, service.cancellations, service.bindingCancellations);
      zoom = find.byKey(const ValueKey('monitoring-chart-zoom-in')).first;
      await tester.ensureVisible(zoom);
      await tester.tap(zoom);
      await settle(tester);
      final visible = controller.viewport.value;
      expect(visible, MonitoringTimeWindow.fromEpoch(375, 625, 10));
      expect((service.queries, service.cancellations, service.bindingCancellations), pendingCalls);
      pending.complete(result);
      await settle(tester);
      expect(controller.viewport.value, visible);
      expect(service.last!.start, 0);
      expect(service.last!.end, 1000);
      expect(service.last!.stepSeconds, 10);
      expect(service.queries, 2);
      expect(service.panels.length, 1);
      expect(controller.requested, window);
    },
  );
  for (final width in [360.0, 1000.0]) {
    testWidgets('Russian shared zoom controls and loaded-only domain remain usable at width $width', (tester) async {
      final controller = MonitoringTimeWindowController(initial: window);
      addTearDown(controller.dispose);
      await charts(tester, controller, width: width, locale: const Locale('ru'));
      await press(tester, 'monitoring-chart-zoom-in');
      await tester.ensureVisible(inside('same-chart-a', 'monitoring-chart-viewport'));
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-chart-zoom-${width < 500 ? 'narrow' : 'wide'}-ru');
      expect(controller.viewport.value, MonitoringTimeWindow.fromEpoch(250, 750, 10));
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
