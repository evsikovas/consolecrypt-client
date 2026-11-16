import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:flutter_test/flutter_test.dart';

int checks = 0;
void check(bool value, String name) {
  expect(value, isTrue, reason: name);
  checks++;
}

Future<void> flush() async {
  for (var i = 0; i < 20; i++) {
    await Future<void>.value();
  }
}

class Ticket implements MonitoringPinTimer {
  Ticket(this.due, this.callback);
  final DateTime due;
  final void Function() callback;
  bool cancelled = false, fired = false;
  @override
  void cancel() {
    cancelled = true;
  }
}

class Clock {
  DateTime value = DateTime.utc(2026, 11, 12);
  final tasks = <Ticket>[];
  MonitoringPinTimer schedule(Duration delay, void Function() callback) {
    final ticket = Ticket(value.add(delay), callback);
    tasks.add(ticket);
    return ticket;
  }

  int get pending => tasks.where((t) => !t.cancelled && !t.fired).length;
  Future<void> advance(Duration duration) async {
    value = value.add(duration);
    final due = tasks.where((t) => !t.cancelled && !t.fired && !t.due.isAfter(value)).toList();
    for (final ticket in due) {
      ticket.fired = true;
      ticket.callback();
      await flush();
    }
  }
}

const source = MonitoringSource(
  id: '11111111-1111-4111-8111-111111111111',
  name: 'Owned source',
  endpoint: 'http://metrics.invalid',
  revision: 1,
);
const outcome = QueryOutcome(
  status: 'success',
  httpStatus: 200,
  partial: true,
  warnings: ['fixture partial'],
  data: QueryData(
    'vector',
    series: [
      MonitoringSeries({'job': 'owned'}, [MonitoringSample(1000, SampleKind.nan)]),
    ],
  ),
);

class Transport implements MonitoringPinTransport {
  MonitoringSource? current = source;
  int loads = 0, reads = 0, cancels = 0;
  Exception? error;
  Completer<QueryOutcome>? pending;
  final expressions = <String>[];
  @override
  Future<MonitoringSource?> loadSource(String id) async {
    loads++;
    return current;
  }

  @override
  Future<QueryOutcome> readCurrent(MonitoringSource value, String expression) async {
    reads++;
    expressions.add(expression);
    if (error case final e?) throw e;
    return pending?.future ?? outcome;
  }

  @override
  void cancel() {
    cancels++;
  }
}

MonitoringPinPlan plan([MonitoringPinInterval interval = MonitoringPinInterval.seconds30]) =>
    MonitoringPinPlan(source: source, expression: 'up', interval: interval);

