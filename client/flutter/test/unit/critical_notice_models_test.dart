import 'package:consolecrypt/app/critical_notice_models.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
String id(int index) => '00000000-0000-4000-8000-${index.toRadixString(16).padLeft(12, '0')}';
JobNoticeChange changed(int job, int revision, JobNoticeState state) =>
    JobNoticeChange(profileId: profile, jobId: id(job), revision: revision, state: state);
JobNoticeFrame frame(JobNoticeChange change, {int epoch = 1}) =>
    JobNoticeFrame(epoch: epoch, profileId: profile, change: change);
void main() {
  test('real Native terminal revision creates exactly one unread item; duplicate/out-of-order cannot regress it', () {
    final scope = Object(), ring = CriticalJobNoticeRing(scope: Object(), profileId: profile);
    final owned = CriticalJobNoticeRing(scope: scope, profileId: profile), now = DateTime.utc(2026, 11, 12);
    owned.ingest(scope, frame(changed(1, 3, JobNoticeState.failed)), now);
    owned.ingest(scope, frame(changed(1, 2, JobNoticeState.running)), now);
    owned.ingest(scope, frame(changed(1, 3, JobNoticeState.failed)), now.add(const Duration(minutes: 1)));
    expect(owned.rows, hasLength(1));
    expect(owned.unread, 1);
    expect(owned.rows.single.change.revision, 3);
    expect(owned.rows.single.observedUtc, now);
    expect(owned.rows.single.severity, CriticalNoticeSeverity.danger);
    ring.ingest(scope, frame(changed(1, 1, JobNoticeState.succeeded)), now);
    expect(ring.rows, isEmpty);
  });
  test('queued/running/cancelling track revisions without inventing completion; acknowledgement resolves same row', () {
    final scope = Object(), now = DateTime.utc(2026), ring = CriticalJobNoticeRing(scope: Object(), profileId: profile);
    final owned = CriticalJobNoticeRing(scope: scope, profileId: profile);
    for (final pair in [(1, JobNoticeState.queued), (2, JobNoticeState.running), (3, JobNoticeState.cancelling)]) {
      owned.ingest(scope, frame(changed(1, pair.$1, pair.$2)), now);
    }
    expect(owned.rows, isEmpty);
    expect(owned.rememberedRecords, 1);
    owned.ingest(scope, frame(changed(1, 4, JobNoticeState.unknown)), now);
    expect(owned.unread, 1);
    owned.ingest(scope, frame(changed(1, 5, JobNoticeState.acknowledged)), now);
    expect(owned.rows, hasLength(1));
    expect(owned.unread, 0);
    expect(owned.rows.single.change.state, JobNoticeState.acknowledged);
    ring.dispose();
    ring.ingest(scope, frame(changed(1, 5, JobNoticeState.failed)), now);
    expect(ring.rows, isEmpty);
  });
  test('same-revision conflicting state marks refresh-needed rather than choosing an invented order', () {
    final scope = Object(), now = DateTime.utc(2026);
    final ring = CriticalJobNoticeRing(scope: scope, profileId: profile);
    ring.ingest(scope, frame(changed(1, 2, JobNoticeState.failed)), now);
    ring.ingest(scope, frame(changed(1, 2, JobNoticeState.succeeded)), now);
    expect(ring.refreshNeeded, isTrue);
    expect(ring.rows.single.change.state, JobNoticeState.failed);
  });
  test('lock/reset blocks old events until fresh original-profile scope; old epoch cannot return after unlock', () {
    final scope = Object(), now = DateTime.utc(2026);
    final ring = CriticalJobNoticeRing(scope: scope, profileId: profile);
    ring.ingest(scope, frame(changed(1, 1, JobNoticeState.failed)), now);
    ring.ingest(scope, const JobNoticeFrame(epoch: 2, reset: true), now);
    ring.ingest(scope, frame(changed(1, 2, JobNoticeState.succeeded), epoch: 2), now);
    expect(ring.rows, isEmpty);
    ring.ingest(scope, const JobNoticeFrame(epoch: 3, profileId: profile, reset: true), now);
    ring.ingest(scope, frame(changed(1, 99, JobNoticeState.failed), epoch: 1), now);
    expect(ring.rows, isEmpty);
    ring.ingest(scope, frame(changed(1, 3, JobNoticeState.succeeded), epoch: 3), now);
    expect(ring.rows.single.change.state, JobNoticeState.succeeded);
  });
  test('lagged says refresh needed with no fake completion; only original-gap refresh clears it', () {
    final scope = Object(), now = DateTime.utc(2026);
    final ring = CriticalJobNoticeRing(scope: scope, profileId: profile);
    ring.ingest(scope, const JobNoticeFrame(epoch: 1, profileId: profile, refreshNeeded: true), now);
    expect(ring.rows, isEmpty);
    expect(ring.refreshNeeded, isTrue);
    final oldGap = ring.gapVersion;
    ring.ingest(scope, const JobNoticeFrame(epoch: 1, profileId: profile, refreshNeeded: true), now);
    expect(
      ring.reconcile(
        scope,
        originalEpoch: 1,
        originalGapVersion: oldGap,
        changes: [changed(1, 1, JobNoticeState.failed)],
        observed: now,
      ),
      isTrue,
    );
    expect(ring.refreshNeeded, isTrue);
    expect(ring.rows.single.unread, isFalse);
    expect(
      ring.reconcile(
        scope,
        originalEpoch: 1,
        originalGapVersion: ring.gapVersion,
        changes: [changed(1, 1, JobNoticeState.failed)],
        observed: now,
      ),
      isTrue,
    );
    expect(ring.refreshNeeded, isFalse);
  });
  test(
    'explicit legacy-zero snapshot is current metadata not a new completion; higher Native event remains unread',
    () {
      final scope = Object(), now = DateTime.utc(2026);
      final ring = CriticalJobNoticeRing(scope: scope, profileId: profile);
      expect(
        ring.reconcile(
          scope,
          originalEpoch: null,
          originalGapVersion: 0,
          changes: [changed(1, 0, JobNoticeState.failed)],
          observed: now,
        ),
        isTrue,
      );
      expect(ring.rows.single.unread, isFalse);
      ring.ingest(scope, frame(changed(1, 1, JobNoticeState.succeeded)), now);
      expect(ring.unread, 1);
      ring.markRead(scope, ring.rows.single.id);
      expect(ring.unread, 0);
      ring.dismiss(scope, ring.rows.single.id);
      expect(ring.rows, isEmpty);
      ring.ingest(scope, frame(changed(1, 1, JobNoticeState.succeeded)), now);
      expect(ring.rows, isEmpty);
    },
  );
  test('bounded rows retain record revisions so eviction cannot resurrect a delayed old notice', () {
    final scope = Object(), now = DateTime.utc(2026);
    final ring = CriticalJobNoticeRing(scope: scope, profileId: profile);
    for (var i = 1; i <= 65; i++) {
      ring.ingest(scope, frame(changed(i, 1, JobNoticeState.failed)), now);
    }
    expect(ring.rows, hasLength(64));
    expect(ring.rememberedRecords, 65);
    expect(ring.rows.any((row) => row.change.jobId == id(1)), isFalse);
    ring.ingest(scope, frame(changed(1, 1, JobNoticeState.failed)), now);
    expect(ring.rows.any((row) => row.change.jobId == id(1)), isFalse);
    for (var i = 66; i <= 2049; i++) {
      ring.ingest(scope, frame(changed(i, 1, JobNoticeState.failed)), now);
    }
    expect(ring.rememberedRecords, 2048);
    expect(ring.rows, hasLength(64));
    expect(ring.refreshNeeded, isTrue);
  });
  test('scope/epoch changes reject pending reconciliation and dispose removes all profile metadata', () {
    final scope = Object(), now = DateTime.utc(2026);
    final ring = CriticalJobNoticeRing(scope: scope, profileId: profile);
    ring.ingest(scope, frame(changed(1, 1, JobNoticeState.failed)), now);
    expect(
      ring.reconcile(
        Object(),
        originalEpoch: 1,
        originalGapVersion: 0,
        changes: [changed(1, 2, JobNoticeState.succeeded)],
        observed: now,
      ),
      isFalse,
    );
    ring.ingest(scope, const JobNoticeFrame(epoch: 2, reset: true), now);
    expect(
      ring.reconcile(
        scope,
        originalEpoch: 1,
        originalGapVersion: 0,
        changes: [changed(1, 2, JobNoticeState.succeeded)],
        observed: now,
      ),
      isFalse,
    );
    ring.dispose();
    expect(ring.rows, isEmpty);
    expect(ring.rememberedRecords, 0);
  });
}
