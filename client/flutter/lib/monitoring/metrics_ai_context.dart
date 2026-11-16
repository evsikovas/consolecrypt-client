import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_accepted_query.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';

enum MetricsAiProjectionIssue { noSelection, invalidSelection, invalidSnapshot, contextChanged, budget }

final class MetricsAiProjectionException implements Exception {
  const MetricsAiProjectionException(this.issue);
  final MetricsAiProjectionIssue issue;
  @override
  String toString() => 'MetricsAiProjectionException(${issue.name})';
}

Never _reject(MetricsAiProjectionIssue issue) => throw MetricsAiProjectionException(issue);

/// Local provenance only. This object is never serialized into model context.
/// The host supplies its original unlocked profile/service/provider leases and
/// compares them through existing scope owners, never after-await latest state.
final class MetricsAiFence {
  const MetricsAiFence({
    required this.profileLease,
    required this.metricsLease,
    required this.providerLease,
    required this.sourceFingerprint,
  });
  final Object profileLease, metricsLease, providerLease;
  final String sourceFingerprint;
  bool matches(MetricsAiFence other) =>
      identical(profileLease, other.profileLease) &&
      identical(metricsLease, other.metricsLease) &&
      identical(providerLease, other.providerLease) &&
      sourceFingerprint == other.sourceFingerprint;
  @override
  String toString() => 'MetricsAiFence(<local provenance>)';
}

List<int> _indices(List<int> selected, int length, int max) {
  if (selected.length > max) _reject(MetricsAiProjectionIssue.budget);
  if (selected.isEmpty ||
      selected.toSet().length != selected.length ||
      selected.any((index) => index < 0 || index >= length)) {
    _reject(MetricsAiProjectionIssue.invalidSelection);
  }
  return List.unmodifiable(selected);
}

final _labelName = RegExp(r'^[A-Za-z_][A-Za-z0-9_]{0,127}$');
final _metricName = RegExp(r'^[A-Za-z_:][A-Za-z0-9_:]{0,127}$');
final _address = RegExp(r'[A-Za-z][A-Za-z0-9+.-]*://|\b(?:[0-9]{1,3}\.){3}[0-9]{1,3}\b|[A-Za-z0-9_.-]+:[0-9]{1,5}\b');
bool _privateLabel(String key) {
  final normalized = key.toLowerCase().replaceAll('-', '_');
  return const {
        'instance',
        '__address__',
        'endpoint',
        'url',
        'uri',
        'address',
        'host',
        'hostname',
        'ip',
        'port',
        'password',
        'passwd',
        'secret',
        'token',
        'authorization',
        'cookie',
        'api_key',
        'apikey',
      }.contains(normalized) ||
      normalized.contains('credential') ||
      normalized.contains('password') ||
      normalized.contains('secret') ||
      normalized.endsWith('_token');
}

bool _controls(String text) => text.runes.any((r) => r < 32 || r >= 127 && r <= 159);

({Map<String, String> values, int omitted}) _labels(Map<String, String> labels, List<String> keys) {
  if (keys.length > 8 || keys.toSet().length != keys.length) {
    _reject(MetricsAiProjectionIssue.budget);
  }
  final values = <String, String>{};
  var omitted = 0;
  for (final key in keys) {
    if (!_labelName.hasMatch(key) || !labels.containsKey(key)) {
      _reject(MetricsAiProjectionIssue.invalidSelection);
    }
    if (_privateLabel(key)) {
      omitted++;
      continue;
    }
    final value = labels[key]!;
    if (value.length > 128 || utf8.encode(value).length > 128) {
      _reject(MetricsAiProjectionIssue.budget);
    }
    if (_controls(value) || _address.hasMatch(value) || value.contains('::')) {
      omitted++;
      continue;
    }
    values[key] = value;
  }
  return (values: Map.unmodifiable(values), omitted: omitted);
}

