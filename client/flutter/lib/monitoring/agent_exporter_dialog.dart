import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_exporter.dart';
import 'package:consolecrypt/monitoring/agent_exporter_controller.dart';
import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

/// No read, preparation or start occurs merely by opening this dialog.
class AgentExporterDialog extends StatefulWidget {
  const AgentExporterDialog({
    super.key,
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    required this.originalChanges,
    this.now,
    this.elapsed,
  });
  final AgentExporterPort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final Listenable originalChanges;
  final DateTime Function()? now;
  final Duration Function()? elapsed;
  @override
  State<AgentExporterDialog> createState() => _State();
}

class _State extends State<AgentExporterDialog> {
  late final AgentExporterController owner;
  late final bool Function() original;
  bool retired = false, leaving = false, queued = false;
  int duration = 30;
  @override
  void initState() {
    super.initState();
    original = widget.original;
    owner = AgentExporterController(
      port: widget.port,
      scope: widget.scope,
      record: widget.record,
      original: () => mounted && !retired && original(),
      now: widget.now,
      elapsed: widget.elapsed,
    )..addListener(refresh);
    widget.originalChanges.addListener(checkOriginal);
  }

  void refresh() {
    if (!mounted) {
      return;
    }
    if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
      if (!queued) {
        queued = true;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          queued = false;
          if (mounted) setState(() {});
        });
      }
    } else {
      setState(() {});
    }
  }

  void checkOriginal() {
    if (!original()) {
      retired = true;
      owner.retire();
    }
  }

  @override
  void didUpdateWidget(covariant AgentExporterDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(checkOriginal);
      widget.originalChanges.addListener(checkOriginal);
    }
    if (!identical(old.port, widget.port) ||
        !identical(old.record, widget.record) ||
        !old.scope.same(widget.scope) ||
        !identical(old.originalChanges, widget.originalChanges) ||
        !original()) {
      retired = true;
      owner.retire();
    }
  }

  @override
  void dispose() {
    widget.originalChanges.removeListener(checkOriginal);
    owner.removeListener(refresh);
    unawaited(owner.close());
    owner.dispose();
    super.dispose();
  }

  Future<void> close() async {
    if (leaving) {
      return;
    }
    setState(() => leaving = true);
    await owner.close();
    if (mounted) {
      Navigator.of(context).pop();
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, v = owner.view, p = v?.policy ?? owner.catalog?.policy;
    String state() {
      if (owner.unknown || owner.retired) {
        return l.agentExporterUnknown;
      }
      return switch (v?.phase) {
        AgentExporterPhase.exporterReview => l.agentExporterReview,
        AgentExporterPhase.running => owner.stopSpent ? l.agentExporterStopRequested : l.agentExporterStartObserved,
        AgentExporterPhase.stopped => l.agentExporterStopped,
        AgentExporterPhase.cancelled => l.agentExporterUnknown,
        AgentExporterPhase.unknown => l.agentExporterUnknown,
        _ => l.agentExporterPrerequisites,
      };
    }

    return PopScope(
      canPop: false,
      child: AlertDialog(
        title: Text(l.agentExporterTitle),
        content: SizedBox(
          width: 600,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 380),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(state(), key: const ValueKey('agent-exporter-state')),
                  InfoBanner(tone: BannerTone.warning, message: l.agentExporterFiniteDisclosure),
                  if (p != null) ...[
                    Text('${p.address} · cpu, meminfo'), // l10n-ignore: endpoint and exporter collector identifiers.
                    Text('${l.agentExporterArtifact}: ${p.artifactVersion} / ${p.platform}'),
                    Text('${l.agentExporterExecutingVersion}: ${l.agentAssociationUnknown}'),
                    if (!owner.prepareSpent)
                      DropdownButton<int>(
                        value: duration,
                        items: [
                          for (final n in [30, 60, 120, 300])
                            if (n <= p.maxSeconds)
                              DropdownMenuItem(value: n, child: Text('$n ${l.agentExporterSeconds}')),
                        ],
                        onChanged: owner.busy
                            ? null
                            : (n) {
                                if (n != null) {
                                  setState(() => duration = n);
                                }
                              },
                      ),
                    if (v != null) ...[
                      Text('${l.agentExporterOriginalExpiry}: ${v.originalExpiresUtc.toIso8601String()}'),
                      Text('${owner.remainingMs ~/ 1000} ${l.agentExporterSeconds}'),
                    ],
                    ExpansionTile(
                      title: Text(l.agentExporterTechnical),
                      children: [
                        SelectableText(widget.record.agentId),
                        const SelectableText('exporters.manage'),
                        SelectableText(v?.manifestSha256 ?? owner.catalog!.manifestSha256),
                        SelectableText(p.binarySha256),
                      ],
                    ),
                  ],
                  if (owner.error != null || owner.cleanupError != null) Text(l.agentExporterUnknown),
                  if (owner.cleanup case final cleanup?)
                    Text(cleanup.closedObserved ? l.agentExporterCleanupObserved : l.agentExporterUnknown),
                ],
              ),
            ),
          ),
        ),
        actions: [
          if (!owner.querySpent)
            OutlinedButton(
              key: const ValueKey('agent-exporter-query'),
              onPressed: owner.canQuery ? owner.readCatalog : null,
              child: Text(l.agentExporterCheck),
            ),
          if (owner.catalog != null && !owner.prepareSpent)
            OutlinedButton(
              key: const ValueKey('agent-exporter-prepare'),
              onPressed: owner.canPrepare ? () => owner.prepare(duration) : null,
              child: Text(l.agentExporterReview),
            ),
          if (v?.phase == AgentExporterPhase.exporterReview)
            FilledButton(
              key: const ValueKey('agent-exporter-approve'),
              onPressed: owner.canApprove ? owner.approveOnce : null,
              child: Text(l.agentExporterStart),
            ),
          if (v != null && owner.current)
            OutlinedButton(onPressed: owner.canRefresh ? owner.refresh : null, child: Text(l.agentMetricsRefresh)),
          if (v?.phase == AgentExporterPhase.running)
            OutlinedButton(
              key: const ValueKey('agent-exporter-stop'),
              onPressed: owner.canStop ? owner.stop : null,
              child: Text(l.agentExporterStop),
            ),
          if (v != null && (owner.stopSpent || v.phase == AgentExporterPhase.stopped || owner.unknown))
            OutlinedButton(
              onPressed: owner.busy || owner.closedObserved ? null : owner.observeCleanup,
              child: Text(l.agentMetricsObserveCleanup),
            ),
          TextButton(
            key: const ValueKey('agent-exporter-close'),
            onPressed: leaving ? null : close,
            child: Text(l.commonClose),
          ),
        ],
      ),
    );
  }
}
