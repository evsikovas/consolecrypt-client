import 'dart:async';

import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111', job = '22222222-2222-4222-8222-222222222222';
Map<String, dynamic> event({int revision = 1, String state = 'failed'}) => {
  'type': 'automation_job_changed',
  'profile_id': profile,
  'job_id': job,
  'revision': revision,
  'state': state,
};
Future<void> flush() async {
  for (var i = 0; i < 30; i++) {
    await Future<void>.value();
  }
}

class _Jobs implements AutomationJobService {
  int reads = 0;
  Completer<AutomationJson>? pending;
  AutomationJson response = {
    'schema_version': 1,
    'read_only': false,
    'jobs': [
      {
        'version': 1,
        'job_id': job,
        'state': 'failed',
        'notification_revision': 1,
        'manifest': {'password': 'must not retain'},
        'runner_result': {'path': 'must not retain'},
      },
    ],
  };
  @override
  Future<AutomationJson> jobs() async {
    reads++;
    return pending?.future ?? response;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw StateError('No execution or mutation allowed: ${invocation.memberName}');
}

class _Transport implements MonitoringPinTransport {
  int reads = 0, cancels = 0;
  Exception? failure;
  static const source = MonitoringSource(
    id: profile,
    name: 'Owned source',
    endpoint: 'http://metrics.invalid',
    revision: 1,
  );
  @override
  Future<MonitoringSource?> loadSource(String id) async => source;
  @override
  Future<QueryOutcome> readCurrent(MonitoringSource source, String expression) async {
    reads++;
    if (failure case final error?) throw error;
    return const QueryOutcome(status: 'success', httpStatus: 200);
  }

