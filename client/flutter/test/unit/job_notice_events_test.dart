import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
const other = '22222222-2222-4222-8222-222222222222';
const job = '33333333-3333-4333-8333-333333333333';
Map<String, dynamic> change({String owner = profile, String state = 'failed', int revision = 1}) => {
  'type': 'automation_job_changed',
  'profile_id': owner,
  'job_id': job,
  'revision': revision,
  'state': state,
};
Future<void> flush() async {
  for (var i = 0; i < 5; i++) {
    await Future<void>.value();
  }
}

void main() {
  test('strict Native metadata allows only exact UUID/revision/state fields and retains no diagnostic payload', () {
    for (final state in JobNoticeState.values) {
      expect(JobNoticeChange.parse(change(state: state.name))!.state, state);
    }
    for (final value in [
      {...change(), 'state': 'successful'},
      {...change(), 'state': 'future'},
      {...change(), 'revision': 0},
      {...change(), 'revision': -1},
      {...change(), 'revision': 9007199254740992},
      {...change(), 'profile_id': 'not-a-profile'},
      {...change(), 'job_id': '00000000-0000-0000-0000-000000000000'},
      {
        ...change(),
        'evidence': {'password': 'never retain'},
      },
      {...change(), 'command': 'never retain'},
    ]) {
      expect(JobNoticeChange.parse(value), isNull);
    }
  });
  test('safe integer maximum and cancelling are accepted; legacy zero is restricted to explicit record parsing', () {
    expect(
      JobNoticeChange.parse(change(state: 'cancelling', revision: 9007199254740991))!.state,
      JobNoticeState.cancelling,
    );
    expect(JobNoticeChange.parse(change(revision: 0)), isNull);
    expect(JobNoticeChange.parse(change(revision: 0), legacyRecord: true)!.revision, 0);
    expect(JobNoticeChange.parse(change(revision: 9007199254740992), legacyRecord: true), isNull);
  });
  test('explicit catalog accepts old V1 zero revision while dropping every evidence/manifest field and rejects unsupported/duplicate records', () {
    final row = {
      'version': 1,
      'job_id': job,
      'state': 'failed',
      'manifest': {'secret': 'never retain'},
      'runner_result': {'path': 'never retain'},
    };
    final payload = {
      'schema_version': 1,
      'read_only': false,
      'jobs': [row],
    };
    final parsed = JobNoticeChange.catalog(profile, payload)!;
    expect(parsed, hasLength(1));
    expect(parsed.single.revision, 0);
    expect(parsed.single.state, JobNoticeState.failed);
    for (final invalid in [
      {...payload, 'schema_version': 2},
      {...payload, 'read_only': true},
      {
        ...payload,
        'jobs': [row, row],
      },
      {
        ...payload,
        'jobs': [
          {...row, 'notification_revision': null},
        ],
      },
      {
        ...payload,
        'jobs': [
          {...row, 'state': 'future'},
        ],
      },
      {...payload, 'jobs': List.filled(2049, row)},
    ]) {
      expect(JobNoticeChange.catalog(profile, invalid), isNull);
    }
  });
  test(
    'lost existing core stream reports unavailable, never creates a new subscription or fabricates completion',
    () async {
      final adapter = JobNoticeEventAdapter(), frames = <JobNoticeFrame>[];
      final sub = adapter.watchJobNotices().listen(frames.add);
      adapter.acceptScope(originalEpoch: adapter.epoch, profileId: profile, unlocked: true);
      await flush();
      frames.clear();
      adapter.streamLost(closed: true);
      await flush();
      expect(frames.single.streamUnavailable, isTrue);
      expect(frames.single.change, isNull);
      expect(adapter.currentScope.streamUnavailable, isTrue);
      await adapter.dispose();
      await sub.cancel();
    },
  );
  test('stream closure while locked remains unavailable after a later confirmed unlocked scope', () async {
    final adapter = JobNoticeEventAdapter();
    adapter.streamLost(closed: true);
    adapter.acceptScope(originalEpoch: adapter.epoch, profileId: profile, unlocked: true);
    expect(adapter.currentScope.streamUnavailable, isTrue);
    expect(adapter.currentScope.refreshNeeded, isTrue);
    await adapter.dispose();
  });
  test('constructing/subscribing has no transport or request and locked scope ignores all job events', () async {
    final adapter = JobNoticeEventAdapter(), frames = <JobNoticeFrame>[];
    final sub = adapter.watchJobNotices().listen(frames.add);
    adapter.ingest(change());
    adapter.ingest({'type': 'lagged'});
    await flush();
    expect(frames, isEmpty);
    await adapter.dispose();
    await sub.cancel();
  });
  test('only current unlocked profile frames are forwarded and old profile data is not stored', () async {
    final adapter = JobNoticeEventAdapter(), frames = <JobNoticeFrame>[];
    final sub = adapter.watchJobNotices().listen(frames.add);
    adapter.acceptScope(originalEpoch: adapter.epoch, profileId: profile, unlocked: true);
    await flush();
    frames.clear();
    adapter.ingest(change(owner: other));
    adapter.ingest(change());
    await flush();
    expect(frames, hasLength(1));
    expect(frames.single.change!.profileId, profile);
    expect(frames.single.change!.jobId, job);
    expect(frames.single.change!.state, JobNoticeState.failed);
    await adapter.dispose();
    await sub.cancel();
  });
  test(
    'synchronous lock boundary defeats a delayed old-epoch refresh even before mirrored app state catches up',
    () async {
      final adapter = JobNoticeEventAdapter(), frames = <JobNoticeFrame>[];
      final sub = adapter.watchJobNotices().listen(frames.add);
      adapter.acceptScope(originalEpoch: adapter.epoch, profileId: profile, unlocked: true);
      final oldRefresh = adapter.epoch;
      adapter.boundary();
      adapter.acceptScope(originalEpoch: oldRefresh, profileId: profile, unlocked: true);
      adapter.ingest(change());
      await flush();
      expect(frames.where((frame) => frame.change != null), isEmpty);
      expect(frames.last.reset, isTrue);
      expect(frames.last.profileId, isNull);
      await adapter.dispose();
      await sub.cancel();
    },
  );
  test('fresh profile refresh accepts only its own epoch and clears before any event from the new profile', () async {
    final adapter = JobNoticeEventAdapter(), frames = <JobNoticeFrame>[];
    final sub = adapter.watchJobNotices().listen(frames.add);
    adapter.acceptScope(originalEpoch: adapter.epoch, profileId: profile, unlocked: true);
    adapter.boundary();
    adapter.acceptScope(originalEpoch: adapter.epoch, profileId: other, unlocked: true);
    adapter.ingest(change());
    adapter.ingest(change(owner: other));
    await flush();
    final changes = frames.where((frame) => frame.change != null).toList();
    expect(changes, hasLength(1));
    expect(changes.single.profileId, other);
    expect(frames[frames.length - 2].reset, isTrue);
    await adapter.dispose();
    await sub.cancel();
  });
  test('lagged or malformed same-profile metadata means refresh-needed without an invented state', () async {
    final adapter = JobNoticeEventAdapter(), frames = <JobNoticeFrame>[];
    final sub = adapter.watchJobNotices().listen(frames.add);
    adapter.acceptScope(originalEpoch: adapter.epoch, profileId: profile, unlocked: true);
    await flush();
    frames.clear();
    adapter.ingest({'type': 'lagged'});
    adapter.ingest({...change(), 'state': 'future'});
    await flush();
    expect(frames, hasLength(2));
    expect(frames.every((frame) => frame.refreshNeeded && frame.change == null), isTrue);
    await adapter.dispose();
    await sub.cancel();
  });
  test('unrelated existing core events are ignored and dispose permanently rejects late state', () async {
    final adapter = JobNoticeEventAdapter(), frames = <JobNoticeFrame>[];
    final sub = adapter.watchJobNotices().listen(frames.add);
    adapter.acceptScope(originalEpoch: adapter.epoch, profileId: profile, unlocked: true);
    await flush();
    frames.clear();
    for (final type in ['objects_changed', 'edit_status', 'ai_index_status', 'transfer_update', 'device_registered']) {
      adapter.ingest({'type': type});
    }
    await flush();
    expect(frames, isEmpty);
    await adapter.dispose();
    final count = frames.length;
    adapter.ingest(change());
    adapter.acceptScope(originalEpoch: adapter.epoch, profileId: profile, unlocked: true);
    await flush();
    expect(frames, hasLength(count));
    await sub.cancel();
  });
}
