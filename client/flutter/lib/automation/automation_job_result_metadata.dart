import 'package:consolecrypt/automation/automation_service.dart';

/// Closed display projection only. Native must separately protect persistence.
/// No arbitrary values, fields, summaries or diagnostics are recursively copied.
final class AutomationResultMetadata {
  const AutomationResultMetadata(this.facts, {required this.recognized});
  final Map<String, Object?> facts;
  final bool recognized;
}

AutomationJson? _map(Object? v) => v is AutomationJson ? v : null;
AutomationJson? _visible(Object? v) {
  final m = _map(v);
  if (m == null) return null;
  if (m.containsKey('_ansible_no_log') && m['_ansible_no_log'] != false ||
      m.containsKey('censored') && m['censored'] != false) {
    return null;
  }
  return m;
}

/// Fixed no_log/censored gate shared by the existing recorded-result fields.
AutomationJson? automationVisibleResultMap(Object? v) => _visible(v);
const _states = {
  'queued',
  'dispatching',
  'running',
  'cancelling',
  'succeeded',
  'failed',
  'cancelled',
  'interrupted',
  'unknown',
  'acknowledged',
};
const _reasons = {
  'control_channel_or_storage_lost',
  'native_operation_not_acknowledged',
  'approval_expired_in_queue',
  'vault_material_unavailable_before_dispatch',
  'vault_material_expired_before_dispatch',
  'preflight_changed',
  'executor_lost',
};
const _failures = {
  'owner_closed',
  'observer_deadline',
  'unexpected_runner_phase',
  'runner_binding_mismatch',
  'rpc_timeout',
  'rpc_closed',
  'rpc_queue_full',
  'rpc_protocol',
  'rpc_remote',
  'rpc_io',
  'rpc_invalid',
};
bool _integer(Object? v, int min, int max) => v is int && v >= min && v <= max;
// Same closed public domains as Native result_projection.rs::environment.
String? _version(Object? value) =>
    value is String && value.length <= 32 && RegExp(r'^[0-9]{1,8}(?:\.[0-9]{1,8}){1,3}$').hasMatch(value)
    ? value
    : null;

/// Shared by both the technical JSON and the existing Environment expansion.
AutomationJson? automationClosedEnvironmentIdentity(Object? value) {
  final identity = _visible(value);
  if (identity == null) return null;
  final kept = <String, dynamic>{};
  if (const {'Linux', 'Darwin', 'Windows'}.contains(identity['system'])) kept['system'] = identity['system'];
  if (const {'x86_64', 'aarch64', 'arm64', 'AMD64'}.contains(identity['machine'])) {
    kept['machine'] = identity['machine'];
  }
  for (final key in ['python', 'helper']) {
    final version = _version(identity[key]);
    if (version != null) kept[key] = version;
  }
  final packages = _visible(identity['packages']);
  if (packages != null) {
    final selected = <String, dynamic>{};
    for (final key in ['ansible-core', 'ansible-runner', 'ansible-lint']) {
      final version = _version(_visible(packages[key])?['actual']);
      if (version != null) selected[key] = {'actual': version};
    }
    if (selected.isNotEmpty) kept['packages'] = selected;
  }
  return kept.isEmpty ? null : kept;
}

