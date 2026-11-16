import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';

enum AgentMetricsCollectorPhase { collectorReview, starting, running, stopped, cancelled, unknown }

enum AgentMetricsCleanupState { pending, closedObserved, unknown }

final class AgentMetricsCollectorIntent {
  AgentMetricsCollectorIntent(this.record, this.durationSeconds) {
    if (!record.canRequestMetricsCollection ||
        record.nodeKeyFingerprint == null ||
        durationSeconds < 30 ||
        durationSeconds > 300) {
      throw const FormatException('Separate MetricsOnly enrollment required');
    }
    checkedAssociationId(record.id);
    checkedAssociationId(record.hostId);
    checkedAssociationId(record.agentId);
    associationNumber(record.hostRevision!, positive: true, signed: true);
    associationHash(record.nodeKeyFingerprint!);
  }
  final AgentAssociationRecord record;
  final int durationSeconds;
  Map<String, Object> request(RuntimeNoticeScope scope) => {
    'version': 1,
    'profileId': scope.profileId,
    'notificationScopeId': scope.nonce,
    'associationId': record.id,
    'durationSeconds': durationSeconds,
  };
}

final class AgentMetricsCollectorView {
  const AgentMetricsCollectorView({
    required this.scope,
    required this.phase,
    required this.associationId,
    required this.hostId,
    required this.hostRevision,
    required this.agentId,
    required this.nodeKeyEpoch,
    required this.nodePublicKey,
    required this.nodeFingerprint,
    required this.ownerKind,
    required this.ownerId,
    required this.ownerDeviceId,
    required this.ownerSigningKeySha256,
    required this.collector,
    required this.reviewRemainingMs,
    required this.originalExpiresUtc,
    required this.leaseHash,
    required this.actorAdmission,
    required this.cleanupState,
  });
  final RuntimeNoticeScope scope;
  final AgentMetricsCollectorPhase phase;
  final String associationId,
      hostId,
      hostRevision,
      agentId,
      nodeKeyEpoch,
      nodePublicKey,
      nodeFingerprint,
      ownerKind,
      ownerId,
      ownerDeviceId,
      ownerSigningKeySha256;
  final AgentMetricsCollectorCatalog collector;
  final int? reviewRemainingMs;
  final DateTime originalExpiresUtc;
  final String? leaseHash;
  final bool actorAdmission;
  final AgentMetricsCleanupState cleanupState;
  String get scopeName => 'metrics.collect';
  bool get persistentDisk => collector.manifest.persistentDisk;
  String get profile => persistentDisk ? agentMetricsDiskProfile : 'volatile-collection-session-v1';
  String? get bufferMode => persistentDisk ? 'persistentNativeWal' : null;
  AgentMetricsDiskBudget? get diskBudget => persistentDisk ? const AgentMetricsDiskBudget() : null;
  bool? get automaticRecovery => persistentDisk ? false : null;
  bool? get requiresFreshGrantForReplay => persistentDisk ? true : null;
  int get maxMemoryBytes => 536870912;
  int get maxTemporaryStateBytes => 33554432;
  bool get lossUnknown => true;
  bool get unacknowledgedDataCanBeLost => true;
  bool? get crashReplay => persistentDisk ? null : false;
  bool get automaticRenewal => false;
  bool get strictRemoteWriteAvailable => false;
  bool get deliveryGuaranteed => false;
  bool get arbitraryReceiversSupported => false;
}

final class AgentMetricsCollectorCleanup {
  const AgentMetricsCollectorCleanup(this.phase, this.state, this.actorAdmission);
  final AgentMetricsCollectorPhase phase;
  final AgentMetricsCleanupState state;
  final bool actorAdmission;
  bool get lossUnknown => true;
  bool get deliveryGuaranteed => false;
}
