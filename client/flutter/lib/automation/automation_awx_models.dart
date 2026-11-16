import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart'
    show MonitoringSource, SourceAuth, SourceRoute;

/// Fixed-client projections only. No endpoint/credential value or launch grant.
final class AwxModelException implements Exception {
  const AwxModelException();
}

Never _invalid() => throw const AwxModelException();
const awxMaxId = 9007199254740991;
int _integer(Object? value, {int min = 1, int max = awxMaxId}) {
  if (value is! int || value < min || value > max) _invalid();
  return value;
}

Map<String, Object?> _object(Object? value, Set<String> keys) {
  if (value is! Map<String, Object?> ||
      value.keys.any((k) => !keys.contains(k)) ||
      keys.any((k) => !value.containsKey(k))) {
    _invalid();
  }
  return value;
}

String _text(Object? value, int maxBytes, {bool empty = false}) {
  if (value is! String ||
      (!empty && value.isEmpty) ||
      utf8.encode(value).length > maxBytes ||
      value.runes.any((c) => c < 32 || (c >= 127 && c <= 159))) {
    _invalid();
  }
  return value;
}

bool _boolean(Object? value) => value is bool ? value : _invalid();
DateTime? _timestamp(Object? value, {bool required = false}) {
  if (value == null && !required) return null;
  final s = _text(value, 64);
  final m = RegExp(
    r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:Z|[+-](\d{2}):(\d{2}))$',
  ).firstMatch(s);
  if (m == null) _invalid();
  final year = int.parse(m[1]!);
  final month = int.parse(m[2]!);
  final day = int.parse(m[3]!);
  final hour = int.parse(m[4]!);
  final minute = int.parse(m[5]!);
  final second = int.parse(m[6]!);
  if (month < 1 ||
      month > 12 ||
      day < 1 ||
      day > DateTime.utc(year, month + 1, 0).day ||
      hour > 23 ||
      minute > 59 ||
      second > 60 ||
      (m[7] != null && int.parse(m[7]!) > 23) ||
      (m[8] != null && int.parse(m[8]!) > 59)) {
    _invalid();
  }
  // chrono permits an RFC3339 leap second. Normalize only display time; retain
  // original wire timestamp alongside it in the model.
  final parsed = DateTime.tryParse(s);
  if (parsed == null) _invalid();
  return parsed.toUtc();
}

final class AwxTemplate {
  AwxTemplate._(this.id, this.name, this.modified, this.modifiedUtc);
  factory AwxTemplate.fromJson(Object? raw) {
    final j = _object(raw, {'id', 'name', 'modified'});
    return AwxTemplate._(
      _integer(j['id']),
      _text(j['name'], 512),
      _text(j['modified'], 64),
      _timestamp(j['modified'], required: true)!,
    );
  }
  final int id;
  final String name, modified;
  final DateTime modifiedUtc;
  Map<String, Object?> toJson() => {
    'id': id,
    'name': name,
    'modified': modified,
  };
}

final class AwxTemplatePage {
  AwxTemplatePage._(this.templates, this.hasNext, this.page);
  factory AwxTemplatePage.fromJson(Object? raw, {required int expectedPage}) {
    final j = _object(raw, {'templates', 'has_next', 'page'});
    final rows = j['templates'];
    if (rows is! List || rows.length > 50 || j['page'] != expectedPage) {
      _invalid();
    }
    final templates = rows.map(AwxTemplate.fromJson).toList(growable: false);
    if (templates.map((t) => t.id).toSet().length != templates.length) {
      _invalid();
    }
    return AwxTemplatePage._(
      List.unmodifiable(templates),
      _boolean(j['has_next']),
      _integer(j['page'], max: 1000),
    );
  }
  final List<AwxTemplate> templates;
  final bool hasNext;
  final int page;
}

enum AwxJobStatus {
  newJob('new'),
  pending('pending'),
  waiting('waiting'),
  running('running'),
  successful('successful'),
  failed('failed'),
  error('error'),
  canceled('canceled');

  const AwxJobStatus(this.wire);
  final String wire;
  bool get terminal =>
      const {successful, failed, error, canceled}.contains(this);
  static AwxJobStatus parse(Object? raw) =>
      values.firstWhere((s) => s.wire == raw, orElse: () => _invalid());
}

final class AwxJob {
  AwxJob._(
    this.id,
    this.name,
    this.status,
    this.started,
    this.finished,
    this.startedUtc,
    this.finishedUtc,
  );
  factory AwxJob.fromJson(Object? raw, {int? expectedId}) {
    final j = _object(raw, {'id', 'name', 'status', 'started', 'finished'});
    final id = _integer(j['id']);
    if (expectedId != null && id != expectedId) _invalid();
    final startedUtc = _timestamp(j['started']),
        finishedUtc = _timestamp(j['finished']);
    return AwxJob._(
      id,
      _text(j['name'], 512),
      AwxJobStatus.parse(j['status']),
      j['started'] as String?,
      j['finished'] as String?,
      startedUtc,
      finishedUtc,
    );
  }
  final int id;
  final String name;
  final AwxJobStatus status;
  final String? started, finished;
  final DateTime? startedUtc, finishedUtc;
}

