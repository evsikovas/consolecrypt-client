import 'dart:convert';
import 'dart:math' as math;

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';

/// Presentation only. The identity distinguishes duplicate labels but never
/// supplies source/binding authority or synthesizes samples.
final class MonitoringChartSeries {
  const MonitoringChartSeries(this.id, this.labelsKey, this.occurrence, this.duplicates, this.source);
  final String id, labelsKey;
  final int occurrence, duplicates;
  final MonitoringSeries source;
  bool get ambiguousIdentity => duplicates > 1;
}

List<MonitoringChartSeries> monitoringChartSeries(List<MonitoringSeries> sources) {
  String canonical(MonitoringSeries source) {
    final keys = source.labels.keys.toList()..sort();
    return jsonEncode({for (final key in keys) key: source.labels[key]});
  }

  final keys = sources.map(canonical).toList();
  final counts = <String, int>{};
  for (final key in keys) {
    counts.update(key, (count) => count + 1, ifAbsent: () => 1);
  }
  final seen = <String, int>{};
  return List.unmodifiable([
    for (var i = 0; i < sources.length; i++)
      (() {
        final key = keys[i], occurrence = seen.update(keys[i], (count) => count + 1, ifAbsent: () => 1);
        return MonitoringChartSeries(jsonEncode([key, occurrence]), key, occurrence, counts[key]!, sources[i]);
      })(),
  ]);
}

/// Retain hidden states only where the label identity is provably unique in
/// both versions. Duplicate groups are intentionally reset on a result update.
Set<String> monitoringRetainedHidden(
  Set<String> hidden,
  List<MonitoringChartSeries> before,
  List<MonitoringChartSeries> after,
) {
  final previous = {for (final row in before.where((r) => !r.ambiguousIdentity)) row.id};
  return {
    for (final row in after)
      if (!row.ambiguousIdentity && previous.contains(row.id) && hidden.contains(row.id)) row.id,
  };
}

final class MonitoringChartDomain {
  const MonitoringChartDomain(this.start, this.end);
  final double start, end;
  bool contains(double timestamp) => timestamp.isFinite && timestamp >= start && timestamp <= end;
  double fraction(double timestamp) {
    final span = end - start;
    if (span.isFinite) return (timestamp - start) / span;
    final scale = math.max(math.max(start.abs(), end.abs()), 1.0);
    return (timestamp / scale - start / scale) / (end / scale - start / scale);
  }

  double timestamp(double fraction) {
    final f = fraction.clamp(0, 1);
    if (f == 0) return start;
    if (f == 1) return end;
    final span = end - start;
    return span.isFinite ? start + f * span : (1 - f) * start + f * end;
  }
}

final class MonitoringChartValueRange {
  const MonitoringChartValueRange(this.low, this.high);
  final double low, high;
  double fraction(double value) {
    final span = high - low;
    if (span.isFinite) return (value - low) / span;
    final scale = math.max(math.max(low.abs(), high.abs()), 1.0);
    return (value / scale - low / scale) / (high / scale - low / scale);
  }
}

final class MonitoringChartInspection {
  const MonitoringChartInspection(this.timestamp, this.snapped, this.rows);
  final double timestamp;
  final bool snapped;
  final List<MonitoringChartInspectedSeries> rows;
}

final class MonitoringChartInspectedSeries {
  const MonitoringChartInspectedSeries(this.series, this.samples);
  final MonitoringChartSeries series;

  /// Empty means missing. Multiple exact values are reported as duplicates.
  final List<MonitoringSample> samples;
}

