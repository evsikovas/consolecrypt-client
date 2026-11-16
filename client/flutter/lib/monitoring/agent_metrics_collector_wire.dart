import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';

Never _bad() => throw const FormatException('Metrics collection observation invalid');
typedef _Json = Map<String, dynamic>;
_Json _decode(String raw, int max) {
  if (raw.isEmpty || utf8.encode(raw).length > max) _bad();
  final v = jsonDecode(raw);
  return v is _Json ? v : _bad();
}

_Json _keys(Object? v, Set<String> keys) {
  if (v is! _Json || v.length != keys.length || !v.keys.every(keys.contains)) _bad();
  return v;
}

String _text(Object? v, int max) =>
    v is String &&
        v.isNotEmpty &&
        utf8.encode(v).length <= max &&
        !v.runes.any((r) => r < 32 || r >= 127 && r <= 159 || r >= 0xd800 && r <= 0xdfff)
    ? v
    : _bad();
String _id(Object? v) => checkedAssociationId(_text(v, 36));
String _sha(Object? v) => associationHash(_text(v, 64));
String _number(Object? v, {bool positive = false, bool signed = false}) =>
    associationNumber(_text(v, 20), positive: positive, signed: signed);
int _int(Object? v, int min, int max) => v is int && v >= min && v <= max ? v : _bad();
bool _bool(Object? v) => v is bool ? v : _bad();
void _fixed(Object? v, Object expected) {
  if (v != expected || v.runtimeType != expected.runtimeType) _bad();
}

AgentMetricsCollectorPhase _phase(Object? v) =>
    AgentMetricsCollectorPhase.values.where((p) => p.name == v).firstOrNull ?? _bad();
AgentMetricsCleanupState _state(Object? v) =>
    AgentMetricsCleanupState.values.where((p) => p.name == v).firstOrNull ?? _bad();
