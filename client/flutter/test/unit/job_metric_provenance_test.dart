import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:flutter_test/flutter_test.dart';

import 'job_metric_correlation_test.dart' as f;

Map<String, dynamic> provenance() => {
  'version': 1,
  'jobId': f.uuid(100),
  'profileId': f.profile,
  'deviceId': f.uuid(7),
  'projectId': f.project,
  'projectRevision': 9,
  'runnerId': f.uuid(6),
  'revisionObjectId': f.uuid(8),
  'revisionDigest': f.digest,
};
Map<String, dynamic> record() {
  final row = f.row()..['runner_id'] = f.uuid(6);
  (row['manifest'] as Map)['runner_id'] = f.uuid(6);
  row['correlation_provenance'] = provenance();
  return row;
}

Map<String, dynamic> receipt() => {
  'version': 1,
  'clockSource': 'runner-wall',
  'scope': 'runner-process',
  'spawnObservedUtc': '2026-11-12T12:00:01.123456Z',
  'exitObservedUtc': '2026-11-12T12:00:03.123456Z',
  'elapsedMonotonicMs': 2050,
};
Map<String, dynamic> timed() {
  final row = record();
  (row['runner_result'] as Map)['result'] = {'runner_execution': receipt()};
  return row;
}

JobMetricMarker projected(Map<String, dynamic> row, {JobCorrelationContext? context}) =>
    f.only([row], selected: context);
JobCorrelationContext clockContext(JobClockAlignment clock) {
  final c = f.context();
  return JobCorrelationContext(
    scope: c.scope,
    hostId: c.hostId,
    hostRevision: c.hostRevision,
    sourceId: c.sourceId,
    sourceRevision: c.sourceRevision,
    bindingId: c.bindingId,
    bindingRevision: c.bindingRevision,
    bindingHostId: c.hostId,
    bindingSourceId: c.sourceId,
    bindingSourceRevision: c.sourceRevision,
    bindingState: JobCorrelationBindingState.current,
    startUtc: c.startUtc,
    endUtc: c.endUtc,
    runnerClock: clock,
  );
}

