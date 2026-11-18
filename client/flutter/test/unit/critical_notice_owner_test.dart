import 'dart:async';

import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
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

void main() {
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
}
