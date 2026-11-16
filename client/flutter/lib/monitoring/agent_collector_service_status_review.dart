import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:material_ui/material_ui.dart';

class AgentCollectorServiceStatusReview extends StatelessWidget {
  const AgentCollectorServiceStatusReview({super.key, required this.status});
  final AgentCollectorServiceStatus status;
  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(label), SelectableText(value)]),
  );
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(l.agentCollectorServiceSavedTitle),
        Text(
          status.localConfigurationRecorded
              ? l.agentCollectorServiceLocalRecorded
              : l.agentCollectorServiceLocalUnknown,
        ),
        Text(l.agentCollectorServiceRemoteRecorded),
        Text(status.remoteRevoked ? l.agentCollectorServiceRemoteRevoked : l.agentCollectorServiceNoRevocationObserved),
        Text(l.agentCollectorServiceRuntimeUnknown),
        fact(l.agentCollectorServiceExpiry, status.expiresAtUtc.toIso8601String()),
        Text(
          status.cleanupState == 'closedObserved'
              ? l.agentCollectorServiceCallerClosed
              : l.agentCollectorServiceCleanupUnknown,
        ),
        ExpansionTile(
          title: Text(l.agentCollectorServiceTechnical),
          children: [
            fact(l.agentAssociationSavedHost, '${status.record.hostId} / ${status.record.hostRevision}'),
            fact(l.agentCollectorServiceAgent, status.node.agentId),
            fact(l.agentAssociationNodeEpoch, status.node.keyEpoch),
            fact(l.agentAssociationNodeFingerprint, status.node.fingerprint),
            fact(l.agentCollectorServiceIdentity, status.serviceId),
            fact(l.agentCollectorServiceHash, status.serviceHash),
            fact(l.agentMetricsCollectorId, status.collectorId),
            fact(l.agentCollectorServiceManifest, status.manifestSha256),
            fact(l.agentCollectorServicePolicyRevision, status.policyRevision),
            fact(l.agentCollectorServiceRevocationEpoch, status.revocationEpoch),
            fact(l.agentRevokeActorRevision, status.actorRevision),
            fact(l.agentCollectorServiceBootId, status.bootStamp.bootId),
            fact(l.agentCollectorServiceBootCreated, status.bootStamp.createdBoottimeNs),
            fact(l.agentCollectorServiceBootDeadline, status.bootStamp.deadlineBoottimeNs),
          ],
        ),
      ],
    );
  }
}
