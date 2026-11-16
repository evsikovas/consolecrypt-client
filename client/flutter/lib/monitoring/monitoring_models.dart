import 'dart:convert';

typedef Json = Map<String, dynamic>;

Json object(Object? value) => Map<String, dynamic>.from(value as Map);
List<String> strings(Object? value) => (value as List? ?? []).cast<String>();

enum SourceRoute { direct, sshTunnel, agentTunnel, sshHost, agentDestination }

enum SourceAuth { none, bearerRef, basicRef }

enum MonitoringBindingState { current, staleSource, missingSource, missingHost }

/// Native identities and exact user-entered labels; never an inferred host match.
class MonitoringBinding {
  MonitoringBinding({
    required this.id,
    required this.name,
    required this.hostId,
    required this.sourceId,
    required this.revision,
    required this.sourceRevision,
    required Map<String, String> selector,
    required this.state,
  }) : selector = Map.unmodifiable(selector);
  final String id, name, hostId, sourceId;
  final int revision, sourceRevision;
  final Map<String, String> selector;
  final MonitoringBindingState state;

  factory MonitoringBinding.fromRecord(Json row) {
    final binding = object(row['binding']);
    if (row.keys.toSet().difference({'id', 'name', 'revision', 'sourceRevision', 'binding', 'state'}).isNotEmpty ||
        binding.keys.toSet().difference({'version', 'hostId', 'sourceId', 'revision', 'selector'}).isNotEmpty ||
        binding['version'] != 1 ||
        row['revision'] != binding['revision']) {
      throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion);
    }
    final value = MonitoringBinding(
      id: row['id'] as String,
      name: row['name'] as String,
      hostId: binding['hostId'] as String,
      sourceId: binding['sourceId'] as String,
      revision: row['revision'] as int,
      sourceRevision: row['sourceRevision'] as int,
      selector: Map<String, String>.from(binding['selector'] as Map),
      state: MonitoringBindingState.values.byName(row['state'] as String),
    );
    value.validate();
    return value;
  }

  void validate({bool create = false}) {
    final canonicalId = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
    const nil = '00000000-0000-0000-0000-000000000000';
    final labelName = RegExp(r'^[a-zA-Z_][a-zA-Z_0-9]*$');
    var bytes = 0;
    if ((!create && (!canonicalId.hasMatch(id) || id == nil || revision < 1)) ||
        !canonicalId.hasMatch(hostId) ||
        hostId == nil ||
        !canonicalId.hasMatch(sourceId) ||
        sourceId == nil ||
        sourceRevision < 1 ||
        name != name.trim() ||
        name.isEmpty ||
        name.runes.length > 120 ||
        name.runes.any((r) => r < 32 || r >= 127 && r <= 159) ||
        selector.isEmpty ||
        selector.length > 32) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidBinding);
    }
    for (final entry in selector.entries) {
      final keyBytes = utf8.encode(entry.key).length, valueBytes = utf8.encode(entry.value).length;
      bytes += keyBytes + valueBytes;
      if (!labelName.hasMatch(entry.key) ||
          entry.key == '__name__' ||
          keyBytes > 128 ||
          valueBytes == 0 ||
          valueBytes > 4096 ||
          bytes > 16384) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidBinding);
      }
    }
  }
}

class MonitoringHostRef {
  const MonitoringHostRef({required this.id, required this.name});
  final String id, name;
}

