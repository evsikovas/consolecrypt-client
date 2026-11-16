import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';

enum RunnerCleanupReason {
  currentInterpreter('cleanup_current_interpreter'),
  unsafePath('cleanup_unsafe_path'),
  cancelled('cleanup_cancelled'),
  unsupported('cleanup_unsupported'),
  nonRoot('cleanup_non_root_required'),
  unsafeHome('cleanup_unsafe_home'),
  unsafeEntry('cleanup_unsafe_entry'),
  mountBoundary('cleanup_mount_boundary'),
  ownerMode('cleanup_unsafe_owner_mode'),
  specialFile('cleanup_special_file'),
  invalidReceipt('cleanup_invalid_receipt'),
  budget('cleanup_budget'),
  invalidRequest('cleanup_invalid_request'),
  invalidId('cleanup_invalid_id'),
  identityChanged('cleanup_identity_changed'),
  candidateChanged('cleanup_candidate_changed'),
  receiptChanged('cleanup_receipt_changed'),
  treeChanged('cleanup_tree_changed'),
  quarantineFailed('cleanup_quarantine_failed'),
  quarantineChanged('cleanup_quarantine_changed'),
  ambiguousId('cleanup_ambiguous_id'),
  expired('cleanup_plan_expired'),
  contextChanged('cleanup_context_changed'),
  selected('cleanup_selected'),
  registered('cleanup_registered'),
  busy('cleanup_busy'),
  catalogUnavailable('cleanup_catalog_unavailable'),
  failed('cleanup_failed'),
  bootstrapFailed('cleanup_bootstrap_failed'),
  commandTooLarge('cleanup_command_too_large'),
  timeout('cleanup_timeout'),
  invalidResponse('cleanup_invalid_response');

  const RunnerCleanupReason(this.wire);
  final String wire;
  static RunnerCleanupReason? parse(String? value) => values.where((r) => r.wire == value).firstOrNull;
}

class AutomationRunnerCleanupFailed implements Exception {
  const AutomationRunnerCleanupFailed(this.reason);
  final RunnerCleanupReason reason;
}

RunnerCleanupReason? runnerCleanupNativeReason(String? field, String? rule) {
  if (field != 'automation_runner_cleanup') return null;
  final reason = RunnerCleanupReason.parse(rule);
  return reason == RunnerCleanupReason.invalidResponse ? null : reason;
}

Never _invalidCleanup() => throw const AutomationRunnerCleanupFailed(RunnerCleanupReason.invalidResponse);
bool _uuid(String value) =>
    RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(value) &&
    value != '00000000-0000-0000-0000-000000000000';
AutomationJson _map(Object? value) => Map<String, dynamic>.from(value as Map);
int _count(Object? value) {
  if (value is! int || value < 0) _invalidCleanup();
  return value;
}

void _version(AutomationJson value) {
  if (value['version'] is! int || value['version'] != 1) _invalidCleanup();
}

void _profileNotice(AutomationJson value) {
  if (value['currentProfileChecked'] != true || value['otherProfilesChecked'] != false) _invalidCleanup();
}

String runnerCleanupContext(AutomationJson target, String? selectedPython) {
  final keys = target.keys.toList()..sort();
  return jsonEncode({
    'target': {for (final key in keys) key: target[key]},
    'selectedPython': selectedPython,
  });
}

class RunnerCleanupCandidate {
  RunnerCleanupCandidate._({
    required this.id,
    required this.path,
    required this.eligible,
    required this.quarantined,
    this.environmentId,
    this.revision,
    this.receiptSha256,
    this.nodes,
    this.bytes,
    this.paths,
    this.blockedReason,
  });
  final String id, path;
  final String? environmentId, revision, receiptSha256;
  final bool eligible, quarantined;
  final int? nodes, bytes;
  final Map<String, String>? paths;
  final RunnerCleanupReason? blockedReason;
  factory RunnerCleanupCandidate.fromJson(AutomationJson row) {
    final id = row['id'] as String, path = row['path'] as String;
    final eligible = row['eligible'] as bool, quarantined = row['quarantined'] as bool;
    if (!_uuid(id) || path.isEmpty || path.length > 4096) _invalidCleanup();
    final paths = row['paths'] == null ? null : Map<String, String>.from(row['paths'] as Map);
    final nodes = row['nodes'] == null ? null : _count(row['nodes']),
        bytes = row['bytes'] == null ? null : _count(row['bytes']);
    if (eligible &&
        (paths == null ||
            !['python', 'module_dir', 'root'].every((key) => (paths[key]?.isNotEmpty ?? false)) ||
            nodes == null ||
            bytes == null ||
            row['revision'] is! String ||
            row['receiptSha256'] is! String ||
            !RegExp(r'^[0-9a-f]{64}$').hasMatch(row['revision'] as String) ||
            !RegExp(r'^[0-9a-f]{64}$').hasMatch(row['receiptSha256'] as String) ||
            row['environmentId'] is! String ||
            !_uuid(row['environmentId'] as String) ||
            row['containsState'] != true ||
            row['profileOwnership'] != 'unknown' ||
            row['blockedReason'] != null)) {
      _invalidCleanup();
    }
    return RunnerCleanupCandidate._(
      id: id,
      path: path,
      eligible: eligible,
      quarantined: quarantined,
      environmentId: row['environmentId'] as String?,
      revision: row['revision'] as String?,
      receiptSha256: row['receiptSha256'] as String?,
      nodes: nodes,
      bytes: bytes,
      paths: paths == null ? null : Map.unmodifiable(paths),
      blockedReason:
          RunnerCleanupReason.parse(row['blockedReason'] as String?) ??
          (eligible ? null : RunnerCleanupReason.unsafeEntry),
    );
  }
}