Map<String, Object?> _sample(MonitoringSample sample) {
  if (!sample.timestamp.isFinite ||
      (sample.kind == SampleKind.finite && (sample.value == null || !sample.value!.isFinite)) ||
      (sample.kind != SampleKind.finite && sample.value != null)) {
    _reject(MetricsAiProjectionIssue.invalidSnapshot);
  }
  return {
    'timestampUnixSecondsUtc': sample.timestamp,
    'value': {'kind': sample.kind.name, if (sample.kind == SampleKind.finite) 'value': sample.value},
  };
}

Map<String, Object?> _window(MonitoringQueryIdentity identity, QueryOutcome outcome) {
  final Object? decoded;
  try {
    decoded = jsonDecode(identity.operation);
  } catch (_) {
    _reject(MetricsAiProjectionIssue.invalidSnapshot);
  }
  if (decoded is! Map<String, dynamic>) {
    _reject(MetricsAiProjectionIssue.invalidSnapshot);
  }
  final effective = outcome.effectiveStepSeconds;
  if (effective != null && (!effective.isFinite || effective <= 0)) {
    _reject(MetricsAiProjectionIssue.invalidSnapshot);
  }
  if (decoded['kind'] == 'query') {
    return {'kind': 'instant', 'requestedAtUtc': null};
  }
  final start = decoded['start'], end = decoded['end'], step = decoded['stepSeconds'];
  if (decoded['kind'] != 'queryRange' ||
      start is! num ||
      end is! num ||
      step is! num ||
      !start.isFinite ||
      !end.isFinite ||
      !step.isFinite ||
      start >= end ||
      step <= 0) {
    _reject(MetricsAiProjectionIssue.invalidSnapshot);
  }
  return {
    'kind': 'range',
    'startUnixSecondsUtc': start,
    'endUnixSecondsUtc': end,
    'requestedStepSeconds': step,
    'effectiveStepSeconds': effective,
  };
}

/// One explicitly selected series and explicit sample indices. This does not
/// fetch, expand, downsample or infer a unit from names/Grafana user hints.
final class MetricsAiMetricSelection {
  MetricsAiMetricSelection._(this._fence, this._json);
  final MetricsAiFence _fence;
  final String _json;

  factory MetricsAiMetricSelection.fromAccepted(
    MonitoringAcceptedQueryState state,
    MetricsAiFence captured, {
    required int seriesIndex,
    required List<int> sampleIndices,
    List<String> labelKeys = const [],
  }) {
    final accepted = state.accepted, identity = state.identity;
    if (accepted == null || identity == null || !identity.matches(accepted.identity)) {
      _reject(MetricsAiProjectionIssue.invalidSelection);
    }
    if (!identical(identity.scope, captured.metricsLease) || identity.sourceFingerprint != captured.sourceFingerprint) {
      _reject(MetricsAiProjectionIssue.contextChanged);
    }
    if (state.error != null && !monitoringMayRetainAcceptedQuery(state.error)) {
      _reject(MetricsAiProjectionIssue.invalidSnapshot);
    }
    final outcome = accepted.outcome, data = accepted.outcome.data;
    if (outcome.status != 'success' ||
        outcome.error != null ||
        data == null ||
        !['matrix', 'vector', 'scalar'].contains(data.kind)) {
      _reject(MetricsAiProjectionIssue.invalidSnapshot);
    }
    return MetricsAiMetricSelection._build(
      captured,
      outcome,
      data,
      seriesIndex,
      sampleIndices,
      labelKeys,
      _window(accepted.identity, outcome),
      accepted.completedUtc,
      state.error != null,
      state.pendingToken != null,
    );
  }

