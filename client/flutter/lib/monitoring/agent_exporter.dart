import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_exporter_policy.dart';

enum AgentExporterPhase { exporterReview, starting, running, stopped, unknown, cancelled }

/// A historical observation from this original operation, not a health probe.
final class AgentExporterView {
  const AgentExporterView({
    required this.phase,
    required this.policy,
    required this.manifestSha256,
    required this.originalExpiresUtc,
    required this.reviewRemainingMs,
    required this.leaseHash,
    required this.runtimeObserved,
    required this.actorAdmission,
    required this.cleanupState,
  });
  final AgentExporterPhase phase;
  final AgentExporterPolicy policy;
  final String manifestSha256, cleanupState;
  final DateTime originalExpiresUtc;
  final int? reviewRemainingMs;
  final String? leaseHash;
  final bool runtimeObserved, actorAdmission;
  String? get runningVersion => null;
}

final class AgentExporterCatalog {
  const AgentExporterCatalog(this.policy, this.manifestSha256, this.cleanupState);
  final AgentExporterPolicy policy;
  final String manifestSha256, cleanupState;
}

final class AgentExporterCleanup {
  const AgentExporterCleanup(this.phase, this.cleanupState, this.actorAdmission, this.runtimeObserved);
  final AgentExporterPhase phase;
  final String cleanupState;
  final bool actorAdmission, runtimeObserved;
  bool get closedObserved => cleanupState == 'closedObserved';
}

abstract interface class AgentExporterCapability {
  AgentExporterPort? get agentExporterPort;
}

abstract interface class AgentExporterPort {
  Future<AgentExporterCatalog> catalog(RuntimeNoticeScope scope, AgentAssociationRecord record);
  Future<AgentExporterHandle> prepare(
    RuntimeNoticeScope scope,
    AgentAssociationRecord record,
    AgentExporterCatalog catalog,
    int durationSeconds,
  );
}

abstract interface class AgentExporterHandle {
  Future<AgentExporterView> details();
  Future<AgentExporterView> approveOnce();
  Future<AgentExporterView> stop();
  void cancel();
  Future<AgentExporterCleanup> joined();
  void dispose();
}

bool canRequestAgentExporter(AgentAssociationRecord r) =>
    r.phase == AgentAssociationPhase.associated &&
    r.purpose == AgentAssociationPurpose.exporters &&
    r.nodeKeyEpoch == '1' &&
    r.rotationState == 'notRequested' &&
    r.revocationState == 'notRequested' &&
    r.certificateDigest != null &&
    r.hostRevision != null;

Map<String, dynamic> _decode(String raw, Set<String> keys, {int cap = 16384}) {
  if (utf8.encode(raw).length > cap) {
    exporterWireDenied();
  }
  return exporterObject(jsonDecode(raw), keys);
}

String _choice(Object? value, Set<String> values) {
  if (value is! String || !values.contains(value)) {
    exporterWireDenied();
  }
  return value;
}

bool _bool(Object? value) {
  if (value is! bool) {
    exporterWireDenied();
  }
  return value;
}

const _cleanup = {'notStarted', 'pending', 'closedObserved', 'unknown'};
void _frame(Map<String, dynamic> m, RuntimeNoticeScope s) {
  if (m['version'] is! int ||
      m['version'] != 1 ||
      m['profileId'] != s.profileId ||
      m['notificationScopeId'] != s.nonce) {
    exporterWireDenied();
  }
}

void _binding(Map<String, dynamic> m, AgentAssociationRecord r) {
  if (!canRequestAgentExporter(r) ||
      exporterUuid(m['associationId']) != r.id ||
      exporterUuid(m['hostId']) != r.hostId ||
      exporterDecimal(m['hostRevision']) != r.hostRevision ||
      m['scope'] != 'exporters.manage' ||
      m['purpose'] != 'exporters') {
    exporterWireDenied();
  }
  final n = exporterObject(m['node'], {'agentId', 'keyEpoch', 'publicKey', 'keyFingerprint'});
  if (exporterUuid(n['agentId']) != r.agentId ||
      n['keyEpoch'] != '1' ||
      exporterSha(n['keyFingerprint']) != r.nodeKeyFingerprint) {
    exporterWireDenied();
  }
  exporterSha(n['publicKey']);
}

void _owner(Object? raw, RuntimeNoticeScope scope) {
  final o = exporterObject(raw, {'kind', 'id', 'deviceId', 'signingKeySha256'});
  final kind = _choice(o['kind'], {'localProfile', 'personalAccount'});
  exporterUuid(o['id']);
  exporterUuid(o['deviceId']);
  exporterSha(o['signingKeySha256']);
  if (kind == 'localProfile' && o['id'] != scope.profileId) {
    exporterWireDenied();
  }
}

DateTime _utc(Object? raw) {
  if (raw is! String ||
      raw.length > 64 ||
      !RegExp(r'^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$').hasMatch(raw)) {
    exporterWireDenied();
  }
  final value = DateTime.tryParse(raw);
  if (value == null || !value.isUtc || value.toIso8601String().replaceFirst('.000Z', 'Z') != raw) {
    exporterWireDenied();
  }
  return value;
}