/// Plain data only. No ANSI parser or arbitrary link detection may consume it.
String awxPlainEventText(String text) => String.fromCharCodes(
  text.runes.where(
    (c) =>
        (c >= 32 || c == 10 || c == 9) &&
        !(c >= 127 && c <= 159) &&
        !(c >= 0x202a && c <= 0x202e) &&
        !(c >= 0x2066 && c <= 0x2069),
  ),
);

final class AwxEvent {
  AwxEvent._(
    this.counter,
    this.event,
    this.created,
    this.createdUtc,
    this.stdout,
    this.truncated,
  );
  factory AwxEvent.fromJson(Object? raw) {
    final j = _object(raw, {
      'counter',
      'event',
      'created',
      'stdout',
      'truncated',
    });
    final stdout = j['stdout'];
    if (stdout is! String || stdout.runes.length > 2048) _invalid();
    final createdUtc = _timestamp(j['created']);
    return AwxEvent._(
      _integer(j['counter']),
      _text(j['event'], 128),
      j['created'] as String?,
      createdUtc,
      awxPlainEventText(stdout),
      _boolean(j['truncated']),
    );
  }
  final int counter;
  final String event, stdout;
  final String? created;
  final DateTime? createdUtc;
  final bool truncated;
}

final class AwxEventPage {
  AwxEventPage._(this.events, this.hasNext, this.after);
  factory AwxEventPage.fromJson(Object? raw, {required int requestedAfter}) {
    _integer(requestedAfter, min: 0);
    final j = _object(raw, {'events', 'has_next', 'after'});
    final rows = j['events'];
    if (rows is! List || rows.length > 50) _invalid();
    var cursor = requestedAfter;
    final events = <AwxEvent>[];
    for (final row in rows) {
      final event = AwxEvent.fromJson(row);
      if (event.counter <= cursor) _invalid();
      cursor = event.counter;
      events.add(event);
    }
    if (_integer(j['after'], min: 0) != cursor) _invalid();
    return AwxEventPage._(
      List.unmodifiable(events),
      _boolean(j['has_next']),
      cursor,
    );
  }
  final List<AwxEvent> events;
  final bool hasNext;
  final int after;
}

Object? _freezeJson(Object? value, int depth) {
  if (depth > 32) _invalid();
  if (value == null || value is bool || value is String || value is int) {
    return value;
  }
  if (value is double && value.isFinite) return value;
  if (value is List) {
    return List<Object?>.unmodifiable(
      value.map((v) => _freezeJson(v, depth + 1)),
    );
  }
  if (value is Map<String, Object?>) {
    final keys = value.keys.toList()..sort();
    return Map<String, Object?>.unmodifiable({
      for (final k in keys) k: _freezeJson(value[k], depth + 1),
    });
  }
  _invalid();
}

final class AwxLaunchParameters {
  factory AwxLaunchParameters({
    String? jobType,
    String? limit,
    String? jobTags,
    String? skipTags,
    Map<String, Object?>? extraVars,
  }) {
    if (jobType != null && jobType != 'run' && jobType != 'check') _invalid();
    for (final text in [limit, jobTags, skipTags]) {
      if (text != null) _text(text, 512, empty: true);
    }
    final json = <String, Object?>{
      'job_type': ?jobType,
      'limit': ?limit,
      'job_tags': ?jobTags,
      'skip_tags': ?skipTags,
      if (extraVars != null) 'extra_vars': _freezeJson(extraVars, 0),
    };
    final canonical = jsonEncode(json);
    if (utf8.encode(canonical).length > 32768) _invalid();
    return AwxLaunchParameters._(Map.unmodifiable(json), canonical);
  }
  factory AwxLaunchParameters.fromJson(Object? raw) {
    if (raw is! Map<String, Object?> ||
        raw.keys.any(
          (k) => !const {
            'job_type',
            'limit',
            'job_tags',
            'skip_tags',
            'extra_vars',
          }.contains(k),
        )) {
      _invalid();
    }
    for (final k in const ['job_type', 'limit', 'job_tags', 'skip_tags']) {
      if (raw.containsKey(k) && raw[k] is! String) _invalid();
    }
    if (raw.containsKey('extra_vars') &&
        raw['extra_vars'] is! Map<String, Object?>) {
      _invalid();
    }
    return AwxLaunchParameters(
      jobType: raw['job_type'] as String?,
      limit: raw['limit'] as String?,
      jobTags: raw['job_tags'] as String?,
      skipTags: raw['skip_tags'] as String?,
      extraVars: raw['extra_vars'] as Map<String, Object?>?,
    );
  }
  const AwxLaunchParameters._(this._json, this.canonicalJson);
  final Map<String, Object?> _json;
  final String canonicalJson;
  Map<String, Object?> toJson() => _json;
  bool sameAs(AwxLaunchParameters other) =>
      canonicalJson == other.canonicalJson;
}

