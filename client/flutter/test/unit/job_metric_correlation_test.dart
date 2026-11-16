import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:flutter_test/flutter_test.dart';

// Standalone: no Flutter, packages, services, network, timer or process launch.
void check(bool condition, [String hint = 'condition']) {
  if (!condition) throw StateError(hint);
}

void rejects(void Function() action) {
  expect(action, throwsArgumentError);
}

void rejectsMutation(void Function() action) {
  expect(action, throwsUnsupportedError);
}

Map<String, Object?> object(Object? value) => value as Map<String, Object?>;
Map<String, Object?> target(Map<String, dynamic> value) =>
    object((object(value['manifest'])['targets'] as List<Object?>).single);

String uuid(int value) => '00000000-0000-0000-0000-${value.toRadixString(16).padLeft(12, '0')}';
final profile = uuid(1), host = uuid(2), source = uuid(3), binding = uuid(4), project = uuid(5);
final digest = List.filled(64, 'a').join();
final at = DateTime.utc(2026, 11, 12, 12);
int seconds(DateTime value) => value.millisecondsSinceEpoch ~/ 1000;
JobClockAlignment measured(Duration? offset, Duration? uncertainty) => JobClockAlignment.measured(
  offset,
  uncertainty,
  validFromProducerUtc: at.subtract(const Duration(days: 1)),
  validThroughProducerUtc: at.add(const Duration(days: 1)),
);

JobCorrelationContext context({
  JobCorrelationScope? scope,
  String? selectedHost,
  int hostRevision = 7,
  int sourceRevision = 3,
  int bindingRevision = 4,
  String? bindingHost,
  String? bindingSource,
  int? bindingSourceRevision,
  JobCorrelationBindingState bindingState = JobCorrelationBindingState.current,
  DateTime? start,
  DateTime? end,
  JobClockAlignment? clock,
}) => JobCorrelationContext(
  scope: scope ?? JobCorrelationScope(profile),
  hostId: selectedHost ?? host,
  hostRevision: hostRevision,
  sourceId: source,
  sourceRevision: sourceRevision,
  bindingId: binding,
  bindingRevision: bindingRevision,
  bindingHostId: bindingHost ?? selectedHost ?? host,
  bindingSourceId: bindingSource ?? source,
  bindingSourceRevision: bindingSourceRevision ?? sourceRevision,
  bindingState: bindingState,
  startUtc: start ?? at.subtract(const Duration(minutes: 10)),
  endUtc: end ?? at.add(const Duration(minutes: 10)),
  clientClock: clock ?? const JobClockAlignment.unknown(),
);

Map<String, dynamic> row({int id = 100, List<String>? hosts, int revision = 1, String state = 'succeeded'}) {
  final ids = hosts ?? [host];
  return {
    'version': 1,
    'job_id': uuid(id),
    'project_id': project,
    'notification_revision': revision,
    'state': state,
    'host_ids': [...ids],
    'manifest_digest': digest,
    'accepted_at': seconds(at),
    'updated_at': seconds(at.add(const Duration(minutes: 1))),
    'manifest': {
      'version': 1,
      'job_id': uuid(id),
      'tenant_id': profile,
      'project_id': project,
      'revision_digest': digest,
      'targets': [
        for (final id in ids) {'host_id': id, 'address': 'never-retain.example', 'username': 'secret-sentinel'},
      ],
    },
    'runner_result': {
      'version': 1,
      'job_id': uuid(id),
      'manifest_digest': digest,
      'accepted_at': '2026-11-12T12:00:10.123456Z',
      'updated_at': '2026-11-12T12:00:50Z',
      'output': 'secret-sentinel',
    },
  };
}

Map<String, dynamic> catalog(List<Object?> rows) => {'schema_version': 1, 'read_only': false, 'jobs': rows};
JobMetricCorrelation reader(JobCorrelationContext c) => JobMetricCorrelation()..bind(c);
JobMetricSnapshot read(JobMetricCorrelation r, JobCorrelationContext c, List<Object?> rows, {DateTime? observed}) {
  final ticket = r.begin(c)!;
  check(r.complete(ticket, catalog(rows), observed ?? at.add(const Duration(hours: 1))));
  return r.snapshot;
}

JobMetricMarker only(List<Object?> rows, {JobCorrelationContext? selected}) {
  final c = selected ?? context(), r = reader(c);
  final snap = read(r, c, rows);
  check(snap.failure == null && snap.markers.length == 1);
  return snap.markers.single;
}

