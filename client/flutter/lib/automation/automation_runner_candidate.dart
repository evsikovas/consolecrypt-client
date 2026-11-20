import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_service.dart';

/// Display-selection data only. These paths never authorize deletion and are
/// never sent to commit: only one freshly issued Native candidate ID is planned.
class RunnerCandidateSelection {
  RunnerCandidateSelection.fromPaths(AutomationJson paths)
    : python = _path(paths['python']),
      module = _path(paths['module_dir']),
      root = _path(paths['root']);

  final String python, module, root;
  static String _path(Object? value) {
    if (value is! String || value.isEmpty || value.length > 4096 || value.runes.any((r) => r < 32 || r == 127)) {
      throw const RunnerCandidateUnavailable();
    }
    return value;
  }

  bool matches(RunnerCleanupCandidate candidate) =>
      candidate.paths?['python'] == python &&
      candidate.paths?['module_dir'] == module &&
      candidate.paths?['root'] == root;

  RunnerCleanupCandidate resolve(List<RunnerCleanupCandidate> candidates) {
    final matching = candidates.where(matches).toList(growable: false);
    if (matching.length != 1) throw const RunnerCandidateUnavailable();
    return matching.single;
  }
}

/// UI-only mismatch; never confuse a path with a Native cleanup capability.
class RunnerCandidateUnavailable implements Exception {
  const RunnerCandidateUnavailable();
}

bool runnerCleanupSameCandidate(RunnerCleanupCandidate before, RunnerCleanupCandidate after) =>
    before.id == after.id &&
    before.path == after.path &&
    before.environmentId == after.environmentId &&
    before.revision == after.revision &&
    before.receiptSha256 == after.receiptSha256 &&
    before.nodes == after.nodes &&
    before.bytes == after.bytes &&
    before.quarantined == after.quarantined &&
    before.paths?['python'] == after.paths?['python'] &&
    before.paths?['module_dir'] == after.paths?['module_dir'] &&
    before.paths?['root'] == after.paths?['root'];
