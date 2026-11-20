import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation.dart';

Never _bad() =>
    throw const FormatException('Service results unavailable or changed');
final _uuid = RegExp(
  r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$',
);
final _digest = RegExp(r'^[0-9a-f]{64}$');
String _id(Object? value) =>
    value is String &&
        _uuid.hasMatch(value) &&
        value != '00000000-0000-0000-0000-000000000000'
    ? value
    : _bad();
String _sha(Object? value) =>
    value is String && _digest.hasMatch(value) ? value : _bad();
Map<String, dynamic> _closed(Object? value, Set<String> keys) =>
    value is Map<String, dynamic> &&
        value.length == keys.length &&
        value.keys.every(keys.contains)
    ? value
    : _bad();
String _choice(Object? value, Set<String> values) =>
    value is String && values.contains(value) ? value : _bad();
bool? _flag(Object? value) =>
    value == null || value is bool ? value as bool? : _bad();
DateTime _utc(Object? value, {bool slot = false}) {
  if (value is! String || value.length > 64) _bad();
  final pattern = slot
      ? RegExp(r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$')
      : RegExp(
          r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$',
        );
  final m = pattern.firstMatch(value);
  final parsed = DateTime.tryParse(slot ? '$value:00Z' : value);
  if (m == null ||
      parsed == null ||
      !parsed.isUtc ||
      parsed.year != int.parse(m[1]!) ||
      parsed.month != int.parse(m[2]!) ||
      parsed.day != int.parse(m[3]!) ||
      parsed.hour != int.parse(m[4]!) ||
      parsed.minute != int.parse(m[5]!) ||
      (!slot && parsed.second != int.parse(m[6]!))) {
    _bad();
  }
  return parsed;
}

/// Native verified terminal metadata only. No raw output, event or task payload.
final class ServiceRecordedJob {
  ServiceRecordedJob._(Map<String, dynamic> raw)
    : jobId = _id(raw['jobId']),
      manifestDigest = _sha(raw['manifestDigest']),
      scheduleId = _id(raw['scheduleId']),
      localSlot = raw['localSlot'] as String,
      dueUtc = DateTime.fromMillisecondsSinceEpoch(
        (raw['dueUtc'] as int) * 1000,
        isUtc: true,
      ),
      state = _choice(raw['state'], _states),
      runnerState = _choice(raw['runnerState'], _states),
      targetOutcome = _choice(raw['targetOutcome'], const {
        'known',
        'pending',
        'uncertain',
      }),
      partial = raw['remoteChangesMayBePartial'] as bool,
      cancelRequested = _flag(raw['cancelRequested']),
      timedOut = _flag(raw['timedOut']),
      exitCode = raw['exitCode'] as int?,
      acceptedUtc = _utc(raw['acceptedUtc']),
      updatedUtc = _utc(raw['updatedUtc']);
  static const _states = {
    'succeeded',
    'failed',
    'cancelled',
    'interrupted',
    'unknown',
  };
  final String jobId,
      manifestDigest,
      scheduleId,
      localSlot,
      state,
      runnerState,
      targetOutcome;
  final DateTime dueUtc, acceptedUtc, updatedUtc;
  final bool partial;
  final bool? cancelRequested, timedOut;
  final int? exitCode;
  static ServiceRecordedJob parse(Object? value) {
    final v = _closed(value, const {
      'jobId',
      'manifestDigest',
      'scheduleId',
      'localSlot',
      'dueUtc',
      'state',
      'runnerState',
      'targetOutcome',
      'remoteChangesMayBePartial',
      'cancelRequested',
      'timedOut',
      'exitCode',
      'acceptedUtc',
      'updatedUtc',
    });
    _utc(v['localSlot'], slot: true);
    if (v['dueUtc'] is! int ||
        (v['dueUtc'] as int) < 1 ||
        (v['dueUtc'] as int) > 253402300799 ||
        v['remoteChangesMayBePartial'] is! bool ||
        v['exitCode'] != null &&
            (v['exitCode'] is! int ||
                (v['exitCode'] as int) < -2147483648 ||
                (v['exitCode'] as int) > 2147483647)) {
      _bad();
    }
    final result = ServiceRecordedJob._(v);
    final uncertain =
        result.partial ||
        result.targetOutcome != 'known' ||
        result.runnerState == 'unknown';
    if (uncertain && (result.state != 'unknown' || !result.partial) ||
        !uncertain && result.state != result.runnerState ||
        result.state == 'succeeded' &&
            (result.exitCode != 0 ||
                result.cancelRequested != false ||
                result.timedOut != false)) {
      _bad();
    }
    return result;
  }
}

final class ServiceRecordedResults {
  ServiceRecordedResults._(this.policyDigest, this.jobs);
  final String policyDigest;
  final List<ServiceRecordedJob> jobs;
  static ServiceRecordedResults parse(
    AutomationJson raw,
    ServiceDelegationScope scope,
    String runnerId,
  ) {
    if (utf8.encode(jsonEncode(raw)).length > 65536) _bad();
    final v = _closed(raw, const {
      'version',
      'profileId',
      'notificationScopeId',
      'runnerId',
      'serviceId',
      'policyDigest',
      'resultsAvailable',
      'jobs',
    });
    if (v['version'] is! int ||
        v['version'] != 1 ||
        !scope.matches(v) ||
        _id(v['runnerId']) != runnerId ||
        _id(v['serviceId']) != runnerId ||
        v['resultsAvailable'] != true ||
        v['jobs'] is! List ||
        (v['jobs'] as List).length > 32) {
      _bad();
    }
    final jobs = (v['jobs'] as List)
        .map(ServiceRecordedJob.parse)
        .toList(growable: false);
    if (jobs.map((j) => j.jobId).toSet().length != jobs.length) _bad();
    return ServiceRecordedResults._(
      _sha(v['policyDigest']),
      List.unmodifiable(jobs),
    );
  }
}