  /// Uses an actual accepted Auto/pinned custom or saved-panel result. This
  /// never creates a fake PromQL request or relabels a rolling result's window.
  factory MetricsAiMetricSelection.fromObservation(
    MonitoringPinState state,
    MetricsAiFence captured, {
    required Object originalOwner,
    required int seriesIndex,
    required List<int> sampleIndices,
    List<String> labelKeys = const [],
  }) {
    final plan = state.plan, value = state.value, completed = state.lastSuccessUtc;
    if (!identical(originalOwner, captured.metricsLease) ||
        plan == null ||
        value == null ||
        completed == null ||
        plan.source.readOnly ||
        plan.source.fingerprint != captured.sourceFingerprint ||
        !monitoringObservationValueMatchesPlan(plan, value) ||
        state.error != null && !monitoringMayRetainAcceptedQuery(state.error)) {
      _reject(MetricsAiProjectionIssue.contextChanged);
    }
    final outcome = value.queryOutcome, actual = value.window;
    final data = outcome?.data;
    if (outcome == null ||
        outcome.status != 'success' ||
        outcome.error != null ||
        data == null ||
        !['matrix', 'vector', 'scalar'].contains(data.kind)) {
      _reject(MetricsAiProjectionIssue.invalidSnapshot);
    }
    final Map<String, Object?> window = actual == null
        ? {'kind': 'instant', 'requestedAtUtc': null}
        : {
            'kind': 'range',
            'startUnixSecondsUtc': actual.startSeconds,
            'endUnixSecondsUtc': actual.endSeconds,
            'requestedStepSeconds': actual.stepSeconds,
            'effectiveStepSeconds': outcome.effectiveStepSeconds,
          };
    return MetricsAiMetricSelection._build(
      captured,
      outcome,
      data,
      seriesIndex,
      sampleIndices,
      labelKeys,
      window,
      completed,
      state.error != null,
      state.busy,
    );
  }

  static MetricsAiMetricSelection _build(
    MetricsAiFence captured,
    QueryOutcome outcome,
    QueryData data,
    int seriesIndex,
    List<int> sampleIndices,
    List<String> labelKeys,
    Map<String, Object?> window,
    DateTime completedUtc,
    bool stale,
    bool refreshPending,
  ) {
    final effective = outcome.effectiveStepSeconds;
    if (effective != null && (!effective.isFinite || effective <= 0)) {
      _reject(MetricsAiProjectionIssue.invalidSnapshot);
    }
    late List<MonitoringSample> samples;
    Map<String, String> labels = const {};
    if (data.kind == 'scalar') {
      if (seriesIndex != 0 || data.sample == null) {
        _reject(MetricsAiProjectionIssue.invalidSelection);
      }
      samples = [data.sample!];
    } else {
      _indices([seriesIndex], data.series.length, 1);
      samples = data.series[seriesIndex].samples;
      labels = data.series[seriesIndex].labels;
    }
    final indices = _indices(sampleIndices, samples.length, 32), selectedLabels = _labels(labels, labelKeys);
    final points = indices.map((index) => _sample(samples[index])).toList(growable: false);
    final name = labels['__name__'];
    final row = <String, Object?>{
      'metricName': name != null && _metricName.hasMatch(name) ? name : null,
      'unit': {'kind': 'unknown'},
      'labels': selectedLabels.values,
      'selectedLabelsOmitted': selectedLabels.omitted,
      'samples': points,
      'availableSampleCount': samples.length,
      'selectedSampleCount': points.length,
      'sampleSelection': 'explicitIndicesCallerOrder',
      'gapKnowledge': 'unselectedOrAbsentSamplesAreUnknownNeverZero',
      'actualAcceptedWindow': window,
      'acceptedCompletedUtc': completedUtc.toUtc().toIso8601String(),
      'stale': stale,
      'refreshPending': refreshPending,
      'partial': outcome.partial,
      'warningCount': outcome.warnings.length,
      'infoCount': outcome.infos.length,
      'clockSkew': {'kind': 'unknown'},
    };
    return MetricsAiMetricSelection._(captured, jsonEncode(row));
  }

  @override
  String toString() => 'MetricsAiMetricSelection(<selected data>)';
}

Map<String, Object?> _alertTime(String? value) {
  if (value == null) return {'kind': 'unknown'};
  // The strict alert DTO validates this timestamp. Leap second 60 is preserved
  // as reported instead of silently normalizing it into a different instant.
  if (RegExp(r':60(?:\.|Z|[+-])').hasMatch(value)) {
    return {'kind': 'reportedLeapSecond', 'reported': value, 'utc': null};
  }
  final parsed = DateTime.tryParse(value);
  if (parsed == null) _reject(MetricsAiProjectionIssue.invalidSnapshot);
  return {'kind': 'known', 'utc': parsed.toUtc().toIso8601String()};
}

