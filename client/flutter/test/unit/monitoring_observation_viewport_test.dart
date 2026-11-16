import 'package:consolecrypt/monitoring/monitoring_chart_viewport.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter_test/flutter_test.dart';

void check(bool value) {
  if (!value) throw StateError('Viewport expectation failed');
}

MonitoringTimeWindow window(double start, double end, [double step = 15]) =>
    MonitoringTimeWindow.fromEpoch(start, end, step);
void main() {
  test('full view follows the actual accepted rolling interval', () {
    final base = window(0, 3600), next = window(15, 3615);
    check(MonitoringChartViewport.rebase(base, base, next) == next);
  });
  test('zoom inside next domain retains absolute UTC rather than silently following now', () {
    final zoom = window(1000, 2000), next = window(15, 3615);
    check(MonitoringChartViewport.rebase(window(0, 3600), zoom, next) == zoom);
  });
  test('rolling edge clamps zoom to loaded bounds while preserving span', () {
    final next = window(1500, 5100), result = MonitoringChartViewport.rebase(window(0, 3600), window(1000, 2000), next);
    check(result == window(1500, 2500));
    check(result.startSeconds >= next.startSeconds && result.endSeconds <= next.endSeconds);
  });
  test('narrower newly accepted base never retains an out-of-bounds viewport', () {
    final next = window(1500, 1600);
    check(MonitoringChartViewport.rebase(window(0, 3600), window(1000, 2000), next) == next);
  });
  test('new sample interval resets viewport instead of reinterpreting old step', () {
    final next = window(15, 3615, 60);
    check(MonitoringChartViewport.rebase(window(0, 3600), window(1000, 2000), next) == next);
  });
  test('invalid old viewport is rejected rather than expanding new data authority', () {
    final next = window(15, 3615);
    check(MonitoringChartViewport.rebase(window(0, 3600), window(-1, 2000), next) == next);
  });
  test('same loaded window preserves viewport; stale retries need no presentation change', () {
    final base = window(0, 3600), zoom = window(1000, 2000);
    check(MonitoringChartViewport.rebase(base, zoom, base) == zoom);
  });
}