String _uuid(Object? raw) {
  final s = _text(raw, 36);
  if (!RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
          .hasMatch(s) ||
      s == '00000000-0000-0000-0000-000000000000') {
    _invalid();
  }
  return s;
}

/// Actual Native capture. The local hub epoch never supplies this nonce.
final class AwxNativeScope {
  AwxNativeScope._(this.profileId, this.notificationScopeId);
  factory AwxNativeScope.fromJson(Object? raw) {
    final j = _object(raw, {'version', 'profileId', 'notificationScopeId'});
    if (j['version'] is! int || j['version'] != 1) _invalid();
    return AwxNativeScope._(
      _uuid(j['profileId']),
      _uuid(j['notificationScopeId']),
    );
  }
  final String profileId, notificationScopeId;
  Map<String, Object?> toJson() => {
    'version': 1,
    'profileId': profileId,
    'notificationScopeId': notificationScopeId,
  };
}

void validateAwxConfiguration(
  MonitoringSource configuration, {
  bool create = false,
}) {
  configuration.validate(allowEmptyId: create);
  if (configuration.route != SourceRoute.direct ||
      Uri.tryParse(configuration.endpoint)?.scheme != 'https' ||
      !const {
        SourceAuth.bearerRef,
        SourceAuth.basicRef,
      }.contains(configuration.auth)) {
    _invalid();
  }
  if (!create) _uuid(configuration.id);
  if (create && configuration.id.isNotEmpty) _invalid();
  _uuid(configuration.credentialId);
}

final class AwxConnection {
  AwxConnection._(this.id, this.revision, this.configuration, this.readOnly);
  factory AwxConnection.fromJson(Object? raw) {
    if (raw is! Map<String, Object?> ||
        raw.keys.any(
          (k) => !const {
            'id',
            'revision',
            'configuration',
            'readOnly',
          }.contains(k),
        )) {
      _invalid();
    }
    final id = _uuid(raw['id']);
    final readOnly = _boolean(raw['readOnly']);
    final revision = raw['revision'] == null ? null : _integer(raw['revision']);
    MonitoringSource? config;
    if (raw['configuration'] != null) {
      final c = _object(raw['configuration'], {
        'id',
        'name',
        'endpoint',
        'route',
        'tls',
        'auth',
      });
      _object(c['route'], {'kind'});
      _object(c['tls'], {'customCaPem'});
      _object(c['auth'], {'kind', 'credentialId'});
      config = MonitoringSource.fromJson(Map<String, dynamic>.from(c));
      validateAwxConfiguration(config);
      if (config.id != id) _invalid();
    }
    if (!readOnly && (config == null || revision == null)) _invalid();
    return AwxConnection._(id, revision, config, readOnly);
  }
  final String id;
  final int? revision;
  final MonitoringSource? configuration;
  final bool readOnly;
  bool sameRevision(AwxConnection other) =>
      id == other.id &&
      revision == other.revision &&
      readOnly == other.readOnly &&
      jsonEncode(configuration?.toJson()) ==
          jsonEncode(other.configuration?.toJson());
}

final class AwxCatalog {
  AwxCatalog._(this.revision, this.readOnly, this.connections);
  factory AwxCatalog.fromJson(Object? raw) {
    final j = _object(raw, {
      'version',
      'catalogRevision',
      'readOnly',
      'connections',
    });
    if (j['version'] is! int ||
        j['version'] != 1 ||
        j['connections'] is! List ||
        (j['connections'] as List).length > 100) {
      _invalid();
    }
    final rows = (j['connections'] as List)
        .map(AwxConnection.fromJson)
        .toList(growable: false);
    if (rows.map((r) => r.id).toSet().length != rows.length) _invalid();
    final readOnly = _boolean(j['readOnly']);
    final revision = j['catalogRevision'] == null
        ? null
        : _integer(j['catalogRevision'], min: 0);
    if (!readOnly && revision == null) _invalid();
    return AwxCatalog._(revision, readOnly, List.unmodifiable(rows));
  }
  final int? revision;
  final bool readOnly;
  final List<AwxConnection> connections;
}