final class MonitoringChartProjection {
  MonitoringChartProjection({
    required List<MonitoringChartSeries> series,
    Set<String> hidden = const {},
    this.requestedWindow,
    this.effectiveStep,
  }) : series = List.unmodifiable(series),
       hidden = Set.unmodifiable(hidden) {
    final times = <double>[
      for (final row in series)
        for (final point in row.source.samples)
          if (point.timestamp.isFinite) point.timestamp,
    ];
    if (requestedWindow case final window?) {
      domain = MonitoringChartDomain(window.startSeconds, window.endSeconds);
    } else if (times.isNotEmpty) {
      final low = times.reduce(math.min), high = times.reduce(math.max);
      final padding = math.max(low.abs() * 0.000001, 0.5);
      domain = low == high
          ? MonitoringChartDomain(
              math.max(-double.maxFinite, low - padding),
              math.min(double.maxFinite, high + padding),
            )
          : MonitoringChartDomain(low, high);
    } else {
      domain = null;
    }
  }
  final List<MonitoringChartSeries> series;
  final Set<String> hidden;
  final MonitoringTimeWindow? requestedWindow;
  final double? effectiveStep;
  late final MonitoringChartDomain? domain;
  late final List<MonitoringChartSeries> visible = series.where((row) => !hidden.contains(row.id)).toList();
  final _segmentCache = <String, List<List<MonitoringSample>>>{};
  late final MonitoringChartValueRange? valueRange = _range();

  MonitoringChartValueRange? _range() {
    final x = domain;
    if (x == null) return null;
    final values = [
      for (final row in visible)
        for (final sample in row.source.samples)
          if (x.contains(sample.timestamp) && _finite(sample)) sample.value!,
    ];
    if (values.isEmpty) return null;
    final low = values.reduce(math.min), high = values.reduce(math.max);
    if (low != high) return MonitoringChartValueRange(low, high);
    final padding = math.max(low.abs() * 0.05, 1.0);
    return MonitoringChartValueRange(
      math.max(-double.maxFinite, low - padding),
      math.min(double.maxFinite, high + padding),
    );
  }

  List<List<MonitoringSample>> segments(MonitoringChartSeries row) {
    if (_segmentCache[row.id] case final cached?) return cached;
    final result = <List<MonitoringSample>>[];
    List<MonitoringSample>? segment;
    MonitoringSample? previous;
    final interval = effectiveStep != null && effectiveStep!.isFinite && effectiveStep! > 0
        ? effectiveStep
        : requestedWindow?.stepSeconds;
    for (final point in row.source.samples) {
      if (domain == null || !domain!.contains(point.timestamp) || !_finite(point)) {
        previous = null;
        segment = null;
        continue;
      }
      final gap =
          previous != null &&
          (point.timestamp <= previous.timestamp ||
              interval == null ||
              point.timestamp - previous.timestamp > interval * 1.5);
      if (segment == null || gap) {
        segment = [];
        result.add(segment);
      }
      segment.add(point);
      previous = point;
    }
    return _segmentCache[row.id] = result;
  }

  late final List<double> timestamps = (<double>{
    for (final row in visible)
      for (final point in row.source.samples)
        if (domain?.contains(point.timestamp) ?? false) point.timestamp,
  }.toList()..sort());

  MonitoringChartInspection? inspect(double fraction, {required double toleranceFraction}) {
    final x = domain;
    if (x == null || !fraction.isFinite || !toleranceFraction.isFinite || toleranceFraction < 0) return null;
    final cursor = x.timestamp(fraction);
    double? nearest;
    for (final timestamp in timestamps) {
      if (nearest == null || (timestamp - cursor).abs() < (nearest - cursor).abs()) nearest = timestamp;
    }
    final snapped = nearest != null && (x.fraction(nearest) - x.fraction(cursor)).abs() <= toleranceFraction;
    return at(snapped ? nearest : cursor, snapped: snapped);
  }

  MonitoringChartInspection at(double timestamp, {bool snapped = true}) =>
      MonitoringChartInspection(timestamp, snapped, [
        for (final row in visible)
          MonitoringChartInspectedSeries(
            row,
            snapped ? row.source.samples.where((point) => point.timestamp == timestamp).toList() : const [],
          ),
      ]);

  MonitoringChartInspection? move(double? current, int direction) {
    final values = timestamps;
    if (values.isEmpty) return null;
    if (current == null) return at(direction < 0 ? values.last : values.first);
    final matches = direction < 0
        ? values.where((time) => time < current).toList()
        : values.where((time) => time > current).toList();
    return at(
      matches.isEmpty ? (direction < 0 ? values.first : values.last) : (direction < 0 ? matches.last : matches.first),
    );
  }

  static bool _finite(MonitoringSample point) => point.kind == SampleKind.finite && (point.value?.isFinite ?? false);
}
