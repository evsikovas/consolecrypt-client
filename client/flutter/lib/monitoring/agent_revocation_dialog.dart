import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_revocation_controller.dart';
import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

class AgentRevocationDialog extends StatefulWidget {
  const AgentRevocationDialog({
    super.key,
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    required this.originalChanges,
    this.elapsedForReview,
  });
  final AgentAssociationRevocationPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final Listenable originalChanges;

  /// Display-clock seam only; actual expiry remains Native-owned.
  final Duration Function()? elapsedForReview;
  @override
  State<AgentRevocationDialog> createState() => _AgentRevocationDialogState();
}

class _AgentRevocationDialogState extends State<AgentRevocationDialog> {
  late final AgentRevocationController owner;
  @override
  void initState() {
    super.initState();
    owner = AgentRevocationController(
      port: widget.port,
      scope: widget.scope,
      record: widget.record,
      original: () => mounted && widget.original(),
      elapsedForReview: widget.elapsedForReview,
    )..addListener(refresh);
    widget.originalChanges.addListener(checkOriginal);
  }

  bool _queuedRefresh = false;
  void refresh() {
    if (!mounted) return;
    if (SchedulerBinding.instance.schedulerPhase ==
        SchedulerPhase.persistentCallbacks) {
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
  void didUpdateWidget(covariant AgentRevocationDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(checkOriginal);
      widget.originalChanges.addListener(checkOriginal);
    }
    if (!identical(old.port, widget.port) ||
        !identical(old.record, widget.record) ||
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
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [Text(label), SelectableText(value)],
    ),
  );
  @override
  Widget build(BuildContext context) {
    if (!owner.current) return const SizedBox.shrink();
    final l = context.l10n, v = owner.result ?? owner.details;
    return AlertDialog(
      title: Text(l.agentRevokeAccess),
      content: SizedBox(
        width: 640,
        height: (MediaQuery.sizeOf(context).height - 240).clamp(120.0, 560.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoBanner(
                tone: BannerTone.warning,
                message: l.agentRevokeDisclosure,
              ),
              if (owner.busy) const LinearProgressIndicator(),
              if (owner.error != null)
                InfoBanner(
                  tone: BannerTone.danger,
                  message:
                      owner.error is AppException &&
                          (owner.error! as AppException).code ==
                              AppErrorCode.vaultKeyMismatch
                      ? errorMessage(l, owner.error!)
                      : l.agentRevokeUnavailable,
                ),
              if (owner.unknown)
                InfoBanner(
                  key: const ValueKey('agent-revoke-unknown'),
                  tone: BannerTone.warning,
                  message: l.agentRevokeUnknown,
                ),
              if (owner.expired) Text(l.agentAssociationExpired),
              if (owner.cancelRequested)
                Text(l.agentAssociationCancelRequested),
              if (owner.awaitingCleanup) Text(l.agentAssociationCleanupWaiting),
              if (owner.cleanupUnknown) Text(l.agentAssociationCleanupUnknown),
              if (owner.cleanup?.cleanupState == 'closedObserved')
                Text(l.agentAssociationCleanupObserved),
              if (v != null) ...[
                if (owner.result == null &&
                    !owner.unknown &&
                    !owner.cancelRequested)
                  Text(
                    l.agentAssociationRemaining(
                      owner.remainingSeconds.toString(),
                    ),
                    key: const ValueKey('agent-revoke-countdown'),
                  ),
                fact(l.agentAssociationNode, v.record.agentId),
                fact(
                  l.agentAssociationSavedHost,
                  '${v.record.hostId} / ${v.record.hostRevision}',
                ),
                fact(
                  l.agentAssociationNodeFingerprint,
                  v.record.nodeKeyFingerprint!,
                ),
                fact(l.agentAssociationPublicKey, v.nodePublicKey),
                fact(l.agentAssociationNodeEpoch, v.nodeKeyEpoch),
                fact(l.agentAssociationOwner, '${v.ownerKind}: ${v.ownerId}'),
                fact(l.agentAssociationDevice, v.ownerDeviceId),
                fact(l.agentAssociationSigningKey, v.ownerSigningKeySha256),
                fact(
                  l.agentAssociationNodeContext,
                  '${v.nodeContextKind}: ${v.nodeContextId} / ${v.nodeDeploymentId}',
                ),
                fact(
                  l.agentAssociationTlsCertificate,
                  v.record.certificateDigest!,
                ),
                fact(l.agentAssociationCheckpoint, v.checkpoint),
                fact(l.agentRevokeActorRevision, v.actorRevision),
                if (owner.result != null)
                  InfoBanner(
                    key: const ValueKey('agent-revoke-observed'),
                    tone: BannerTone.info,
                    message: l.agentRevokeObserved,
                  ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('agent-revoke-close'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l.commonClose),
        ),
        if (!owner.busy &&
            owner.details == null &&
            owner.error == null &&
            !owner.cancelRequested)
          OutlinedButton(
            key: const ValueKey('agent-revoke-prepare'),
            onPressed: owner.prepare,
            child: Text(l.agentRevokeReview),
          ),
        if (owner.busy || owner.canConfirm)
          TextButton(
            key: const ValueKey('agent-revoke-cancel'),
            onPressed: owner.cancel,
            child: Text(l.commonCancel),
          ),
        if (owner.details != null && owner.result == null)
          FilledButton(
            key: const ValueKey('agent-revoke-confirm'),
            onPressed: owner.canConfirm ? owner.confirm : null,
            child: Text(l.agentRevokeConfirm),
          ),
      ],
    );
  }
}