void main() {
  test('exact Native9 persisted metadata retains enrolled device and uploaded object as distinct facts', () {
    final marker = projected(record()), p = projected(record()).provenance!;
    expect(marker.initiator, JobInitiator.nativeDevice);
    expect(marker.provenanceStatus, JobProvenanceStatus.available);
    expect(p.deviceId, f.uuid(7));
    expect(p.revisionObjectId, f.uuid(8));
    expect(p.projectRevision, 9);
    expect(p.jobId, f.uuid(100));
    expect(p.profileId, f.profile);
    expect(p.runnerId, f.uuid(6));
    expect(p.revisionDigest, f.digest);
    expect(p.revisionObjectId, isNot(p.revisionDigest));
    expect(marker.executionStartedUtc, isNull);
    expect(marker.executionFinishedUtc, isNull);
    expect(marker.hostRevisionAtExecution, isNull);
  });
  test('legacy absence never guesses current device review ID snapshot digest or human initiator', () {
    final row = f.row()
      ..addAll({
        'device_id': f.uuid(7),
        'review_id': f.uuid(8),
        'current_user': 'SYNTHETIC_PRIVATE',
        'revision_id': f.uuid(8),
      });
    final marker = projected(row);
    expect(marker.provenance, isNull);
    expect(marker.provenanceStatus, JobProvenanceStatus.legacyUnknown);
    expect(marker.initiator, JobInitiator.unknown);
  });
  for (final field in ['jobId', 'profileId', 'projectId', 'runnerId', 'revisionDigest']) {
    test('foreign provenance $field is unavailable without discarding approved legacy marker', () {
      final row = record(), p = provenance();
      p[field] = field == 'revisionDigest' ? 'e' * 64 : f.uuid(999);
      row['correlation_provenance'] = p;
      final marker = projected(row);
      expect(marker.provenance, isNull);
      expect(marker.provenanceStatus, JobProvenanceStatus.unavailable);
      expect(marker.jobId, f.uuid(100));
      expect(marker.initiator, JobInitiator.unknown);
    });
  }
  test('both actual row and manifest Runner identity must match the saved provenance', () {
    final row = record()..['runner_id'] = f.uuid(999);
    expect(projected(row).provenance, isNull);
    final other = record();
    (other['manifest'] as Map)['runner_id'] = f.uuid(999);
    expect(projected(other).provenanceStatus, JobProvenanceStatus.unavailable);
  });
  test('missing uploaded object has no review-ID fallback and content digest is not a snapshot object ID', () {
    final row = record()..['review_id'] = f.uuid(8), p = provenance()..remove('revisionObjectId');
    row['correlation_provenance'] = p;
    expect(projected(row).provenance, isNull);
    row['correlation_provenance'] = {...provenance(), 'revisionObjectId': f.digest};
    expect(projected(row).provenance, isNull);
  });
  test('future floating malformed and extra-field Native metadata remains typed unavailable', () {
    for (final p in [
      null,
      <String, Object?>{},
      {...provenance(), 'version': 2},
      {...provenance(), 'version': 1.0},
      {...provenance(), 'version': true},
      {...provenance(), 'accountName': 'SYNTHETIC_PRIVATE'},
      {...provenance(), 'deviceId': f.uuid(0)},
      {...provenance(), 'projectRevision': 0},
      {...provenance(), 'projectRevision': 9007199254740992},
    ]) {
      final row = record()..['correlation_provenance'] = p;
      expect(projected(row).provenanceStatus, JobProvenanceStatus.unavailable);
    }
  });
  test('project revision is recorded admission data and no current host revision is substituted', () {
    final marker = projected(record(), context: f.context(hostRevision: 999));
    expect(marker.provenance!.projectRevision, 9);
    expect(marker.hostRevisionAtExecution, isNull);
  });
  test('projected metadata is immutable after caller mutates the raw Native catalog object', () {
    final row = record(), marker = projected(row);
    (row['correlation_provenance'] as Map)['deviceId'] = f.uuid(999);
    expect(marker.provenance!.deviceId, f.uuid(7));
  });
  test('same-job same-manifest state owns process UTC and monotonic envelope observations', () {
    final m = projected(timed()), p = m.runnerObservation!;
    expect(m.runnerObservationStatus, JobRunnerObservationStatus.available);
    expect(p.spawnObservedUtc, DateTime.utc(2026, 11, 12, 12, 0, 1, 123, 456));
    expect(p.exitObservedUtc, DateTime.utc(2026, 11, 12, 12, 0, 3, 123, 456));
    expect(p.elapsedMonotonicMs, 2050);
    expect(p.hasApplicableClockMeasurement, isFalse);
    expect(p.wallClockReversed, isFalse);
    expect(m.executionStartedUtc, isNull);
    expect(m.executionFinishedUtc, isNull);
  });
  test('process receipt validates parent job and manifest digest rather than content digest', () {
    final row = timed();
    (row['runner_result'] as Map)['job_id'] = f.uuid(999);
    expect(projected(row).runnerObservation, isNull);
    final other = timed()..['manifest_digest'] = 'b' * 64;
    expect(projected(other).runnerObservationStatus, JobRunnerObservationStatus.unavailable);
    (other['runner_result'] as Map)['manifest_digest'] = 'b' * 64;
    expect(projected(other).runnerObservation, isNotNull);
  });
  test('known uncertain wrapper is explicit one-level and still validates exact inner job digest', () {
    final row = timed()..['state'] = 'unknown', state = row['runner_result'];
    row['runner_result'] = {'reason': 'native_operation_not_acknowledged', 'runner_result': state};
    expect(projected(row).runnerObservation, isNotNull);
    expect(projected(row).state, JobMetricState.unknown);
    (state as Map)['manifest_digest'] = 'f' * 64;
    expect(projected(row).runnerObservation, isNull);
  });
  test('unknown extra and recursively nested wrappers do not become a process receipt', () {
    for (final mode in [0, 1, 2]) {
      final row = timed(), state = row['runner_result'];
      final wrapper = {'reason': 'native_operation_not_acknowledged', 'runner_result': state};
      row['runner_result'] = mode == 0
          ? {...wrapper, 'reason': 'future'}
          : mode == 1
          ? {...wrapper, 'extra': true}
          : {'reason': 'native_operation_not_acknowledged', 'runner_result': wrapper};
      expect(projected(row).runnerObservationStatus, JobRunnerObservationStatus.unavailable);
    }
  });
  test('queued updated receipt received and guest output never synthesize process time', () {
    final row = record();
    (row['runner_result'] as Map)['output'] = {'runner_execution': receipt()};
    expect(projected(row).runnerObservation, isNull);
    expect(projected(row).runnerObservationStatus, JobRunnerObservationStatus.legacyUnknown);
  });
  test('future scope clock code unknown field and noninteger or negative duration fail closed', () {
    for (final p in [
      {...receipt(), 'version': 2},
      {...receipt(), 'version': 1.0},
      {...receipt(), 'scope': 'target-host'},
      {...receipt(), 'clockSource': 'client-wall'},
      {...receipt(), 'hostFinishedUtc': '2026-11-12T12:00:03Z'},
      {...receipt(), 'elapsedMonotonicMs': -1},
      {...receipt(), 'elapsedMonotonicMs': 0.0},
      {...receipt(), 'elapsedMonotonicMs': 9007199254740992},
    ]) {
      final row = timed();
      ((row['runner_result'] as Map)['result'] as Map)['runner_execution'] = p;
      expect(projected(row).runnerObservationStatus, JobRunnerObservationStatus.unavailable);
    }
  });
  test('invalid calendar naive UTC and trailing control timestamps are unavailable', () {
    for (final value in [
      '2026-02-31T12:00:01Z',
      '2026-11-12T25:00:00Z',
      '2026-11-12T12:61:00Z',
      '2026-11-12T12:00:01',
      '2026-11-12T12:00:01Z\n',
      '0000-01-01T00:00:00Z',
    ]) {
      final row = timed();
      ((row['runner_result'] as Map)['result'] as Map)['runner_execution'] = {...receipt(), 'spawnObservedUtc': value};
      expect(projected(row).runnerObservation, isNull);
    }
  });
  test('wall reversal stays factual and warns skew even if a measured offset was supplied', () {
    final row = timed();
    ((row['runner_result'] as Map)['result'] as Map)['runner_execution'] = {
      ...receipt(),
      'exitObservedUtc': '2026-11-12T11:59:59Z',
    };
    final m = projected(row, context: clockContext(f.measured(Duration.zero, Duration.zero)));
    expect(m.runnerObservation!.wallClockReversed, isTrue);
    expect(m.runnerObservation!.hasApplicableClockMeasurement, isFalse);
    expect(m.runnerObservation!.elapsedMonotonicMs, 2050);
    expect(m.executionFinishedUtc, isNull);
  });
  test('elapsed envelope versus wall difference does not infer clock skew or manufacture zero', () {
    for (final duration in [0, 1, 999999]) {
      final row = timed();
      ((row['runner_result'] as Map)['result'] as Map)['runner_execution'] = {
        ...receipt(),
        'elapsedMonotonicMs': duration,
      };
      final p = projected(row).runnerObservation!;
      expect(p.wallClockReversed, isFalse);
      expect(p.elapsedMonotonicMs, duration);
      expect(p.hasApplicableClockMeasurement, isFalse);
    }
  });
  test('only explicit applicable clock measurement marks process alignment known', () {
    final m = projected(timed(), context: clockContext(f.measured(Duration.zero, const Duration(seconds: 1))));
    expect(m.runnerObservation!.hasApplicableClockMeasurement, isTrue);
  });
  test('a process receipt never changes existing client-admission position semantics or permissions', () {
    final c = f.context(clock: f.measured(Duration.zero, Duration.zero)), r = f.reader(c);
    final snapshot = f.read(r, c, [timed()]), m = snapshot.markers.single;
    expect(m.position!.kind, JobMarkerPositionKind.clientAdmissionNotExecution);
    expect(m.position!.earliestUtc, f.at);
    expect(m.outcomeScope, JobOutcomeScope.wholeJobNotPerHost);
    expect(r.navigation(snapshot, m)!.jobId, m.jobId);
    expect(m.hostRevisionAtExecution, isNull);
  });
}
