import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';

/// Read-only Native facts. Neither this DTO nor a selection grants a capture,
/// provider call or execution; Native resolves the retained capture ID again.
final class AutomationAiCapturedContext {
  const AutomationAiCapturedContext._({
    required this.captureId,
    required this.profileId,
    required this.notificationScopeId,
    required this.projectId,
    required this.projectRevision,
    required this.kind,
    required this.source,
    required this.subjectId,
    required this.subjectRevision,
    required this.capturedUtc,
    required this.expiresUtc,
    required this.fields,
  });

  factory AutomationAiCapturedContext.fromJson(
    AutomationJson value, {
    required String expectedProfileId,
    required String expectedNotificationScopeId,
    required String expectedProjectId,
    required int expectedProjectRevision,
    required AutomationAiContextKind expectedKind,
    required String expectedSubjectId,
  }) {
    const keys = {
      'version',
      'captureId',
      'profileId',
      'notificationScopeId',
      'projectId',
      'projectRevision',
      'kind',
      'source',
      'subjectId',
      'subjectRevision',
      'capturedUtc',
      'expiresUtc',
      'fields',
    };
    if (utf8.encode(jsonEncode(value)).length > 65536 ||
        value.length != keys.length ||
        !value.keys.every(keys.contains) ||
        value['version'] is! int ||
        value['version'] != 1 ||
        value['profileId'] != expectedProfileId ||
        value['notificationScopeId'] != expectedNotificationScopeId ||
        value['projectId'] != expectedProjectId ||
        value['projectRevision'] != expectedProjectRevision ||
        value['subjectId'] != expectedSubjectId ||
        value['kind'] != expectedKind.name ||
        !_id(value['captureId']) ||
        !_id(value['profileId']) ||
        !_id(value['notificationScopeId']) ||
        !_id(value['projectId']) ||
        !_id(value['subjectId']) ||
        !_revision(value['projectRevision']) ||
        !_revision(value['subjectRevision'])) {
      throw const FormatException('ai_context');
    }
    final host = expectedKind == AutomationAiContextKind.hostOs;
    final source = host ? 'sshFixedOsV1' : 'runnerProbeProjectPinsV1';
    if (value['source'] != source || value['fields'] is! Map<String, dynamic>) {
      throw const FormatException('ai_context_source');
    }
    final fields = value['fields'] as AutomationJson;
    final allowed = host
        ? const {'osId', 'osVersion', 'architecture', 'kernelRelease'}
        : const {'ansibleCore', 'python', 'collections'};
    if (!fields.keys.every(allowed.contains)) throw const FormatException('ai_context_fields');
    final copied = <String, Object>{};
    for (final entry in fields.entries) {
      if (entry.value == null) continue; // Explicit missing remains unknown.
      if (entry.key != 'collections') {
        if (!_text(entry.value, 256)) throw const FormatException('ai_context_field');
        copied[entry.key] = entry.value as String;
      } else {
        final rows = entry.value;
        if (rows is! List || rows.length > 32) throw const FormatException('ai_context_collections');
        final names = <String>{};
        final pins = <Map<String, String>>[];
        for (final row in rows) {
          if (row is! Map ||
              row.length != 2 ||
              !_text(row['name'], 128) ||
              !_text(row['version'], 128) ||
              !names.add(row['name'] as String)) {
            throw const FormatException('ai_context_collection');
          }
          pins.add(Map.unmodifiable({'name': row['name'] as String, 'version': row['version'] as String}));
        }
        copied['collections'] = List<Map<String, String>>.unmodifiable(pins);
      }
    }
    final captured = _utc(value['capturedUtc']), expires = _utc(value['expiresUtc']);
    if (!expires.isAfter(captured) || expires.difference(captured) > const Duration(seconds: 120)) {
      throw const FormatException('ai_context_time');
    }
    return AutomationAiCapturedContext._(
      captureId: value['captureId'] as String,
      profileId: expectedProfileId,
      notificationScopeId: value['notificationScopeId'] as String,
      projectId: expectedProjectId,
      projectRevision: value['projectRevision'] as int,
      kind: expectedKind,
      source: source,
      subjectId: expectedSubjectId,
      subjectRevision: value['subjectRevision'] as int,
      capturedUtc: captured,
      expiresUtc: expires,
      fields: Map.unmodifiable(copied),
    );
  }

  final String captureId, profileId, notificationScopeId, projectId, source, subjectId;
  final int projectRevision, subjectRevision;
  final AutomationAiContextKind kind;
  final DateTime capturedUtc, expiresUtc;
  final Map<String, Object> fields;

  AutomationJson selection(Set<String> selected) {
    if (selected.isEmpty || !selected.every(fields.containsKey)) throw const FormatException('ai_context_selection');
    return {'captureId': captureId, 'fieldIds': selected.toList()..sort()};
  }

  static bool _id(Object? value) =>
      value is String && RegExp(r'^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$').hasMatch(value);
  static bool _revision(Object? value) => value is int && value > 0 && value <= 9007199254740991;
  static bool _text(Object? value, int limit) =>
      value is String &&
      value.isNotEmpty &&
      value.runes.length <= limit &&
      !RegExp(r'[\x00-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u2069]').hasMatch(value);
  static DateTime _utc(Object? value) {
    if (value is! String) throw const FormatException('ai_context_time');
    final match = RegExp(r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$').firstMatch(value);
    final parsed = DateTime.tryParse(value);
    if (match == null ||
        parsed == null ||
        [
          parsed.year,
          parsed.month,
          parsed.day,
          parsed.hour,
          parsed.minute,
          parsed.second,
        ].asMap().entries.any((e) => e.value != int.parse(match.group(e.key + 1)!))) {
      throw const FormatException('ai_context_time');
    }
    return parsed.toUtc();
  }
}
