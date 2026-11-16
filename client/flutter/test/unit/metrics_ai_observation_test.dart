import 'dart:convert';

import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter_test/flutter_test.dart';

import 'metrics_ai_context_test.dart' as fixture;

void main() {
  final outcome = fixture.accepted().accepted!.outcome;
  final window = MonitoringTimeWindow.fromEpoch(10, 100, 10);
  final plan = MonitoringPinPlan.forTarget(
    source: fixture.source,
    target: MonitoringCustomRangeTarget(
      expression: 'PRIVATE_QUERY_EXCLUDED',
      windowPolicy: MonitoringFixedObservationWindow(window),
    ),
  );
  MonitoringPinState state({Object? error, MonitoringPinPlan? changedPlan}) => MonitoringPinState(
    plan: changedPlan ?? plan,
    value: MonitoringObservedQuery(outcome, window),
    lastSuccessUtc: fixture.instant,
    phase: MonitoringPinPhase.backoff,
    error: error,
  );
  String project(MonitoringPinState value, {Object? owner}) => MetricsAiProjection.selected(
    fixture.fence,
    metrics: [
      MetricsAiMetricSelection.fromObservation(
        value,
        fixture.fence,
        originalOwner: owner ?? fixture.metrics,
        seriesIndex: 0,
        sampleIndices: [0, 1],
      ),
    ],
    alerts: [],
    createdUtc: fixture.instant,
  ).dataJson;
  test('accepted Auto range uses actual UTC window and finite-zero/NaN without another query or expression', () {
    final data = project(state());
    expect(data, isNot(contains('PRIVATE_')));
    final row = (jsonDecode(data) as Map<String, Object?>)['metrics']! as List<Object?>;
    final metric = row.single! as Map<String, Object?>;
    expect(metric['actualAcceptedWindow'], {
      'kind': 'range',
      'startUnixSecondsUtc': 10.0,
      'endUnixSecondsUtc': 100.0,
      'requestedStepSeconds': 10.0,
      'effectiveStepSeconds': 20.0,
    });
    expect(metric['samples'], [
      {
        'timestampUnixSecondsUtc': 10.0,
        'value': {'kind': 'finite', 'value': 0.0},
      },
      {
        'timestampUnixSecondsUtc': 20.0,
        'value': {'kind': 'nan'},
      },
    ]);
  });
  test('transient stale observation remains explicitly stale and keeps accepted completed timestamp', () {
    final data = project(state(error: const MonitoringFailure.local(MonitoringErrorCode.transport)));
    expect(data, contains('"stale":true'));
    expect(data, contains(fixture.instant.toIso8601String()));
  });
  test('schema unknown and source failures do not resurrect a previously accepted observation', () {
    for (final error in [
      const MonitoringFailure('unknown_native', 'private', httpStatus: 503),
      const MonitoringFailure('invalid_response', 'private', httpStatus: 500),
      const MonitoringFailure.local(MonitoringErrorCode.sourceChanged),
    ]) {
      expect(() => project(state(error: error)), throwsA(isA<MetricsAiProjectionException>()));
    }
  });
  test('foreign original owner or changed source fingerprint cannot reuse accepted Auto data', () {
    expect(() => project(state(), owner: Object()), throwsA(isA<MetricsAiProjectionException>()));
    final changed = MonitoringPinPlan.forTarget(source: fixture.source.withRecord(revision: 5), target: plan.target);
    expect(() => project(state(changedPlan: changed)), throwsA(isA<MetricsAiProjectionException>()));
  });
  test('changed fixed window never relabels accepted sample timestamps or last-success window', () {
    final changed = MonitoringPinPlan.forTarget(
      source: fixture.source,
      target: MonitoringCustomRangeTarget(
        expression: 'PRIVATE_QUERY_EXCLUDED',
        windowPolicy: MonitoringFixedObservationWindow(MonitoringTimeWindow.fromEpoch(100, 200, 10)),
      ),
    );
    expect(() => project(state(changedPlan: changed)), throwsA(isA<MetricsAiProjectionException>()));
  });
}