void main() {
  test('exact original host/source/binding metadata retained', () {
    final c = context(), r = reader(c), snap = read(r, c, [row()]);
    check(identical(snap.context, c));
    check(c.hostId == host && c.hostRevision == 7 && c.sourceId == source && c.sourceRevision == 3);
    check(c.bindingId == binding && c.bindingRevision == 4 && c.scope.profileId == profile);
    final m = snap.markers.single;
    check(m.jobId == uuid(100) && m.projectId == project && m.revisionDigest == digest && m.notificationRevision == 1);
  });
  test('binding cannot cross host/source/source revision', () {
    rejects(() => context(bindingHost: uuid(8)));
    rejects(() => context(bindingSource: uuid(8)));
    rejects(() => context(bindingSourceRevision: 2));
    for (final state in JobCorrelationBindingState.values.where((s) => s != JobCorrelationBindingState.current)) {
      rejects(() => context(bindingState: state));
    }
  });
  test('canonical nonnil IDs and positive safe revisions only', () {
    rejects(() => JobCorrelationScope('A0000000-0000-0000-0000-000000000001'));
    rejects(() => JobCorrelationScope(uuid(0)));
    rejects(() => context(selectedHost: 'host-name'));
    rejects(() => context(hostRevision: 0));
    rejects(() => context(sourceRevision: 9007199254740992));
    rejects(() => context(bindingRevision: -1));
  });
  test('requested domain is exact UTC and ordered', () {
    rejects(() => context(start: DateTime(2026, 11, 12)));
    rejects(() => context(start: at, end: at));
    rejects(() => context(start: at.add(const Duration(seconds: 1)), end: at));
    final c = context(start: at.add(const Duration(microseconds: 1)), end: at.add(const Duration(microseconds: 2)));
    check(c.endUtc.difference(c.startUtc).inMicroseconds == 1);
  });
  test('exact approved target membership; aliases/addresses never infer host', () {
    final c = context(), r = reader(c);
    final other = row(hosts: [uuid(8)]);
    target(other)['address'] = host;
    check(read(r, c, [other]).markers.isEmpty);
    check(
      read(r, c, [
            row(hosts: [host, uuid(8)]),
          ]).markers.length ==
          1,
    );
  });
  test('wrong profile and project fail whole projection', () {
    for (final field in ['tenant_id', 'project_id']) {
      final c = context(), r = reader(c), value = row();
      object(value['manifest'])[field] = uuid(99);
      check(read(r, c, [value]).failure == JobCorrelationFailure.invalidMetadata);
    }
  });
  test('unknown catalog/record/manifest versions rejected without partial rows', () {
    final c = context(), r = reader(c);
    for (final schema in [2, 1.0, null]) {
      final raw = catalog([row()])..['schema_version'] = schema;
      r.complete(r.begin(c)!, raw, at);
      check(r.snapshot.failure == JobCorrelationFailure.unsupportedSchema && r.snapshot.markers.isEmpty);
    }
    final future = row()..['version'] = 2;
    check(read(r, c, [row(), future]).failure == JobCorrelationFailure.unsupportedSchema);
    final futureManifest = row();
    object(futureManifest['manifest'])['version'] = 2;
    check(read(r, c, [futureManifest]).failure == JobCorrelationFailure.invalidMetadata);
    final raw = catalog([row()])..['read_only'] = true;
    r.complete(r.begin(c)!, raw, at);
    check(r.snapshot.failure == JobCorrelationFailure.unsupportedSchema);
  });
  test('duplicate jobs and duplicate/mismatched targets are explicit errors', () {
    final c = context(), r = reader(c);
    check(read(r, c, [row(), row()]).failure == JobCorrelationFailure.duplicateJob);
    check(
      read(r, c, [
            row(hosts: [host, host]),
          ]).failure ==
          JobCorrelationFailure.invalidMetadata,
    );
    final value = row();
    target(value)['host_id'] = uuid(90);
    check(read(r, c, [value]).failure == JobCorrelationFailure.invalidMetadata);
  });
  test('unknown state remains unknown and whole-job result is not per-host success', () {
    check(only([row(state: 'future-success')]).state == JobMetricState.unknown);
    final acknowledged = only([row(state: 'acknowledged')]);
    check(acknowledged.state == JobMetricState.acknowledged);
    check(acknowledged.outcomeScope == JobOutcomeScope.wholeJobNotPerHost);
    check(acknowledged.executionFinishedUtc == null && acknowledged.executionStartedUtc == null);
  });
  test('missing signer/revision object/host revision never inferred', () {
    final m = only([row()]);
    check(m.initiator == JobInitiator.unknown && m.hostRevisionAtExecution == null);
    // Only a revision DIGEST is persisted. No guessed revision ID, current user
    // or current host revision substitutes for absent historical facts.
    check(m.revisionDigest == digest);
  });
  test('timestamps carry distinct producer and meaning, not execution duration', () {
    final m = only([row()]);
    check(m.admitted!.utc == at && m.admitted!.origin == JobClockOrigin.nativeClient);
    check(m.admitted!.meaning == JobTimeMeaning.admitted);
    check(m.updated!.meaning == JobTimeMeaning.latestStateUpdate);
    check(m.runnerQueued!.origin == JobClockOrigin.runner && m.runnerQueued!.meaning == JobTimeMeaning.queuedOnRunner);
    check(m.runnerUpdated!.meaning == JobTimeMeaning.latestRunnerState);
    check(m.received.origin == JobClockOrigin.observedClient && m.received.meaning == JobTimeMeaning.received);
    check(m.executionFinishedUtc == null);
  });
  test('unknown clock alignment gives no invented coordinate', () {
    final m = only([row()]);
    check(m.position == null && m.unpositioned == JobMarkerUnpositioned.unknownClockSkew);
  });
  test('missing or malformed native time never uses received/update/window start', () {
    for (final value in [null, '2026-11-12T12:00:00Z', double.nan, double.infinity, -1, 253402300800, 1.5]) {
      final raw = row()..['accepted_at'] = value;
      final m = only([raw], selected: context(clock: measured(Duration.zero, Duration.zero)));
      check(m.admitted == null && m.position == null && m.unpositioned == JobMarkerUnpositioned.missingAdmissionTime);
      check(m.updated != null && m.received.utc != at);
    }
  });
  test('known producer skew is subtracted, uncertainty remains interval', () {
    final c = context(clock: measured(const Duration(seconds: 30), const Duration(seconds: 2)));
    final m = only([row()], selected: c), p = m.position!;
    check(p.earliestUtc == at.subtract(const Duration(seconds: 32)));
    check(p.latestUtc == at.subtract(const Duration(seconds: 28)));
    check(p.kind == JobMarkerPositionKind.clientAdmissionNotExecution && !p.clipped);
    check(p.offset == const Duration(seconds: 30) && p.uncertainty == const Duration(seconds: 2));
    check(m.admitted!.utc == at);
  });
  test('known negative skew and exact point at window edge', () {
    final c = context(
      start: at,
      end: at.add(const Duration(seconds: 30)),
      clock: measured(const Duration(seconds: -30), Duration.zero),
    );
    final p = only([row()], selected: c).position!;
    check(p.earliestUtc == c.endUtc && p.latestUtc == c.endUtc && !p.clipped);
  });
  test('uncertainty clips exact requested microsecond period, not sample extents', () {
    final c = context(
      start: at.subtract(const Duration(microseconds: 1)),
      end: at.add(const Duration(microseconds: 2)),
      clock: measured(Duration.zero, const Duration(seconds: 2)),
    );
    final p = only([row()], selected: c).position!;
    check(p.clipped && p.earliestUtc == c.startUtc && p.latestUtc == c.endUtc);
  });
  test('fully outside requested range is unpositioned, never pinned to edge', () {
    final c = context(
      start: at.add(const Duration(hours: 1)),
      end: at.add(const Duration(hours: 2)),
      clock: measured(Duration.zero, Duration.zero),
    );
    final m = only([row()], selected: c);
    check(m.position == null && m.unpositioned == JobMarkerUnpositioned.outsidePeriod);
  });
  test('clock measurement bounds reject missing, negative or enormous evidence', () {
    rejects(() => measured(null, Duration.zero));
    rejects(() => measured(Duration.zero, null));
    rejects(() => measured(const Duration(days: 8), Duration.zero));
    rejects(() => measured(Duration.zero, const Duration(microseconds: -1)));
    rejects(() => measured(Duration.zero, const Duration(minutes: 6)));
  });
  test('Runner UTC parser rejects normalization/offset/leap seconds', () {
    for (final value in [
      '2026-02-31T12:00:00Z',
      '2026-11-12T24:00:00Z',
      '2026-11-12T12:00:60Z',
      '2026-11-12T12:00:00+00:00',
      '2026-11-12T12:00:00',
      '2026-11-12T12:00:00.1234567Z',
      1,
    ]) {
      final raw = row();
      object(raw['runner_result'])['accepted_at'] = value;
      check(only([raw]).runnerQueued == null);
    }
    check(only([row()]).runnerQueued!.utc.microsecond == 456);
  });
  test('Runner evidence needs exact job/digest/version and nested uncertainty is not promoted', () {
    for (final field in ['job_id', 'manifest_digest', 'version']) {
      final value = row();
      object(value['runner_result'])[field] = field == 'version' ? 2 : 'mismatch';
      check(only([value]).runnerQueued == null);
    }
    final value = row();
    value['runner_result'] = {'reason': 'unknown', 'runner_result': value['runner_result']};
    check(only([value]).runnerUpdated == null);
  });
  test('superseded explicit read cannot replace accepted result', () {
    final c = context(), r = reader(c), old = r.begin(c)!, latest = r.begin(c)!;
    check(!r.complete(old, catalog([row()]), at));
    check(r.complete(latest, catalog([row(revision: 2, state: 'failed')]), at));
    check(!r.complete(latest, catalog([row()]), at));
    check(r.snapshot.markers.single.state == JobMetricState.failed);
  });
  test('source/host revision/range replacements reject late completion and navigation', () {
    for (final next in [
      context(sourceRevision: 4),
      context(hostRevision: 8),
      context(bindingRevision: 5),
      context(start: at),
      context(selectedHost: uuid(90)),
    ]) {
      final c = context(), r = reader(c), old = read(r, c, [row()]), pending = r.begin(c)!;
      r.bind(next);
      check(!r.complete(pending, catalog([row()]), at));
      check(r.navigation(old, old.markers.single) == null && r.snapshot.markers.isEmpty);
      check(r.begin(c) == null);
    }
  });
  test('same profile reopened lifetime rejects former ticket', () {
    final originalScope = JobCorrelationScope(profile), c = context(scope: originalScope), r = reader(c);
    final old = r.begin(c)!;
    r.bind(context(scope: JobCorrelationScope(profile)));
    check(!r.complete(old, catalog([row()]), at));
    r.bind(null);
    check(r.snapshot.context == null && r.begin(c) == null);
  });
  test('navigation is immutable original read-only target, stale marker denied', () {
    final c = context(), r = reader(c), before = read(r, c, [row()]), marker = before.markers.single;
    final target = r.navigation(before, marker)!;
    check(target.jobId == marker.jobId && target.projectId == project && target.revisionDigest == digest);
    read(r, c, [row(revision: 2, state: 'failed')]);
    check(r.navigation(before, marker) == null);
    final current = r.snapshot;
    check(r.navigation(current, current.markers.single) != null);
  });
  test('dispose clears rows/scope and rejects all former work', () {
    final c = context(), r = reader(c), snap = read(r, c, [row()]), pending = r.begin(c)!;
    r.dispose();
    r.bind(c);
    check(r.snapshot.context == null && r.snapshot.markers.isEmpty);
    check(!r.complete(pending, catalog([row()]), at) && r.begin(c) == null);
    check(r.navigation(snap, snap.markers.single) == null);
  });
  test('revision regression and conflicting equal revision do not resurrect old state', () {
    final c = context(), r = reader(c);
    read(r, c, [row(revision: 3, state: 'failed')]);
    check(read(r, c, [row(revision: 2)]).failure == JobCorrelationFailure.invalidMetadata);
    check(read(r, c, [row(revision: 3, state: 'succeeded')]).failure == JobCorrelationFailure.invalidMetadata);
    check(read(r, c, [row(revision: 4, state: 'acknowledged')]).markers.single.state == JobMetricState.acknowledged);
  });
  test('legacy missing revision distinct from malformed explicit revision', () {
    final legacy = row()..remove('notification_revision');
    check(only([legacy]).notificationRevision == 0);
    for (final value in [null, -1, 1.0, 9007199254740992]) {
      final c = context(), r = reader(c), raw = row()..['notification_revision'] = value;
      check(read(r, c, [raw]).failure == JobCorrelationFailure.invalidMetadata);
    }
  });
  test('payload/route/material never retained; source mutation cannot affect projected DTO', () {
    final value = row(), m = only([value]);
    target(value)['address'] = 'changed';
    object(value['manifest'])['revision_digest'] = 'changed';
    object(value['runner_result'])['output'] = 'changed';
    value['job_id'] = uuid(999);
    check(m.jobId == uuid(100) && m.revisionDigest == digest);
    check(!m.toString().contains('secret-sentinel') && !m.toString().contains('never-retain.example'));
  });
  test('marker limit is deterministic and explicitly truncated', () {
    final c = context(), r = reader(c), values = [for (var i = 0; i < 130; i++) row(id: 1000 + i)];
    final snap = read(r, c, values.reversed.toList());
    check(snap.markers.length == 128 && snap.truncated && snap.failure == null);
    check(snap.markers.first.jobId == uuid(1000) && snap.markers.last.jobId == uuid(1127));
    rejectsMutation(() => snap.markers.add(snap.markers.first));
  });
  test('catalog jobs and total target budgets fail closed', () {
    final c = context(), r = reader(c);
    final tooMany = [for (var i = 0; i < 2049; i++) row(id: 1000 + i)];
    check(read(r, c, tooMany).failure == JobCorrelationFailure.budget);
    final hosts = [host, for (var i = 0; i < 999; i++) uuid(30000 + i)];
    final targets = [for (var i = 0; i < 21; i++) row(id: 1000 + i, hosts: hosts)];
    check(read(r, c, targets).failure == JobCorrelationFailure.budget && r.snapshot.markers.isEmpty);
  });
  test('revision fence cardinality is bounded across reads without silent eviction', () {
    final c = context(), r = reader(c);
    final full = read(r, c, [for (var i = 0; i < 2048; i++) row(id: 1000 + i)]);
    check(full.failure == null && full.truncated);
    check(read(r, c, [row(id: 8000)]).failure == JobCorrelationFailure.budget);
    // Only an explicit lifecycle change may discard old revision fences.
    final next = context(sourceRevision: 4);
    r.bind(next);
    check(read(r, next, [row(id: 8000)]).failure == null);
  });
  test('observed local time is not silently converted to an authority timestamp', () {
    final c = context(), r = reader(c);
    check(read(r, c, [row()], observed: DateTime(2026, 11, 12)).failure == JobCorrelationFailure.invalidMetadata);
  });
  test('clock evidence outside its explicit validity period cannot position history', () {
    final clock = JobClockAlignment.measured(
      Duration.zero,
      Duration.zero,
      validFromProducerUtc: at.add(const Duration(seconds: 1)),
      validThroughProducerUtc: at.add(const Duration(minutes: 1)),
    );
    final marker = only([row()], selected: context(clock: clock));
    check(marker.position == null && marker.unpositioned == JobMarkerUnpositioned.unknownClockSkew);
    final exact = JobClockAlignment.measured(
      Duration.zero,
      Duration.zero,
      validFromProducerUtc: at,
      validThroughProducerUtc: at,
    );
    check(only([row()], selected: context(clock: exact)).position!.earliestUtc == at);
  });
  test('clock validity needs explicit ordered producer UTC bounds', () {
    rejects(
      () => JobClockAlignment.measured(
        Duration.zero,
        Duration.zero,
        validFromProducerUtc: null,
        validThroughProducerUtc: at,
      ),
    );
    rejects(
      () => JobClockAlignment.measured(
        Duration.zero,
        Duration.zero,
        validFromProducerUtc: at,
        validThroughProducerUtc: at.subtract(const Duration(seconds: 1)),
      ),
    );
    rejects(
      () => JobClockAlignment.measured(
        Duration.zero,
        Duration.zero,
        validFromProducerUtc: DateTime(2026, 11, 12),
        validThroughProducerUtc: at,
      ),
    );
  });
  test('canonical metadata and Runner timestamp reject trailing controls', () {
    rejects(() => JobCorrelationScope('$profile\n'));
    final c = context(), r = reader(c), value = row();
    object(value['manifest'])['revision_digest'] = '$digest\n';
    check(read(r, c, [value]).failure == JobCorrelationFailure.invalidMetadata);
    final time = row();
    object(time['runner_result'])['accepted_at'] = '2026-11-12T12:00:00Z\n';
    check(only([time]).runnerQueued == null);
  });
}
