import 'dart:math' as math;

import 'package:consolecrypt/monitoring/monitoring_time_window.dart';

/// Bounded presentation math, not a second period/controller or query plan.
/// Only already-loaded samples inside [base] can be viewed. Native request
/// bounds and interval remain the original applied window.
abstract final class MonitoringChartViewport {
  static double minimumSpan(MonitoringTimeWindow base) {
    final full = base.endSeconds - base.startSeconds;
    final precision = math.max(base.startSeconds.abs(), base.endSeconds.abs()) * 4 * 2.220446049250313e-16;
    return math.min(full, math.max(base.stepSeconds, math.max(0.000001, precision)));
  }

  static bool _inside(MonitoringTimeWindow base, MonitoringTimeWindow current) =>
      current.stepSeconds == base.stepSeconds &&
      current.startSeconds >= base.startSeconds &&
      current.endSeconds <= base.endSeconds;

  static MonitoringTimeWindow? zoom(
    MonitoringTimeWindow base,
    MonitoringTimeWindow current,
    double factor, {
    double anchor = .5,
  }) {
    if (!_inside(base, current) || !factor.isFinite || factor <= 0 || !anchor.isFinite) return null;
    final full = base.endSeconds - base.startSeconds, currentSpan = current.endSeconds - current.startSeconds;
    final fraction = anchor.clamp(0.0, 1.0);
    final span = (currentSpan * factor).clamp(minimumSpan(base), full);
    if (span == full) return base;
    final focus = current.startSeconds + currentSpan * fraction;
    final start = (focus - span * fraction).clamp(base.startSeconds, base.endSeconds - span);
    return _window(base, start, start + span);
  }

  static MonitoringTimeWindow? pan(MonitoringTimeWindow base, MonitoringTimeWindow current, double fraction) {
    if (!_inside(base, current) || !fraction.isFinite) return null;
    if (current == base) return base;
    final span = current.endSeconds - current.startSeconds;
    final start = (current.startSeconds + span * fraction).clamp(base.startSeconds, base.endSeconds - span);
    return _window(base, start, start + span);
  }

  /// Rebase only a passive result viewport after an explicitly rolling read
  /// completes. It cannot change a query plan or request samples outside next.
  static MonitoringTimeWindow rebase(
    MonitoringTimeWindow base,
    MonitoringTimeWindow current,
    MonitoringTimeWindow next,
  ) {
    if (base.stepSeconds != next.stepSeconds || !_inside(base, current) || current == base) return next;
    final span = math.min(current.endSeconds - current.startSeconds, next.endSeconds - next.startSeconds);
    if (span >= next.endSeconds - next.startSeconds) return next;
    final start = current.startSeconds.clamp(next.startSeconds, next.endSeconds - span);
    return _window(next, start, start + span) ?? next;
  }

  static MonitoringTimeWindow? _window(MonitoringTimeWindow base, double start, double end) {
    var first = (start * Duration.microsecondsPerSecond).round().clamp(
      base.startUtc.microsecondsSinceEpoch,
      base.endUtc.microsecondsSinceEpoch,
    );
    var last = (end * Duration.microsecondsPerSecond).round().clamp(
      base.startUtc.microsecondsSinceEpoch,
      base.endUtc.microsecondsSinceEpoch,
    );
    // Two half-microsecond ties can round to the same integer. Retain one
    // representable microsecond within the original bounds instead of failing.
    if (first >= last) {
      if (first < base.endUtc.microsecondsSinceEpoch) {
        last = first + 1;
      } else if (last > base.startUtc.microsecondsSinceEpoch) {
        first = last - 1;
      } else {
        return null;
      }
    }
    return MonitoringTimeWindow(
      startUtc: DateTime.fromMicrosecondsSinceEpoch(first, isUtc: true),
      endUtc: DateTime.fromMicrosecondsSinceEpoch(last, isUtc: true),
      stepSeconds: base.stepSeconds,
    );
  }
}
