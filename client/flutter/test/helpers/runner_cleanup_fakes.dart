import 'dart:async';

import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_service.dart';

const cleanupTarget = <String, dynamic>{'kind': 'wsl', 'distribution': 'Ubuntu', 'user': 'runner'};
const cleanupId = '11111111-1111-4111-8111-111111111111';
const protectedCleanupId = '22222222-2222-4222-8222-222222222222';
const cleanupPath = '/home/runner/.local/share/consolecrypt/runner-environments/33333333-3333-4333-8333-333333333333';
const currentCleanupPython = '/home/runner/current/venv/bin/python';
const cleanupToken = 'opaque-one-use-fixture-token';
AutomationJson cleanupCandidateJson({
  String id = cleanupId,
  String path = cleanupPath,
  bool eligible = true,
  String? blockedReason,
  bool quarantined = false,
  bool omitUnsafeFields = false,
}) => {
  'id': id,
  'environmentId': '33333333-3333-4333-8333-333333333333',
  'path': path,
  if (!omitUnsafeFields) ...{
    'paths': {'python': '$path/venv/bin/python', 'module_dir': '$path/src', 'root': '$path/state'},
    'revision': 'a' * 64,
    'receiptSha256': 'b' * 64,
    'device': 1,
    'inode': 3,
    'nodes': 4000,
    'bytes': 123456,
  },
  'quarantined': quarantined,
  'eligible': eligible,
  'blockedReason': blockedReason,
  'profileOwnership': 'unknown',
  'containsState': true,
};
AutomationJson cleanupScanJson({List<AutomationJson>? candidates}) => {
  'version': 1,
  'identity': {
    'uid': 1000,
    'user': 'runner',
    'home': '/home/runner',
    'parent': '/home/runner/.local/share/consolecrypt/runner-environments',
    'device': 1,
    'inode': 2,
    'mode': 448,
  },
  'candidates':
      candidates ??
      [
        cleanupCandidateJson(),
        cleanupCandidateJson(
          id: protectedCleanupId,
          path: '/home/runner/protected',
          eligible: false,
          blockedReason: 'cleanup_registered',
        ),
      ],
  'currentProfileChecked': true,
  'otherProfilesChecked': false,
};
AutomationJson cleanupPlanJson({
  String token = cleanupToken,
  AutomationJson target = cleanupTarget,
  List<AutomationJson>? candidates,
}) => {
  'version': 1,
  'planToken': token,
  'expiresInSeconds': 60,
  'target': target,
  'candidates': candidates ?? [cleanupCandidateJson()],
  'currentProfileChecked': true,
  'otherProfilesChecked': false,
  'containsState': true,
};
AutomationJson cleanupResultJson() => {
  'version': 1,
  'removed': [
    {'id': cleanupId, 'removed': true, 'nodes': 4000, 'bytes': 123456},
  ],
  'otherProfilesChecked': false,
};

/// Synthetic UI boundary only. Never touches a real directory or invokes Native.
class RunnerCleanupFixtureService implements AutomationRunnerCleanupService {
  final scans = <(String, AutomationJson, String?)>[],
      plans = <(String, List<String>, String?)>[],
      commits = <(String, String, String?)>[];
  final cancelled = <String>[];
  AutomationRunnerCleanupFailed? scanError, commitError;
  Completer<RunnerCleanupScan>? pendingScan;
  Completer<RunnerCleanupPlan>? pendingPlan;
  Completer<RunnerCleanupResult>? pendingCommit;
  RunnerCleanupScan scanValue = RunnerCleanupScan.fromJson(cleanupScanJson());
  @override
  Future<RunnerCleanupScan> scanRunnerCleanup(
    AutomationJson target, {
    required String operationId,
    String? selectedPython,
  }) async {
    scans.add((operationId, Map.of(target), selectedPython));
    if (scanError != null) throw scanError!;
    return pendingScan?.future ?? scanValue;
  }

  @override
  Future<RunnerCleanupPlan> planRunnerCleanup(
    AutomationJson target,
    List<String> candidateIds, {
    required String operationId,
    String? selectedPython,
  }) async {
    plans.add((operationId, List.of(candidateIds), selectedPython));
    return pendingPlan?.future ?? RunnerCleanupPlan.fromJson(cleanupPlanJson(target: target));
  }

  @override
  Future<RunnerCleanupResult> commitRunnerCleanup(
    String planToken, {
    required String operationId,
    String? selectedPython,
  }) async {
    commits.add((operationId, planToken, selectedPython));
    if (commitError != null) throw commitError!;
    final result = await (pendingCommit?.future ?? Future.value(RunnerCleanupResult.fromJson(cleanupResultJson())));
    final removed = result.removed.map((r) => r.id).toSet();
    scanValue = RunnerCleanupScan(
      candidates: List.unmodifiable(scanValue.candidates.where((r) => !removed.contains(r.id))),
      identity: scanValue.identity,
    );
    return result;
  }

  @override
  Future<void> cancelRunnerCleanup(String operationId) async {
    cancelled.add(operationId);
  }
}
