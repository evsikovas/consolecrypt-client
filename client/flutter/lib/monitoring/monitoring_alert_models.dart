import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart';

enum MonitoringAlertKind { alerts, alertRules }

enum MonitoringAlertState { inactive, pending, firing, unknown }

enum MonitoringRuleHealth { ok, error, unknown }

Never _badAlerts() => throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
Never _alertBudget() => throw const MonitoringFailure.local(MonitoringErrorCode.budgetExceeded);

Json _shape(Object? value, Set<String> allowed, [Set<String> required = const {}]) {
  if (value is! Map<String, dynamic> ||
      value.keys.any((key) => !allowed.contains(key)) ||
      !required.every(value.containsKey)) {
    _badAlerts();
  }
  return value;
}

class _Budget {
  int groups = 0, rules = 0, alerts = 0, labels = 0, text = 0;
  String string(Object? value, [int limit = 4096]) {
    if (value is! String) {
      _badAlerts();
    }
    if (value.length > limit) {
      _alertBudget();
    }
    final bytes = utf8.encode(value).length;
    text += bytes;
    if (bytes > limit || text > 2 * 1024 * 1024) {
      _alertBudget();
    }
    return value;
  }

  String? optional(Object? value, [int limit = 4096]) => value == null ? null : string(value, limit);
  Map<String, String> metadata(Object? value) {
    if (value is! Map<String, dynamic>) {
      _badAlerts();
    }
    if (value.length > 64) {
      _alertBudget();
    }
    final result = <String, String>{};
    for (final entry in value.entries) {
      final key = string(entry.key), item = string(entry.value);
      labels += utf8.encode(key).length + utf8.encode(item).length;
      if (labels > 1024 * 1024) {
        _alertBudget();
      }
      result[key] = item;
    }
    return Map.unmodifiable(result);
  }

  List<String> unsupported(Object? value) {
    final rows = _list(value, 32);
    final names = rows.map((v) => string(v, 128)).toList(growable: false);
    if (names.toSet().length != names.length) {
      _badAlerts();
    }
    return List.unmodifiable(names);
  }

  List<String> messages(Object? value) {
    var bytes = 0;
    final result = _list(value, 100)
        .map((item) {
          final message = string(item);
          bytes += utf8.encode(message).length;
          if (bytes > 256 * 1024) {
            _alertBudget();
          }
          return message;
        })
        .toList(growable: false);
    return List.unmodifiable(result);
  }

  String? timestamp(Object? value) {
    final text = optional(value, 128);
    if (text == null) return null;
    final match = RegExp(
      r'^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.[0-9]+)?(Z|[+-]([0-9]{2}):([0-9]{2}))$',
    ).firstMatch(text);
    if (match == null) {
      _badAlerts();
    }
    int part(int index) => int.parse(match.group(index)!);
    final year = part(1), month = part(2), day = part(3);
    if (month < 1 ||
        month > 12 ||
        day < 1 ||
        day > DateTime.utc(year, month + 1, 0).day ||
        part(4) > 23 ||
        part(5) > 59 ||
        part(6) > 60 ||
        (match.group(7) != 'Z' && (part(8) > 23 || part(9) > 59)) ||
        DateTime.tryParse(text) == null) {
      _badAlerts();
    }
    return text;
  }
}

List<dynamic> _list(Object? value, int max) {
  if (value is! List<dynamic>) {
    _badAlerts();
  }
  if (value.length > max) {
    _alertBudget();
  }
  return value;
}

double? _duration(Object? value) {
  if (value == null) return null;
  if (value is! num || !value.toDouble().isFinite || value < 0) {
    _badAlerts();
  }
  return value.toDouble();
}

class MonitoringAlertStatus {
  const MonitoringAlertStatus(this.state, [this.raw]);
  final MonitoringAlertState state;
  final String? raw;
  factory MonitoringAlertStatus._parse(Object? value, _Budget budget) {
    final row = _shape(value, {'kind', 'value'}, {'kind'});
    if (row['kind'] == 'unknown') {
      return MonitoringAlertStatus(MonitoringAlertState.unknown, budget.optional(row['value'], 64));
    }
    if (row.containsKey('value')) {
      _badAlerts();
    }
    return MonitoringAlertStatus(switch (row['kind']) {
      'inactive' => MonitoringAlertState.inactive,
      'pending' => MonitoringAlertState.pending,
      'firing' => MonitoringAlertState.firing,
      _ => _badAlerts(),
    });
  }
}

class MonitoringHealthStatus {
  const MonitoringHealthStatus(this.health, [this.raw]);
  final MonitoringRuleHealth health;
  final String? raw;
  factory MonitoringHealthStatus._parse(Object? value, _Budget budget) {
    final row = _shape(value, {'kind', 'value'}, {'kind'});
    if (row['kind'] == 'unknown') {
      return MonitoringHealthStatus(MonitoringRuleHealth.unknown, budget.optional(row['value'], 64));
    }
    if (row.containsKey('value')) {
      _badAlerts();
    }
    return MonitoringHealthStatus(switch (row['kind']) {
      'ok' => MonitoringRuleHealth.ok,
      'error' => MonitoringRuleHealth.error,
      _ => _badAlerts(),
    });
  }
}

