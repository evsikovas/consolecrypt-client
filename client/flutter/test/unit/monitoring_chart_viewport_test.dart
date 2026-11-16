import 'package:consolecrypt/monitoring/monitoring_chart_viewport.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:flutter_test/flutter_test.dart';

MonitoringTimeWindow w(double start, double end, [double step = 10]) =>
    MonitoringTimeWindow.fromEpoch(start, end, step);

void main() {
  test('zoom anchoring and pan stay inside the loaded base with a requested-step minimum span', () {
    final base = w(0, 1000), center = MonitoringChartViewport.zoom(base, base, .5)!;
    expect(center, w(250, 750));
    expect(MonitoringChartViewport.zoom(base, base, .5, anchor: 0), w(0, 500));
    expect(MonitoringChartViewport.zoom(base, base, .5, anchor: 1), w(500, 1000));
    expect(MonitoringChartViewport.pan(base, center, .25), w(375, 875));
    expect(MonitoringChartViewport.pan(base, center, 1e308), w(500, 1000));
    expect(MonitoringChartViewport.pan(base, center, -1e308), w(0, 500));
    var smallest = base;
    for (var i = 0; i < 100; i++) {
      smallest = MonitoringChartViewport.zoom(base, smallest, .5, anchor: 0)!;
    }
    expect(smallest, w(0, 10));
    expect(MonitoringChartViewport.zoom(base, smallest, .5, anchor: 0), smallest);
    expect(MonitoringChartViewport.zoom(base, smallest, 1e308), base);
  });
  test('nonfinite commands and stale query interval/domain inputs are rejected', () {
    final base = w(0, 1000);
    for (final factor in [0.0, -1.0, double.nan, double.infinity]) {
      expect(MonitoringChartViewport.zoom(base, base, factor), isNull);
    }
    expect(MonitoringChartViewport.zoom(base, base, .5, anchor: double.nan), isNull);
    expect(MonitoringChartViewport.pan(base, base, double.infinity), isNull);
    expect(MonitoringChartViewport.zoom(base, w(-1, 1000), .5), isNull);
    expect(MonitoringChartViewport.zoom(base, w(0, 1000, 20), .5), isNull);
  });
  test('fractional, submicrosecond and extreme valid UTC bounds remain representable', () {
    final fractional = w(.1, .2, .01), view = MonitoringChartViewport.zoom(fractional, fractional, .5)!;
    expect(view, w(.125, .175, .01));
    final micro = w(0, 1, .00000001), one = MonitoringChartViewport.zoom(micro, micro, 1e-20)!;
    expect(one.endUtc.microsecondsSinceEpoch - one.startUtc.microsecondsSinceEpoch, greaterThanOrEqualTo(1));
    for (final base in [
      w(-1000, 0),
      w(8640000000000 - 1, 8640000000000, .000001),
      w(-8640000000000, 8640000000000, 1),
    ]) {
      final zoomed = MonitoringChartViewport.zoom(base, base, 1e-20)!;
      expect(zoomed.startUtc.isBefore(zoomed.endUtc), isTrue);
      expect(zoomed.startSeconds, greaterThanOrEqualTo(base.startSeconds));
      expect(zoomed.endSeconds, lessThanOrEqualTo(base.endSeconds));
      expect(zoomed.stepSeconds, base.stepSeconds);
    }
  });
  test('a period narrower than its interval does not invent additional detail', () {
    final base = w(0, 1, 15);
    expect(MonitoringChartViewport.zoom(base, base, .5), same(base));
    expect(MonitoringChartViewport.pan(base, base, .5), same(base));
  });
  test('presentation changes notify only the viewport and preserve every request/draft field', () {
    final controller = MonitoringTimeWindowController(initial: w(0, 1000));
    addTearDown(controller.dispose);
    var periodEvents = 0, viewportEvents = 0;
    controller.addListener(() => periodEvents++);
    controller.viewport.addListener(() => viewportEvents++);
    final before = (
      controller.requested,
      controller.draftStart,
      controller.draftEnd,
      controller.draftStep,
      controller.preset,
      controller.revision,
    );
    expect(controller.zoom(.5), isTrue);
    expect(controller.pan(.25), isTrue);
    expect(controller.resetViewport(), isTrue);
    expect(periodEvents, 0);
    expect(viewportEvents, 3);
    expect((
      controller.requested,
      controller.draftStart,
      controller.draftEnd,
      controller.draftStep,
      controller.preset,
      controller.revision,
    ), before);
    expect(controller.dirty, isFalse);
    expect(controller.zoom(0), isFalse);
    expect(viewportEvents, 3);
  });
  test('draft edits and explicit new periods reset the viewport through the existing lifecycle', () {
    final controller = MonitoringTimeWindowController(initial: w(0, 1000));
    addTearDown(controller.dispose);
    controller.zoom(.5);
    controller.edit(step: '');
    expect(controller.viewport.value, controller.applied);
    expect(controller.zoom(.5), isFalse);
    expect(controller.apply(), isFalse);
    expect(controller.dirty, isTrue);
    controller.edit(start: '1970-01-01T00:00:10Z', end: '1970-01-01T00:01:40Z', step: '5');
    expect(controller.apply(), isTrue);
    expect(controller.viewport.value, w(10, 100, 5));
    expect(controller.requested, w(10, 100, 5));
  });
  test('history restore and preset drafts reset view without changing the captured absolute period', () {
    final controller = MonitoringTimeWindowController(initial: w(0, 1000), now: () => DateTime.utc(2026, 11, 11));
    addTearDown(controller.dispose);
    controller.zoom(.5);
    final history = w(2000, 3000, 15);
    controller.restore(history);
    expect(controller.viewport.value, history);
    expect(controller.requested, history);
    controller.zoom(.5);
    controller.select(MonitoringTimePreset.hour1);
    expect(controller.viewport.value, history);
    expect(controller.applied, history);
    expect(controller.dirty, isTrue);
  });
  test('disposed scope rejects late presentation callbacks and a new scope starts at its full window', () {
    final old = MonitoringTimeWindowController(initial: w(0, 1000));
    old.zoom(.5);
    old.dispose();
    expect(old.zoom(.5), isFalse);
    expect(old.pan(.25), isFalse);
    expect(old.resetViewport(), isFalse);
    final current = MonitoringTimeWindowController(initial: w(0, 1000));
    addTearDown(current.dispose);
    expect(current.viewport.value, current.requested);
    expect(current.canZoomOut, isFalse);
  });
}
