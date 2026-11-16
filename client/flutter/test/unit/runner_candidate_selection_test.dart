import 'package:consolecrypt/automation/automation_runner_candidate.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/runner_cleanup_fakes.dart';

void main() {
  final paths = Map<String, dynamic>.from(cleanupCandidateJson()['paths'] as Map);
  test('discovery paths select only one fresh Native triple, never a directory guess', () {
    final selected = RunnerCandidateSelection.fromPaths(paths);
    final candidate = RunnerCleanupCandidate.fromJson(cleanupCandidateJson());
    expect(selected.resolve([candidate]).id, cleanupId);
    expect(() => selected.resolve([]), throwsA(isA<RunnerCandidateUnavailable>()));
    final differentModule = cleanupCandidateJson(id: protectedCleanupId);
    (differentModule['paths'] as Map)['module_dir'] = '/different/module';
    final other = RunnerCleanupCandidate.fromJson(differentModule);
    expect(selected.resolve([other, candidate]).id, cleanupId);
    expect(() => selected.resolve([other]), throwsA(isA<RunnerCandidateUnavailable>()));
  });
  test('duplicate matching triples reject instead of choosing the first candidate', () {
    final selected = RunnerCandidateSelection.fromPaths(paths);
    expect(
      () => selected.resolve([
        RunnerCleanupCandidate.fromJson(cleanupCandidateJson()),
        RunnerCleanupCandidate.fromJson(cleanupCandidateJson(id: protectedCleanupId)),
      ]),
      throwsA(isA<RunnerCandidateUnavailable>()),
    );
  });
  test('protected Native row without paths remains unresolvable and never becomes authority', () {
    final protected = RunnerCleanupCandidate.fromJson(
      cleanupCandidateJson(eligible: false, blockedReason: 'cleanup_registered', omitUnsafeFields: true),
    );
    expect(
      () => RunnerCandidateSelection.fromPaths(paths).resolve([protected]),
      throwsA(isA<RunnerCandidateUnavailable>()),
    );
    expect(protected.blockedReason, RunnerCleanupReason.registered);
  });
  test('missing, oversized or control-character discovery metadata fails closed', () {
    for (final value in [null, '', 'a' * 4097, '/home/a\n/python', '/home/a\u007f/python']) {
      expect(
        () => RunnerCandidateSelection.fromPaths({...paths, 'python': value}),
        throwsA(isA<RunnerCandidateUnavailable>()),
      );
    }
  });
  test('scan to plan comparison includes Native receipt, revision, path and counters', () {
    final original = RunnerCleanupCandidate.fromJson(cleanupCandidateJson());
    expect(runnerCleanupSameCandidate(original, RunnerCleanupCandidate.fromJson(cleanupCandidateJson())), isTrue);
    for (final entry in {
      'revision': 'c' * 64,
      'receiptSha256': 'd' * 64,
      'nodes': 4001,
      'bytes': 123457,
      'quarantined': true,
    }.entries) {
      expect(
        runnerCleanupSameCandidate(
          original,
          RunnerCleanupCandidate.fromJson({...cleanupCandidateJson(), entry.key: entry.value}),
        ),
        isFalse,
      );
    }
  });
}
