import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart';

enum MonitoringExporterKind { nodeExporter, cadvisor }

enum MonitoringExporterUnavailable {
  readFailed,
  incompleteResponse,
  invalidResponse,
  missingSeries,
  ambiguousTarget,
  targetDown,
  staleSeries,
  clockUnknown,
}

/// Existing profile service only; context capture never probes an endpoint.
abstract interface class MonitoringExporterReader {
  Future<MonitoringExporterContext> exporterContext(
    MonitoringSource source, {
    required String profileId,
    required String hostId,
    required String jobId,
    required int jobRevision,
  });
  Future<MonitoringExporterObservation> verifyExporter(
    MonitoringSource source,
    MonitoringExporterContext original,
    MonitoringExporterKind exporter,
    Map<String, String> selector, {
    int maxAgeSeconds = 120,
  });
  void cancelExporter();
}

Never _invalid() => throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
bool _id(Object? v) => v is String && _uuid.hasMatch(v) && v != '00000000-0000-0000-0000-000000000000';
bool _revision(Object? v) => v is int && v > 0 && v <= 9007199254740991;
void _keys(Json row, Set<String> expected) {
  if (row.length != expected.length || !row.keys.every(expected.contains)) _invalid();
}

const _contextKeys = {
  'version',
  'profileId',
  'notificationScopeId',
  'hostId',
  'hostRevision',
  'protocol',
  'sourceId',
  'sourceRevision',
  'jobId',
  'jobRevision',
  'projectId',
  'revisionDigest',
};

/// Original Native metadata, not installation/host identity attestation.
final class MonitoringExporterContext {
  MonitoringExporterContext._(this._data);
  final Json _data;
  String get profileId => _data['profileId'] as String;
  String get hostId => _data['hostId'] as String;
  String get sourceId => _data['sourceId'] as String;
  int get sourceRevision => _data['sourceRevision'] as int;
  String get jobId => _data['jobId'] as String;
  int get jobRevision => _data['jobRevision'] as int;
  String get projectId => _data['projectId'] as String;

  factory MonitoringExporterContext.fromJson(Json row, {String kind = 'exporterContext'}) {
    _keys(row, {..._contextKeys, 'kind'});
    if (row['kind'] != kind || row['version'] != 1 || row['protocol'] != 'ssh') _invalid();
    for (final key in ['profileId', 'notificationScopeId', 'hostId', 'sourceId', 'jobId', 'projectId']) {
      if (!_id(row[key])) _invalid();
    }
    for (final key in ['hostRevision', 'sourceRevision', 'jobRevision']) {
      if (!_revision(row[key])) _invalid();
    }
    if (row['revisionDigest'] is! String || !RegExp(r'^[0-9a-f]{64}$').hasMatch(row['revisionDigest'] as String)) {
      _invalid();
    }
    return MonitoringExporterContext._(Map.unmodifiable({...row}..remove('kind')));
  }
  bool same(MonitoringExporterContext other) => _contextKeys.every((k) => _data[k] == other._data[k]);
  Json get bindingOrigin => {..._data}..remove('protocol');
  Json verification(MonitoringExporterKind kind, Map<String, String> selector, int maxAgeSeconds) {
    validateExporterSelector(selector, kind);
    if (maxAgeSeconds < 5 || maxAgeSeconds > 300) _invalid();
    return {
      'kind': 'exporterVerification',
      'version': 1,
      'profileId': profileId,
      'notificationScopeId': _data['notificationScopeId'],
      'hostId': hostId,
      'hostRevision': _data['hostRevision'],
      'jobId': jobId,
      'jobRevision': jobRevision,
      'exporter': kind.name,
      'selector': Map<String, String>.unmodifiable(selector),
      'maxAgeSeconds': maxAgeSeconds,
    };
  }
}

void validateExporterSelector(Map<String, String> selector, MonitoringExporterKind kind) {
  var bytes = 0;
  if (selector.isEmpty ||
      selector.length > 32 ||
      !selector.containsKey('job') ||
      !selector.containsKey('instance') ||
      kind == MonitoringExporterKind.cadvisor && selector.containsKey('image')) {
    _invalid();
  }
  for (final e in selector.entries) {
    bytes += utf8.encode(e.key).length + utf8.encode(e.value).length;
    if (!RegExp(r'^[a-zA-Z_][a-zA-Z_0-9]*$').hasMatch(e.key) ||
        e.key == '__name__' ||
        e.key.length > 128 ||
        e.value.isEmpty ||
        utf8.encode(e.value).length > 4096 ||
        bytes > 16384 ||
        e.value.runes.any((r) => r < 32 || r >= 127 && r <= 159)) {
      _invalid();
    }
  }
}

final class MonitoringExporterObservation {
  const MonitoringExporterObservation._(
    this.original,
    this.exporter,
    this.maxSampleAgeSeconds,
    this.evaluatedAtSourceSeconds,
    this.unavailable,
  );
  final MonitoringExporterContext original;
  final MonitoringExporterKind exporter;
  final double? maxSampleAgeSeconds, evaluatedAtSourceSeconds;
  final MonitoringExporterUnavailable? unavailable;
  bool get observed => unavailable == null;
  factory MonitoringExporterObservation.fromJson(
    Json row,
    MonitoringExporterContext original,
    MonitoringExporterKind exporter,
    int maxAgeSeconds,
  ) {
    _keys(row, {..._contextKeys, 'kind', 'exporter', 'observation'});
    final context = MonitoringExporterContext.fromJson(
      {...row}
        ..remove('exporter')
        ..remove('observation'),
      kind: 'exporterVerification',
    );
    if (!original.same(context) || row['exporter'] != exporter.name || row['observation'] is! Map) _invalid();
    final observation = object(row['observation']);
    if (observation['state'] == 'unavailable') {
      _keys(observation, {'state', 'reason'});
      final reason = MonitoringExporterUnavailable.values.where((r) => r.name == observation['reason']).firstOrNull;
      if (reason == null) _invalid();
      return MonitoringExporterObservation._(original, exporter, null, null, reason);
    }
    _keys(observation, {'state', 'maxSampleAgeSeconds', 'evaluatedAtSourceSeconds'});
    final age = observation['maxSampleAgeSeconds'], time = observation['evaluatedAtSourceSeconds'];
    if (observation['state'] != 'observed' ||
        age is! num ||
        !age.isFinite ||
        age < 0 ||
        age > maxAgeSeconds ||
        time is! num ||
        !time.isFinite ||
        time < 0 ||
        time > 253402300799) {
      _invalid();
    }
    return MonitoringExporterObservation._(original, exporter, age.toDouble(), time.toDouble(), null);
  }
}