List<RunnerCleanupCandidate> _candidates(Object? rows) {
  final result = (rows as List).map((row) => RunnerCleanupCandidate.fromJson(_map(row))).toList();
  if (result.length > 256 || result.map((r) => r.id).toSet().length != result.length) _invalidCleanup();
  return List.unmodifiable(result);
}

class RunnerCleanupScan {
  const RunnerCleanupScan({required this.candidates, this.identity});
  final List<RunnerCleanupCandidate> candidates;
  final AutomationJson? identity;
  factory RunnerCleanupScan.fromJson(AutomationJson json) {
    try {
      _version(json);
      _profileNotice(json);
      return RunnerCleanupScan(
        candidates: _candidates(json['candidates']),
        identity: json['identity'] == null ? null : Map.unmodifiable(_map(json['identity'])),
      );
    } catch (_) {
      _invalidCleanup();
    }
  }
}

class RunnerCleanupPlan {
  const RunnerCleanupPlan._({
    required this.token,
    required this.candidates,
    required this.target,
    required this.expiresInSeconds,
  });
  final String token;
  final List<RunnerCleanupCandidate> candidates;
  final AutomationJson target;
  final int expiresInSeconds;
  factory RunnerCleanupPlan.fromJson(AutomationJson json) {
    try {
      _version(json);
      _profileNotice(json);
      final token = json['planToken'] as String, candidates = _candidates(json['candidates']);
      if (token.isEmpty ||
          token.length > 4096 ||
          json['expiresInSeconds'] != 60 ||
          json['containsState'] != true ||
          candidates.isEmpty ||
          candidates.length > 8 ||
          candidates.any((c) => !c.eligible)) {
        _invalidCleanup();
      }
      return RunnerCleanupPlan._(
        token: token,
        candidates: candidates,
        target: Map.unmodifiable(_map(json['target'])),
        expiresInSeconds: 60,
      );
    } catch (_) {
      _invalidCleanup();
    }
  }
}

class RunnerCleanupRemoved {
  const RunnerCleanupRemoved({required this.id, required this.nodes, required this.bytes});
  final String id;
  final int nodes, bytes;
}

class RunnerCleanupResult {
  const RunnerCleanupResult(this.removed);
  final List<RunnerCleanupRemoved> removed;
  factory RunnerCleanupResult.fromJson(AutomationJson json) {
    try {
      _version(json);
      if (json['otherProfilesChecked'] != false) _invalidCleanup();
      final removed = (json['removed'] as List).map((value) {
        final row = _map(value);
        if (row['removed'] != true) _invalidCleanup();
        return RunnerCleanupRemoved(id: row['id'] as String, nodes: _count(row['nodes']), bytes: _count(row['bytes']));
      }).toList();
      if (removed.isEmpty || removed.length > 8 || removed.map((r) => r.id).toSet().length != removed.length) {
        _invalidCleanup();
      }
      return RunnerCleanupResult(List.unmodifiable(removed));
    } catch (_) {
      _invalidCleanup();
    }
  }
}

abstract interface class AutomationRunnerCleanupService {
  Future<RunnerCleanupScan> scanRunnerCleanup(
    AutomationJson target, {
    required String operationId,
    String? selectedPython,
  });
  Future<RunnerCleanupPlan> planRunnerCleanup(
    AutomationJson target,
    List<String> candidateIds, {
    required String operationId,
    String? selectedPython,
  });
  Future<RunnerCleanupResult> commitRunnerCleanup(
    String planToken, {
    required String operationId,
    String? selectedPython,
  });
  Future<void> cancelRunnerCleanup(String operationId);
}

typedef RunnerCleanupRpc = Future<AutomationJson> Function(String method, AutomationJson request);

/// Typed RPC seam. Commit accepts no target, path, IDs, receipt or UI verdict.
class AutomationRunnerCleanupClient implements AutomationRunnerCleanupService {
  const AutomationRunnerCleanupClient({required this.rpc, required this.cancel});
  final RunnerCleanupRpc rpc;
  final Future<void> Function(String operationId) cancel;
  @override
  Future<RunnerCleanupScan> scanRunnerCleanup(
    AutomationJson target, {
    required String operationId,
    String? selectedPython,
  }) async => RunnerCleanupScan.fromJson(
    await rpc('scan', {'operationId': operationId, 'target': target, 'selectedPython': selectedPython}),
  );
  @override
  Future<RunnerCleanupPlan> planRunnerCleanup(
    AutomationJson target,
    List<String> candidateIds, {
    required String operationId,
    String? selectedPython,
  }) async {
    if (candidateIds.isEmpty ||
        candidateIds.length > 8 ||
        candidateIds.toSet().length != candidateIds.length ||
        candidateIds.any((id) => !_uuid(id))) {
      throw const AutomationRunnerCleanupFailed(RunnerCleanupReason.invalidRequest);
    }
    final plan = RunnerCleanupPlan.fromJson(
      await rpc('plan', {
        'operationId': operationId,
        'target': target,
        'candidateIds': candidateIds,
        'selectedPython': selectedPython,
      }),
    );
    if (runnerCleanupContext(plan.target, selectedPython) != runnerCleanupContext(target, selectedPython) ||
        plan.candidates.length != candidateIds.length ||
        !plan.candidates.every((c) => candidateIds.contains(c.id))) {
      _invalidCleanup();
    }
    return plan;
  }

  @override
  Future<RunnerCleanupResult> commitRunnerCleanup(
    String planToken, {
    required String operationId,
    String? selectedPython,
  }) async => RunnerCleanupResult.fromJson(
    await rpc('commit', {'operationId': operationId, 'planToken': planToken, 'selectedPython': selectedPython}),
  );
  @override
  Future<void> cancelRunnerCleanup(String operationId) => cancel(operationId);
}
