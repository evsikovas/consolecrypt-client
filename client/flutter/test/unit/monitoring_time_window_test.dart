import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:flutter_test/flutter_test.dart';

final fixedEnd = DateTime.utc(2026, 11, 11, 12);
MonitoringTimeWindow initialWindow() => MonitoringTimeWindow.fromEpoch(0, 1000, 15);

void main() {
  test('all five preset spans share the exact selected UTC end', () {
    for (final preset in MonitoringTimePreset.values.where((v) => v != MonitoringTimePreset.custom)) {
      final window = MonitoringTimeWindow.preset(preset, fixedEnd);
      expect(window.endUtc, fixedEnd);
      expect(window.endUtc.difference(window.startUtc), preset.duration);
      expect(window.stepSeconds, 15);
    }
  });
  test('custom UTC parsing rejects normalized invalid dates and absent or nonfinite intervals', () {
    for (final start in [
      '',
      '2026-02-30T00:00:00Z',
      '2026-13-11T00:00:00Z',
      '2026-11-11T24:00:00Z',
      '2026-11-11T11:00:00+00:00',
      '2026-11-11T11:00:60Z',
    ]) {
      expect(
        () => MonitoringTimeWindow.parse(start: start, end: fixedEnd.toIso8601String(), step: '15'),
        throwsA(
          isA<MonitoringTimeWindowFailure>().having((e) => e.reason, 'reason', MonitoringTimeWindowError.invalidStart),
        ),
      );
    }
    for (final step in ['', 'NaN', 'Infinity', '1e309', '0', '-1']) {
      expect(
        () => MonitoringTimeWindow.parse(start: '2026-11-11T11:00:00Z', end: fixedEnd.toIso8601String(), step: step),
        throwsA(
          isA<MonitoringTimeWindowFailure>().having((e) => e.reason, 'reason', MonitoringTimeWindowError.invalidStep),
        ),
      );
    }
    expect(
      () => MonitoringTimeWindow.parse(start: fixedEnd.toIso8601String(), end: '', step: '15'),
      throwsA(
        isA<MonitoringTimeWindowFailure>().having((e) => e.reason, 'reason', MonitoringTimeWindowError.invalidEnd),
      ),
    );
    expect(
      () => MonitoringTimeWindow.parse(start: fixedEnd.toIso8601String(), end: fixedEnd.toIso8601String(), step: '15'),
      throwsA(
        isA<MonitoringTimeWindowFailure>().having((e) => e.reason, 'reason', MonitoringTimeWindowError.invalidOrder),
      ),
    );
  });
  test('fractional interval and microsecond UTC boundaries are retained', () {
    final window = MonitoringTimeWindow.parse(
      start: '2026-11-11T11:00:00.123456Z',
      end: '2026-11-11T12:00:00.123456Z',
      step: '.25',
    );
    expect(window.startText, '2026-11-11T11:00:00.123456Z');
    expect(window.stepSeconds, .25);
    expect(window.fraction(window.startSeconds), 0);
    expect(window.fraction(window.endSeconds), 1);
    expect(window.fraction((window.startSeconds + window.endSeconds) / 2), closeTo(.5, 1e-9));
    expect(window.contains(double.nan), isFalse);
  });
  test('editing and preset selection invalidate requests without applying a new domain', () {
    final original = initialWindow();
    final controller = MonitoringTimeWindowController(initial: original, now: () => fixedEnd);
    addTearDown(controller.dispose);
    var notifications = 0;
    controller.addListener(() => notifications++);
    controller.select(MonitoringTimePreset.days7);
    expect(controller.applied, original);
    expect(controller.dirty, isTrue);
    expect(() => controller.requested, throwsA(isA<MonitoringTimeWindowFailure>()));
    expect(notifications, 1);
    expect(controller.apply(), isTrue);
    expect(controller.requested.endUtc, fixedEnd);
    expect(controller.requested.endUtc.difference(controller.requested.startUtc), const Duration(days: 7));
    expect(notifications, 2);
  });
  test('invalid apply retains the last accepted window with no fallback step', () {
    final original = initialWindow(), controller = MonitoringTimeWindowController(initial: initialWindow());
    addTearDown(controller.dispose);
    controller.edit(step: '');
    expect(controller.apply(), isFalse);
    expect(controller.error, MonitoringTimeWindowError.invalidStep);
    expect(controller.applied, original);
    expect(controller.dirty, isTrue);
    controller.edit(step: '30');
    expect(controller.apply(), isTrue);
    expect(controller.requested.stepSeconds, 30);
  });
  test('choosing a preset never silently replaces an invalid custom interval', () {
    final controller = MonitoringTimeWindowController(initial: initialWindow(), now: () => fixedEnd);
    addTearDown(controller.dispose);
    controller.edit(step: '');
    controller.select(MonitoringTimePreset.days7);
    expect(controller.draftStep, isEmpty);
    expect(controller.apply(), isFalse);
    expect(controller.error, MonitoringTimeWindowError.invalidStep);
  });
  test('the same preset can explicitly move end to now without applying or resetting the interval', () {
    var now = fixedEnd;
    final controller = MonitoringTimeWindowController(now: () => now);
    addTearDown(controller.dispose);
    controller.edit(step: '30');
    controller.apply();
    now = now.add(const Duration(hours: 1));
    controller.select(MonitoringTimePreset.hour1);
    expect(controller.applied.endUtc, fixedEnd);
    expect(controller.draftEnd, now.toIso8601String());
    expect(controller.draftStep, '30.0');
    expect(controller.dirty, isTrue);
    controller.apply();
    expect(controller.requested.endUtc, now);
    expect(controller.requested.stepSeconds, 30);
  });
  test('history restore preserves old absolute time and invalidates same-value pending work', () {
    final old = initialWindow(), controller = MonitoringTimeWindowController(now: () => fixedEnd);
    addTearDown(controller.dispose);
    final before = controller.revision;
    controller.restore(old);
    expect(controller.requested, old);
    expect(controller.preset, MonitoringTimePreset.custom);
    expect(controller.revision, greaterThan(before));
    final revision = controller.revision;
    controller.edit(step: '15');
    controller.apply();
    expect(controller.requested, old);
    expect(controller.revision, greaterThan(revision));
  });
}
