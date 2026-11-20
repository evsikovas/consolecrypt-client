import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/runner_cleanup_fakes.dart';

void main() {
  test('Native cleanup errors transform only known rules under the exact cleanup field', () {
    for (final reason in RunnerCleanupReason.values.where((r) => r != RunnerCleanupReason.invalidResponse)) {
      expect(runnerCleanupNativeReason('automation_runner_cleanup', reason.wire), reason);
      expect(runnerCleanupNativeReason('automation_runner', reason.wire), isNull);
    }
    expect(runnerCleanupNativeReason('automation_runner_cleanup', 'cleanup_future_unknown'), isNull);
    expect(runnerCleanupNativeReason('automation_runner_cleanup', 'untrusted raw body'), isNull);
    expect(runnerCleanupNativeReason('automation_runner_cleanup', 'cleanup_invalid_response'), isNull);
  });
  test('cleanup transport commits only operation ID, opaque one-use token and deny-only selected Python', () async {
    final calls = <(String, AutomationJson)>[];
    final client = AutomationRunnerCleanupClient(
      rpc: (method, request) async {
        calls.add((method, request));
        return switch (method) {
          'scan' => cleanupScanJson(),
          'plan' => cleanupPlanJson(),
          'commit' => cleanupResultJson(),
          _ => throw StateError('Unexpected method'),
        };
      },
      cancel: (_) async {},
    );
    await client.scanRunnerCleanup(
      cleanupTarget,
      operationId: '11111111-aaaa-4aaa-8aaa-111111111111',
      selectedPython: currentCleanupPython,
    );
    final plan = await client.planRunnerCleanup(
      cleanupTarget,
      [cleanupId],
      operationId: '22222222-aaaa-4aaa-8aaa-222222222222',
      selectedPython: currentCleanupPython,
    );
    expect(calls.map((c) => c.$1), ['scan', 'plan']);
    expect(calls[1].$2.keys, unorderedEquals(['operationId', 'target', 'candidateIds', 'selectedPython']));
    expect(calls[1].$2['candidateIds'], [cleanupId]);
    final result = await client.commitRunnerCleanup(
      plan.token,
      operationId: '33333333-aaaa-4aaa-8aaa-333333333333',
      selectedPython: currentCleanupPython,
    );
    expect(calls.last.$2, {
      'operationId': '33333333-aaaa-4aaa-8aaa-333333333333',
      'planToken': cleanupToken,
      'selectedPython': currentCleanupPython,
    });
    expect(result.removed.single.nodes, 4000);
    expect(result.removed.single.bytes, 123456);
  });

  test('candidate IDs cannot be filesystem paths or duplicates, and mismatched Native review is rejected', () async {
    var calls = 0;
    final client = AutomationRunnerCleanupClient(
      rpc: (method, request) async {
        calls++;
        return cleanupPlanJson(target: {'kind': 'native'});
      },
      cancel: (_) async {},
    );
    for (final ids in [
      <String>[],
      [cleanupId, cleanupId],
      ['/tmp/arbitrary-path'],
    ]) {
      await expectLater(
        client.planRunnerCleanup(cleanupTarget, ids, operationId: 'fixture'),
        throwsA(isA<AutomationRunnerCleanupFailed>()),
      );
    }
    expect(calls, 0);
    await expectLater(
      client.planRunnerCleanup(cleanupTarget, [cleanupId], operationId: 'fixture'),
      throwsA(isA<AutomationRunnerCleanupFailed>()),
    );
    expect(calls, 1);
  });

  test('unsafe candidates may omit counts and paths, retain quarantine and never gain eligibility from UI data', () {
    final scan = RunnerCleanupScan.fromJson(
      cleanupScanJson(
        candidates: [
          cleanupCandidateJson(
            eligible: false,
            blockedReason: 'cleanup_invalid_receipt',
            omitUnsafeFields: true,
            quarantined: true,
          ),
        ],
      ),
    );
    final candidate = scan.candidates.single;
    expect(candidate.eligible, false);
    expect(candidate.nodes, isNull);
    expect(candidate.bytes, isNull);
    expect(candidate.paths, isNull);
    expect(candidate.quarantined, true);
    expect(candidate.blockedReason, RunnerCleanupReason.invalidReceipt);
    final unknown = RunnerCleanupScan.fromJson(
      cleanupScanJson(
        candidates: [
          cleanupCandidateJson(
            eligible: false,
            blockedReason: 'Untrusted body with synthetic-secret',
            omitUnsafeFields: true,
          ),
        ],
      ),
    );
    expect(unknown.candidates.single.blockedReason, RunnerCleanupReason.unsafeEntry);
  });

  test('review requires exact Native disclosure flags, bounded lifetime, eligible records and known version', () {
    final malformed = <AutomationJson>[
      {...cleanupPlanJson(), 'version': 2},
      {...cleanupPlanJson(), 'planToken': ''},
      {...cleanupPlanJson(), 'expiresInSeconds': 600},
      {...cleanupPlanJson(), 'currentProfileChecked': false},
      {...cleanupPlanJson(), 'otherProfilesChecked': true},
      {...cleanupPlanJson(), 'containsState': false},
      cleanupPlanJson(candidates: [cleanupCandidateJson(eligible: false, blockedReason: 'cleanup_selected')]),
      cleanupPlanJson(candidates: [cleanupCandidateJson(omitUnsafeFields: true)]),
    ];
    for (final value in malformed) {
      expect(
        () => RunnerCleanupPlan.fromJson(value),
        throwsA(
          isA<AutomationRunnerCleanupFailed>().having((e) => e.reason, 'reason', RunnerCleanupReason.invalidResponse),
        ),
      );
    }
    final valid = RunnerCleanupPlan.fromJson(cleanupPlanJson());
    expect(valid.candidates.single.path, cleanupPath);
    expect(valid.candidates.single.paths!['root'], '$cleanupPath/state');
    expect(valid.expiresInSeconds, 60);
  });

  test('result keeps true zero and exact counters; missing counters and false removal never become success', () {
    final zero = {
      ...cleanupResultJson(),
      'removed': [
        {'id': cleanupId, 'removed': true, 'nodes': 0, 'bytes': 0},
      ],
    };
    expect(RunnerCleanupResult.fromJson(zero).removed.single.bytes, 0);
    for (final removed in [
      [
        {'id': cleanupId, 'removed': false, 'nodes': 0, 'bytes': 0},
      ],
      [
        {'id': cleanupId, 'removed': true, 'nodes': 3},
      ],
    ]) {
      expect(
        () => RunnerCleanupResult.fromJson({...cleanupResultJson(), 'removed': removed}),
        throwsA(isA<AutomationRunnerCleanupFailed>()),
      );
    }
  });
}