final class AwxPreparedReview {
  AwxPreparedReview._(
    this._ticket,
    this.template,
    this.parameters,
    this.expiresUtc,
    this.connectionId,
    this.connectionRevision,
  );
  factory AwxPreparedReview.fromJson(
    Object? raw, {
    required AwxConnection connection,
    required int templateId,
    required AwxLaunchParameters parameters,
    required DateTime now,
  }) {
    final j = _object(raw, {
      'reviewTicket',
      'template',
      'parameters',
      'expiresUtc',
      'connectionId',
      'connectionRevision',
    });
    final parsed = AwxLaunchParameters.fromJson(j['parameters']);
    final template = AwxTemplate.fromJson(j['template']);
    final expiry = _timestamp(j['expiresUtc'], required: true)!;
    if (j['connectionId'] != connection.id ||
        j['connectionRevision'] != connection.revision ||
        template.id != templateId ||
        !parsed.sameAs(parameters) ||
        !expiry.isAfter(now.toUtc()) ||
        expiry.difference(now.toUtc()) > const Duration(seconds: 120)) {
      _invalid();
    }
    return AwxPreparedReview._(
      _text(j['reviewTicket'], 4096),
      template,
      parsed,
      expiry,
      connection.id,
      _integer(j['connectionRevision']),
    );
  }
  // Opaque Native ticket, not a serializable approval or replacement payload.
  final String _ticket;
  String get nativeTicket => _ticket;
  final AwxTemplate template;
  final AwxLaunchParameters parameters;
  final DateTime expiresUtc;
  final String connectionId;
  final int connectionRevision;
}

sealed class AwxLaunchOutcome {
  const AwxLaunchOutcome();
  factory AwxLaunchOutcome.fromJson(
    Object? raw, {
    required AwxConnection connection,
  }) {
    if (raw is! Map<String, Object?>) _invalid();
    if (raw['status'] == 'unknown') {
      final j = _object(raw, {'status', 'reviewConsumed'});
      if (j['reviewConsumed'] != true) _invalid();
      return const AwxLaunchUnknown();
    }
    final j = _object(raw, {
      'status',
      'job',
      'parametersIgnored',
      'originalJobUrl',
    });
    if (j['status'] != 'started') _invalid();
    final job = AwxJob.fromJson(j['job']);
    final link = _text(j['originalJobUrl'], 4096);
    final url = Uri.tryParse(link);
    final base = Uri.tryParse(connection.configuration?.endpoint ?? '');
    if (url == null ||
        base == null ||
        url.scheme != 'https' ||
        url.userInfo.isNotEmpty ||
        url.hasQuery ||
        url.hasFragment ||
        url.origin != base.origin ||
        url.path !=
            '${base.path.replaceFirst(RegExp(r'/$'), '')}/api/v2/jobs/${job.id}/') {
      _invalid();
    }
    return AwxLaunchStarted(job, _boolean(j['parametersIgnored']), link);
  }
}

final class AwxLaunchStarted extends AwxLaunchOutcome {
  const AwxLaunchStarted(this.job, this.parametersIgnored, this.originalJobUrl);
  final AwxJob job;
  final bool parametersIgnored;
  final String originalJobUrl;
}

final class AwxLaunchUnknown extends AwxLaunchOutcome {
  const AwxLaunchUnknown();
}

/// Decode only the agreed fixed facade envelope; unknown kind/schema is denied.
Object? awxEnvelopeData(Object? raw, String expectedKind) {
  final j = _object(raw, {'version', 'kind', 'data'});
  if (j['version'] is! int || j['version'] != 1 || j['kind'] != expectedKind) {
    _invalid();
  }
  return j['data'];
}

/// Manual variables only. Reject ambiguity before jsonDecode can discard a
/// duplicate key; no survey/default fetch and no YAML/plugin evaluation.
Map<String, Object?> parseAwxExtraVars(String raw) {
  if (utf8.encode(raw).length > 32768) _invalid();
  final keys = <Set<String>?>[], expectingKey = <bool>[];
  final tokens = RegExp(r'"(?:[^"\\]|\\.)*"|[{}\[\],:]');
  try {
    for (final match in tokens.allMatches(raw)) {
      final token = match[0]!;
      if (token == '{' || token == '[') {
        if (keys.length >= 32) _invalid();
        keys.add(token == '{' ? <String>{} : null);
        expectingKey.add(token == '{');
      } else if (token == '}' || token == ']') {
        if (keys.isEmpty) _invalid();
        keys.removeLast();
        expectingKey.removeLast();
      } else if (keys.isNotEmpty && keys.last != null) {
        if (token == ',') expectingKey[expectingKey.length - 1] = true;
        if (token == ':') expectingKey[expectingKey.length - 1] = false;
        if (token.startsWith('"') &&
            expectingKey.last &&
            !keys.last!.add(jsonDecode(token) as String)) {
          _invalid();
        }
      }
    }
    final value = jsonDecode(raw);
    if (value is! Map<String, Object?>) _invalid();
    return _freezeJson(value, 0) as Map<String, Object?>;
  } on FormatException {
    _invalid();
  }
}
