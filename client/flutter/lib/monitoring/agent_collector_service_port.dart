import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';

abstract interface class AgentCollectorServiceCapability {
  AgentCollectorServicePort? get agentCollectorServicePort;
}

abstract interface class AgentCollectorServicePort {
  Future<AgentCollectorServiceCatalogView> readCatalog(RuntimeNoticeScope scope, AgentAssociationRecord record);
  Future<AgentCollectorServiceHandle> prepare(RuntimeNoticeScope scope, AgentCollectorServiceIntent intent);
}

/// Optional capability; legacy ports remain SingleStart-only.
abstract interface class AgentCollectorServiceLifetimePort {
  Future<AgentCollectorServiceHandle> prepareLifetime(RuntimeNoticeScope scope, AgentCollectorServiceIntent intent);
}

abstract interface class AgentCollectorServiceHandle {
  Future<AgentCollectorServiceView> details();
  Future<AgentCollectorServiceView> confirmConfiguration();
  void cancel();
  Future<AgentCollectorServiceCleanup> joined();
  void dispose();
}

/// Actual catalog references only. No caller duration, receiver, path, budget,
/// signed statement or authority flag; Native repeats the original probe.
final class AgentCollectorServiceIntent {
  AgentCollectorServiceIntent(
    this.record,
    this.catalog, {
    List<AgentCollectorServiceCredentialSelection> credentials = const [],
    this.credentialUse = AgentCollectorServiceCredentialUse.singleStart,
  }) : credentials = checkedCollectorCredentialSelections(credentials, catalog) {
    if (record.phase != AgentAssociationPhase.associated ||
        record.purpose != AgentAssociationPurpose.metrics ||
        record.hostRevision == null ||
        record.certificateDigest == null ||
        record.nodeKeyEpoch == null ||
        record.nodeKeyFingerprint == null ||
        catalog.manifest.hostId != record.hostId ||
        !catalog.manifest.persistentDisk) {
      throw const FormatException('Original Metrics service context unavailable');
    }
    if (credentialUse == AgentCollectorServiceCredentialUse.serviceLifetime && this.credentials.isEmpty) {
      throw const FormatException('Explicit service-lifetime credential references required');
    }
    checkedAssociationId(catalog.collectorId);
    associationHash(catalog.manifestSha256);
    associationNumber(record.hostRevision!, positive: true, signed: true);
    associationNumber(record.nodeKeyEpoch!, positive: true);
    associationHash(record.certificateDigest!);
    associationHash(record.nodeKeyFingerprint!);
  }
  final AgentAssociationRecord record;
  final AgentMetricsCollectorCatalog catalog;
  final List<AgentCollectorServiceCredentialSelection> credentials;
  final AgentCollectorServiceCredentialUse credentialUse;
  Map<String, Object> request(RuntimeNoticeScope scope) => {
    'version': 1,
    'profileId': scope.profileId,
    'notificationScopeId': scope.nonce,
    'associationId': record.id,
    'collectorId': catalog.collectorId,
    'manifestSha256': catalog.manifestSha256,
    if (credentials.isNotEmpty) 'credentials': credentials.map((c) => c.request()).toList(),
  };
  bool matches(AgentCollectorServiceView view) =>
      (credentialUse == AgentCollectorServiceCredentialUse.serviceLifetime) == (view.lifetimeCredentials != null) &&
      identical(view.record, record) &&
      view.catalog.collectorId == catalog.collectorId &&
      view.catalog.manifestSha256 == catalog.manifestSha256 &&
      sameCollectorCredentialSelections(credentials, view.credentialSelections);
}

final class AgentCollectorServiceCatalogView {
  const AgentCollectorServiceCatalogView(this.scope, this.record, this.catalog, this.cleanupState);
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final AgentMetricsCollectorCatalog catalog;
  final String cleanupState;
}

