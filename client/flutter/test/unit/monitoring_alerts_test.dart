import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/monitoring_alert_fakes.dart';
import '../helpers/monitoring_fakes.dart';

void main() {
  test('Native alerts preserve partial warnings unknown states and special values without zero fallback', () {
    for (final kind in ['nan', 'positiveInfinity', 'negativeInfinity', 'finite']) {
      final wire = alertEnvelope(partial: false);
      wire['data'] = {
        'alerts': [alertRow(value: kind)],
        'unsupportedFields': <String>[],
      };
      final parsed = MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, wire);
      expect(parsed.alerts.single.value.display, switch (kind) {
        'nan' => 'NaN',
        'positiveInfinity' => '+Inf',
        'negativeInfinity' => '-Inf',
        _ => '0.0',
      });
      expect(parsed.alerts.single.state.state, MonitoringAlertState.firing);
    }
    final rules = MonitoringAlertSnapshot.fromJson(
      MonitoringAlertKind.alertRules,
      alertEnvelope(kind: MonitoringAlertKind.alertRules, status: 'error'),
    );
    expect(rules.status, 'error');
    expect(rules.httpStatus, 503);
    expect(rules.partial, isTrue);
    expect(rules.groups.single.rules.single.state.state, MonitoringAlertState.unknown);
    expect(rules.groups.single.rules.single.state.raw, 'new-backend-state');
    expect(rules.groups.single.rules.single.health.health, MonitoringRuleHealth.error);
    expect(rules.groupNextToken, 'opaque-never-followed');
    expect(rules.error!.isServerMessage, isTrue);
  });
  test('duplicate alert/rule records remain separate untrusted records, never deduplicated identities', () {
    final wire = alertEnvelope(partial: false);
    wire['data'] = {
      'alerts': [alertRow(), alertRow()],
      'unsupportedFields': <String>[],
    };
    expect(MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, wire).alerts.length, 2);
    final rules = alertEnvelope(kind: MonitoringAlertKind.alertRules);
    rules['data'] = {
      'groups': [alertGroupRow(), alertGroupRow()],
      'groupNextToken': null,
      'unsupportedFields': <String>[],
    };
    expect(MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alertRules, rules).groups.length, 2);
  });
  test('unknown/future fields unsupported schema missing typed data and inconsistent partial fail closed', () {
    for (final value in [
      {...alertEnvelope(), 'version': 2},
      {...alertEnvelope(), 'status': 'future'},
      {...alertEnvelope(), 'partial': false},
      {...alertEnvelope(), 'httpStatus': 0},
      {...alertEnvelope(), 'data': null},
      {
        ...alertEnvelope(),
        'error': {'type': 'x', 'message': 'y'},
      },
      {
        ...alertEnvelope(),
        'data': {
          'alerts': [
            <String, dynamic>{...alertRow(), 'execute': true},
          ],
          'unsupportedFields': <String>[],
        },
      },
      {
        ...alertEnvelope(),
        'data': {
          'alerts': [
            <String, dynamic>{
              ...alertRow(),
              'state': {'kind': 'future'},
            },
          ],
          'unsupportedFields': <String>[],
        },
      },
    ]) {
      expect(
        () => MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, value),
        throwsA(isA<MonitoringFailure>()),
      );
    }
  });
  test('typed optional values reject invalid date duration health and nonfinite numeric sample', () {
    for (final field in [
      {
        'value': {'kind': 'finite', 'value': double.nan},
      },
      {'activeAt': 'invalid-date'},
      {'activeAt': '2026-02-31T12:00:00Z'},
      {'activeAt': '2026-11-11T25:00:00Z'},
      {'activeAt': '2026-11-11T12:61:00+00:00'},
      {
        'state': {'kind': 'inactive', 'value': 'must not exist'},
      },
    ]) {
      final wire = alertEnvelope();
      wire['data'] = {
        'alerts': [
          {...alertRow(), ...field},
        ],
        'unsupportedFields': <String>[],
      };
      expect(
        () => MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, wire),
        throwsA(isA<MonitoringFailure>()),
      );
    }
    for (final field in [
      {'durationSeconds': -1},
      {'evaluationTimeSeconds': 'wrong'},
      {
        'health': {'kind': 'ok', 'value': 'wrong'},
      },
    ]) {
      final wire = alertEnvelope(kind: MonitoringAlertKind.alertRules);
      wire['data'] = {
        'groups': [
          {
            ...alertGroupRow(),
            'rules': [
              {...alertRuleRow(), ...field},
            ],
          },
        ],
        'groupNextToken': null,
        'unsupportedFields': <String>[],
      };
      expect(
        () => MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alertRules, wire),
        throwsA(isA<MonitoringFailure>()),
      );
    }
  });
  test('quotas are cumulative across rule groups and preserve records instead of truncating', () {
    for (final data in [
      {'alerts': List.generate(1001, (_) => alertRow()), 'unsupportedFields': <String>[]},
      {
        'alerts': [
          <String, dynamic>{
            ...alertRow(),
            'labels': {for (var i = 0; i < 65; i++) 'k$i': 'v'},
          },
        ],
        'unsupportedFields': <String>[],
      },
      {
        'alerts': [
          <String, dynamic>{
            ...alertRow(),
            'annotations': {'long': 'x' * 4097},
          },
        ],
        'unsupportedFields': <String>[],
      },
    ]) {
      expect(
        () => MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, {...alertEnvelope(), 'data': data}),
        throwsA(isA<MonitoringFailure>()),
      );
    }
    final groups = List.generate(
      2,
      (_) => <String, dynamic>{
        ...alertGroupRow(),
        'rules': List.generate(501, (_) => <String, dynamic>{...alertRuleRow(), 'alerts': <Json>[]}),
      },
    );
    expect(
      () => MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alertRules, {
        ...alertEnvelope(kind: MonitoringAlertKind.alertRules),
        'data': {'groups': groups, 'groupNextToken': null, 'unsupportedFields': <String>[]},
      }),
      throwsA(isA<MonitoringFailure>()),
    );
  });
  test('warnings/infos, names-only unknown field lists and labels have byte budgets', () {
    for (final wire in [
      {...alertEnvelope(), 'warnings': List.filled(101, 'warning')},
      {
        ...alertEnvelope(),
        'infos': ['i' * 4097],
      },
      {
        ...alertEnvelope(),
        'data': {'alerts': <Json>[], 'unsupportedFields': List.filled(33, 'future')},
      },
      {
        ...alertEnvelope(),
        'data': {
          'alerts': List.generate(
            5,
            (_) => <String, dynamic>{
              ...alertRow(),
              'labels': {for (var i = 0; i < 64; i++) 'k$i': 'x' * 4096},
            },
          ),
          'unsupportedFields': <String>[],
        },
      },
    ]) {
      expect(
        () => MonitoringAlertSnapshot.fromJson(MonitoringAlertKind.alerts, wire),
        throwsA(isA<MonitoringFailure>()),
      );
    }
  });
  test(
    'same Native facade emits fixed parameter-free actions with saved source/revision and cancels pending read',
    () async {
      final requests = <Json>[], cancelled = <String>[];
      final pending = Completer<Json>();
      final service = AppCoreMonitoringService(
        read: (request) async {
          requests.add(request);
          return pending.future;
        },
        cancelOperation: (id) async {
          cancelled.add(id);
        },
      );
      final future = service.readAlerts(monitoringFixture, MonitoringAlertKind.alertRules);
      await Future<void>.delayed(Duration.zero);
      service.cancel();
      expect(cancelled, [requests.single['operationId']]);
      expect(requests.single.keys.toSet(), {'operationId', 'sourceId', 'sourceRevision', 'action'});
      expect(requests.single['sourceId'], monitoringFixture.id);
      expect(requests.single['sourceRevision'], 1);
      expect(requests.single['action'], {'kind': 'alertRules'});
      pending.complete({
        'sourceId': monitoringFixture.id,
        'sourceRevision': 1,
        'result': alertEnvelope(kind: MonitoringAlertKind.alertRules),
      });
      await future;
      expect(requests.length, 1); // No continuation/URL follow-up.
    },
  );
  test('wrong source/revision and bounded Native client errors never become backend alert success', () async {
    for (final response in [
      {'sourceId': monitoringFixture.id, 'sourceRevision': 2, 'result': alertEnvelope()},
      {'sourceId': 'wrong', 'sourceRevision': 1, 'result': alertEnvelope()},
      {
        'sourceId': monitoringFixture.id,
        'sourceRevision': 1,
        'result': {
          'error': {'code': 'timeout', 'message': 'Native-only diagnostic'},
        },
      },
    ]) {
      final service = AppCoreMonitoringService(read: (_) async => response, cancelOperation: (_) async {});
      await expectLater(
        service.readAlerts(monitoringFixture, MonitoringAlertKind.alerts),
        throwsA(isA<MonitoringFailure>()),
      );
    }
  });
}
