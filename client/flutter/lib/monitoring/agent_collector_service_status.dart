import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart' show AgentCollectorServiceBootStamp;
import 'package:consolecrypt/monitoring/agent_rotation.dart' show AgentRotationNode;

/// Optional readonly existing-policy observation. It never repairs/adopts a
/// local Unknown, signs a new policy, refreshes its expiry or starts collection.
abstract interface class AgentCollectorServiceStatusPort {
  Future<AgentCollectorServiceStatus> readSavedStatus(RuntimeNoticeScope scope, AgentAssociationRecord record);
}

final class AgentCollectorServiceStatus {
  const AgentCollectorServiceStatus({
    required this.scope,
    required this.record,
    required this.node,
    required this.serviceId,
    required this.serviceHash,
    required this.collectorId,
    required this.manifestSha256,
    required this.policyRevision,
    required this.revocationEpoch,
    required this.actorRevision,
    required this.localConfigurationRecorded,
    required this.remoteRevoked,
    required this.expiresAtUtc,
    required this.bootStamp,
    required this.cleanupState,
  });
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final AgentRotationNode node;
  final String serviceId,
      serviceHash,
      collectorId,
      manifestSha256,
      policyRevision,
      revocationEpoch,
      actorRevision,
      cleanupState;
  final bool localConfigurationRecorded, remoteRevoked;
  final DateTime expiresAtUtc;
  final AgentCollectorServiceBootStamp bootStamp;
  bool get remoteConfigurationRecorded => true;
  bool get runtimeObserved => false;
}

Never _bad() => throw const FormatException('Saved collector service observation unavailable');
Map<String, dynamic> _keys(Object? value, Set<String> keys) {
  if (value is! Map<String, dynamic> || value.length != keys.length || !value.keys.every(keys.contains)) _bad();
  return value;
}

String _id(Object? value) => value is String ? checkedAssociationId(value) : _bad();
String _hash(Object? value) => value is String ? associationHash(value) : _bad();
String _decimal(Object? value, {bool signed = false}) =>
    value is String && value.length <= 20 ? associationNumber(value, positive: true, signed: signed) : _bad();
DateTime _utc(Object? value) {
  if (value is! String ||
      value.length > 32 ||
      RegExp(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$').stringMatch(value) != value) {
    _bad();
  }
  final date = DateTime.tryParse(value);
  if (date == null || !date.isUtc || date.toIso8601String().replaceFirst('.000Z', 'Z') != value) _bad();
  return date;
}

AgentCollectorServiceStatus parseAgentCollectorServiceStatus(
  String raw,
  RuntimeNoticeScope scope,
  AgentAssociationRecord record,
) {
  if (raw.isEmpty ||
      utf8.encode(raw).length > 16384 ||
      record.phase != AgentAssociationPhase.associated ||
      record.purpose != AgentAssociationPurpose.metrics ||
      record.hostRevision == null ||
      record.nodeKeyEpoch == null ||
      record.nodeKeyFingerprint == null ||
      record.certificateDigest == null) {
    _bad();
  }
  final m = _keys(jsonDecode(raw), {
    'version',
    'profileId',
    'notificationScopeId',
    'kind',
    'associationId',
    'hostId',
    'hostRevision',
    'node',
    'serviceId',
    'serviceHash',
    'collectorId',
    'manifestSha256',
    'policyRevision',
    'revocationEpoch',
    'actorRevision',
    'localConfigurationRecorded',
    'remoteConfigurationRecorded',
    'remoteRevoked',
    'expiresAtUtc',
    'bootStamp',
    'runtimeState',
    'deliveryGuaranteed',
    'lossUnknown',
    'cleanupState',
  });
  if (m['version'] is! int ||
      m['version'] != 1 ||
      m['kind'] != 'finiteCollectorServiceStatus' ||
      m['profileId'] != scope.profileId ||
      m['notificationScopeId'] != scope.nonce ||
      _id(m['associationId']) != record.id ||
      _id(m['hostId']) != record.hostId ||
      _decimal(m['hostRevision'], signed: true) != record.hostRevision ||
      m['localConfigurationRecorded'] is! bool ||
      m['remoteConfigurationRecorded'] != true ||
      m['remoteRevoked'] is! bool ||
      m['runtimeState'] != 'notObserved' ||
      m['deliveryGuaranteed'] != false ||
      m['lossUnknown'] != true ||
      !const {'notStarted', 'pending', 'closedObserved', 'unknown'}.contains(m['cleanupState'])) {
    _bad();
  }
  final node = AgentRotationNode.parse(m['node']);
  if (node.agentId != record.agentId ||
      node.keyEpoch != record.nodeKeyEpoch ||
      node.fingerprint != record.nodeKeyFingerprint) {
    _bad();
  }
  final boot = _keys(m['bootStamp'], {'bootId', 'createdBoottimeNs', 'deadlineBoottimeNs'}),
      created = _decimal(boot['createdBoottimeNs']),
      deadline = _decimal(boot['deadlineBoottimeNs']),
      difference = BigInt.parse(deadline) - BigInt.parse(created);
  if (difference <= BigInt.zero || difference > BigInt.from(86400) * BigInt.from(1000000000)) _bad();
  final serviceId = _id(m['serviceId']);
  if (serviceId[14] != '4') _bad();
  return AgentCollectorServiceStatus(
    scope: scope,
    record: record,
    node: node,
    serviceId: serviceId,
    serviceHash: _hash(m['serviceHash']),
    collectorId: _id(m['collectorId']),
    manifestSha256: _hash(m['manifestSha256']),
    policyRevision: _decimal(m['policyRevision']),
    revocationEpoch: _decimal(m['revocationEpoch']),
    actorRevision: _decimal(m['actorRevision']),
    localConfigurationRecorded: m['localConfigurationRecorded'] as bool,
    remoteRevoked: m['remoteRevoked'] as bool,
    expiresAtUtc: _utc(m['expiresAtUtc']),
    bootStamp: AgentCollectorServiceBootStamp(_id(boot['bootId']), created, deadline),
    cleanupState: m['cleanupState'] as String,
  );
}