void main() {
  test('pin policy rejects Native schema/unknown/budget despite HTTP and retries only explicit transport/server categories', () {
    for (final failure in [
      const MonitoringFailure.local(MonitoringErrorCode.invalidResponse, httpStatus: 500),
      const MonitoringFailure('unknown_native', 'private', httpStatus: 503),
      const MonitoringFailure.local(MonitoringErrorCode.invalidQuery, httpStatus: 429),
      const MonitoringFailure.local(MonitoringErrorCode.budgetExceeded, httpStatus: 503),
      const MonitoringFailure.local(MonitoringErrorCode.unsupportedResult, httpStatus: 503),
    ]) {
      expect(MonitoringPinnedObservation.classifyFailure(failure), MonitoringPinDisposition.stop);
    }
    for (final failure in [
      const MonitoringFailure.local(MonitoringErrorCode.transport, httpStatus: 503),
      const MonitoringFailure.local(MonitoringErrorCode.timeout),
      const MonitoringFailure('internal', 'bounded API failure', httpStatus: 500, isServerMessage: true),
    ]) {
      expect(MonitoringPinnedObservation.classifyFailure(failure), MonitoringPinDisposition.retry);
    }
  });

  test('authoritative source catalog acknowledgement invalidates immediately and clears late read', () async {
    final transport = Transport()..pending = Completer<QueryOutcome>();
    final clock = Clock();
    final owner = MonitoringPinnedObservation(
      createTransport: () => transport,
      isCurrent: () => true,
      now: () => clock.value,
      schedule: clock.schedule,
    );
    addTearDown(owner.dispose);
    owner.pin(plan());
    await flush();
    owner.sourcesChanged([source], readOnly: false);
    expect(owner.state.busy, isTrue);
    owner.sourcesChanged([], readOnly: false);
    expect(owner.state.stopReason, MonitoringPinStop.sourceChanged);
    transport.pending!.complete(outcome);
    await flush();
    expect(owner.state.outcome, isNull);
    expect(clock.pending, 0);
    expect(transport.reads, 1);
  });

  for (final interval in MonitoringPinInterval.values) {
    test('interval ${interval.name} and three-error backoff are bounded', () async {
      final clock = Clock(), transport = Transport();
      final owner = MonitoringPinnedObservation(
        createTransport: () => transport,
        isCurrent: () => true,
        now: () => clock.value,
        schedule: clock.schedule,
      );
      addTearDown(owner.dispose);
      owner.pin(plan(interval));
      await flush();
      expect(owner.state.nextDueUtc, clock.value.add(interval.duration));
      transport.error = const MonitoringFailure.local(MonitoringErrorCode.timeout);
      await clock.advance(interval.duration);
      expect(owner.state.nextDueUtc!.difference(clock.value), interval.duration);
      await clock.advance(interval.duration);
      expect(owner.state.nextDueUtc!.difference(clock.value).inSeconds, lessThanOrEqualTo(300));
      await clock.advance(const Duration(seconds: 300));
      expect(owner.state.stopReason, MonitoringPinStop.retryLimit);
      expect(clock.pending, 0);
    });
  }
  for (final replacement in <MonitoringSource?>[
    null,
    MonitoringSource(id: source.id, name: source.name, endpoint: 'http://changed.invalid', revision: 1),
  ]) {
    test('removed or same-revision changed source never issues a hidden query', () async {
      final clock = Clock(), transport = Transport();
      final owner = MonitoringPinnedObservation(
        createTransport: () => transport,
        isCurrent: () => true,
        now: () => clock.value,
        schedule: clock.schedule,
      );
      addTearDown(owner.dispose);
      owner.pin(plan());
      await flush();
      transport.current = replacement;
      await clock.advance(const Duration(seconds: 30));
      expect(transport.reads, 1);
      expect(owner.state.stopReason, MonitoringPinStop.sourceChanged);
      expect(owner.state.outcome, isNull);
      expect(clock.pending, 0);
    });
  }
  test('invalid draft never constructs a transport; transient success clears stale failure count', () async {
    final clock = Clock(), transport = Transport();
    var factories = 0;
    final owner = MonitoringPinnedObservation(
      createTransport: () {
        factories++;
        return transport;
      },
      isCurrent: () => true,
      now: () => clock.value,
      schedule: clock.schedule,
    );
    addTearDown(owner.dispose);
    expect(
      owner.pin(const MonitoringPinPlan(source: source, expression: '  ', interval: MonitoringPinInterval.minute1)),
      isFalse,
    );
    expect(factories, 0);
    owner.pin(plan());
    await flush();
    transport.error = const MonitoringFailure.local(MonitoringErrorCode.timeout);
    await clock.advance(const Duration(seconds: 30));
    expect(owner.state.failures, 1);
    transport.error = null;
    await clock.advance(const Duration(seconds: 30));
    expect(owner.state.failures, 0);
    expect(owner.state.error, isNull);
    expect(owner.state.outcome, same(outcome));
  });
  test('completion scheduler, scope and lifecycle preserve 28 invariants', () async {
    final clock = Clock(), transport = Transport();
    var factories = 0, current = true;
    final owner = MonitoringPinnedObservation(
      createTransport: () {
        factories++;
        return transport;
      },
      isCurrent: () => current,
      now: () => clock.value,
      schedule: clock.schedule,
    );
    check(factories == 0 && clock.pending == 0, 'watching/constructing has no transport or timer');
    check(owner.pin(plan()), 'explicit Pin admitted');
    await flush();
    check(
      factories == 1 && transport.reads == 1 && transport.loads == 2,
      'dedicated transport and before/after source fence',
    );
    check(
      identical(owner.state.outcome, outcome) &&
          owner.state.outcome!.data!.series.single.samples.single.kind == SampleKind.nan,
      'partial/NaN snapshot preserved without zero',
    );
    check(owner.state.nextDueUtc == clock.value.add(const Duration(seconds: 30)), 'completion schedules interval');
    await clock.advance(const Duration(seconds: 29));
    check(transport.reads == 1, 'no early tick');
    transport.pending = Completer<QueryOutcome>();
    await clock.advance(const Duration(seconds: 1));
    check(owner.state.busy && transport.reads == 2 && clock.pending == 0, 'one pending read owns the cycle');
    await clock.advance(const Duration(seconds: 300));
    check(transport.reads == 2 && !owner.refresh(), 'slow read has no overlap/manual duplicate/catchup');
    transport.pending!.complete(outcome);
    await flush();
    transport.pending = null;
    check(
      owner.state.nextDueUtc == clock.value.add(const Duration(seconds: 30)),
      'slow completion restarts cadence from completion',
    );
    check(owner.refresh(), 'manual refresh explicit');
    await flush();
    check(transport.reads == 3 && clock.pending == 1, 'manual refresh replaces scheduled tick');
    owner.lifecycle(MonitoringPinLifecycle.inactive);
    await clock.advance(const Duration(seconds: 30));
    check(owner.state.active && transport.reads == 4, 'desktop inactive does not stop authorized pin');
    owner.lifecycle(MonitoringPinLifecycle.paused);
    final pausedReads = transport.reads;
    await clock.advance(const Duration(minutes: 10));
    check(
      !owner.state.active &&
          owner.state.plan == null &&
          owner.state.outcome == null &&
          clock.pending == 0 &&
          transport.reads == pausedReads,
      'paused stops and clears timer/data',
    );
    owner.lifecycle(MonitoringPinLifecycle.resumed);
    await clock.advance(const Duration(minutes: 10));
    check(transport.reads == pausedReads, 'resume never auto-restores');
    check(owner.pin(plan()), 'fresh explicit Pin after pause');
    await flush();
    transport.current = source.withRecord(revision: 2);
    final reads = transport.reads;
    await clock.advance(const Duration(seconds: 30));
    check(
      transport.reads == reads &&
          owner.state.stopReason == MonitoringPinStop.sourceChanged &&
          owner.state.outcome == null,
      'source revision stops without silent rebind',
    );
    transport.current = source;
    transport.pending = Completer<QueryOutcome>();
    owner.pin(plan());
    await flush();
    transport.current = source.withRecord(revision: 2);
    transport.pending!.complete(outcome);
    await flush();
    transport.pending = null;
    check(
      owner.state.stopReason == MonitoringPinStop.sourceChanged && owner.state.outcome == null,
      'revision changed during read cannot accept late response',
    );
    transport.current = source;
    owner.pin(plan());
    await flush();
    current = false;
    await clock.advance(const Duration(seconds: 30));
    check(
      owner.state.stopReason == MonitoringPinStop.scopeChanged && clock.pending == 0,
      'scope loss fences next tick',
    );
    final count = factories;
    check(!owner.pin(plan()) && factories == count, 'locked scope cannot allocate transport');
    current = true;
    transport.pending = Completer<QueryOutcome>();
    owner.pin(plan());
    await flush();
    owner.stop(MonitoringPinStop.manual);
    final cancelled = transport.cancels;
    transport.pending!.complete(outcome);
    await flush();
    transport.pending = null;
    check(
      owner.state.plan == null && owner.state.outcome == null && clock.pending == 0 && transport.cancels == cancelled,
      'unpin clears and ignores late result',
    );
    owner.dispose();
    check(!owner.pin(plan()) && !owner.refresh(), 'disposed owner cannot resume');

    final retryClock = Clock(), retryTransport = Transport();
    final retries = MonitoringPinnedObservation(
      createTransport: () => retryTransport,
      isCurrent: () => true,
      now: () => retryClock.value,
      schedule: retryClock.schedule,
    );
    retries.pin(plan());
    await flush();
    final success = retries.state.lastSuccessUtc;
    retryTransport.error = const MonitoringFailure.local(MonitoringErrorCode.timeout);
    await retryClock.advance(const Duration(seconds: 30));
    check(
      retries.state.phase == MonitoringPinPhase.backoff &&
          retries.state.failures == 1 &&
          identical(retries.state.outcome, outcome) &&
          retries.state.lastSuccessUtc == success,
      'failed refresh retains explicitly stale accepted data/last success',
    );
    await retryClock.advance(const Duration(seconds: 30));
    check(
      retries.state.failures == 2 && retries.state.nextDueUtc == retryClock.value.add(const Duration(seconds: 60)),
      'bounded exponential backoff',
    );
    await retryClock.advance(const Duration(seconds: 60));
    check(
      retries.state.stopReason == MonitoringPinStop.retryLimit && !retries.state.active && retryClock.pending == 0,
      'third failure stops rather than retrying forever',
    );
    retries.stop(MonitoringPinStop.manual);
    check(retries.state.plan == null && retries.state.outcome == null, 'clear removes retained stale data');
    retries.dispose();

    for (final event in [MonitoringPinLifecycle.detached, MonitoringPinLifecycle.exit]) {
      final c = Clock(),
          t = Transport(),
          o = MonitoringPinnedObservation(
            createTransport: () => t,
            isCurrent: () => true,
            now: () => c.value,
            schedule: c.schedule,
          );
      o.pin(plan());
      await flush();
      o.lifecycle(event);
      final before = t.reads;
      await c.advance(const Duration(days: 1));
      check(t.reads == before && !o.state.active && c.pending == 0, 'closed/detached app never polls');
      o.dispose();
    }
    final rejected = Transport()
      ..error = const MonitoringFailure('unknown', 'fixture private diagnostic', httpStatus: 401);
    final rejectClock = Clock(),
        rejectOwner = MonitoringPinnedObservation(
          createTransport: () => rejected,
          isCurrent: () => true,
          now: () => rejectClock.value,
          schedule: rejectClock.schedule,
        );
    rejectOwner.pin(plan());
    await flush();
    check(
      rejectOwner.state.stopReason == MonitoringPinStop.rejected && rejectClock.pending == 0,
      'auth denial never retries',
    );
    rejectOwner.dispose();
    expect(checks, 28);
  });
}
