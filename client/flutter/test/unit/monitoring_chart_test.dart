import 'package:consolecrypt/monitoring/monitoring_chart_model.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter_test/flutter_test.dart';

MonitoringSample p(double time, double value) => MonitoringSample(time, SampleKind.finite, value);
MonitoringSeries s(String name, List<MonitoringSample> samples) => MonitoringSeries({'instance': name}, samples);
MonitoringChartProjection project(
  List<MonitoringSeries> input, {
  Set<String> hidden = const {},
  MonitoringTimeWindow? window,
  double? step,
}) => MonitoringChartProjection(
  series: monitoringChartSeries(input),
  hidden: hidden,
  requestedWindow: window,
  effectiveStep: step,
);

void main() {
  test('unknown sampling cadence renders separate markers and does not invent continuity', () {
    final view = project([
      s('a', [p(0, 1), p(10, 2)]),
    ]);
    expect(view.segments(view.series.single).map((points) => points.length), [1, 1]);
    final explicit = project([
      s('a', [p(0, 1), p(10, 2), p(30, 3)]),
    ], window: MonitoringTimeWindow.fromEpoch(0, 40, 10));
    expect(explicit.segments(explicit.series.single).map((points) => points.length), [2, 1]);
  });
  test('sparse rows use the requested common UTC domain even when another row is hidden', () {
    final rows = monitoringChartSeries([
      s('a', [p(100, 2)]),
      s('b', [p(900, 30)]),
    ]);
    final window = MonitoringTimeWindow.fromEpoch(0, 1000, 10);
    for (final hidden in [
      <String>{},
      {rows.last.id},
      rows.map((r) => r.id).toSet(),
    ]) {
      final view = MonitoringChartProjection(series: rows, hidden: hidden, requestedWindow: window);
      expect(view.domain!.start, 0);
      expect(view.domain!.end, 1000);
      expect(view.domain!.fraction(100), .1);
      expect(view.domain!.fraction(900), .9);
    }
    final fallback = MonitoringChartProjection(series: rows, hidden: {rows.last.id});
    expect(fallback.domain!.start, 100);
    expect(fallback.domain!.end, 900);
  });
  test('visible finite values share a Y domain without inserting zero or including out-of-window values', () {
    final rows = monitoringChartSeries([
      s('a', [p(-1, -100), p(0, 2), p(10, 3)]),
      s('b', [p(0, 100), const MonitoringSample(10, SampleKind.positiveInfinity)]),
    ]);
    final view = MonitoringChartProjection(series: rows, requestedWindow: MonitoringTimeWindow.fromEpoch(0, 10, 1));
    expect(view.valueRange!.low, 2);
    expect(view.valueRange!.high, 100);
    final one = MonitoringChartProjection(series: rows, hidden: {rows.last.id}, requestedWindow: view.requestedWindow);
    expect(one.valueRange!.low, 2);
    expect(one.valueRange!.high, 3);
    expect(MonitoringChartProjection(series: rows, hidden: rows.map((r) => r.id).toSet()).valueRange, isNull);
  });
  test('NaN, infinities, missing steps and nonincreasing times break paths', () {
    final view = project([
      s('a', [
        p(0, 1),
        p(10, 2),
        const MonitoringSample(20, SampleKind.nan),
        p(30, 3),
        p(100, 4),
        const MonitoringSample(110, SampleKind.negativeInfinity),
        p(120, 5),
        p(120, 6),
        p(119, 7),
      ]),
    ], step: 10);
    expect(view.segments(view.series.single).map((segment) => segment.map((p) => p.timestamp).toList()).toList(), [
      [0, 10],
      [30],
      [100],
      [120],
      [120],
      [119],
    ]);
  });
  test('cursor in a long gap does not carry a nearby row value across the gap', () {
    final view = project([
      s('a', [p(0, 1), p(100, 2)]),
      s('b', [p(20, 3), p(90, 4)]),
    ], window: MonitoringTimeWindow.fromEpoch(0, 1000, 10));
    final gap = view.inspect(.05, toleranceFraction: .01)!;
    expect(gap.timestamp, 50);
    expect(gap.snapped, isFalse);
    expect(gap.rows.every((row) => row.samples.isEmpty), isTrue);
    final hit = view.inspect(.101, toleranceFraction: .01)!;
    expect(hit.timestamp, 100);
    expect(hit.snapped, isTrue);
    expect(hit.rows.first.samples.single.value, 2);
    expect(hit.rows.last.samples, isEmpty); // No value from t=90 at t=100.
  });
  test('NaN and infinity inspection remain tagged and never become finite zero', () {
    final view = project([
      s('a', [const MonitoringSample(10, SampleKind.nan)]),
      s('b', [const MonitoringSample(10, SampleKind.positiveInfinity)]),
    ]);
    expect(view.valueRange, isNull);
    expect(view.at(10).rows.map((r) => r.samples.single.display), ['NaN', '+Inf']);
  });
  test('canonical label identity survives key order while duplicate-label rows never merge', () {
    final first = MonitoringSeries({'job': 'node', 'instance': 'a'}, [p(0, 1)]),
        second = MonitoringSeries({'instance': 'a', 'job': 'node'}, [p(0, 2)]);
    final rows = monitoringChartSeries([first, second]);
    expect(rows.length, 2);
    expect(rows.map((r) => r.id).toSet().length, 2);
    expect(rows.first.labelsKey, rows.last.labelsKey);
    expect(rows.every((r) => r.ambiguousIdentity), isTrue);
    expect(rows.map((r) => r.occurrence), [1, 2]);
    expect(monitoringRetainedHidden({rows.first.id}, rows, monitoringChartSeries([second, first])), isEmpty);
  });
  test('unique hidden choices survive reordered rows and disappear when the row is removed', () {
    final a = s('a', [p(0, 1)]), b = s('b', [p(0, 2)]);
    final before = monitoringChartSeries([a, b]), after = monitoringChartSeries([b, a]);
    expect(monitoringRetainedHidden({before.last.id}, before, after), {after.first.id});
    expect(monitoringRetainedHidden({before.last.id}, before, monitoringChartSeries([a])), isEmpty);
  });
  test('keyboard inspection follows only actual visible timestamps and clamps at both ends', () {
    final view = project([
      s('a', [p(0, 1), p(20, 2)]),
      s('b', [p(10, 3)]),
    ]);
    expect(view.move(null, 1)!.timestamp, 0);
    expect(view.move(0, 1)!.timestamp, 10);
    expect(view.move(10, 1)!.timestamp, 20);
    expect(view.move(20, 1)!.timestamp, 20);
    expect(view.move(null, -1)!.timestamp, 20);
    expect(view.move(20, -1)!.timestamp, 10);
    expect(view.move(0, -1)!.timestamp, 0);
    final hidden = MonitoringChartProjection(series: view.series, hidden: view.series.map((r) => r.id).toSet());
    expect(hidden.move(null, 1), isNull);
  });
  test('duplicate samples at a timestamp are not silently reduced to one value', () {
    final view = project([
      s('a', [p(10, 2), p(10, 5)]),
    ]);
    expect(view.at(10).rows.single.samples.map((p) => p.value), [2, 5]);
    expect(view.segments(view.series.single).length, 2);
  });
  test('fractional requested domain and very large finite Y values remain drawable', () {
    final view = project([
      s('a', [p(.1, -1e308), p(.2, 1e308)]),
    ], window: MonitoringTimeWindow.fromEpoch(.1, .2, .01));
    expect(view.domain!.fraction(.15), closeTo(.5, 1e-12));
    expect(view.valueRange!.fraction(-1e308), 0);
    expect(view.valueRange!.fraction(0), .5);
    expect(view.valueRange!.fraction(1e308), 1);
    final constant = project([
      s('max', [p(10, double.maxFinite)]),
    ]);
    expect(constant.valueRange!.low.isFinite, isTrue);
    expect(constant.valueRange!.high.isFinite, isTrue);
    expect(constant.valueRange!.fraction(double.maxFinite).isFinite, isTrue);
  });
  test('empty or wholly outside returned data never creates samples inside the requested window', () {
    final window = MonitoringTimeWindow.fromEpoch(10, 20, 1);
    final empty = project([], window: window);
    expect(empty.domain!.start, 10);
    expect(empty.valueRange, isNull);
    expect(empty.inspect(.5, toleranceFraction: .1)!.rows, isEmpty);
    final outside = project([
      s('a', [p(0, 5), p(30, 7)]),
    ], window: window);
    expect(outside.segments(outside.series.single), isEmpty);
    expect(outside.valueRange, isNull);
    expect(outside.inspect(.5, toleranceFraction: .1)!.rows.single.samples, isEmpty);
    expect(outside.series.single.source.samples.length, 2); // Original table stays intact.
    expect(project([]).inspect(.5, toleranceFraction: .1), isNull);
  });
}