const _common = {
  'version',
  'profileId',
  'notificationScopeId',
  'phase',
  'associationId',
  'hostId',
  'hostRevision',
  'node',
  'purpose',
  'scope',
  'manifestSha256',
  'policy',
  'runtimeObserved',
  'actorAdmission',
  'installationPerformed',
  'automaticRenewal',
  'cleanupState',
};
AgentExporterCatalog parseAgentExporterCatalog(String raw, RuntimeNoticeScope s, AgentAssociationRecord r) {
  final m = _decode(raw, _common);
  _frame(m, s);
  _binding(m, r);
  if (m['phase'] != 'exporterCatalog' ||
      m['runtimeObserved'] != false ||
      m['actorAdmission'] != false ||
      m['installationPerformed'] != false ||
      m['automaticRenewal'] != false) {
    exporterWireDenied();
  }
  return AgentExporterCatalog(
    parseAgentExporterPolicy(m['policy'], r.hostId, agentId: r.agentId),
    exporterSha(m['manifestSha256']),
    _choice(m['cleanupState'], _cleanup),
  );
}

AgentExporterView parseAgentExporterView(
  String raw,
  RuntimeNoticeScope s,
  AgentAssociationRecord r,
  AgentExporterCatalog original,
) {
  final m = _decode(raw, {
    ..._common,
    'owner',
    'reviewRemainingMs',
    'originalExpiresUtc',
    'leaseHash',
    'artifactVersion',
    'runningVersion',
    'childPrivilege',
    'remoteDrainObserved',
  });
  _frame(m, s);
  _binding(m, r);
  _owner(m['owner'], s);
  final phase = AgentExporterPhase.values.byName(
    _choice(m['phase'], AgentExporterPhase.values.map((x) => x.name).toSet()),
  );
  final policy = parseAgentExporterPolicy(m['policy'], r.hostId, agentId: r.agentId);
  // Exact catalog projection equality is a display binding; Native verifies its full digest.
  if (exporterSha(m['manifestSha256']) != original.manifestSha256 ||
      !_samePolicy(policy, original.policy) ||
      m['artifactVersion'] != policy.artifactVersion ||
      m['runningVersion'] != null ||
      m['childPrivilege'] != 'nonroot-no-capabilities' ||
      m['installationPerformed'] != false ||
      m['automaticRenewal'] != false ||
      m['remoteDrainObserved'] != false) {
    exporterWireDenied();
  }
  final admitted = _bool(m['actorAdmission']), observed = _bool(m['runtimeObserved']);
  if (admitted != observed || phase == AgentExporterPhase.running && !admitted) {
    exporterWireDenied();
  }
  final lease = m['leaseHash'] == null ? null : exporterSha(m['leaseHash']);
  if (lease != null && !admitted || phase == AgentExporterPhase.running && lease == null) {
    exporterWireDenied();
  }
  final remaining = m['reviewRemainingMs'] == null ? null : exporterInt(m['reviewRemainingMs'], 0, 60000);
  if (phase == AgentExporterPhase.exporterReview && (remaining == null || remaining == 0 || admitted)) {
    exporterWireDenied();
  }
  return AgentExporterView(
    phase: phase,
    policy: policy,
    manifestSha256: original.manifestSha256,
    originalExpiresUtc: _utc(m['originalExpiresUtc']),
    reviewRemainingMs: remaining,
    leaseHash: lease,
    runtimeObserved: observed,
    actorAdmission: admitted,
    cleanupState: _choice(m['cleanupState'], _cleanup),
  );
}

bool _samePolicy(AgentExporterPolicy a, AgentExporterPolicy b) =>
    a.hostId == b.hostId &&
    a.agentId == b.agentId &&
    a.platform == b.platform &&
    a.artifactVersion == b.artifactVersion &&
    a.archiveSha256 == b.archiveSha256 &&
    a.binarySha256 == b.binarySha256 &&
    a.noticeInventorySha256 == b.noticeInventorySha256 &&
    a.binaryBytes == b.binaryBytes &&
    a.maxSeconds == b.maxSeconds;
AgentExporterCleanup parseAgentExporterCleanup(String raw) {
  final m = _decode(raw, {
    'version',
    'phase',
    'actorAdmission',
    'runtimeObserved',
    'remoteDrainObserved',
    'cleanupState',
  });
  if (m['version'] is! int || m['version'] != 1 || m['remoteDrainObserved'] != false) {
    exporterWireDenied();
  }
  final admitted = _bool(m['actorAdmission']), observed = _bool(m['runtimeObserved']);
  if (admitted != observed) {
    exporterWireDenied();
  }
  return AgentExporterCleanup(
    AgentExporterPhase.values.byName(_choice(m['phase'], AgentExporterPhase.values.map((x) => x.name).toSet())),
    _choice(m['cleanupState'], {'closedObserved', 'unknown'}),
    admitted,
    observed,
  );
}