/// Only credential and route references cross the UI boundary.
class MonitoringSource {
  const MonitoringSource({
    required this.id,
    required this.name,
    required this.endpoint,
    this.route = SourceRoute.direct,
    this.routeId,
    this.associationId,
    this.destinationIp,
    this.destinationPort,
    this.hostId,
    this.auth = SourceAuth.none,
    this.credentialId,
    this.customCaPem,
    this.revision = 0,
    this.readOnly = false,
  });
  final String id, name, endpoint;
  final int revision;
  final bool readOnly;
  final SourceRoute route;
  final SourceAuth auth;
  final String? routeId, hostId, associationId, destinationIp, credentialId, customCaPem;
  final int? destinationPort;
  void validate({bool allowEmptyId = false}) {
    final uri = Uri.tryParse(endpoint);
    final canonicalHost = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
    if (auth == SourceAuth.bearerRef && uri?.scheme != 'https') {
      throw const MonitoringFailure.local(MonitoringErrorCode.authRequiresTls);
    }
    if ((!allowEmptyId && id.isEmpty) ||
        id.length > 256 ||
        name.isEmpty ||
        name.length > 256 ||
        endpoint.length > 2048 ||
        uri == null ||
        !['http', 'https'].contains(uri.scheme) ||
        uri.host.isEmpty ||
        uri.userInfo.isNotEmpty ||
        uri.hasQuery ||
        uri.hasFragment ||
        (route == SourceRoute.sshHost && (hostId == null || !canonicalHost.hasMatch(hostId!) ||
            hostId == '00000000-0000-0000-0000-000000000000')) ||
        ((route == SourceRoute.sshTunnel || route == SourceRoute.agentTunnel) && (routeId?.trim().isEmpty ?? true)) ||
        (route == SourceRoute.agentDestination &&
            (associationId == null || associationId!.length != 36 || !canonicalHost.hasMatch(associationId!) ||
             associationId == '00000000-0000-0000-0000-000000000000' ||
             hostId == null || hostId!.length != 36 || !canonicalHost.hasMatch(hostId!) || hostId == '00000000-0000-0000-0000-000000000000' ||
             !const {'127.0.0.1', '::1'}.contains(destinationIp) ||
             destinationPort == null || destinationPort! < 1 || destinationPort! > 65535 ||
             routeId != null)) ||
        (auth != SourceAuth.none && (credentialId?.trim().isEmpty ?? true))) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidConfig);
    }
  }

  Json toJson() => {
    'id': id,
    'name': name,
    'endpoint': endpoint,
    'route': {'kind': route.name,
      if (route == SourceRoute.sshHost || route == SourceRoute.agentDestination) 'hostId': hostId,
      if (route == SourceRoute.agentDestination) ...{
        'associationId': associationId, 'destinationIp': destinationIp, 'destinationPort': destinationPort},
      if (route == SourceRoute.sshTunnel || route == SourceRoute.agentTunnel) 'routeId': routeId},
    'tls': {'customCaPem': customCaPem},
    'auth': {'kind': auth.name, if (auth != SourceAuth.none) 'credentialId': credentialId},
  };
  factory MonitoringSource.fromJson(Json json) {
    final route = object(json['route']), auth = object(json['auth']);
    if (route['kind'] == 'sshHost' && (route.length != 2 || route['hostId'] is! String ||
        !route.keys.every(const {'kind', 'hostId'}.contains))) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidConfig);
    }
    if (route['kind'] == 'agentDestination' && (route.length != 5 ||
        !route.keys.every(const {'kind','associationId','hostId','destinationIp','destinationPort'}.contains) ||
        route['associationId'] is! String || route['hostId'] is! String ||
        route['destinationIp'] is! String || route['destinationPort'] is! int)) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidConfig);
    }
    final source = MonitoringSource(
      id: json['id'] as String,
      name: json['name'] as String,
      endpoint: json['endpoint'] as String,
      route: SourceRoute.values.byName(route['kind'] as String),
      routeId: route['routeId'] as String?,
      associationId: route['associationId'] as String?,
      destinationIp: route['destinationIp'] as String?,
      destinationPort: route['destinationPort'] as int?,
      hostId: route['hostId'] as String?,
      auth: SourceAuth.values.byName(auth['kind'] as String),
      credentialId: auth['credentialId'] as String?,
      customCaPem: object(json['tls'])['customCaPem'] as String?,
    );
    source.validate();
    return source;
  }
  factory MonitoringSource.fromRecord(Json record) {
    final config = MonitoringSource.fromJson(object(record['configuration']));
    if (record['id'] != config.id) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
    }
    return config.withRecord(revision: record['revision'] as int, readOnly: record['readOnly'] as bool);
  }
  MonitoringSource withRecord({String? id, required int revision, bool readOnly = false}) => MonitoringSource(
    id: id ?? this.id,
    name: name,
    endpoint: endpoint,
    route: route,
    routeId: routeId,
    associationId: associationId,
    destinationIp: destinationIp,
    destinationPort: destinationPort,
    hostId: hostId,
    auth: auth,
    credentialId: credentialId,
    customCaPem: customCaPem,
    revision: revision,
    readOnly: readOnly,
  );
  /// One approved Agent TCP stream is an explicit finite read, not a poll lease.
  bool get allowsBackgroundObservation => route != SourceRoute.agentDestination && route != SourceRoute.agentTunnel;
  String get fingerprint => jsonEncode({'configuration': toJson(), 'revision': revision});
}

