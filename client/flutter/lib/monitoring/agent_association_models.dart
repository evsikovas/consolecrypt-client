import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_exporter_policy.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';

final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
final _hash = RegExp(r'^[0-9a-f]{64}$');
Never _invalid() => throw const FormatException('Agent association metadata unavailable');
String checkedAssociationId(String value) =>
    _uuid.hasMatch(value) && value != '00000000-0000-0000-0000-000000000000' ? value : _invalid();
String associationHash(String value) => _hash.hasMatch(value) ? value : _invalid();
String associationNumber(String value, {bool positive = false, bool signed = false}) {
  if (!RegExp(r'^(0|[1-9][0-9]*)$').hasMatch(value)) _invalid();
  final n = BigInt.parse(value), max = BigInt.parse(signed ? '9223372036854775807' : '18446744073709551615');
  if (n > max || positive && n == BigInt.zero) _invalid();
  return value;
}

enum AgentAssociationPhase { unenrolled, pending, unknown, associated, blocked, revoked }

enum AgentClientIdentityState { missing, pending, storedMetadata, unavailable }

enum AgentAssociationReviewStage { offer, certificate }

enum AgentAssociationPurpose { tunnel, metrics, exporters }

final class AgentAssociationHost {
  AgentAssociationHost({required String id, required String revision, required this.name})
    : id = checkedAssociationId(id),
      revision = associationNumber(revision, positive: true, signed: true) {
    if (name.isEmpty || name.length > 256 || name.runes.any((r) => r < 32 || r >= 127 && r <= 159)) {
      _invalid();
    }
  }
  final String id, revision, name;
}

final class AgentAssociationRecord {
  AgentAssociationRecord({
    required String associationId,
    required String hostId,
    required String agentId,
    required this.phase,
    this.hostRevision,
    this.nodeKeyFingerprint,
    this.certificateDigest,
    this.certificateExpiresUtc,
    this.revocationState,
    this.remoteRevocationObserved,
    this.nodeKeyEpoch,
    this.rotationState,
    this.purpose,
  }) : id = associationIdChecked(associationId),
       hostId = associationIdChecked(hostId),
       agentId = associationIdChecked(agentId);
  final String id, hostId, agentId;
  final AgentAssociationPhase phase;
  final String? hostRevision, nodeKeyFingerprint, certificateDigest;
  final DateTime? certificateExpiresUtc;
  final String? revocationState;
  final bool? remoteRevocationObserved;
  final String? nodeKeyEpoch, rotationState;

  /// Verified Native display domain; absent on historical response variants.
  final AgentAssociationPurpose? purpose;
  String? get scopeName => switch (purpose) {
    AgentAssociationPurpose.tunnel => 'tunnel.open',
    AgentAssociationPurpose.metrics => 'metrics.collect',
    AgentAssociationPurpose.exporters => 'exporters.manage',
    null => null,
  };
  bool get canRequestTunnel =>
      phase == AgentAssociationPhase.associated &&
      (purpose == null || purpose == AgentAssociationPurpose.tunnel) &&
      hostRevision != null &&
      certificateDigest != null;
  bool get canRequestMetricsCollection =>
      phase == AgentAssociationPhase.associated &&
      purpose == AgentAssociationPurpose.metrics &&
      nodeKeyEpoch == '1' &&
      rotationState == 'notRequested' &&
      hostRevision != null &&
      certificateDigest != null;
  static String associationIdChecked(String v) => checkedAssociationId(v);
}

final class AgentAssociationStatus {
  AgentAssociationStatus({
    required this.scope,
    required this.clientIdentityState,
    required List<AgentAssociationHost> hosts,
    required List<AgentAssociationRecord> records,
    this.clientIdentityId,
    this.clientCertificateSha256,
  }) : hosts = List.unmodifiable(hosts),
       records = List.unmodifiable(records) {
    if (hosts.length > 64 ||
        records.length > 32 ||
        hosts.map((h) => h.id).toSet().length != hosts.length ||
        records.map((r) => r.id).toSet().length != records.length) {
      _invalid();
    }
  }
  final RuntimeNoticeScope scope;
  final AgentClientIdentityState clientIdentityState;
  final String? clientIdentityId, clientCertificateSha256;
  final List<AgentAssociationHost> hosts;
  final List<AgentAssociationRecord> records;
}

