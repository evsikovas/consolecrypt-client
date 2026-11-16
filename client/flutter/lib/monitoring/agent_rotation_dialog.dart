import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';
import 'package:consolecrypt/monitoring/agent_rotation_controller.dart';
import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

class AgentRotationDialog extends StatefulWidget {
  const AgentRotationDialog({
    super.key,
    required this.port,
    required this.scope,
    required this.record,
    required this.mode,
    required this.original,
    required this.originalChanges,
    this.elapsedForReview,
  });
  final AgentAssociationRotationPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final AgentRotationMode mode;
  final bool Function() original;
  final Listenable originalChanges;

  /// Display-clock seam only; actual expiry remains Native-owned.
  final Duration Function()? elapsedForReview;
  @override
  State<AgentRotationDialog> createState() => _AgentRotationDialogState();
}

class _AgentRotationDialogState extends State<AgentRotationDialog> {
  late final AgentRotationController owner;
  @override
  void initState() {
    super.initState();
    owner = AgentRotationController(
      port: widget.port,
      scope: widget.scope,
      record: widget.record,
      mode: widget.mode,
      original: () => mounted && widget.original(),
      elapsedForReview: widget.elapsedForReview,
    )..addListener(refresh);
    widget.originalChanges.addListener(checkOriginal);
  }

  bool _queuedRefresh = false;
  void refresh() {
    if (!mounted) return;
    if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
      if (!_queuedRefresh) {
        _queuedRefresh = true;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          _queuedRefresh = false;
          if (mounted) setState(() {});
        });
      }
    } else {
      setState(() {});
    }
  }

  void checkOriginal() {
    if (!widget.original()) owner.retire();
  }

  @override
  void didUpdateWidget(covariant AgentRotationDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(checkOriginal);
      widget.originalChanges.addListener(checkOriginal);
    }
    if (!identical(old.port, widget.port) ||
        !identical(old.record, widget.record) ||
        old.mode != widget.mode ||
        !old.scope.same(widget.scope) ||
        !widget.original()) {
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
  @override
  Widget build(BuildContext context) {
    if (!owner.current) return const SizedBox.shrink();
    final l = context.l10n, v = owner.result ?? owner.details;
    return AlertDialog(
      title: Text(widget.mode == AgentRotationMode.rotate ? l.agentRotationTitle : l.agentRotationReconcileTitle),
      content: SizedBox(
        width: 640,
        height: (MediaQuery.sizeOf(context).height - 240).clamp(120.0, 560.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoBanner(
                tone: BannerTone.warning,
                message: widget.mode == AgentRotationMode.rotate
                    ? l.agentRotationDisclosure
                    : l.agentRotationReconcileDisclosure,
              ),
              if (owner.busy) const LinearProgressIndicator(),
              if (owner.error != null)
                InfoBanner(
                  tone: BannerTone.danger,
                  message:
                      owner.error is AppException &&
                          (owner.error! as AppException).code == AppErrorCode.vaultKeyMismatch
                      ? errorMessage(l, owner.error!)
                      : l.agentRotationUnavailable,
                ),
              if (owner.unknown)
                InfoBanner(
                  key: const ValueKey('agent-rotation-unknown'),
                  tone: BannerTone.warning,
                  message: l.agentRotationUnknown,
                ),
              if (owner.expired) Text(l.agentAssociationExpired),
              if (owner.cancelRequested) Text(l.agentAssociationCancelRequested),
              if (owner.awaitingCleanup) Text(l.agentAssociationCleanupWaiting),
              if (owner.cleanupUnknown) Text(l.agentAssociationCleanupUnknown),
              if (owner.cleanup?.cleanupState == 'closedObserved') Text(l.agentAssociationCleanupObserved),
              if (v != null) ...[
                if (owner.result == null && !owner.unknown && !owner.cancelRequested)
                  Text(
                    l.agentAssociationRemaining(owner.remainingSeconds.toString()),
                    key: const ValueKey('agent-rotation-countdown'),
                  ),
                fact(l.agentAssociationNode, v.record.agentId),
                fact(l.agentAssociationSavedHost, '${v.record.hostId} / ${v.record.hostRevision}'),
                fact(l.agentAssociationOwner, '${v.ownerKind}: ${v.ownerId}'),
                fact(l.agentAssociationDevice, v.ownerDeviceId),
                fact(l.agentAssociationSigningKey, v.ownerSigningKeySha256),
                fact(l.agentAssociationNodeContext, '${v.nodeContextKind}: ${v.nodeContextId} / ${v.deploymentId}'),
                fact(l.agentRotationPrevious, '${v.previousNode.keyEpoch} / ${v.previousNode.fingerprint}'),
                fact(l.agentAssociationPublicKey, v.previousNode.publicKey),
                if (v.proposedNode case final AgentRotationNode proposed) ...[
                  fact(l.agentRotationProposed, '${proposed.keyEpoch} / ${proposed.fingerprint}'),
                  fact(l.agentAssociationPublicKey, proposed.publicKey),
                  fact(l.agentRotationProposal, v.rotationId!),
                ],
                fact(l.agentAssociationTlsCertificate, v.certificateDigest),
                fact(l.agentAssociationActualExpiry, v.certificateExpiresUtc.toIso8601String()),
                fact(l.agentAssociationCheckpoint, v.checkpoint),
                fact(l.agentRevokeActorRevision, v.actorRevision),
                if (v.localBlocked == true) InfoBanner(tone: BannerTone.warning, message: l.agentRotationLocalBlocked),
                if (owner.result != null)
                  InfoBanner(
                    key: const ValueKey('agent-rotation-observed'),
                    tone: BannerTone.info,
                    message: l.agentRotationObserved,
                  ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('agent-rotation-close'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l.commonClose),
        ),
        if (!owner.busy && owner.details == null && owner.error == null && !owner.cancelRequested)
          OutlinedButton(
            key: const ValueKey('agent-rotation-prepare'),
            onPressed: owner.prepare,
            child: Text(l.agentRotationReview),
          ),
        if (owner.busy || owner.canConfirm || owner.canPrepareCandidate)
          TextButton(
            key: const ValueKey('agent-rotation-cancel'),
            onPressed: owner.cancel,
            child: Text(l.commonCancel),
          ),
        if (owner.canPrepareCandidate)
          FilledButton(
            key: const ValueKey('agent-rotation-candidate'),
            onPressed: owner.prepareCandidate,
            child: Text(l.agentRotationPrepareCandidate),
          ),
        if (owner.details != null && owner.result == null && owner.details?.proposedNode != null)
          FilledButton(
            key: const ValueKey('agent-rotation-confirm'),
            onPressed: owner.canConfirm ? owner.confirm : null,
            child: Text(l.agentRotationConfirm),
          ),
      ],
    );
  }
}