enum MonitoringErrorCode {
  invalidBinding('invalid_binding'),
  bindingChanged('binding_changed'),
  sourceInUse('source_in_use'),
  authRequiresTls('auth_requires_tls'),
  invalidConfig('invalid_config'),
  invalidQuery('invalid_query'),
  authUnavailable('auth_unavailable'),
  transport('transport'),
  timeout('timeout'),
  budgetExceeded('budget_exceeded'),
  responseTooLarge('response_too_large'),
  busy('busy'),
  sourceChanged('source_changed'),
  unsupportedRoute('unsupported_route'),
  cancelled('cancelled'),
  invalidResponse('invalid_response'),
  unsupportedResult('unsupported_result'),
  readOnly('read_only'),
  unsupportedCatalogVersion('unsupported_catalog_version'),
  unknown('unknown');

  const MonitoringErrorCode(this.wire);
  final String wire;

  static MonitoringErrorCode parse(String code) => values.where((value) => value.wire == code).firstOrNull ?? unknown;
}

class MonitoringFailure implements Exception {
  /// Wire compatibility constructor. The message is diagnostic only unless it
  /// came from a Prometheus API `type` error, rather than a Native `code` error.
  const MonitoringFailure(String code, this.message, {this.httpStatus, this.isServerMessage = false})
    : _code = code,
      _kind = null;
  const MonitoringFailure.local(MonitoringErrorCode code, {this.httpStatus})
    : _kind = code,
      _code = null,
      message = '',
      isServerMessage = false;
  final String? _code;
  final MonitoringErrorCode? _kind;
  String get code => _code ?? _kind!.wire;
  MonitoringErrorCode get kind => _kind ?? MonitoringErrorCode.parse(_code!);
  final String message;
  final bool isServerMessage;
  final int? httpStatus;
  factory MonitoringFailure.fromJson(Json json, {int? envelopeHttpStatus}) {
    final code = json['code'] ?? json['type'];
    final message = json['message'];
    // Native QueryOutcome/LabelValues carry the HTTP status on the envelope;
    // their nested Prometheus ApiError has only type/message. Keep native-code
    // errors separate: an HTTP status must not turn unknown/schema failures
    // into retryable server errors.
    final httpStatus = json['httpStatus'] ?? (json['code'] == null ? envelopeHttpStatus : null);
    if (code is! String || message is! String || (httpStatus != null && httpStatus is! int)) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
    }
    return MonitoringFailure(
      code,
      message,
      httpStatus: httpStatus as int?,
      isServerMessage: json['code'] == null && json['type'] is String,
    );
  }
  @override
  String toString() => 'MonitoringFailure(${kind.name}${httpStatus == null ? '' : ', HTTP $httpStatus'})';
}

class EndpointCapability {
  const EndpointCapability({required this.supported, this.httpStatus, this.error, this.warnings = const []});
  final bool supported;
  final int? httpStatus;
  final MonitoringFailure? error;
  final List<String> warnings;
  factory EndpointCapability.fromJson(Json json) => EndpointCapability(
    supported: json['supported'] as bool,
    httpStatus: json['httpStatus'] as int?,
    error: json['error'] == null ? null : MonitoringFailure.fromJson(object(json['error'])),
    warnings: strings(json['warnings']),
  );
}

class SourceCapabilities {
  const SourceCapabilities(this.query, this.queryRange);
  final EndpointCapability query, queryRange;
  bool get compatible => query.supported && queryRange.supported;
  factory SourceCapabilities.fromJson(Json json) => SourceCapabilities(
    EndpointCapability.fromJson(object(json['query'])),
    EndpointCapability.fromJson(object(json['queryRange'])),
  );
}

enum SampleKind { finite, nan, positiveInfinity, negativeInfinity }

class MonitoringSample {
  const MonitoringSample(this.timestamp, this.kind, [this.value]);
  final double timestamp;
  final SampleKind kind;
  final double? value;
  factory MonitoringSample.fromJson(Json json) {
    final v = object(json['value']);
    final kind = SampleKind.values.byName(v['kind'] as String);
    final value = kind == SampleKind.finite ? (v['value'] as num).toDouble() : null;
    final timestamp = (json['timestamp'] as num).toDouble();
    if (!timestamp.isFinite || (value != null && !value.isFinite)) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
    }
    return MonitoringSample(timestamp, kind, value);
  }
  String get display => switch (kind) {
    SampleKind.finite => value.toString(),
    SampleKind.nan => 'NaN',
    SampleKind.positiveInfinity => '+Inf',
    SampleKind.negativeInfinity => '-Inf',
  };
}

