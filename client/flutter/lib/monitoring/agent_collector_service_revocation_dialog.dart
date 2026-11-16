import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation_controller.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

class AgentCollectorServiceRevocationDialog extends StatefulWidget {
  const AgentCollectorServiceRevocationDialog({
    super.key,
    required this.port,
    required this.target,
    required this.original,
    required this.originalChanges,
    this.elapsed,
  });
  final AgentCollectorServiceRevocationPort port;
  final AgentCollectorServiceStatus target;
  final bool Function() original;
  final Listenable originalChanges;
  final Duration Function()? elapsed;
  @override
  State<AgentCollectorServiceRevocationDialog> createState() => _RevokeState();
}

class _RevokeState extends State<AgentCollectorServiceRevocationDialog> {
  late final AgentCollectorServiceRevocationController owner;
  bool consent = false, closing = false, refreshQueued = false;
  @override
  void initState() {
    super.initState();
    final captured = widget;
    owner = AgentCollectorServiceRevocationController(
      port: captured.port,
      target: captured.target,
      elapsed: captured.elapsed,
      original: () =>
          mounted &&
          identical(widget.port, captured.port) &&
          identical(widget.target, captured.target) &&
          widget.original(),
    )..addListener(changed);
    widget.originalChanges.addListener(checkOriginal);
  }

  void changed() {
    if (!mounted) return;
    if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
      if (refreshQueued) return;
      refreshQueued = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        refreshQueued = false;
        if (mounted) setState(() {});
      });
    } else {
      setState(() {});
    }
  }

  void checkOriginal() {
    if (!widget.original()) owner.retire();
  }

  @override
  void didUpdateWidget(covariant AgentCollectorServiceRevocationDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(checkOriginal);
      widget.originalChanges.addListener(checkOriginal);
    }
    if (!identical(old.port, widget.port) || !identical(old.target, widget.target) || !widget.original()) {
      owner.retire();
    }
  }

  Future<void> close() async {
    if (closing) return;
    setState(() => closing = true);
    await owner.close();
    if (mounted) Navigator.pop(context);
  }

  @override
  void dispose() {
    widget.originalChanges.removeListener(checkOriginal);
    owner.removeListener(changed);
    owner.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, o = owner, v = o.view;
    final canCancel = o.prepareSpent && !o.cancelRequested && (!o.confirmSpent || o.busy);
    return AlertDialog(
      title: Text(l.agentCollectorServiceRevokeTitle),
      content: SizedBox(
        width: 560,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (!o.current) Text(l.agentCollectorServiceOriginalChanged),
              if (o.current) ...[
                Text(l.agentCollectorServiceRevokeDisclosure),
                if (o.current &&
                    v?.phase == AgentCollectorServiceRevocationPhase.serviceRevokeReview &&
                    !o.confirmSpent &&
                    !o.cancelRequested) ...[
                  Text(l.agentCollectorServiceRemaining(o.remainingSeconds)),
                  CheckboxListTile(
                    key: const ValueKey('collector-service-revoke-consent'),
                    value: consent,
                    onChanged: o.canConfirm ? (value) => setState(() => consent = value ?? false) : null,
                    title: Text(l.agentCollectorServiceRevokeConsent),
                  ),
                ],
                if (o.localBlocked) Text(l.agentCollectorServiceRevokeLocalBlocked),
                if (o.remoteRevoked) Text(l.agentCollectorServiceRemoteRevoked),
                if (o.unknown || o.error != null) Text(l.agentCollectorServiceRevokeUnknown),
                if (o.expired) Text(l.agentCollectorServiceExpired),
                if (o.awaitingCleanup) Text(l.agentCollectorServiceJoining),
                if (o.cleanupUnknown) Text(l.agentCollectorServiceCleanupUnknown),
                if (o.cleanup?.state == 'closedObserved') Text(l.agentCollectorServiceCallerClosed),
                ExpansionTile(
                  title: Text(l.agentCollectorServiceTechnical),
                  children: [
                    SelectableText(widget.target.record.hostId),
                    SelectableText(widget.target.serviceId),
                    SelectableText(widget.target.serviceHash),
                    if (v != null) ...[
                      SelectableText(v.ownerId),
                      SelectableText(v.ownerDeviceId),
                      SelectableText(v.node.fingerprint),
                    ],
                  ],
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        if (o.canPrepare)
          TextButton(
            key: const ValueKey('collector-service-revoke-prepare'),
            onPressed: o.prepare,
            child: Text(l.agentCollectorServiceRevokePrepare),
          ),
        if (o.canConfirm)
          FilledButton(
            key: const ValueKey('collector-service-revoke-confirm'),
            onPressed: consent ? o.confirmRevocation : null,
            child: Text(l.agentCollectorServiceRevokeConfirm),
          ),
        if (canCancel)
          TextButton(
            key: const ValueKey('collector-service-revoke-cancel'),
            onPressed: o.cancel,
            child: Text(l.agentCollectorServiceCancelLocal),
          ),
        if (!canCancel)
          TextButton(
            key: const ValueKey('collector-service-revoke-close'),
            onPressed: closing ? null : close,
            child: Text(l.commonClose),
          ),
      ],
    );
  }
}
