enum MonitoringTimePreset {
  minutes15(Duration(minutes: 15)),
  hour1(Duration(hours: 1)),
  hours6(Duration(hours: 6)),
  day1(Duration(days: 1)),
  days7(Duration(days: 7)),
  custom(null);

  const MonitoringTimePreset(this.duration);
  final Duration? duration;
}

enum MonitoringTimeWindowError { invalidStart, invalidEnd, invalidOrder, invalidStep, notApplied }

final class MonitoringTimeWindowFailure implements Exception {
  const MonitoringTimeWindowFailure(this.reason);
  final MonitoringTimeWindowError reason;
}

/// Explicit requested UTC domain, independent of where returned samples begin.
/// Native still decides effective step, budgets, data availability and limits.
final class MonitoringTimeWindow {
  MonitoringTimeWindow({required this.startUtc, required this.endUtc, required this.stepSeconds}) {
    if (!startUtc.isUtc) throw const MonitoringTimeWindowFailure(MonitoringTimeWindowError.invalidStart);
    if (!endUtc.isUtc) throw const MonitoringTimeWindowFailure(MonitoringTimeWindowError.invalidEnd);
    if (!startUtc.isBefore(endUtc) || startSeconds >= endSeconds) {
      throw const MonitoringTimeWindowFailure(MonitoringTimeWindowError.invalidOrder);
    }
    if (!stepSeconds.isFinite || stepSeconds <= 0) {
      throw const MonitoringTimeWindowFailure(MonitoringTimeWindowError.invalidStep);
    }
  }

  factory MonitoringTimeWindow.parse({required String start, required String end, required String step}) {
    final first = _utc(start.trim(), MonitoringTimeWindowError.invalidStart);
    final last = _utc(end.trim(), MonitoringTimeWindowError.invalidEnd);
    final interval = double.tryParse(step.trim());
    if (interval == null || !interval.isFinite || interval <= 0) {
      throw const MonitoringTimeWindowFailure(MonitoringTimeWindowError.invalidStep);
    }
    return MonitoringTimeWindow(startUtc: first, endUtc: last, stepSeconds: interval);
  }

  factory MonitoringTimeWindow.preset(MonitoringTimePreset preset, DateTime endUtc, {double stepSeconds = 15}) {
    final duration = preset.duration;
    if (duration == null) throw const MonitoringTimeWindowFailure(MonitoringTimeWindowError.invalidOrder);
    return MonitoringTimeWindow(startUtc: endUtc.subtract(duration), endUtc: endUtc, stepSeconds: stepSeconds);
  }

  factory MonitoringTimeWindow.fromEpoch(double start, double end, double stepSeconds) {
    DateTime date(double seconds, MonitoringTimeWindowError reason) {
      if (!seconds.isFinite) throw MonitoringTimeWindowFailure(reason);
      final microseconds = seconds * Duration.microsecondsPerSecond;
      // DateTime's documented range is +/-100,000,000 days from the epoch.
      // Reject unsupported values before its constructor rather than catching Error.
      if (!microseconds.isFinite || microseconds.abs() > const Duration(days: 100000000).inMicroseconds) {
        throw MonitoringTimeWindowFailure(reason);
      }
      return DateTime.fromMicrosecondsSinceEpoch(microseconds.round(), isUtc: true);
    }

    return MonitoringTimeWindow(
      startUtc: date(start, MonitoringTimeWindowError.invalidStart),
      endUtc: date(end, MonitoringTimeWindowError.invalidEnd),
      stepSeconds: stepSeconds,
    );
  }

  final DateTime startUtc, endUtc;
  final double stepSeconds;
  double get startSeconds => startUtc.microsecondsSinceEpoch / Duration.microsecondsPerSecond;
  double get endSeconds => endUtc.microsecondsSinceEpoch / Duration.microsecondsPerSecond;
  String get startText => startUtc.toIso8601String();
  String get endText => endUtc.toIso8601String();
  bool contains(double timestamp) => timestamp.isFinite && timestamp >= startSeconds && timestamp <= endSeconds;
  double fraction(double timestamp) => (timestamp - startSeconds) / (endSeconds - startSeconds);

  @override
  bool operator ==(Object other) =>
      other is MonitoringTimeWindow &&
      startUtc == other.startUtc &&
      endUtc == other.endUtc &&
      stepSeconds == other.stepSeconds;
  @override
  int get hashCode => Object.hash(startUtc, endUtc, stepSeconds);
}

DateTime _utc(String value, MonitoringTimeWindowError reason) {
  final match = RegExp(r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,6}))?Z$').firstMatch(value);
  final parsed = DateTime.tryParse(value);
  if (match == null || parsed == null || !parsed.isUtc) throw MonitoringTimeWindowFailure(reason);
  // DateTime.tryParse normalizes invalid days/months/hours. Reject those rather
  // than silently moving a user's custom requested interval to another date.
  final parts = [parsed.year, parsed.month, parsed.day, parsed.hour, parsed.minute, parsed.second];
  for (var i = 0; i < parts.length; i++) {
    if (parts[i] != int.parse(match[i + 1]!)) throw MonitoringTimeWindowFailure(reason);
  }
  return parsed;
}