DateTime _utc(Object? v) {
  final raw = _text(v, 32);
  if (RegExp(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$').stringMatch(raw) != raw) _bad();
  final d = DateTime.tryParse(raw);
  if (d == null || !d.isUtc || d.toIso8601String().replaceFirst('.000Z', 'Z') != raw) _bad();
  return d;
}

AgentMetricsCollectorView parseAgentMetricsCollectorView(
  String raw,
  RuntimeNoticeScope scope,
  AgentMetricsCollectorIntent intent,
) {
  final object = _decode(raw, 16384), leasePresent = object.containsKey('leaseHash');
  final disk = object['profile'] == agentMetricsDiskProfile;
  final m = _keys(object, {
    'version',
    'profileId',
    'notificationScopeId',
    'phase',
    'associationId',
    'hostId',
    'hostRevision',
    'owner',
    'node',
    'scope',
    'collectorId',
    'manifestSha256',
    'manifest',
    'reviewRemainingMs',
    'originalExpiresUtc',
    'profile',
    'maxMemoryBytes',
    'maxTemporaryStateBytes',
    'lossUnknown',
    'unacknowledgedDataCanBeLost',
    if (!disk) 'crashReplay',
    if (disk) 'bufferMode',
    if (disk) 'diskBudget',
    if (disk) 'automaticRecovery',
    if (disk) 'requiresFreshGrantForReplay',
    'automaticRenewal',
    'strictRemoteWriteAvailable',
    'deliveryGuaranteed',
    'receiverPolicy',
    'arbitraryReceiversSupported',
    'actorAdmission',
    'cleanupState',
    if (leasePresent) 'leaseHash',
  });
  _fixed(m['version'], 1);
  _fixed(m['profileId'], scope.profileId);
  _fixed(m['notificationScopeId'], scope.nonce);
  _fixed(m['scope'], 'metrics.collect');
  _fixed(m['profile'], disk ? agentMetricsDiskProfile : 'volatile-collection-session-v1');
  _fixed(m['maxMemoryBytes'], 536870912);
  _fixed(m['maxTemporaryStateBytes'], 33554432);
  for (final flag in ['lossUnknown', 'unacknowledgedDataCanBeLost']) {
    _fixed(m[flag], true);
  }
  for (final flag in [
    if (!disk) 'crashReplay',
    'automaticRenewal',
    'strictRemoteWriteAvailable',
    'deliveryGuaranteed',
    'arbitraryReceiversSupported',
  ]) {
    _fixed(m[flag], false);
  }
  _fixed(m['receiverPolicy'], 'operator-controlled-nonredirecting-loopback');
  if (disk) {
    _fixed(m['bufferMode'], 'persistentNativeWal');
    _fixed(m['automaticRecovery'], false);
    _fixed(m['requiresFreshGrantForReplay'], true);
    const expected = {
      'maxChargedBytes': 536870912,
      'maxResidenceSeconds': 7200,
      'minBavailBytes': 134217728,
      'operationalWalBytes': 67108864,
      'checkpointBytes': 67108864,
      'reservedBytes': 134217728,
      'maxRootEntries': 256,
    };
    final budget = _keys(m['diskBudget'], expected.keys.toSet());
    for (final e in expected.entries) {
      _fixed(budget[e.key], e.value);
    }
  }

  final association = _id(m['associationId']),
      host = _id(m['hostId']),
      revision = _number(m['hostRevision'], positive: true, signed: true);
  if (association != intent.record.id || host != intent.record.hostId || revision != intent.record.hostRevision) _bad();
  final owner = _keys(m['owner'], {'kind', 'id', 'deviceId', 'signingKeySha256'}), ownerId = _id(owner['id']);
  if (!const {'localProfile', 'personalAccount'}.contains(owner['kind']) ||
      owner['kind'] == 'localProfile' && ownerId != scope.profileId) {
    _bad();
  }
  final node = _keys(m['node'], {'agentId', 'keyEpoch', 'publicKey', 'keyFingerprint'}),
      agent = _id(node['agentId']),
      epoch = _number(node['keyEpoch'], positive: true),
      fingerprint = _sha(node['keyFingerprint']);
  if (agent != intent.record.agentId ||
      epoch != '1' ||
      epoch != intent.record.nodeKeyEpoch ||
      fingerprint != intent.record.nodeKeyFingerprint) {
    _bad();
  }
  final phase = _phase(m['phase']),
      admitted = _bool(m['actorAdmission']),
      lease = leasePresent ? _sha(m['leaseHash']) : null;
  if ((lease != null) != admitted ||
      const {AgentMetricsCollectorPhase.running, AgentMetricsCollectorPhase.stopped}.contains(phase) && !admitted ||
      const {
            AgentMetricsCollectorPhase.collectorReview,
            AgentMetricsCollectorPhase.starting,
            AgentMetricsCollectorPhase.cancelled,
          }.contains(phase) &&
          admitted) {
    _bad();
  }
  final remaining = m['reviewRemainingMs'] == null ? null : _int(m['reviewRemainingMs'], 0, 60000);
  if (phase == AgentMetricsCollectorPhase.collectorReview && (remaining == null || remaining == 0) ||
      admitted && remaining != null) {
    _bad();
  }
  final catalog = AgentMetricsCollectorCatalog(
    _id(m['collectorId']),
    _sha(m['manifestSha256']),
    parseAgentMetricsManifest(m['manifest'], host),
  );
  if (catalog.manifest.persistentDisk != disk) _bad();
  return AgentMetricsCollectorView(
    scope: scope,
    phase: phase,
    associationId: association,
    hostId: host,
    hostRevision: revision,
    agentId: agent,
    nodeKeyEpoch: epoch,
    nodePublicKey: _sha(node['publicKey']),
    nodeFingerprint: fingerprint,
    ownerKind: owner['kind'] as String,
    ownerId: ownerId,
    ownerDeviceId: _id(owner['deviceId']),
    ownerSigningKeySha256: _sha(owner['signingKeySha256']),
    collector: catalog,
    reviewRemainingMs: remaining,
    originalExpiresUtc: _utc(m['originalExpiresUtc']),
    leaseHash: lease,
    actorAdmission: admitted,
    cleanupState: _state(m['cleanupState']),
  );
}

AgentMetricsCollectorCleanup parseAgentMetricsCollectorCleanup(String raw) {
  final m = _keys(_decode(raw, 4096), {
    'version',
    'phase',
    'actorAdmission',
    'lossUnknown',
    'deliveryGuaranteed',
    'cleanupState',
  });
  _fixed(m['version'], 1);
  _fixed(m['lossUnknown'], true);
  _fixed(m['deliveryGuaranteed'], false);
  if (!const {'closedObserved', 'unknown'}.contains(m['cleanupState'])) _bad();
  final phase = _phase(m['phase']), admitted = _bool(m['actorAdmission']);
  if (phase == AgentMetricsCollectorPhase.stopped && !admitted) _bad();
  return AgentMetricsCollectorCleanup(phase, _state(m['cleanupState']), admitted);
}
