import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';

enum AgentTunnelRoutePhase {
  routeReview,
  starting,
  listening,
  connecting,
  connected,
  stopRequested,
  cancelled,
  unknown,
}

enum AgentTunnelCleanupState { notStarted, pending, closedObserved, unknown }

enum AgentTunnelAccessEvent { openRequested, opened, stopRequested, cancelled, revoked, unknown }

/// User choices reference an actual saved association; they grant nothing.
final class AgentTunnelRouteIntent {
  AgentTunnelRouteIntent({
    required this.record,
    required this.listenIp,
    required this.listenPort,
    required this.destinationIp,
    required this.destinationPort,
    required this.ttlSeconds,
    required this.idleSeconds,
    this.resumeOriginal = false,
  }) {
    checkedAssociationId(record.id);
    checkedAssociationId(record.hostId);
    checkedAssociationId(record.agentId);
    if ((record.purpose != null && record.purpose != AgentAssociationPurpose.tunnel) ||
        record.phase != AgentAssociationPhase.associated ||
        record.hostRevision == null ||
        record.certificateDigest == null) {
      throw const FormatException('Tunnel association metadata unavailable');
    }
    associationNumber(record.hostRevision!, positive: true, signed: true);
    associationHash(record.certificateDigest!);
    if (!const {'127.0.0.1', '::1'}.contains(listenIp) ||
        !const {'127.0.0.1', '::1'}.contains(destinationIp) ||
        listenPort < 0 ||
        listenPort > 65535 ||
        destinationPort < 1 ||
        destinationPort > 65535 ||
        ttlSeconds < 1 ||
        ttlSeconds > 60 ||
        idleSeconds < 1 ||
        idleSeconds > 30 ||
        idleSeconds > ttlSeconds) {
      throw const FormatException('Tunnel choices invalid');
    }
  }
  final AgentAssociationRecord record;
  final String listenIp, destinationIp;
  final int listenPort, destinationPort, ttlSeconds, idleSeconds;
  final bool resumeOriginal;
  Map<String, Object> request(RuntimeNoticeScope scope) => {
    'version': 1,
    'profileId': scope.profileId,
    'notificationScopeId': scope.nonce,
    'associationId': record.id,
    'listenIp': listenIp,
    'listenPort': listenPort,
    'destinationIp': destinationIp,
    'destinationPort': destinationPort,
    'ttlSeconds': ttlSeconds,
    'idleSeconds': idleSeconds,
    'resumeOriginal': resumeOriginal,
  };
}

final class AgentTunnelOwner {
  const AgentTunnelOwner(this.kind, this.id, this.deviceId, this.signingKeySha256);
  final String kind, id, deviceId, signingKeySha256;
}

final class AgentTunnelNode {
  const AgentTunnelNode(this.agentId, this.keyEpoch, this.publicKey, this.keyFingerprint);
  final String agentId, keyEpoch, publicKey, keyFingerprint;
}

final class AgentTunnelEndpoint {
  const AgentTunnelEndpoint(this.ip, this.port);
  final String ip;
  final int port;
}

final class AgentTunnelLimits {
  const AgentTunnelLimits({
    required this.maxTtlSeconds,
    required this.idleTimeoutSeconds,
    required this.maxChannels,
    required this.maxFrameBytes,
    required this.maxPendingFramesPerChannel,
    required this.maxPendingFramesTotal,
    required this.maxQueuedBytesPerChannel,
    required this.maxQueuedBytesTotal,
    required this.maxBytesPerChannel,
    required this.maxBytesTotal,
  });
  final int maxTtlSeconds,
      idleTimeoutSeconds,
      maxChannels,
      maxFrameBytes,
      maxPendingFramesPerChannel,
      maxPendingFramesTotal,
      maxQueuedBytesPerChannel,
      maxQueuedBytesTotal,
      maxBytesPerChannel,
      maxBytesTotal;
}

/// Native observations, not a fresh grant or a remote-drain receipt.
final class AgentTunnelRouteView {
  const AgentTunnelRouteView({
    required this.scope,
    required this.phase,
    this.routeId,
    required this.associationId,
    required this.hostId,
    required this.hostRevision,
    required this.owner,
    required this.node,
    required this.certificateDigest,
    required this.destination,
    required this.clientBind,
    required this.limits,
    required this.grantExpiresUtc,
    required this.remainingMs,
    required this.boundAddress,
    required this.actorAdmission,
    required this.cleanupState,
    required this.sentBytes,
    required this.receivedBytes,
    required this.resumeOriginalSignedOptIn,
    required this.resumeTransportImplemented,
  });
  final RuntimeNoticeScope scope;
  /// Observed original policy ID; metadata alone never admits a connection.
  final String? routeId;
  final AgentTunnelRoutePhase phase;
  final String associationId, hostId, hostRevision, certificateDigest, sentBytes, receivedBytes;
  final AgentTunnelOwner owner;
  final AgentTunnelNode node;
  final AgentTunnelEndpoint destination, clientBind;
  final AgentTunnelLimits limits;
  final DateTime grantExpiresUtc;
  final int remainingMs;
  final String? boundAddress;
  final bool actorAdmission;
  final AgentTunnelCleanupState cleanupState;
  bool get remoteDrainObserved => false;
  bool get countersExact => false;
  bool get nativeAccountingOnly => true;
  final bool resumeOriginalSignedOptIn;
  final bool resumeTransportImplemented;
}

final class AgentTunnelRouteCleanup {
  const AgentTunnelRouteCleanup({
    required this.phase,
    required this.state,
    required this.sentBytes,
    required this.receivedBytes,
    required this.actorAdmission,
  });
  final AgentTunnelRoutePhase phase;
  final AgentTunnelCleanupState state;
  final String sentBytes, receivedBytes;
  final bool actorAdmission;
  bool get remoteDrainObserved => false;
  bool get countersExact => false;
  bool get nativeAccountingOnly => true;
}

final class AgentTunnelAccessRecord {
  const AgentTunnelAccessRecord({
    required this.operationId,
    required this.associationId,
    required this.hostId,
    required this.hostRevision,
    required this.grantId,
    required this.grantDigest,
    required this.policyDigest,
    required this.agentId,
    required this.keyEpoch,
    required this.destination,
    required this.expiresAt,
    required this.observedAt,
    required this.event,
    required this.sent,
    required this.received,
    required this.countersExact,
    required this.nativeControlFinished,
  });
  final String operationId, associationId, hostId, hostRevision, grantId, grantDigest, policyDigest, agentId, keyEpoch;
  final AgentTunnelEndpoint destination;
  final DateTime expiresAt, observedAt;
  final AgentTunnelAccessEvent event;
  final int sent, received;
  final bool countersExact, nativeControlFinished;
}

final class AgentTunnelAccessRow {
  const AgentTunnelAccessRow({
    required this.sequence,
    required this.profileId,
    required this.deviceId,
    required this.personalUserId,
    required this.record,
  });
  final String sequence, profileId, deviceId;
  final String? personalUserId;
  final AgentTunnelAccessRecord record;
}

final class AgentTunnelAccessAudit {
  AgentTunnelAccessAudit(this.scope, List<AgentTunnelAccessRow> rows) : rows = List.unmodifiable(rows);
  final RuntimeNoticeScope scope;
  final List<AgentTunnelAccessRow> rows;
  bool get boundedLocalObservations => true;
  bool get immutableLog => false;
  bool get remoteDeliveryGuarantee => false;
}