AgentCollectorServiceCatalogView parseAgentCollectorServiceCatalogView(
  String raw,
  RuntimeNoticeScope scope,
  AgentAssociationRecord record,
) {
  if (raw.isEmpty || utf8.encode(raw).length > 16384) throw const FormatException('Service catalog unavailable');
  final m = jsonDecode(raw);
  const keys = {
    'version',
    'profileId',
    'notificationScopeId',
    'kind',
    'associationId',
    'hostId',
    'hostRevision',
    'node',
    'collectorId',
    'manifestSha256',
    'manifest',
    'cleanupState',
  };
  if (m is! Map<String, dynamic> ||
      m.length != keys.length ||
      !m.keys.every(keys.contains) ||
      m['version'] is! int ||
      m['version'] != 1 ||
      m['kind'] != 'finiteCollectorServiceCatalog' ||
      m['profileId'] != scope.profileId ||
      m['notificationScopeId'] != scope.nonce ||
      m['associationId'] != record.id ||
      m['hostId'] != record.hostId ||
      m['hostRevision'] != record.hostRevision ||
      !const {'notStarted', 'pending', 'closedObserved', 'unknown'}.contains(m['cleanupState'])) {
    throw const FormatException('Service catalog unavailable');
  }
  final node = m['node'];
  if (node is! Map<String, dynamic> ||
      node.length != 4 ||
      !node.keys.every({'agentId', 'keyEpoch', 'publicKey', 'keyFingerprint'}.contains) ||
      node['agentId'] != record.agentId ||
      node['keyEpoch'] != record.nodeKeyEpoch ||
      node['keyFingerprint'] != record.nodeKeyFingerprint ||
      node['publicKey'] is! String) {
    throw const FormatException('Original service node changed');
  }
  associationHash(node['publicKey'] as String);
  final catalog = parseAgentMetricsCollectorCatalog({
    'collectorId': m['collectorId'],
    'manifestSha256': m['manifestSha256'],
    'manifest': m['manifest'],
  }, record.hostId);
  AgentCollectorServiceIntent(record, catalog); // Validate display domain; this issues no authority.
  return AgentCollectorServiceCatalogView(scope, record, catalog, m['cleanupState'] as String);
}

/// Exactly the actual operation.rs joined projection: caller carrier/worker
/// cleanup only. It can never establish that a persistent service has stopped.
final class AgentCollectorServiceCleanup {
  const AgentCollectorServiceCleanup(this.phase, this.state, this.serviceMayRemainActive);
  final AgentCollectorServicePhase phase;
  final String state;
  final bool serviceMayRemainActive;
}

AgentCollectorServiceCleanup parseAgentCollectorServiceCleanup(String raw) {
  if (raw.isEmpty || utf8.encode(raw).length > 16384) {
    throw const FormatException('Service caller cleanup unavailable');
  }
  final m = jsonDecode(raw);
  const keys = {
    'version',
    'phase',
    'cleanupState',
    'serviceMayRemainActive',
    'runtimeState',
    'deliveryGuaranteed',
    'lossUnknown',
  };
  if (m is! Map<String, dynamic> ||
      m.length != keys.length ||
      !m.keys.every(keys.contains) ||
      m['version'] is! int ||
      m['version'] != 1 ||
      !const {'closedObserved', 'unknown'}.contains(m['cleanupState']) ||
      m['serviceMayRemainActive'] is! bool ||
      m['runtimeState'] != 'notObserved' ||
      m['deliveryGuaranteed'] != false ||
      m['lossUnknown'] != true) {
    throw const FormatException('Service caller cleanup unavailable');
  }
  final phase = AgentCollectorServicePhase.values.where((v) => v.name == m['phase']).firstOrNull;
  if (phase == null ||
      phase == AgentCollectorServicePhase.configurationRecorded && m['serviceMayRemainActive'] != true) {
    throw const FormatException('Service caller cleanup unavailable');
  }
  return AgentCollectorServiceCleanup(phase, m['cleanupState'] as String, m['serviceMayRemainActive'] as bool);
}
