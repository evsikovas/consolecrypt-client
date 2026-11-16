import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_controller.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_models.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_port.dart';
import 'package:consolecrypt/monitoring/agent_metrics_runtime_review.dart';
import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

/// Explicit review entry; mounting this dialog never probes or starts a unit.
class AgentMetricsCollectorDialog extends StatefulWidget {
  const AgentMetricsCollectorDialog({
    super.key,
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    required this.originalChanges,
    this.elapsed,
    this.now,
  });
  final AgentMetricsCollectorPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final Listenable originalChanges;
  final Duration Function()? elapsed;
  final DateTime Function()? now;
  @override
  State<AgentMetricsCollectorDialog> createState() => _State();
}

class _State extends State<AgentMetricsCollectorDialog> {
  late final AgentMetricsCollectorController owner;
  late final bool Function() _original;
  bool _sourceRetired = false, _queued = false;
  @override
  void initState() {
    super.initState();
    _original = widget.original;
    owner = AgentMetricsCollectorController(
      port: widget.port,
      scope: widget.scope,
      record: widget.record,
      original: () => mounted && !_sourceRetired && _original(),
      elapsed: widget.elapsed,
      now: widget.now,
    )..addListener(refresh);
    widget.originalChanges.addListener(checkOriginal);
  }

  void refresh() {
    if (!mounted) return;
    if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
      if (!_queued) {
        _queued = true;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          _queued = false;
          if (mounted) setState(() {});
        });
      }
    } else {
      setState(() {});
    }
  }

  void checkOriginal() {
    if (!_original()) {
      _sourceRetired = true;
      owner.retire();
      refresh();
    }
  }

  @override
  void didUpdateWidget(covariant AgentMetricsCollectorDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(checkOriginal);
      widget.originalChanges.addListener(checkOriginal);
    }
    if (!identical(old.port, widget.port) ||
        !identical(old.record, widget.record) ||
        !old.scope.same(widget.scope) ||
        !identical(old.originalChanges, widget.originalChanges) ||
        !_original()) {
      _sourceRetired = true;
      owner.retire();
    }
  }

  @override
  void dispose() {
    widget.originalChanges.removeListener(checkOriginal);
    owner.removeListener(refresh);
    owner.dispose();
    super.dispose();
  }

  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(label), SelectableText(value)]),
  );
  String phase(AppLocalizations l, AgentMetricsCollectorPhase p) => switch (p) {
    AgentMetricsCollectorPhase.collectorReview => l.agentMetricsReview,
    AgentMetricsCollectorPhase.starting => l.agentMetricsStarting,
    AgentMetricsCollectorPhase.running => l.agentMetricsRunning,
    AgentMetricsCollectorPhase.stopped => l.agentMetricsStopped,
    AgentMetricsCollectorPhase.cancelled => l.agentAssociationCancelRequested,
    AgentMetricsCollectorPhase.unknown => l.agentMetricsUnknown,
  };
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, v = owner.view, showOriginal = !_sourceRetired && _original();
    final cancelLocalAvailable =
        showOriginal &&
        owner.prepareSpent &&
        !owner.cancelled &&
        owner.cleanup?.state != AgentMetricsCleanupState.closedObserved;

    return AlertDialog(
      title: Text(l.agentMetricsTitle),
      content: SizedBox(
        width: 640,
        height: (MediaQuery.sizeOf(context).height - 240).clamp(120.0, 600.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoBanner(
                tone: BannerTone.warning,
                message: v == null
                    ? l.agentMetricsModePending
                    : v.persistentDisk
                    ? l.agentMetricsDiskDisclosure
                    : l.agentMetricsDisclosure,
              ),

              if (!owner.supported)
                InfoBanner(
                  key: const ValueKey('agent-metrics-unsupported'),
                  tone: BannerTone.warning,
                  message: l.agentMetricsEnrollmentRequired,
                ),
              if (!showOriginal) Text(l.agentMetricsOriginalRetired),
              if (owner.busy || owner.joining || owner.awaitingCleanup) const LinearProgressIndicator(),
              if (owner.error != null)
                InfoBanner(
                  key: const ValueKey('agent-metrics-error'),
                  tone: BannerTone.danger,
                  message:
                      owner.error is AppException &&
                          (owner.error! as AppException).code == AppErrorCode.vaultKeyMismatch
                      ? errorMessage(l, owner.error!)
                      : l.agentMetricsUnavailable,
                ),
              if (owner.unknown || owner.cleanupError != null)
                InfoBanner(
                  key: const ValueKey('agent-metrics-unknown'),
                  tone: BannerTone.warning,
                  message: l.agentMetricsUnknown,
                ),
              if (owner.expired) Text(l.agentAssociationExpired),
              if (owner.cancelled) Text(l.agentAssociationCancelRequested),
              if (owner.awaitingCleanup || owner.joining) Text(l.agentAssociationCleanupWaiting),
              if (owner.cleanup?.state == AgentMetricsCleanupState.closedObserved)
                Text(l.agentMetricsCleanupObserved, key: const ValueKey('agent-metrics-cleanup')),
              if (owner.cleanup?.state == AgentMetricsCleanupState.unknown) Text(l.agentAssociationCleanupUnknown),
              if (showOriginal) ...[
                Text(l.agentMetricsDuration),
                GlassSelect<int>(
                  key: const ValueKey('agent-metrics-duration'),
                  value: owner.durationSeconds,
                  semanticLabel: l.agentMetricsDuration,
                  items: [
                    for (final value in const [30, 60, 120, 300]) GlassSelectItem(value: value, label: '$value'),
                  ],
                  onChanged: owner.canPrepare
                      ? (value) {
                          owner.setDuration(value);
                        }
                      : null,
                ),
                if (v != null) ...[
                  Text(phase(l, v.phase), key: const ValueKey('agent-metrics-phase')),
                  if (v.phase == AgentMetricsCollectorPhase.running && owner.stopSpent)
                    Text(l.agentMetricsStopRequested, key: const ValueKey('agent-metrics-stop-requested')),
                  Text(
                    l.agentAssociationRemaining(owner.remainingSeconds.toString()),
                    key: const ValueKey('agent-metrics-countdown'),
                  ),
                  fact(l.agentMetricsDelivery, l.agentMetricsConfiguredReceiver),
                  if (v.persistentDisk)
                    Text(l.agentMetricsDiskBuffer, key: const ValueKey('agent-metrics-disk-buffer')),
                  fact(l.agentMetricsReceiver, v.collector.manifest.receiverUrl),
                  fact(l.agentMetricsActualExpiry, v.originalExpiresUtc.toIso8601String()),
                  if (v.collector.manifest.runtime case final runtime?)
                    AgentMetricsRuntimeReview(runtime: runtime),
                  ExpansionTile(
                    key: const ValueKey('agent-metrics-review-details'),
                    title: Text(l.agentMetricsIdentityDetails),
                    children: [
                      fact(l.agentMetricsScope, v.scopeName),
                      fact(l.agentMetricsMode, v.collector.manifest.mode),
                      fact(l.agentAssociationSavedHost, '${v.hostId} / ${v.hostRevision}'),
                      fact(l.agentAssociationNode, v.agentId),
                      fact(l.agentAssociationNodeEpoch, v.nodeKeyEpoch),
                      fact(l.agentAssociationNodeFingerprint, v.nodeFingerprint),
                      fact(l.agentAssociationOwner, '${v.ownerKind}: ${v.ownerId}'),
                      fact(l.agentAssociationDevice, v.ownerDeviceId),
                      fact(l.agentAssociationSigningKey, v.ownerSigningKeySha256),
                      fact(l.agentMetricsCollectorId, v.collector.collectorId),
                      fact(l.agentMetricsManifestSha, v.collector.manifestSha256),
                      fact(l.agentMetricsLabels, '${v.collector.manifest.site} / ${v.collector.manifest.environment}'),
                      fact(l.agentMetricsInterval, '${v.collector.manifest.scrapeIntervalSeconds} s'),
                      fact(l.agentMetricsBounds, '${v.maxMemoryBytes} / ${v.maxTemporaryStateBytes}'),
                      fact(
                        l.agentMetricsBuffer,
                        '${v.collector.manifest.maxBufferBytes} / ${v.collector.manifest.maxBufferAgeSeconds} s',
                      ),
                      for (final target in v.collector.manifest.targets) ...[
                        fact('${l.agentMetricsTarget}: ${target.id} (${target.kind})', target.url),
                        fact(
                          l.agentMetricsTargetLimits,
                          '${target.sampleLimit} / ${target.seriesLimit} / ${target.responseBytes} / ${target.labelLimit}',
                        ),
                      ],
                    ],
                  ),
                ],
              ],
              ExpansionTile(
                key: const ValueKey('agent-metrics-disclosure-details'),
                title: Text(l.agentMetricsDeliveryDetails),
                children: [
                  Padding(
                    padding: const EdgeInsets.all(12),
                    child: Text(
                      v == null
                          ? l.agentMetricsModePending
                          : v.persistentDisk
                          ? l.agentMetricsDiskDetails
                          : l.agentMetricsDisclosureDetails,
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
      actions: [
        if (!cancelLocalAvailable)
          TextButton(
            key: const ValueKey('agent-metrics-close'),
            onPressed: () => Navigator.of(context).pop(),
            child: Text(l.commonClose),
          ),
        if (showOriginal && owner.canPrepare)
          OutlinedButton(
            key: const ValueKey('agent-metrics-prepare'),
            onPressed: owner.prepare,
            child: Text(l.agentMetricsPrepare),
          ),
        if (showOriginal && v != null && !owner.cancelled)
          OutlinedButton(
            key: const ValueKey('agent-metrics-refresh'),
            onPressed: owner.busy || owner.joining ? null : owner.refresh,
            child: Text(l.agentMetricsRefresh),
          ),
        if (cancelLocalAvailable && v?.phase == AgentMetricsCollectorPhase.collectorReview)
          FilledButton(
            key: const ValueKey('agent-metrics-approve'),
            onPressed: owner.canApprove ? owner.approveOnce : null,
            child: Text(l.agentMetricsApprove),
          ),
        if (cancelLocalAvailable && v?.phase == AgentMetricsCollectorPhase.running)
          FilledButton(
            key: const ValueKey('agent-metrics-stop'),
            onPressed: owner.canStop ? owner.stop : null,
            child: Text(l.agentMetricsStop),
          ),
        if (showOriginal &&
            v != null &&
            !owner.cancelled &&
            (owner.stopSpent ||
                const {AgentMetricsCollectorPhase.stopped, AgentMetricsCollectorPhase.unknown}.contains(v.phase)))
          OutlinedButton(
            key: const ValueKey('agent-metrics-joined'),
            onPressed: owner.busy || owner.joining ? null : owner.observeCleanup,
            child: Text(l.agentMetricsObserveCleanup),
          ),
        if (cancelLocalAvailable)
          TextButton(
            key: const ValueKey('agent-metrics-cancel'),
            onPressed: owner.cancel,
            child: Text(l.agentMetricsCancelLocalRequest),
          ),
      ],
    );
  }
}
