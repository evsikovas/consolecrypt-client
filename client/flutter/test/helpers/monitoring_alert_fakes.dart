import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';

import 'monitoring_fakes.dart';

Json alertRow({String state = 'firing', String value = 'nan'}) => {
  'labels': {'alertname': 'FixtureCPU', 'instance': 'untrusted.example:9100'},
  'annotations': {'description': '<b>display only</b>', 'runbook_url': 'https://untrusted.example.invalid/do-not-open'},
  'state': {'kind': state},
  'value': value == 'finite' ? {'kind': 'finite', 'value': 0} : {'kind': value},
  'activeAt': '2026-11-11T12:00:00Z',
  'keepFiringSince': null,
  'unsupportedFields': <String>[],
};
Json alertRuleRow() => {
  'name': 'Fixture CPU rule',
  'query': 'rate(example_total{job="quoted"}[5m]) > 1',
  'state': {'kind': 'unknown', 'value': 'new-backend-state'},
  'health': {'kind': 'error'},
  'durationSeconds': 30,
  'keepFiringForSeconds': 0,
  'labels': <String, String>{},
  'annotations': <String, String>{},
  'alerts': [alertRow()],
  'lastError': 'Backend evaluation fixture error',
  'evaluationTimeSeconds': 0.012,
  'lastEvaluation': '2026-11-11T12:00:01Z',
  'unsupportedFields': ['futureRuleField'],
};
Json alertGroupRow() => {
  'name': 'Fixture group',
  'file': '/backend/opaque.yml',
  'rules': [alertRuleRow()],
  'intervalSeconds': 15,
  'limit': 0,
  'evaluationTimeSeconds': 0.02,
  'lastEvaluation': null,
  'unsupportedFields': <String>[],
};
Json alertEnvelope({
  MonitoringAlertKind kind = MonitoringAlertKind.alerts,
  bool partial = true,
  String status = 'success',
  bool empty = false,
}) => {
  'status': status,
  'httpStatus': status == 'success' ? 200 : 503,
  'data': kind == MonitoringAlertKind.alerts
      ? {
          'alerts': empty ? <Json>[] : [alertRow()],
          'unsupportedFields': <String>[],
        }
      : {
          'groups': empty ? <Json>[] : [alertGroupRow()],
          'groupNextToken': partial ? 'opaque-never-followed' : null,
          'unsupportedFields': <String>[],
        },
  'warnings': partial ? ['Fixture partial backend warning'] : <String>[],
  'infos': ['Fixture backend annotation'],
  'partial': partial,
  'error': status == 'error' ? {'type': 'unavailable', 'message': 'Backend fixture partial error'} : null,
};

class MonitoringAlertTestService extends MonitoringTestService implements MonitoringAlertReader {
  final reads = <(String, int, MonitoringAlertKind)>[];
  Completer<MonitoringAlertSnapshot>? pendingAlerts;
  MonitoringFailure? alertError;
  Json? response;
  @override
  Future<MonitoringAlertSnapshot> readAlerts(MonitoringSource source, MonitoringAlertKind kind) async {
    reads.add((source.id, source.revision, kind));
    if (alertError != null) throw alertError!;
    return pendingAlerts?.future ?? MonitoringAlertSnapshot.fromJson(kind, response ?? alertEnvelope(kind: kind));
  }
}