class MonitoringSeries {
  const MonitoringSeries(this.labels, this.samples);
  final Map<String, String> labels;
  final List<MonitoringSample> samples;
  String get label => labels.entries.map((e) => '${e.key}="${e.value}"').join(', ');
  factory MonitoringSeries.fromJson(Json json) => MonitoringSeries(
    Map<String, String>.from(json['labels'] as Map),
    (json['samples'] as List).map((e) => MonitoringSample.fromJson(object(e))).toList(),
  );
}

class QueryData {
  const QueryData(this.kind, {this.series = const [], this.sample, this.text, this.timestamp});
  final String kind;
  final List<MonitoringSeries> series;
  final MonitoringSample? sample;
  final String? text;
  final double? timestamp;
  factory QueryData.fromJson(Json json) => switch (json['kind']) {
    'matrix' || 'vector' => QueryData(
      json['kind'] as String,
      series: (json['series'] as List).map((e) => MonitoringSeries.fromJson(object(e))).toList(),
    ),
    'scalar' => QueryData('scalar', sample: MonitoringSample.fromJson(object(json['sample']))),
    'string' => QueryData('string', text: json['value'] as String, timestamp: (json['timestamp'] as num).toDouble()),
    _ => throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedResult),
  };
  bool get empty => (kind == 'matrix' || kind == 'vector') && series.isEmpty;
}

class QueryOutcome {
  const QueryOutcome({
    required this.status,
    required this.httpStatus,
    this.data,
    this.warnings = const [],
    this.infos = const [],
    this.partial = false,
    this.error,
    this.effectiveStepSeconds,
  });
  final String status;
  final int httpStatus;
  final QueryData? data;
  final List<String> warnings, infos;
  final bool partial;
  final MonitoringFailure? error;
  final double? effectiveStepSeconds;
  factory QueryOutcome.fromJson(Json json) => QueryOutcome(
    status: json['status'] as String,
    httpStatus: json['httpStatus'] as int,
    data: json['data'] == null ? null : QueryData.fromJson(object(json['data'])),
    warnings: strings(json['warnings']),
    infos: strings(json['infos']),
    partial: json['partial'] as bool,
    error: json['error'] == null
        ? null
        : MonitoringFailure.fromJson(object(json['error']), envelopeHttpStatus: json['httpStatus'] as int),
    effectiveStepSeconds: (json['effectiveStepSeconds'] as num?)?.toDouble(),
  );
}

class LabelValues {
  const LabelValues({
    required this.values,
    this.warnings = const [],
    this.infos = const [],
    this.partial = false,
    this.cached = false,
    this.error,
  });
  final List<String> values, warnings, infos;
  final bool partial, cached;
  final MonitoringFailure? error;
  factory LabelValues.fromJson(Json json) => LabelValues(
    values: strings(json['values']),
    warnings: strings(json['warnings']),
    infos: strings(json['infos']),
    partial: json['partial'] as bool,
    cached: json['cached'] as bool,
    error: json['error'] == null
        ? null
        : MonitoringFailure.fromJson(object(json['error']), envelopeHttpStatus: json['httpStatus'] as int?),
  );
  LabelValues asCached() =>
      LabelValues(values: values, warnings: warnings, infos: infos, partial: partial, cached: true, error: error);
}

class MonitoringQuery {
  const MonitoringQuery({required this.expression, this.start, this.end, this.stepSeconds = 15});
  final String expression;
  final double? start, end;
  final double stepSeconds;
  bool get range => start != null;
  Json toOperation() => range
      ? {'kind': 'queryRange', 'expression': expression, 'start': start, 'end': end, 'stepSeconds': stepSeconds}
      : {'kind': 'query', 'expression': expression};
  void validate() {
    if (expression.trim().isEmpty ||
        expression.length > 16384 ||
        (range &&
            (end == null ||
                !start!.isFinite ||
                !end!.isFinite ||
                start! >= end! ||
                !stepSeconds.isFinite ||
                stepSeconds <= 0))) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
    }
  }
}
