import 'package:consolecrypt/monitoring/monitoring_accepted_query.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter_test/flutter_test.dart';

const source = MonitoringSource(id: 'owned', name: 'Owned', endpoint: 'http://metrics.invalid', revision: 1);
const good = QueryOutcome(
  status: 'success',
  httpStatus: 200,
  partial: true,
  warnings: ['bounded Native warning'],
  data: QueryData(
    'vector',
    series: [
      MonitoringSeries({'job': 'owned'}, [MonitoringSample(1000, SampleKind.nan)]),
    ],
  ),
);
const empty = QueryOutcome(
  status: 'success',
  httpStatus: 200,
  data: QueryData('vector', series: []),
);
const timeout = MonitoringFailure.local(MonitoringErrorCode.timeout);

void main() {
  test('HTTP status cannot promote unknown Native, schema, query, or budget errors to stale retention', () {
    for (final failure in [
      const MonitoringFailure.local(MonitoringErrorCode.invalidResponse, httpStatus: 500),
      const MonitoringFailure('unknown_native', 'never infer from message', httpStatus: 503),
      const MonitoringFailure.local(MonitoringErrorCode.invalidQuery, httpStatus: 429),
      const MonitoringFailure.local(MonitoringErrorCode.unsupportedResult, httpStatus: 503),
      const MonitoringFailure.local(MonitoringErrorCode.invalidBinding, httpStatus: 503),
      const MonitoringFailure.local(MonitoringErrorCode.responseTooLarge, httpStatus: 503),
      const MonitoringFailure.local(MonitoringErrorCode.budgetExceeded, httpStatus: 503),
      const MonitoringFailure('unknown_server', 'unknown API type', httpStatus: 503, isServerMessage: true),
      const MonitoringFailure('internal', 'native code is not API errorType', httpStatus: 503),
    ]) {
      expect(monitoringMayRetainAcceptedQuery(failure), isFalse, reason: failure.code);
    }
    for (final failure in [
      const MonitoringFailure.local(MonitoringErrorCode.transport, httpStatus: 503),
      const MonitoringFailure.local(MonitoringErrorCode.timeout, httpStatus: 500),
      const MonitoringFailure('internal', 'bounded Prometheus API error', httpStatus: 500, isServerMessage: true),
      const MonitoringFailure('unavailable', 'bounded Prometheus API error', httpStatus: 429, isServerMessage: true),
    ]) {
      expect(monitoringMayRetainAcceptedQuery(failure), isTrue, reason: failure.code);
    }
  });

  late Object scope;
  MonitoringQueryIdentity identity({
    MonitoringSource value = source,
    MonitoringQuery query = const MonitoringQuery(expression: 'up'),
  }) => MonitoringQueryIdentity(scope: scope, source: value, query: query);
  MonitoringAcceptedQueryState accepted() =>
      MonitoringAcceptedQueryState.empty.begin(identity(), 1).complete(1, good, DateTime.utc(2026, 11, 12));
  setUp(() {
    scope = Object();
  });

  test('same-context refresh retains explicit previous partial/nonfinite snapshot and failed refresh is stale', () {
    final before = accepted();
    final refreshing = before.begin(identity(), 2);
    expect(refreshing.phase, MonitoringAcceptedQueryPhase.refreshing);
    expect(refreshing.accepted, same(before.accepted));
    final failed = refreshing.fail(2, timeout);
    expect(failed.phase, MonitoringAcceptedQueryPhase.stale);
    expect(failed.accepted!.outcome, same(good));
    expect(failed.accepted!.completedUtc, DateTime.utc(2026, 11, 12));
    expect(failed.accepted!.outcome.data!.series.single.samples.single.kind, SampleKind.nan);
    expect(failed.error, same(timeout));
  });
  test('successful empty replaces old measurements and advances completion time without inventing zero', () {
    final state = accepted().begin(identity(), 2).complete(2, empty, DateTime.utc(2026, 11, 12, 1));
    expect(state.phase, MonitoringAcceptedQueryPhase.fresh);
    expect(state.accepted!.outcome.data!.series, isEmpty);
    expect(state.accepted!.completedUtc, DateTime.utc(2026, 11, 12, 1));
    expect(state.error, isNull);
  });
  test('failed Native partial outcome is preserved separately and never relabelled a success', () {
    const failure = QueryOutcome(
      status: 'error',
      httpStatus: 503,
      partial: true,
      data: QueryData('vector', series: []),
      error: timeout,
      infos: ['Native info'],
    );
    final before = accepted(), state = accepted().begin(identity(), 2).complete(2, failure, DateTime.utc(2026, 11, 13));
    expect(state.phase, MonitoringAcceptedQueryPhase.stale);
    expect(state.latestAttempt, same(failure));
    expect(state.accepted!.completedUtc, before.accepted!.completedUtc);
    expect(state.accepted!.outcome, same(good));
  });
  test('a first failed attempt has no accepted data or invented last-success', () {
    final state = MonitoringAcceptedQueryState.empty.begin(identity(), 1).fail(1, timeout);
    expect(state.phase, MonitoringAcceptedQueryPhase.failed);
    expect(state.accepted, isNull);
  });
  test('source revision, query expression, interval and step are distinct identities', () {
    final before = accepted();
    for (final next in [
      identity(value: source.withRecord(revision: 2)),
      identity(query: const MonitoringQuery(expression: 'other')),
      identity(query: const MonitoringQuery(expression: 'up', start: 0, end: 1000, stepSeconds: 15)),
      identity(query: const MonitoringQuery(expression: 'up', start: 0, end: 1000, stepSeconds: 30)),
    ]) {
      expect(before.begin(next, 2).accepted, isNull);
    }
    final range = identity(query: const MonitoringQuery(expression: 'up', start: 0, end: 1000, stepSeconds: 15));
    expect(
      range.matches(identity(query: const MonitoringQuery(expression: 'up', start: 0, end: 1000, stepSeconds: 30))),
      isFalse,
    );
  });
  test('new scope with same endpoint and IDs cannot reuse an accepted result', () {
    final before = accepted();
    scope = Object();
    expect(before.begin(identity(), 2).accepted, isNull);
  });
  test('late old completion and reset cannot restore data or last-success', () {
    final pending = accepted().begin(identity(), 3);
    expect(pending.complete(2, empty, DateTime.utc(2026)), same(pending));
    expect(pending.fail(2, timeout), same(pending));
    expect(
      MonitoringAcceptedQueryState.empty.complete(3, good, DateTime.utc(2026)),
      same(MonitoringAcceptedQueryState.empty),
    );
  });
  test('authority/source/schema and unknown failures clear rather than retain old measurements', () {
    for (final error in [
      const MonitoringFailure.local(MonitoringErrorCode.sourceChanged),
      const MonitoringFailure.local(MonitoringErrorCode.authUnavailable),
      const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion),
      const MonitoringFailure('unknown', 'private', httpStatus: 401),
      StateError('unknown fixture'),
    ]) {
      final state = accepted().begin(identity(), 2).fail(2, error);
      expect(state.accepted, isNull);
      expect(state.phase, MonitoringAcceptedQueryPhase.failed);
    }
  });
  test('pure identity rejects invalid custom interval and does not infer units', () {
    expect(
      () => identity(query: const MonitoringQuery(expression: 'up', start: 10, end: 5)),
      throwsA(isA<MonitoringFailure>()),
    );
    expect(accepted().accepted!.outcome, same(good));
  });
}