class MonitoringAlertValue {
  const MonitoringAlertValue(this.kind, [this.value]);
  final SampleKind kind;
  final double? value;
  factory MonitoringAlertValue._parse(Object? value) {
    final row = _shape(value, {'kind', 'value'}, {'kind'});
    if (row['kind'] == 'finite') {
      final number = row['value'];
      if (number is! num || !number.toDouble().isFinite) {
        _badAlerts();
      }
      return MonitoringAlertValue(SampleKind.finite, number.toDouble());
    }
    if (row.containsKey('value')) {
      _badAlerts();
    }
    return MonitoringAlertValue(switch (row['kind']) {
      'nan' => SampleKind.nan,
      'positiveInfinity' => SampleKind.positiveInfinity,
      'negativeInfinity' => SampleKind.negativeInfinity,
      _ => _badAlerts(),
    });
  }
  String get display => switch (kind) {
    SampleKind.finite => value.toString(),
    SampleKind.nan => 'NaN',
    SampleKind.positiveInfinity => '+Inf',
    SampleKind.negativeInfinity => '-Inf',
  };
}

class MonitoringActiveAlert {
  MonitoringActiveAlert._(
    this.labels,
    this.annotations,
    this.state,
    this.value,
    this.activeAt,
    this.keepFiringSince,
    this.unsupportedFields,
  );
  final Map<String, String> labels, annotations;
  final MonitoringAlertStatus state;
  final MonitoringAlertValue value;
  final String? activeAt, keepFiringSince;
  final List<String> unsupportedFields;
  factory MonitoringActiveAlert._parse(Object? value, _Budget budget) {
    final row = _shape(
      value,
      {'labels', 'annotations', 'state', 'value', 'activeAt', 'keepFiringSince', 'unsupportedFields'},
      {'labels', 'annotations', 'state', 'value', 'unsupportedFields'},
    );
    if (++budget.alerts > 1000) {
      _alertBudget();
    }
    return MonitoringActiveAlert._(
      budget.metadata(row['labels']),
      budget.metadata(row['annotations']),
      MonitoringAlertStatus._parse(row['state'], budget),
      MonitoringAlertValue._parse(row['value']),
      budget.timestamp(row['activeAt']),
      budget.timestamp(row['keepFiringSince']),
      budget.unsupported(row['unsupportedFields']),
    );
  }
}

class MonitoringAlertRule {
  MonitoringAlertRule._(
    this.name,
    this.query,
    this.state,
    this.health,
    this.labels,
    this.annotations,
    this.alerts,
    this.lastError,
    this.durationSeconds,
    this.keepFiringForSeconds,
    this.evaluationTimeSeconds,
    this.lastEvaluation,
    this.unsupportedFields,
  );
  final String name, query;
  final MonitoringAlertStatus state;
  final MonitoringHealthStatus health;
  final Map<String, String> labels, annotations;
  final List<MonitoringActiveAlert> alerts;
  final String? lastError, lastEvaluation;
  final double? durationSeconds, keepFiringForSeconds, evaluationTimeSeconds;
  final List<String> unsupportedFields;
  factory MonitoringAlertRule._parse(Object? value, _Budget budget) {
    final row = _shape(
      value,
      {
        'name',
        'query',
        'state',
        'health',
        'durationSeconds',
        'keepFiringForSeconds',
        'labels',
        'annotations',
        'alerts',
        'lastError',
        'evaluationTimeSeconds',
        'lastEvaluation',
        'unsupportedFields',
      },
      {'name', 'query', 'state', 'health', 'labels', 'annotations', 'alerts', 'unsupportedFields'},
    );
    if (++budget.rules > 1000) {
      _alertBudget();
    }
    final alerts = _list(
      row['alerts'],
      1000,
    ).map((a) => MonitoringActiveAlert._parse(a, budget)).toList(growable: false);
    return MonitoringAlertRule._(
      budget.string(row['name']),
      budget.string(row['query'], 16384),
      MonitoringAlertStatus._parse(row['state'], budget),
      MonitoringHealthStatus._parse(row['health'], budget),
      budget.metadata(row['labels']),
      budget.metadata(row['annotations']),
      List.unmodifiable(alerts),
      budget.optional(row['lastError']),
      _duration(row['durationSeconds']),
      _duration(row['keepFiringForSeconds']),
      _duration(row['evaluationTimeSeconds']),
      budget.timestamp(row['lastEvaluation']),
      budget.unsupported(row['unsupportedFields']),
    );
  }
}

