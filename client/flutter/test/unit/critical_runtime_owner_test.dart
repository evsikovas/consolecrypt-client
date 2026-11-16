import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
const nonce = '22222222-2222-4222-8222-222222222222';
const next = '33333333-3333-4333-8333-333333333333';
const record = '44444444-4444-4444-8444-444444444444';
void establish(RuntimeNoticeEventAdapter events, {String scope = nonce}) => events.acceptScope(
  originalEpoch: events.epoch,
  nativeProfile: {'id': profile, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': scope},
);
Map<String, dynamic> change({int revision = 1, String scope = nonce, String id = record}) => {
  'type': 'critical_state_changed',
  'profile_id': profile,
  'scope_id': scope,
  'record_id': id,
  'revision': revision,
  'change': {'domain': 'terminal', 'state': 'failed'},
};
Future<void> flush() async {
  for (var i = 0; i < 150; i++) {
    await Future<void>.value();
  }
}

class _Jobs implements AutomationJobService {
  int reads = 0;
  @override
  Future<AutomationJson> jobs() async {
    reads++;
    return {'schema_version': 1, 'read_only': false, 'jobs': <Object?>[]};
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('No operation allowed: ${invocation.memberName}');
}

void main() {
  late RuntimeNoticeEventAdapter runtime;
  late JobNoticeEventAdapter jobs;
  late _Jobs service;
  late CriticalNoticeOwner owner;
  bool current = true;
  setUp(() {
    runtime = RuntimeNoticeEventAdapter();
    establish(runtime);
    jobs = JobNoticeEventAdapter();
    jobs.acceptScope(originalEpoch: jobs.epoch, profileId: profile, unlocked: true);
    service = _Jobs();
    current = true;
    owner = CriticalNoticeOwner(
      scope: Object(),
      profileId: profile,
      isCurrent: () => current,
      jobs: service,
      events: jobs,
      runtimeEvents: runtime,
    );
  });
  tearDown(() async {
    await owner.dispose();
    await runtime.dispose();
    await jobs.dispose();
  });
  test('actual common owner observes runtime metadata without reading jobs or executing operations', () async {
    runtime.ingest(change());
    await flush();
    final row = owner.state.rows.single;
    expect(row.runtime!.value.state, RuntimeNoticeState.failed);
    expect(row.target, isA<CriticalRuntimeTarget>());
    expect((row.target as CriticalRuntimeTarget).domain, RuntimeNoticeDomain.terminal);
    expect(service.reads, 0);
    owner.markReadRow(row);
    expect(owner.state.unread, 0);
    owner.clear();
    expect(owner.state.rows, isEmpty);
    expect(service.reads, 0);
  });
  test('old nonce captured UI row cannot dismiss or mark replacement notice', () async {
    runtime.ingest(change());
    await flush();
    final old = owner.state.rows.single;
    establish(runtime, scope: next);
    runtime.ingest(change(scope: next));
    await flush();
    expect(owner.rowCurrent(old), isFalse);
    owner.markReadRow(old);
    owner.dismissRow(old);
    expect(owner.state.rows, hasLength(1));
    expect(owner.state.unread, 1);
  });
  test('old revision captured UI row cannot hide newer critical state', () async {
    runtime.ingest(change());
    await flush();
    final old = owner.state.rows.single;
    runtime.ingest(change(revision: 2));
    await flush();
    owner.dismissRow(old);
    owner.markReadRow(old);
    expect(owner.state.rows.single.runtime!.value.revision, 2);
    expect(owner.state.unread, 1);
  });
  test('jobs boundary hides runtime rows before its own queued boundary arrives', () async {
    runtime.ingest(change());
    await flush();
    jobs.boundary();
    await flush();
    expect(owner.state.rows, isEmpty);
    runtime.boundary();
    establish(runtime);
    await flush();
    expect(owner.state.rows, hasLength(1));
  });
  test('jobs manual refresh cannot heal a runtime event gap', () async {
    runtime.ingest({...change(), 'extra': 'synthetic untrusted data'});
    await flush();
    expect(owner.state.runtimeIncomplete, isTrue);
    await owner.refreshJobs();
    expect(service.reads, 1);
    expect(owner.state.runtimeIncomplete, isTrue);
    expect(owner.state.rows, isEmpty);
  });
  test('jobs manual refresh cannot reopen closed runtime feed', () async {
    runtime.streamLost(closed: true);
    await flush();
    expect(owner.state.runtimeFeedUnavailable, isTrue);
    await owner.refreshJobs();
    expect(owner.state.runtimeFeedUnavailable, isTrue);
    expect(service.reads, 1);
  });
  test('unsupported old Native runtime is separate from jobs capability', () async {
    runtime.acceptScope(
      originalEpoch: runtime.epoch,
      nativeProfile: {'id': profile, 'active': true, 'vault_state': 'unlocked'},
    );
    await flush();
    expect(owner.state.runtimeUnsupported, isTrue);
    expect(owner.state.canRefresh, isTrue);
  });
  test('profile owner invalidation ignores late events', () async {
    current = false;
    runtime.ingest(change());
    await flush();
    expect(owner.state.rows, isEmpty);
    expect(service.reads, 0);
  });
  test('runtime dismissal preserves ring revision fence', () async {
    runtime.ingest(change());
    await flush();
    owner.dismissRow(owner.state.rows.single);
    runtime.ingest(change());
    await flush();
    expect(owner.state.rows, isEmpty);
    runtime.ingest(change(revision: 2));
    await flush();
    expect(owner.state.rows, hasLength(1));
  });
  test('common owner enforces64 rows across runtime and jobs', () async {
    for (var i = 1; i <= 40; i++) {
      final id = '${i.toRadixString(16).padLeft(8, '0')}-7777-4777-8777-777777777777';
      runtime.ingest(change(id: id));
      jobs.ingest({
        'type': 'automation_job_changed',
        'profile_id': profile,
        'job_id': id,
        'revision': 1,
        'state': 'failed',
      });
    }
    await flush();
    expect(owner.state.rows, hasLength(64));
    expect(owner.state.rows.any((r) => r.runtime != null), isTrue);
    expect(owner.state.rows.any((r) => r.job != null), isTrue);
    expect(service.reads, 0);
  });
}
