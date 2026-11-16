import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap_controller.dart';
import 'package:material_ui/material_ui.dart';

class AgentOutboundBootstrapDialog extends StatefulWidget {
  const AgentOutboundBootstrapDialog({
    super.key,
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    required this.originalChanges,
  });
  final AgentOutboundBootstrapPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final Listenable originalChanges;
  @override
  State<AgentOutboundBootstrapDialog> createState() => _State();
}

class _State extends State<AgentOutboundBootstrapDialog> {
  AgentOutboundBootstrapController? owner;
  bool enabled = false;
  int ttl = 3600;
  @override
  void initState() {
    super.initState();
    widget.originalChanges.addListener(checkOriginal);
  }

  void refresh() {
    if (mounted) setState(() {});
  }

  void checkOriginal() {
    if (!widget.original()) owner?.retire();
  }

  @override
  void didUpdateWidget(covariant AgentOutboundBootstrapDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(checkOriginal);
      widget.originalChanges.addListener(checkOriginal);
    }
    if (!identical(old.port, widget.port) ||
        !old.scope.same(widget.scope) ||
        !identical(old.record, widget.record) ||
        !widget.original()) {
      owner?.retire();
    }
  }

  Future<void> prepare() async {
    if (!enabled || owner != null || !widget.original()) return;
    final captured = widget;
    final created = AgentOutboundBootstrapController(
      port: captured.port,
      scope: captured.scope,
      intent: AgentOutboundBootstrapIntent(captured.record, ttlSeconds: ttl),
      original: () =>
          mounted &&
          identical(widget.port, captured.port) &&
          identical(widget.record, captured.record) &&
          widget.scope.same(captured.scope) &&
          widget.original(),
    )..addListener(refresh);
    setState(() => owner = created);
    await created.prepare();
  }

  @override
  void dispose() {
    widget.originalChanges.removeListener(checkOriginal);
    final captured = owner;
    captured?.removeListener(refresh);
    captured?.dispose();
    super.dispose();
  }

  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(label), SelectableText(value)]),
  );
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, o = owner, v = o?.view;
    return AlertDialog(
      title: Text(l.agentOutboundTitle),
      content: SizedBox(
        width: 560,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(l.agentOutboundSummary),
              if (o == null) ...[
                CheckboxListTile(
                  key: const ValueKey('agent-outbound-opt-in'),
                  value: enabled,
                  title: Text(l.agentOutboundOptIn),
                  onChanged: widget.original() ? (v) => setState(() => enabled = v ?? false) : null,
                ),
                DropdownButtonFormField<int>(
                  key: const ValueKey('agent-outbound-ttl'),
                  initialValue: ttl,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.agentOutboundLifetime),
                  items: [
                    DropdownMenuItem(
                      value: 3600,
                      child: Tooltip(
                        message: l.agentOutboundOneHour,
                        child: Text(l.agentOutboundOneHour, maxLines: 1, overflow: TextOverflow.ellipsis),
                      ),
                    ),
                    DropdownMenuItem(
                      value: 21600,
                      child: Tooltip(
                        message: l.agentOutboundSixHours,
                        child: Text(l.agentOutboundSixHours, maxLines: 1, overflow: TextOverflow.ellipsis),
                      ),
                    ),
                    DropdownMenuItem(
                      value: 86400,
                      child: Tooltip(
                        message: l.agentOutboundOneDay,
                        child: Text(l.agentOutboundOneDay, maxLines: 1, overflow: TextOverflow.ellipsis),
                      ),
                    ),
                  ],
                  onChanged: widget.original() ? (v) => setState(() => ttl = v ?? 3600) : null,
                ),
              ],
              if (v != null) ...[
                Text(l.agentOutboundCaps),
                fact(l.agentOutboundCappedLifetime, l.agentOutboundSeconds(v.maximumLifetimeSeconds)),
                if (!o!.approvalSpent) Text(l.agentOutboundReviewRemaining(o.remainingSeconds)),
                if (v.registrationExpiresAt != null)
                  fact(l.agentOutboundExpiry, v.registrationExpiresAt!.toIso8601String()),
                if (v.phase == 'configured' && !o.unknown) Text(l.agentOutboundConfigured),
                ExpansionTile(
                  title: Text(l.agentOutboundDetails),
                  children: [
                    fact(l.agentOutboundAssociation, widget.record.id),
                    fact(l.agentOutboundAgent, v.node.agentId),
                    fact(l.agentOutboundOwnerAccount, v.ownerAccountId),
                    fact(l.agentAssociationNodeFingerprint, v.node.fingerprint),
                  ],
                ),
              ],
              if (o?.expired ?? false) Text(l.agentOutboundExpired),
              if (o?.unknown ?? false) Text(l.agentOutboundUnknown),
              if (o?.awaitingCleanup ?? false) Text(l.agentOutboundJoining),
              if (o?.cleanup?.state == 'closedObserved') Text(l.agentOutboundClosed),
              if (o?.error != null) Text(errorMessage(l, o!.error!)),
            ],
          ),
        ),
      ),
      actions: [
        if (o == null)
          TextButton(
            key: const ValueKey('agent-outbound-prepare'),
            onPressed: enabled && widget.original() ? prepare : null,
            child: Text(l.agentOutboundReview),
          ),
        if (o?.canApprove ?? false)
          TextButton(
            key: const ValueKey('agent-outbound-approve'),
            onPressed: o!.approveOnce,
            child: Text(l.agentOutboundConfirm),
          ),
        if (o != null && !o.cancelRequested && v?.phase != 'configured')
          TextButton(key: const ValueKey('agent-outbound-cancel'), onPressed: o.cancel, child: Text(l.commonCancel)),
        if (o == null || o.cancelRequested || v?.phase == 'configured')
          TextButton(
            key: const ValueKey('agent-outbound-close'),
            onPressed: () => Navigator.pop(context),
            child: Text(l.commonClose),
          ),
      ],
    );
  }
}