/// Public facts obtained from the held original Native review. Never authority.
final class AgentAssociationReview {
  AgentAssociationReview({
    required this.scope,
    required this.stage,
    required String hostId,
    required String hostRevision,
    required String agentId,
    required String nodeFingerprint,
    required this.ownerKind,
    required String ownerId,
    required String ownerEpoch,
    required String checkpoint,
    required String actorRevision,
    required String clientIdentityId,
    required String certificateSha256,
    required this.remainingMs,
    String? associationId,
    String? claimDigest,
    this.nodePublicKey,
    this.nodeKeyEpoch,
    this.ownerDeviceId,
    this.ownerSigningKeySha256,
    this.nodeContextKind,
    this.nodeContextId,
    this.nodeDeploymentId,
    this.maxCertificateLifetimeSeconds = 0,
    this.purpose = AgentAssociationPurpose.tunnel,
    this.metricsCollector,
    this.exporterCatalog,
  }) : hostId = associationIdChecked(hostId),
       hostRevision = associationNumber(hostRevision, positive: true, signed: true),
       agentId = associationIdChecked(agentId),
       nodeFingerprint = associationHash(nodeFingerprint),
       ownerId = associationIdChecked(ownerId),
       ownerEpoch = associationNumber(ownerEpoch, positive: true),
       checkpoint = associationNumber(checkpoint),
       actorRevision = associationNumber(actorRevision),
       clientIdentityId = associationIdChecked(clientIdentityId),
       certificateSha256 = associationHash(certificateSha256),
       associationId = associationId == null ? null : associationIdChecked(associationId),
       claimDigest = claimDigest == null ? null : associationHash(claimDigest) {
    if (!const {'localProfile', 'personalAccount'}.contains(ownerKind) ||
        remainingMs < 1 ||
        remainingMs > 60000 ||
        stage == AgentAssociationReviewStage.offer && (associationId != null || claimDigest != null) ||
        stage == AgentAssociationReviewStage.certificate && (associationId == null || claimDigest == null)) {
      _invalid();
    }
  }
  static String associationIdChecked(String v) => checkedAssociationId(v);
  final RuntimeNoticeScope scope;
  final AgentAssociationReviewStage stage;
  final String hostId,
      hostRevision,
      agentId,
      nodeFingerprint,
      ownerKind,
      ownerId,
      ownerEpoch,
      checkpoint,
      actorRevision,
      clientIdentityId,
      certificateSha256;
  final String? associationId, claimDigest;
  final int remainingMs;
  final String? nodePublicKey,
      nodeKeyEpoch,
      ownerDeviceId,
      ownerSigningKeySha256,
      nodeContextKind,
      nodeContextId,
      nodeDeploymentId;
  final int maxCertificateLifetimeSeconds;
  final AgentAssociationPurpose purpose;
  final AgentMetricsCollectorCatalog? metricsCollector;
  final AgentExporterEnrollmentCatalog? exporterCatalog;
  String get scopeName => switch (purpose) {
    AgentAssociationPurpose.metrics => 'metrics.collect',
    AgentAssociationPurpose.exporters => 'exporters.manage',
    AgentAssociationPurpose.tunnel => 'tunnel.open',
  };
}

final class AgentAssociationResult {
  AgentAssociationResult({
    required this.scope,
    required String associationId,
    required String agentId,
    required String certificateDigest,
    required String checkpoint,
    required bool actorAdmission,
    this.certificateExpiresUtc,
    this.purpose = AgentAssociationPurpose.tunnel,
    this.metricsCollector,
    this.exporterCatalog,
  }) : associationId = associationIdChecked(associationId),
       agentId = associationIdChecked(agentId),
       certificateDigest = associationHash(certificateDigest),
       checkpoint = associationNumber(checkpoint) {
    if (actorAdmission) _invalid();
  }
  static String associationIdChecked(String v) => checkedAssociationId(v);
  final RuntimeNoticeScope scope;
  final String associationId, agentId, certificateDigest, checkpoint;
  final DateTime? certificateExpiresUtc;
  final AgentAssociationPurpose purpose;
  final AgentMetricsCollectorCatalog? metricsCollector;
  final AgentExporterEnrollmentCatalog? exporterCatalog;
}

final class AgentAssociationCleanup {
  const AgentAssociationCleanup(this.phase, this.state);
  final String phase, state;
}

final class AgentClientIdentityReview {
  const AgentClientIdentityReview({
    required this.scope,
    required this.ownerKind,
    required this.ownerId,
    required this.deviceId,
    required this.signingKeySha256,
    required this.remainingMs,
  });
  final RuntimeNoticeScope scope;
  final String ownerKind, ownerId, deviceId, signingKeySha256;
  final int remainingMs;
}

final class AgentClientIdentityResult {
  const AgentClientIdentityResult({required this.scope, required this.identityId, required this.certificateSha256});
  final RuntimeNoticeScope scope;
  final String identityId, certificateSha256;
}

/// Metadata from the retained original Native revocation handle, not authority.
final class AgentRevocationDetails {
  const AgentRevocationDetails({
    required this.scope,
    required this.phase,
    required this.record,
    required this.remainingMs,
    required this.nodePublicKey,
    required this.nodeKeyEpoch,
    required this.ownerKind,
    required this.ownerId,
    required this.ownerDeviceId,
    required this.ownerSigningKeySha256,
    required this.nodeContextKind,
    required this.nodeContextId,
    required this.nodeDeploymentId,
    required this.actorRevision,
    required this.checkpoint,
    required this.localBlocked,
    required this.remoteRevocationObserved,
    required this.cleanupState,
  });
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final String phase,
      nodePublicKey,
      nodeKeyEpoch,
      ownerKind,
      ownerId,
      ownerDeviceId,
      ownerSigningKeySha256,
      nodeContextKind,
      nodeContextId,
      nodeDeploymentId,
      actorRevision,
      checkpoint,
      cleanupState;
  final int remainingMs;
  final bool? localBlocked;
  final bool remoteRevocationObserved;
}

final class AgentRevocationCleanup {
  const AgentRevocationCleanup(this.phase, this.cleanupState, this.remoteRevocationObserved);
  final String phase, cleanupState;
  final bool remoteRevocationObserved;
}