class MonitoringAlertGroup {
  MonitoringAlertGroup._(
    this.name,
    this.file,
    this.rules,
    this.intervalSeconds,
    this.limit,
    this.evaluationTimeSeconds,
    this.lastEvaluation,
    this.unsupportedFields,
  );
  final String name, file;
  final List<MonitoringAlertRule> rules;
  final double? intervalSeconds, evaluationTimeSeconds;
  final int? limit;
  final String? lastEvaluation;
  final List<String> unsupportedFields;
  factory MonitoringAlertGroup._parse(Object? value, _Budget budget) {
    final row = _shape(
      value,
      {
        'name',
        'file',
        'rules',
        'intervalSeconds',
        'limit',
        'evaluationTimeSeconds',
        'lastEvaluation',
        'unsupportedFields',
      },
      {'name', 'file', 'rules', 'unsupportedFields'},
    );
    if (++budget.groups > 100) {
      _alertBudget();
    }
    final limit = row['limit'];
    if (limit != null && (limit is! int || limit < 0 || limit > 9007199254740991)) {
      _badAlerts();
    }
    final rules = _list(row['rules'], 1000).map((r) => MonitoringAlertRule._parse(r, budget)).toList(growable: false);
    return MonitoringAlertGroup._(
      budget.string(row['name']),
      budget.string(row['file']),
      List.unmodifiable(rules),
      _duration(row['intervalSeconds']),
      limit as int?,
      _duration(row['evaluationTimeSeconds']),
      budget.timestamp(row['lastEvaluation']),
      budget.unsupported(row['unsupportedFields']),
    );
  }
}

class MonitoringAlertSnapshot {
  MonitoringAlertSnapshot._(
    this.kind,
    this.status,
    this.httpStatus,
    this.alerts,
    this.groups,
    this.groupNextToken,
    this.warnings,
    this.infos,
    this.partial,
    this.error,
    this.unsupportedFields,
  );
  final MonitoringAlertKind kind;
  final String status;
  final int httpStatus;
  final List<MonitoringActiveAlert> alerts;
  final List<MonitoringAlertGroup> groups;
  final String? groupNextToken;
  final List<String> warnings, infos, unsupportedFields;
  final bool partial;
  final MonitoringFailure? error;
  bool get empty => kind == MonitoringAlertKind.alerts ? alerts.isEmpty : groups.isEmpty;
  factory MonitoringAlertSnapshot.fromJson(MonitoringAlertKind kind, Json value) {
    final row = _shape(
      value,
      {'status', 'httpStatus', 'data', 'warnings', 'infos', 'partial', 'error'},
      {'status', 'httpStatus', 'data', 'warnings', 'infos', 'partial', 'error'},
    );
    final status = row['status'], http = row['httpStatus'], partial = row['partial'];
    if (!['success', 'error'].contains(status) || http is! int || http < 100 || http > 599 || partial is! bool) {
      _badAlerts();
    }
    final budget = _Budget();
    final warnings = budget.messages(row['warnings']), infos = budget.messages(row['infos']);
    MonitoringFailure? error;
    if (row['error'] != null) {
      final e = _shape(row['error'], {'type', 'message'}, {'type', 'message'});
      error = MonitoringFailure(
        budget.string(e['type']),
        budget.string(e['message']),
        httpStatus: http,
        isServerMessage: true,
      );
    }
    if ((status == 'success' && (row['data'] == null || error != null)) || (status == 'error' && error == null)) {
      _badAlerts();
    }
    List<MonitoringActiveAlert> alerts = const [];
    List<MonitoringAlertGroup> groups = const [];
    List<String> unsupported = const [];
    String? next;
    if (row['data'] != null) {
      final data = _shape(
        row['data'],
        kind == MonitoringAlertKind.alerts
            ? {'alerts', 'unsupportedFields'}
            : {'groups', 'groupNextToken', 'unsupportedFields'},
        kind == MonitoringAlertKind.alerts ? {'alerts', 'unsupportedFields'} : {'groups', 'unsupportedFields'},
      );
      unsupported = budget.unsupported(data['unsupportedFields']);
      if (kind == MonitoringAlertKind.alerts) {
        alerts = _list(
          data['alerts'],
          1000,
        ).map((a) => MonitoringActiveAlert._parse(a, budget)).toList(growable: false);
      } else {
        groups = _list(data['groups'], 100).map((g) => MonitoringAlertGroup._parse(g, budget)).toList(growable: false);
        next = budget.optional(data['groupNextToken']);
      }
    }
    if (!partial && (warnings.isNotEmpty || (status == 'error' && row['data'] != null) || next != null)) {
      _badAlerts();
    }
    if (utf8.encode(jsonEncode(value)).length > 2 * 1024 * 1024) {
      _alertBudget();
    }
    return MonitoringAlertSnapshot._(
      kind,
      status as String,
      http,
      List.unmodifiable(alerts),
      List.unmodifiable(groups),
      next,
      warnings,
      infos,
      partial,
      error,
      unsupported,
    );
  }
}