bool _sha(Object? v) => v is String && RegExp(r'^[0-9a-f]{64}$').hasMatch(v);
bool _utc(Object? v) {
  if (v is! String || v.length > 64) return false;
  final m = RegExp(r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$').firstMatch(v),
      d = DateTime.tryParse(v);
  return m != null &&
      d != null &&
      d.isUtc &&
      d.year == int.parse(m[1]!) &&
      d.month == int.parse(m[2]!) &&
      d.day == int.parse(m[3]!) &&
      d.hour == int.parse(m[4]!) &&
      d.minute == int.parse(m[5]!) &&
      d.second == int.parse(m[6]!);
}

Map<String, Object?>? _interval(Object? value) {
  final m = _visible(value);
  const fields = {'version', 'clockSource', 'scope', 'spawnObservedUtc', 'exitObservedUtc', 'elapsedMonotonicMs'};
  if (m == null ||
      m.length != fields.length ||
      !m.keys.every(fields.contains) ||
      m['version'] is! int ||
      m['version'] != 1 ||
      m['clockSource'] != 'runner-wall' ||
      m['scope'] != 'runner-process' ||
      !_utc(m['spawnObservedUtc']) ||
      !_utc(m['exitObservedUtc']) ||
      !_integer(m['elapsedMonotonicMs'], 0, 9007199254740991)) {
    return null;
  }
  return {for (final k in fields) k: m[k] as Object?};
}

AutomationResultMetadata automationResultMetadata(AutomationJson job) {
  final facts = <String, Object?>{}, native = <String, Object?>{};
  if (_states.contains(job['state'])) native['state'] = job['state'];
  for (final k in ['accepted_at', 'updated_at']) {
    if (_integer(job[k], 1, 253402300799)) {
      native[k] = job[k];
    }
  }
  if (job['runner_result_redacted'] is bool) native['runner_result_redacted'] = job['runner_result_redacted'];
  if (_sha(job['runner_result_original_sha256'])) {
    native['original_evidence_sha256'] = job['runner_result_original_sha256'];
  }
  if (native.isNotEmpty) facts['native'] = native;
  var raw = _visible(job['runner_result']);
  if (raw == null) return AutomationResultMetadata(facts, recognized: false);
  // Exactly the existing Native uncertainty wrapper; no recursive state hunt.
  if (raw.length == 2 && raw['reason'] == 'native_operation_not_acknowledged' && raw.containsKey('runner_result')) {
    facts['native_observation'] = {'reason': 'native_operation_not_acknowledged'};
    raw = _visible(raw['runner_result']);
  }
  if (raw == null) return AutomationResultMetadata(facts, recognized: false);
  if (_reasons.contains(raw['reason'])) {
    final observed = <String, Object?>{'reason': raw['reason']};
    if (_failures.contains(raw['control_failure'])) observed['control_failure'] = raw['control_failure'];
    for (final key in ['remote_changes_may_be_partial', 'cancel_requested', 'timed_out']) {
      if (raw[key] is bool) {
        observed[key] = raw[key];
      }
    }
    facts['control_observation'] = observed;
    return AutomationResultMetadata(facts, recognized: true);
  }
  if (raw['version'] is! int ||
      raw['version'] != 1 ||
      raw['job_id'] != job['job_id'] ||
      !_sha(job['manifest_digest']) ||
      raw['manifest_digest'] != job['manifest_digest']) {
    return AutomationResultMetadata(facts, recognized: false);
  }
  final runner = <String, Object?>{'version': 1, 'job_id': raw['job_id'], 'manifest_digest': raw['manifest_digest']};
  if (_states.contains(raw['state'])) runner['state'] = raw['state'];
  for (final k in ['sequence', 'output_bytes']) {
    if (_integer(raw[k], 0, 9007199254740991)) {
      runner[k] = raw[k];
    }
  }
  if (raw['output_truncated'] is bool) runner['output_truncated'] = raw['output_truncated'];
  for (final k in ['accepted_at', 'updated_at']) {
    if (_utc(raw[k])) {
      runner[k] = raw[k];
    }
  }
  final result = _visible(raw['result']);
  if (result != null) {
    final kept = <String, Object?>{};
    if (_reasons.contains(result['reason'])) kept['reason'] = result['reason'];
    if (_integer(result['exit_code'], -2147483648, 2147483647)) kept['exit_code'] = result['exit_code'];
    for (final k in ['remote_changes_may_be_partial', 'cancel_requested', 'timed_out']) {
      if (result[k] is bool) {
        kept[k] = result[k];
      }
    }
    if (_integer(result['elapsed_ms'], 0, 9007199254740991)) kept['elapsed_ms'] = result['elapsed_ms'];
    final interval = _interval(result['runner_execution']);
    if (interval != null) kept['runner_execution'] = interval;
    final summary = _visible(result['summary']);
    if (summary != null) {
      final s = <String, Object?>{};
      if (const {'successful', 'failed', 'canceled', 'timeout'}.contains(summary['status'])) {
        s['status'] = summary['status'];
      }
      if (_integer(summary['exit_code'], -2147483648, 2147483647)) s['exit_code'] = summary['exit_code'];
      if (s.isNotEmpty) kept['summary'] = s;
    }
    if (kept.isNotEmpty) runner['result'] = kept;
  }
  final environment = _visible(raw['environment']), manifest = _map(job['manifest']);
  if (environment != null &&
      environment['version'] is int &&
      environment['version'] == 1 &&
      _sha(environment['environment_digest']) &&
      environment['environment_digest'] == manifest?['environment_digest']) {
    final kept = <String, Object?>{'version': 1, 'environment_digest': environment['environment_digest']};
    final identity = automationClosedEnvironmentIdentity(environment['identity']);
    if (identity != null) kept['identity'] = identity;
    runner['environment'] = kept;
  }
  facts['runner'] = runner;
  return AutomationResultMetadata(facts, recognized: true);
}