final class MetricsAiAlertSelection {
  MetricsAiAlertSelection._(this._fence, this._json);
  final MetricsAiFence _fence;
  final String _json;
  factory MetricsAiAlertSelection.fromAccepted(
    MonitoringAlertSnapshot snapshot,
    MetricsAiFence captured, {
    required int alertIndex,
    List<String> labelKeys = const [],
    DateTime? acceptedUtc,
  }) {
    if (snapshot.kind != MonitoringAlertKind.alerts || snapshot.status != 'success' || snapshot.error != null) {
      _reject(MetricsAiProjectionIssue.invalidSelection);
    }
    _indices([alertIndex], snapshot.alerts.length, 1);
    final alert = snapshot.alerts[alertIndex], labels = _labels(snapshot.alerts[alertIndex].labels, labelKeys);
    final value = alert.value;
    if (value.kind == SampleKind.finite && (value.value == null || !value.value!.isFinite) ||
        value.kind != SampleKind.finite && value.value != null) {
      _reject(MetricsAiProjectionIssue.invalidSnapshot);
    }
    final row = <String, Object?>{
      'labels': labels.values, 'selectedLabelsOmitted': labels.omitted,
      'state': alert.state.state.name, // Unknown raw strings do not gain meaning.
      'value': {'kind': value.kind.name, if (value.kind == SampleKind.finite) 'value': value.value},
      'activeAt': _alertTime(alert.activeAt),
      'keepFiringSince': _alertTime(alert.keepFiringSince),
      'acceptedCompletedUtc': acceptedUtc?.toUtc().toIso8601String(),
      'partial': snapshot.partial,
      'warningCount': snapshot.warnings.length,
      'infoCount': snapshot.infos.length,
      'unsupportedFieldCount': snapshot.unsupportedFields.length + alert.unsupportedFields.length,
      'annotationsIncluded': false, 'clockSkew': {'kind': 'unknown'},
    };
    return MetricsAiAlertSelection._(captured, jsonEncode(row));
  }
  @override
  String toString() => 'MetricsAiAlertSelection(<selected data>)';
}

/// Local bounded data projection, not a sanitizer or provider-send permission.
/// The final Native sanitizer must issue an exact frozen preview before send.
final class MetricsAiProjection {
  MetricsAiProjection._(this._fence, this.dataJson);
  final MetricsAiFence _fence;
  final String dataJson;
  bool matchesFence(MetricsAiFence captured) => _fence.matches(captured);
  static const int maxBytes = 32768;
  factory MetricsAiProjection.selected(
    MetricsAiFence fence, {
    required List<MetricsAiMetricSelection> metrics,
    required List<MetricsAiAlertSelection> alerts,
    required DateTime createdUtc,
  }) {
    if (metrics.isEmpty && alerts.isEmpty) {
      _reject(MetricsAiProjectionIssue.noSelection);
    }
    if (metrics.length > 8 || alerts.length > 8) {
      _reject(MetricsAiProjectionIssue.budget);
    }
    if (metrics.any((m) => !m._fence.matches(fence)) || alerts.any((a) => !a._fence.matches(fence))) {
      _reject(MetricsAiProjectionIssue.contextChanged);
    }
    final payload = jsonEncode({
      'schemaVersion': 1,
      'dataClassification': 'untrustedSelectedMonitoringData',
      'projectionCreatedUtc': createdUtc.toUtc().toIso8601String(),
      'metrics': [
        for (var i = 0; i < metrics.length; i++)
          {'selectionRef': 'metric-${i + 1}', ...jsonDecode(metrics[i]._json) as Map<String, dynamic>},
      ],
      'alerts': [
        for (var i = 0; i < alerts.length; i++)
          {'selectionRef': 'alert-${i + 1}', ...jsonDecode(alerts[i]._json) as Map<String, dynamic>},
      ],
    });
    if (payload.length > maxBytes || utf8.encode(payload).length > maxBytes) {
      _reject(MetricsAiProjectionIssue.budget);
    }
    return MetricsAiProjection._(fence, payload);
  }
  @override
  String toString() => 'MetricsAiProjection(<bounded selected data>)';
}
