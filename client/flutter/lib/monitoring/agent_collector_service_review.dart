import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

/// Read-only presenter for one original Native service view. The opaque worker
/// contract is still being authored; no preparation/approval callback is guessed.
class AgentCollectorServiceReview extends StatelessWidget {
  const AgentCollectorServiceReview({super.key, required this.view});
  final AgentCollectorServiceView view;
  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(label), SelectableText(value)]),
  );
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final phase = switch (view.phase) {
      AgentCollectorServicePhase.serviceReview => l.agentCollectorServiceReviewPhase,
      AgentCollectorServicePhase.configuring => l.agentCollectorServiceConfiguring,
      AgentCollectorServicePhase.configurationRecorded => l.agentCollectorServiceRecorded,
      AgentCollectorServicePhase.unknown => l.agentCollectorServiceUnknown,
      AgentCollectorServicePhase.cancelled => l.agentCollectorServiceCancelled,
    };
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        InfoBanner(message: l.agentCollectorServiceSummary),
        const SizedBox(height: 8),
        Text(phase, key: const ValueKey('collector-service-phase')),
        Text(l.agentCollectorServiceRuntimeUnknown, key: const ValueKey('collector-service-runtime-unknown')),
        fact(l.agentCollectorServiceExpiry, view.expiresAtUtc.toIso8601String()),
        Text(l.agentCollectorServiceLimits),
        Text(l.agentCollectorServiceDisk),
        Text(l.agentCollectorServiceReboot),
        Text(l.agentCollectorServiceCloseDisclosure),
        fact(l.agentMetricsConfiguredReceiver, view.catalog.manifest.receiverUrl),
        if (view.lifetimeCredentials case final lifetime?) ...[
          Text(l.agentCollectorServiceLifetimeReview, key: const ValueKey('collector-service-lifetime-review')),
          fact(l.agentCollectorServiceCredentialRetention, lifetime.retentionUntilUtc.toIso8601String()),
          fact(l.agentCollectorServiceCredentialPurpose, lifetime.purpose),
        ],
        if (view.credentialSelections.isNotEmpty) ...[
          Text(l.agentCollectorServiceCredentialsReview),
          for (final ref in view.credentialSelections)
            fact(
              ref.targetId == null
                  ? l.agentCollectorServiceCredentialReceiver
                  : '${l.agentMetricsTarget}: ${ref.targetId}',
              '${ref.credentialId} / ${ref.credentialRevision}',
            ),
        ],
        ExpansionTile(
          key: const ValueKey('collector-service-technical'),
          title: Text(l.agentCollectorServiceTechnical),
          children: [
            fact(l.agentAssociationSavedHost, '${view.record.hostId} / ${view.record.hostRevision}'),
            fact(l.agentCollectorServiceAgent, view.node.agentId),
            fact(l.agentAssociationNodeEpoch, view.node.keyEpoch),
            fact(l.agentAssociationNodeFingerprint, view.node.fingerprint),
            fact(l.agentAssociationOwner, '${view.ownerKind}: ${view.ownerId}'),
            fact(l.agentAssociationDevice, view.ownerDeviceId),
            fact(l.agentAssociationSigningKey, view.ownerSigningKeySha256),
            fact(l.agentAssociationOwnerEpoch, view.ownerEpoch),
            fact(l.agentAssociationCheckpoint, view.checkpoint),
            fact(l.agentRevokeActorRevision, view.actorRevision),
            fact(l.agentCollectorServiceIdentity, view.serviceId),
            fact(l.agentCollectorServiceHash, view.serviceHash),
            fact(l.agentCollectorServiceManifest, view.catalog.manifestSha256),
            fact(l.agentCollectorServicePolicyRevision, view.policyRevision),
            fact(l.agentCollectorServiceRevocationEpoch, view.revocationEpoch),
            if (view.bootStamp case final boot?) ...[
              fact(l.agentCollectorServiceBootId, boot.bootId),
              fact(l.agentCollectorServiceBootCreated, boot.createdBoottimeNs),
              fact(l.agentCollectorServiceBootDeadline, boot.deadlineBoottimeNs),
            ],
            if (view.configurationAck case final ack?)
              fact(l.agentCollectorServiceRecordedAt, ack.recordedAtUtc.toIso8601String()),
            fact(l.agentMetricsLabels, '${view.catalog.manifest.site} / ${view.catalog.manifest.environment}'),
            fact(l.agentMetricsInterval, view.catalog.manifest.scrapeIntervalSeconds.toString()),
            for (final target in view.catalog.manifest.targets) ...[
              fact('${l.agentMetricsTarget}: ${target.id} (${target.kind})', target.url),
              fact(
                l.agentMetricsTargetLimits,
                '${target.sampleLimit} / ${target.seriesLimit} / ${target.responseBytes} / ${target.labelLimit}',
              ),
            ],
          ],
        ),
      ],
    );
  }
}

/// Inert review modal until the actual opaque service port is available.
/// It publishes no callback that could pretend to configure or start a service.
class AgentCollectorServiceViewDialog extends StatefulWidget {
  const AgentCollectorServiceViewDialog({
    super.key,
    required this.view,
    required this.original,
    required this.originalChanges,
  });
  final AgentCollectorServiceView view;
  final bool Function() original;
  final Listenable originalChanges;
  @override
  State<AgentCollectorServiceViewDialog> createState() => _DialogState();
}

class _DialogState extends State<AgentCollectorServiceViewDialog> {
  late final AgentCollectorServiceView captured = widget.view;
  bool retired = false;
  bool refreshQueued = false;
  @override
  void initState() {
    super.initState();
    widget.originalChanges.addListener(check);
  }

  void check() {
    if (!mounted || retired) return;
    if (!widget.original()) {
      retired = true;
      if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
        if (!refreshQueued) {
          refreshQueued = true;
          WidgetsBinding.instance.addPostFrameCallback((_) {
            refreshQueued = false;
            if (mounted) setState(() {});
          });
        }
      } else {
        setState(() {});
      }
    }
  }

  @override
  void didUpdateWidget(covariant AgentCollectorServiceViewDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(check);
      widget.originalChanges.addListener(check);
    }
    if (!identical(captured, widget.view) || !widget.original()) retired = true;
  }

  @override
  void dispose() {
    widget.originalChanges.removeListener(check);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (!widget.original()) retired = true;
    return AlertDialog(
      title: Text(l.agentCollectorServiceTitle),
      content: SizedBox(
        width: 560,
        child: SingleChildScrollView(
          child: retired ? Text(l.agentCollectorServiceOriginalChanged) : AgentCollectorServiceReview(view: captured),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('collector-service-close'),
          onPressed: () => Navigator.pop(context),
          child: Text(l.commonClose),
        ),
      ],
    );
  }
}