  @override
  void cancel() {
    cancels++;
  }
}

class _PanelTransport extends _Transport implements MonitoringGeneralPinTransport {
  late MonitoringBinding binding;
  @override
  Future<MonitoringObservationContext?> loadContext(MonitoringPinPlan plan) async =>
      MonitoringObservationContext(source: _Transport.source, binding: binding);
  @override
  Future<MonitoringObservationValue> readTarget(
    MonitoringPinPlan plan,
    MonitoringObservationContext context,
    MonitoringTimeWindow? window,
  ) async {
    reads++;
    if (failure case final error?) throw error;
    return MonitoringObservedBindingPanel(
      BindingReadResult(
        kind: 'bindingPanel',
        bindingId: binding.id,
        bindingRevision: binding.revision,
        panel: MonitoringPanel.cpu,
        state: BindingReadState.missing,
      ),
      window!,
    );
  }
}

void main() {
  test('pin scopeChanged immediately publishes cleared snapshot even before provider disposal', () async {
    final transport = _Transport()..failure = const MonitoringFailure.local(MonitoringErrorCode.timeout);
    final pin = MonitoringPinnedObservation(createTransport: () => transport, isCurrent: () => true);
    final owner = CriticalNoticeOwner(scope: Object(), profileId: profile, isCurrent: () => true, pin: pin);
    pin.pin(
      const MonitoringPinPlan(source: _Transport.source, expression: 'up', interval: MonitoringPinInterval.minutes5),
    );
    await flush();
    expect(owner.state.rows, hasLength(1));
    pin.stop(MonitoringPinStop.scopeChanged);
    await flush();
    expect(owner.current, isTrue);
    expect(owner.state.rows, isEmpty);
    await owner.dispose();
    pin.dispose();
  });

  test('observing current scope creates no job reads, metric requests, timer or execution; real event supplies typed read-only target', () async {
    final scope = Object(), events = JobNoticeEventAdapter(), jobs = _Jobs();
    events.acceptScope(originalEpoch: events.epoch, profileId: profile, unlocked: true);
    final owner = CriticalNoticeOwner(
      scope: scope,
      profileId: profile,
      isCurrent: () => true,
      jobs: jobs,
      events: events,
    );
    expect(jobs.reads, 0);
    expect(owner.state.rows, isEmpty);
    events.ingest(event());
    await flush();
    expect(owner.state.unread, 1);
    expect((owner.target(owner.state.rows.single.id) as CriticalJobTarget).jobId, job);
    owner.markRead(owner.state.rows.single.id);
    expect(owner.state.unread, 0);
    expect(jobs.reads, 0);
    await owner.dispose();
    await events.dispose();
  });
  test(
    'explicit Refresh reads existing jobs exactly once, strips payload and does not claim old rows are new completions',
    () async {
      final events = JobNoticeEventAdapter(), jobs = _Jobs();
      events.acceptScope(originalEpoch: events.epoch, profileId: profile, unlocked: true);
      final owner = CriticalNoticeOwner(
        scope: Object(),
        profileId: profile,
        isCurrent: () => true,
        jobs: jobs,
        events: events,
      );
      await owner.refreshJobs();
      expect(jobs.reads, 1);
      expect(owner.state.rows, hasLength(1));
      expect(owner.state.unread, 0);
      expect(owner.state.rows.single.job!.change.state, JobNoticeState.failed);
      await flush();
      expect(jobs.reads, 1);
      owner.clear();
      expect(owner.state.rows, isEmpty);
      events.ingest(event());
      await flush();
      expect(owner.state.rows, isEmpty);
      await owner.dispose();
      await events.dispose();
    },
  );
  test('concurrent Refresh has one in-flight call and reset rejects a late current-catalog response', () async {
    final events = JobNoticeEventAdapter(), jobs = _Jobs()..pending = Completer<AutomationJson>();
    events.acceptScope(originalEpoch: events.epoch, profileId: profile, unlocked: true);
    final owner = CriticalNoticeOwner(
      scope: Object(),
      profileId: profile,
      isCurrent: () => true,
      jobs: jobs,
      events: events,
    );
    final read = owner.refreshJobs();
    await flush();
    await owner.refreshJobs();
    expect(jobs.reads, 1);
    events.boundary();
    await flush();
    jobs.pending!.complete(jobs.response);
    await read;
    await flush();
    expect(owner.state.rows, isEmpty);
    expect(owner.state.refreshing, isFalse);
    expect(owner.state.canRefresh, isFalse);
    await owner.dispose();
    await events.dispose();
  });
  test('scope loss/dispose clears snapshots and a late response cannot reopen or execute anything', () async {
    var current = true;
    final jobs = _Jobs()..pending = Completer<AutomationJson>();
    final owner = CriticalNoticeOwner(scope: Object(), profileId: profile, isCurrent: () => current, jobs: jobs);
    final read = owner.refreshJobs();
    current = false;
    await owner.dispose();
    jobs.pending!.complete(jobs.response);
    await read;
    expect(owner.state.rows, isEmpty);
    expect(owner.target('job:$job'), isNull);
    expect(jobs.reads, 1);
  });
  test('unsupported/malformed catalog produces only safe enum error without retaining Native diagnostics', () async {
    final jobs = _Jobs()
      ..response = {'schema_version': 2, 'read_only': true, 'jobs': <Object?>[], 'error': 'never display this body'};
    final owner = CriticalNoticeOwner(scope: Object(), profileId: profile, isCurrent: () => true, jobs: jobs);
    await owner.refreshJobs();
    expect(owner.state.error, CriticalRefreshError.invalidResponse);
    expect(owner.state.rows, isEmpty);
    await owner.dispose();
  });
  test('Native vault-locked response clears current notices before mirrored profile state catches up', () async {
    final events = JobNoticeEventAdapter(), jobs = _Jobs()..pending = Completer<AutomationJson>();
    events.acceptScope(originalEpoch: events.epoch, profileId: profile, unlocked: true);
    final owner = CriticalNoticeOwner(
      scope: Object(),
      profileId: profile,
      isCurrent: () => true,
      jobs: jobs,
      events: events,
    );
    events.ingest(event());
    await flush();
    expect(owner.state.rows, hasLength(1));
    final read = owner.refreshJobs();
    jobs.pending!.completeError(
      const AppException(AppErrorCode.cancelled, 'private', reason: AppErrorReason.vaultLocked),
    );
    await read;
    expect(owner.state.rows, isEmpty);
    expect(owner.state.canRefresh, isFalse);
    events.ingest(event(revision: 2));
    await flush();
    expect(owner.state.rows, isEmpty);
    await owner.dispose();
    await events.dispose();
  });
  test('existing pin stream reports error/recovery/manual Stop without creating a service, copying query or cancelling the pin', () async {
    final transport = _Transport();
    var factories = 0;
    final pin = MonitoringPinnedObservation(
      createTransport: () {
        factories++;
        return transport;
      },
      isCurrent: () => true,
    );
    final owner = CriticalNoticeOwner(scope: Object(), profileId: profile, isCurrent: () => true, pin: pin);
    expect(factories, 0);
    pin.pin(
      const MonitoringPinPlan(
        source: _Transport.source,
        expression: 'private_query_not_in_notice',
        interval: MonitoringPinInterval.minutes5,
      ),
    );
    await flush();
    expect(owner.state.rows, isEmpty);
    expect(factories, 1);
    transport.failure = const MonitoringFailure.local(MonitoringErrorCode.timeout);
    pin.refresh();
    await flush();
    expect(owner.state.rows.single.pin!.status, CriticalPinStatus.retrying);
    expect((owner.target(owner.state.rows.single.id) as CriticalMonitoringTarget).sourceRevision, 1);
    transport.failure = null;
    pin.refresh();
    await flush();
    expect(owner.state.rows.single.pin!.status, CriticalPinStatus.recovered);
    final cancels = transport.cancels;
    await owner.dispose();
    expect(transport.cancels, cancels);
    expect(pin.state.active, isTrue);
    pin.stop(MonitoringPinStop.manual);
    pin.dispose();
  });
  test('foreground failures create no pin notices; scopeChanged still clears old pinned history immediately', () async {
    final transport = _Transport()..failure = const MonitoringFailure.local(MonitoringErrorCode.timeout);
    final pin = MonitoringPinnedObservation(createTransport: () => transport, isCurrent: () => true);
    final owner = CriticalNoticeOwner(scope: Object(), profileId: profile, isCurrent: () => true, pin: pin);
    const plan = MonitoringPinPlan(
      source: _Transport.source,
      expression: 'private-notice-free-query',
      interval: MonitoringPinInterval.minutes5,
    );
    pin.pin(plan);
    await flush();
    expect(owner.state.rows, hasLength(1));
    pin.stop(MonitoringPinStop.manual);
    await flush();
    final previous = owner.state.rows.single;
    pin.setForegroundActive(true);
    pin.observeForeground(
      plan,
      const MonitoringObservedQuery(QueryOutcome(status: 'success', httpStatus: 200, data: QueryData('vector')), null),
    );
    await flush();
    pin.refresh();
    await flush();
    expect(owner.state.rows, hasLength(1));
    expect(owner.state.rows.single, same(previous));
    pin.stop(MonitoringPinStop.scopeChanged);
    await flush();
    expect(owner.state.rows, isEmpty);
    expect(owner.current, isTrue);
    await owner.dispose();
    pin.dispose();
  });
  test('typed binding accepted value can recover a pin notice without any generic query outcome', () async {
    final transport = _PanelTransport()
      ..binding = MonitoringBinding(
        id: job,
        name: 'Owned binding',
        revision: 2,
        hostId: '33333333-3333-4333-8333-333333333333',
        sourceId: profile,
        sourceRevision: 1,
        selector: const {'instance': 'owned'},
        state: MonitoringBindingState.current,
      );
    final window = MonitoringTimeWindow.preset(MonitoringTimePreset.hour1, DateTime.utc(2026, 11, 12));
    final plan = MonitoringPinPlan.forTarget(
      source: _Transport.source,
      target: MonitoringBindingPanelTarget(
        binding: transport.binding,
        panel: MonitoringPanel.cpu,
        windowPolicy: MonitoringFixedObservationWindow(window),
      ),
    );
    final pin = MonitoringPinnedObservation(createTransport: () => transport, isCurrent: () => true);
    final owner = CriticalNoticeOwner(scope: Object(), profileId: profile, isCurrent: () => true, pin: pin);
    pin.pin(plan);
    await flush();
    expect(pin.state.outcome, isNull);
    expect(pin.state.hasAcceptedResult, isTrue);
    transport.failure = const MonitoringFailure.local(MonitoringErrorCode.timeout);
    pin.refresh();
    await flush();
    expect(owner.state.rows.single.pin!.status, CriticalPinStatus.retrying);
    transport.failure = null;
    pin.refresh();
    await flush();
    expect(pin.state.outcome, isNull);
    expect(pin.state.hasAcceptedResult, isTrue);
    expect(owner.state.rows.single.pin!.status, CriticalPinStatus.recovered);
    await owner.dispose();
    pin.dispose();
  });
}
