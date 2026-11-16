import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_resume_preference.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_controller.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_models.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_saved_route_port.dart';
import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

/// Only loopback choices and an original saved association enter this view.
/// Its controller retains the actual issued opaque handle through cleanup.
class AgentTunnelRouteDialog extends StatefulWidget {
  const AgentTunnelRouteDialog({
    super.key,
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    required this.originalChanges,
    this.elapsedForReview,
  });
  final AgentTunnelRoutePort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final Listenable originalChanges;
  final Duration Function()? elapsedForReview;
  @override
  State<AgentTunnelRouteDialog> createState() => _State();
}

class _State extends State<AgentTunnelRouteDialog> {
  final listen = TextEditingController(text: '0'),
      destination = TextEditingController(text: '9090'),
      ttl = TextEditingController(text: '60'),
      idle = TextEditingController(text: '30');
  String listenIp = '127.0.0.1', destinationIp = '127.0.0.1';
  AgentTunnelRouteController? owner;
  late final AgentTunnelResumePreferenceController resumePreference;
  bool resumeOriginal = false;
  AgentTunnelAccessAudit? audit;
  Object? formError, auditError;
  int generation = 0, page = 0;
  bool auditBusy = false, leaving = false, queuedRefresh = false, retired = false;
  bool get current => mounted && !retired && widget.original();
  bool get frozen => owner?.prepareSpent == true || owner?.busy == true;
  bool get needsSavedLookup => resumeOriginal && widget.port is AgentTunnelSavedRoutePort;
  bool get matchingLookupNone {
    final previous = owner;
    if (previous?.noSavedOriginal != true) return false;
    try {
      final requested = intent(), captured = previous!.intent;
      return identical(requested.record, captured.record) &&
          requested.listenIp == captured.listenIp &&
          requested.listenPort == captured.listenPort &&
          requested.destinationIp == captured.destinationIp &&
          requested.destinationPort == captured.destinationPort &&
          requested.ttlSeconds == captured.ttlSeconds &&
          requested.idleSeconds == captured.idleSeconds &&
          requested.resumeOriginal == captured.resumeOriginal;
    } catch (_) {
      return false;
    }
  }

  @override
  void initState() {
    super.initState();
    widget.originalChanges.addListener(checkOriginal);
    AgentTunnelRouteIntent? cached;
    final originalPort = widget.port, originalScope = widget.scope, originalRecord = widget.record;
    if (widget.original() && originalPort is AgentTunnelSavedRoutePort) {
      cached = (originalPort as AgentTunnelSavedRoutePort).savedIntents.restore(originalScope, originalRecord);
    }
    if (cached != null) {
      // Values come only from a validated prior explicit detach in this SAME
      // service/scope. They are choices, not evidence that a tunnel is active.
      listenIp = cached.listenIp;
      listen.text = cached.listenPort.toString();
      destinationIp = cached.destinationIp;
      destination.text = cached.destinationPort.toString();
      ttl.text = cached.ttlSeconds.toString();
      idle.text = cached.idleSeconds.toString();
      resumeOriginal = true;
    }
    resumePreference = AgentTunnelResumePreferenceController(
      port: widget.port is AgentTunnelResumePreferencePort ? widget.port as AgentTunnelResumePreferencePort : null,
      scope: widget.scope,
      original: () => current,
    );
    resumePreference.select(resumeDestination());
    resumePreference.addListener(refresh);
    destination.addListener(destinationChanged);
    for (final control in [listen, ttl, idle]) {
      control.addListener(refresh);
    }
    if (cached != null) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (current &&
            identical(widget.port, originalPort) &&
            widget.scope.same(originalScope) &&
            identical(widget.record, originalRecord)) {
          unawaited(attachSaved());
        }
      });
    }
  }

  void refresh() {
    if (!mounted) return;
    if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
      if (!queuedRefresh) {
        queuedRefresh = true;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          queuedRefresh = false;
          if (mounted) setState(() {});
        });
      }
    } else {
      setState(() {});
    }
  }

  void checkOriginal() {
    if (!widget.original()) {
      retired = true;
      generation++;
      owner?.retire();
      resumePreference.retire();
      refresh();
    }
  }

  @override
  void didUpdateWidget(covariant AgentTunnelRouteDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(checkOriginal);
      widget.originalChanges.addListener(checkOriginal);
    }
    if (!identical(old.port, widget.port) ||
        !identical(old.record, widget.record) ||
        !old.scope.same(widget.scope) ||
        !widget.original()) {
      retired = true;
      generation++;
      owner?.retire();
      resumePreference.retire();
    }
  }

  @override
  void dispose() {
    generation++;
    widget.originalChanges.removeListener(checkOriginal);
    destination.removeListener(destinationChanged);
    for (final control in [listen, ttl, idle]) {
      control.removeListener(refresh);
    }
    resumePreference.removeListener(refresh);
    resumePreference.dispose();
    owner?.removeListener(refresh);
    owner?.dispose();
    for (final c in [listen, destination, ttl, idle]) {
      c.dispose();
    }
    super.dispose();
  }

  AgentTunnelResumeQuery? resumeDestination() {
    try {
      return AgentTunnelResumeQuery(widget.record, destinationIp, int.tryParse(destination.text) ?? -1);
    } catch (_) {
      return null;
    }
  }

  void destinationChanged() {
    if (!current || frozen) return;
    resumePreference.select(resumeDestination());
    setState(() => resumeOriginal = false);
  }

  AgentTunnelRouteIntent intent() => AgentTunnelRouteIntent(
    record: widget.record,
    listenIp: listenIp,
    listenPort: int.tryParse(listen.text) ?? -1,
    destinationIp: destinationIp,
    destinationPort: int.tryParse(destination.text) ?? -1,
    ttlSeconds: int.tryParse(ttl.text) ?? -1,
    idleSeconds: int.tryParse(idle.text) ?? -1,
    resumeOriginal: resumeOriginal,
  );
  Future<void> prepare() => startReview(attach: false);
  Future<void> attachSaved() => startReview(attach: true);
  Future<void> startReview({required bool attach}) async {
    if (!current || frozen || leaving) return;
    if (!attach && needsSavedLookup && !matchingLookupNone) return;
    if (attach && !needsSavedLookup) return;
    try {
      final frozenIntent = intent();
      resumePreference.retire();
      final previous = owner;
      previous?.removeListener(refresh);
      previous?.dispose();
      final next = AgentTunnelRouteController(
        port: widget.port,
        scope: widget.scope,
        intent: frozenIntent,
        original: () => current,
        elapsed: widget.elapsedForReview,
      );
      owner = next..addListener(refresh);
      formError = null;
      refresh();
      if (attach) {
        await next.attachSaved();
      } else {
        await next.prepare();
      }
    } catch (e) {
      if (current) setState(() => formError = e);
    }
  }

  Future<void> leaveOriginal() async {
    final captured = owner;
    if (!current || leaving || captured?.canDetach != true) return;
    await captured!.detachAutoOpen();
    if (mounted && identical(owner, captured) && captured.detached) await leave();
  }

  Future<void> loadAudit() async {
    if (!current || auditBusy || leaving) return;
    final originalPort = widget.port, originalScope = widget.scope, token = ++generation;
    setState(() {
      auditBusy = true;
      auditError = null;
    });
    try {
      final value = await originalPort.audit(originalScope);
      if (current && token == generation && identical(widget.port, originalPort) && widget.scope.same(originalScope)) {
        setState(() {
          audit = value;
          page = 0;
        });
      }
    } catch (e) {
      if (current && token == generation) setState(() => auditError = e);
    } finally {
      if (current && token == generation) setState(() => auditBusy = false);
    }
  }

  Future<void> leave() async {
    if (leaving || !mounted) return;
    final route = ModalRoute.of(context), navigator = Navigator.of(context);
    setState(() => leaving = true);
    await owner?.close();
    // Never pop a newer unrelated route after asynchronous original cleanup.
    if (mounted && route?.isActive == true && navigator.mounted && identical(route?.navigator, navigator)) {
      navigator.removeRoute(route!);
    }
  }

  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(label), SelectableText(value)]),
  );
  String phase(AgentTunnelRoutePhase value) => switch (value) {
    AgentTunnelRoutePhase.routeReview => context.l10n.agentTunnelReviewPhase,
    AgentTunnelRoutePhase.starting => context.l10n.agentTunnelStarting,
    AgentTunnelRoutePhase.listening => context.l10n.agentTunnelListening,
    AgentTunnelRoutePhase.connecting => context.l10n.agentTunnelConnecting,
    AgentTunnelRoutePhase.connected => context.l10n.agentTunnelConnected,
    AgentTunnelRoutePhase.stopRequested => context.l10n.agentTunnelStopRequested,
    AgentTunnelRoutePhase.cancelled => context.l10n.agentAssociationCancelRequested,
    AgentTunnelRoutePhase.unknown => context.l10n.agentTunnelUnknown,
  };
  String event(AgentTunnelAccessEvent value) => switch (value) {
    AgentTunnelAccessEvent.openRequested => context.l10n.agentTunnelOpenRequested,
    AgentTunnelAccessEvent.opened => context.l10n.agentTunnelActorAdmission,
    AgentTunnelAccessEvent.stopRequested => context.l10n.agentTunnelStopRequested,
    AgentTunnelAccessEvent.cancelled => context.l10n.agentAssociationCancelRequested,
    AgentTunnelAccessEvent.revoked => context.l10n.agentRevokeObserved,
    AgentTunnelAccessEvent.unknown => context.l10n.agentTunnelUnknown,
  };
  Widget number(TextEditingController c, String key, String label) => TextField(
    key: ValueKey(key),
    controller: c,
    enabled: !frozen && !leaving,
    keyboardType: TextInputType.number,
    decoration: InputDecoration(labelText: label),
  );
  Widget ip(String value, String key, String label, ValueChanged<String> changed) => DropdownButtonFormField<String>(
    key: ValueKey(key),
    initialValue: value,
    isExpanded: true,
    decoration: InputDecoration(labelText: label),
    items: [
      for (final ip in ['127.0.0.1', '::1']) DropdownMenuItem(value: ip, child: Text(ip)),
    ],
    onChanged: frozen || leaving
        ? null
        : (v) {
            if (v != null) setState(() => changed(v));
          },
  );
  @override
  Widget build(BuildContext context) {
    if (!current) return const SizedBox.shrink();
    final l = context.l10n, c = owner, v = c?.view;
    final rows =
        audit?.rows.where((r) => r.record.associationId == widget.record.id).toList() ?? const <AgentTunnelAccessRow>[];
    final pages = (rows.length + 7) ~/ 8, offset = page * 8;
    return PopScope<void>(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) unawaited(leave());
      },
      child: AlertDialog(
        key: const ValueKey('agent-tunnel-dialog'),
        title: Text(l.agentTunnelTitle),
        content: SizedBox(
          width: 680,
          height: (MediaQuery.sizeOf(context).height - 240).clamp(120.0, 600.0),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                InfoBanner(tone: BannerTone.warning, message: l.agentTunnelDisclosure),
                fact(l.agentAssociationNode, widget.record.agentId),
                fact(
                  l.agentAssociationSavedHost,
                  '${widget.record.hostId} / ${widget.record.hostRevision ?? l.commonUnknown}',
                ),
                if (c == null || !c.prepareSpent)
                  ContentSurface(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        ip(listenIp, 'agent-tunnel-listen-ip', l.agentTunnelListenIp, (v) => listenIp = v),
                        number(listen, 'agent-tunnel-listen-port', l.agentTunnelListenPort),
                        ip(destinationIp, 'agent-tunnel-destination-ip', l.agentTunnelDestinationIp, (v) {
                          destinationIp = v;
                          destinationChanged();
                        }),
                        number(destination, 'agent-tunnel-destination-port', l.agentTunnelDestinationPort),
                        number(ttl, 'agent-tunnel-ttl', l.agentTunnelTtl),
                        number(idle, 'agent-tunnel-idle', l.agentTunnelIdle),
                        CheckboxListTile(
                          key: const ValueKey('agent-tunnel-resume-original'),
                          value: resumeOriginal,
                          onChanged: resumePreference.port == null || frozen || leaving
                              ? null
                              : (value) => setState(() => resumeOriginal = value == true),
                          title: Text(l.agentTunnelResumeOriginal),
                          subtitle: Text(l.agentTunnelResumeHelp),
                        ),
                        OutlinedButton(
                          key: const ValueKey('agent-tunnel-resume-read'),
                          onPressed: resumePreference.canRead && !leaving ? resumePreference.read : null,
                          child: Text(l.agentTunnelResumeRead),
                        ),
                        if (resumePreference.busy) const LinearProgressIndicator(),
                        if (resumePreference.error case final Object error)
                          InfoBanner(tone: BannerTone.warning, message: errorMessage(l, error)),
                        if (resumePreference.value case final AgentTunnelResumePreference saved)
                          Text(
                            saved.resumeOriginal ? l.agentTunnelResumeSavedOn : l.agentTunnelResumeSavedOff,
                            key: const ValueKey('agent-tunnel-resume-saved'),
                          ),
                        if (resumePreference.port == null) Text(l.agentTunnelResumeUnsupported),
                      ],
                    ),
                  ),
                if (formError != null) InfoBanner(tone: BannerTone.warning, message: l.agentTunnelChoicesInvalid),
                if (!frozen && needsSavedLookup && !matchingLookupNone) Text(l.agentTunnelLookupFirst),
                if (matchingLookupNone)
                  InfoBanner(
                    key: const ValueKey('agent-tunnel-original-missing'),
                    tone: BannerTone.info,
                    message: l.agentTunnelOriginalMissing,
                  ),
                if (c?.busy == true || auditBusy || leaving || c?.awaitingCleanup == true)
                  const LinearProgressIndicator(),
                if (c?.error case final Object error)
                  InfoBanner(tone: BannerTone.danger, message: errorMessage(l, error)),
                if (c?.cleanupError case final Object error)
                  InfoBanner(tone: BannerTone.warning, message: errorMessage(l, error)),
                if (c?.unknown == true)
                  InfoBanner(
                    key: const ValueKey('agent-tunnel-unknown'),
                    tone: BannerTone.warning,
                    message: l.agentTunnelUnknown,
                  ),
                if (c?.expired == true) Text(l.agentAssociationExpired),
                if (c?.cancelRequested == true) Text(l.agentAssociationCancelRequested),
                if (c?.awaitingCleanup == true) Text(l.agentAssociationCleanupWaiting),
                if (c?.cleanup?.state == AgentTunnelCleanupState.closedObserved)
                  InfoBanner(
                    key: const ValueKey('agent-tunnel-closed'),
                    tone: BannerTone.info,
                    message: l.agentTunnelLocalClosed,
                  ),
                if (c?.cleanup?.state == AgentTunnelCleanupState.unknown)
                  InfoBanner(tone: BannerTone.warning, message: l.agentAssociationCleanupUnknown),
                if (v != null)
                  ContentSurface(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        fact(l.agentTunnelLastObserved, phase(v.phase)),
                        fact(
                          l.agentTunnelResumeOriginal,
                          v.resumeOriginalSignedOptIn ? l.agentTunnelResumeSelectedOn : l.agentTunnelResumeSelectedOff,
                        ),
                        Text(
                          !v.resumeTransportImplemented
                              ? l.agentTunnelResumeUnavailable
                              : v.resumeOriginalSignedOptIn
                              ? l.agentTunnelResumeTransportSelected
                              : l.agentTunnelResumeTransportOff,
                        ),
                        if (c?.current == true)
                          Text(
                            l.agentAssociationRemaining(c!.remainingSeconds.toString()),
                            key: const ValueKey('agent-tunnel-countdown'),
                          ),
                        fact(l.agentTunnelDestinationIp, '${v.destination.ip} : ${v.destination.port}'),
                        fact(l.agentTunnelListenIp, '${v.clientBind.ip} : ${v.clientBind.port}'),
                        fact(l.agentTunnelBoundAddress, v.boundAddress ?? l.commonUnknown),
                        fact(l.agentTunnelActualExpiry, v.grantExpiresUtc.toUtc().toIso8601String()),
                        if (c?.canDetach == true) Text(l.agentTunnelLeaveHint),
                        fact(l.agentTunnelLimitsTime, '${v.limits.maxTtlSeconds} / ${v.limits.idleTimeoutSeconds}'),
                        ExpansionTile(
                          key: const ValueKey('agent-tunnel-review-details'),
                          title: Text(l.agentTunnelReviewDetails),
                          children: [
                            fact(l.agentAssociationOwner, '${v.owner.kind}: ${v.owner.id}'),
                            fact(l.agentAssociationDevice, v.owner.deviceId),
                            fact(l.agentAssociationSigningKey, v.owner.signingKeySha256),
                            fact(l.agentAssociationNodeEpoch, v.node.keyEpoch),
                            fact(l.agentAssociationNodeFingerprint, v.node.keyFingerprint),
                            fact(l.agentAssociationPublicKey, v.node.publicKey),
                            fact(l.agentTunnelCertificate, v.certificateDigest),
                            fact(l.agentTunnelLimitsFrames, '${v.limits.maxChannels} / ${v.limits.maxFrameBytes}'),
                            fact(
                              l.agentTunnelLimitsPending,
                              '${v.limits.maxPendingFramesPerChannel} / ${v.limits.maxPendingFramesTotal}',
                            ),
                            fact(
                              l.agentTunnelLimitsQueued,
                              '${v.limits.maxQueuedBytesPerChannel} / ${v.limits.maxQueuedBytesTotal}',
                            ),
                            fact(
                              l.agentTunnelLimitsBytes,
                              '${v.limits.maxBytesPerChannel} / ${v.limits.maxBytesTotal}',
                            ),
                          ],
                        ),
                        if (v.actorAdmission)
                          InfoBanner(
                            key: const ValueKey('agent-tunnel-actor-admission'),
                            tone: BannerTone.info,
                            message: l.agentTunnelActorAdmission,
                          ),
                        fact(
                          l.agentTunnelCounters,
                          '${c?.cleanup?.sentBytes ?? v.sentBytes} / ${c?.cleanup?.receivedBytes ?? v.receivedBytes}',
                        ),
                        Text(l.agentTunnelNoDeliveryClaim),
                      ],
                    ),
                  ),
                const SizedBox(height: 16),
                Text(l.agentTunnelAuditTitle),
                Text(l.agentTunnelAuditDisclosure),
                OutlinedButton(
                  key: const ValueKey('agent-tunnel-audit-read'),
                  onPressed: auditBusy || leaving ? null : loadAudit,
                  child: Text(l.agentTunnelAuditRead),
                ),
                if (auditError case final Object error)
                  InfoBanner(tone: BannerTone.warning, message: errorMessage(l, error)),
                if (audit != null && rows.isEmpty) Text(l.agentTunnelAuditEmpty),
                for (final row in rows.skip(offset).take(8))
                  ContentSurface(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        fact(row.record.operationId, event(row.record.event)),
                        fact(
                          l.agentTunnelDestinationIp,
                          '${row.record.destination.ip} : ${row.record.destination.port}',
                        ),
                        fact(l.agentTunnelAuditObserved, row.record.observedAt.toUtc().toIso8601String()),
                        fact(l.agentTunnelCounters, '${row.record.sent} / ${row.record.received}'),
                        fact(l.agentAssociationDevice, row.deviceId),
                      ],
                    ),
                  ),
                if (pages > 1)
                  Wrap(
                    spacing: 8,
                    children: [
                      Text('${page + 1} / $pages'),
                      IconButton(
                        key: const ValueKey('agent-tunnel-audit-previous'),
                        tooltip: l.commonBack,
                        onPressed: page > 0 ? () => setState(() => page--) : null,
                        icon: const Icon(Icons.chevron_left),
                      ),
                      IconButton(
                        key: const ValueKey('agent-tunnel-audit-next'),
                        tooltip: l.commonNext,
                        onPressed: page + 1 < pages ? () => setState(() => page++) : null,
                        icon: const Icon(Icons.chevron_right),
                      ),
                    ],
                  ),
              ],
            ),
          ),
        ),
        actions: [
          TextButton(
            key: const ValueKey('agent-tunnel-close'),
            onPressed: leaving ? null : leave,
            child: Text(l.commonClose),
          ),
          if (!frozen &&
              needsSavedLookup &&
              (owner?.attachSpent != true || !matchingLookupNone && owner?.error == null))
            OutlinedButton(
              key: const ValueKey('agent-tunnel-attach-saved'),
              onPressed: leaving ? null : attachSaved,
              child: Text(l.agentTunnelAttachSaved),
            ),
          if (!frozen)
            OutlinedButton(
              key: const ValueKey('agent-tunnel-prepare'),
              onPressed: leaving || needsSavedLookup && !matchingLookupNone ? null : prepare,
              child: Text(l.agentTunnelReview),
            ),
          if (c?.canApprove == true)
            FilledButton(
              key: const ValueKey('agent-tunnel-approve'),
              onPressed: c!.approveOnce,
              child: Text(l.agentTunnelApprove),
            ),
          if (c?.current == true && c?.prepareSpent == true)
            OutlinedButton(
              key: const ValueKey('agent-tunnel-refresh'),
              onPressed: c!.busy ? null : c.refresh,
              child: Text(l.devicesRefresh),
            ),
          if (c?.canDetach == true)
            OutlinedButton(
              key: const ValueKey('agent-tunnel-leave-original'),
              onPressed: leaving ? null : leaveOriginal,
              child: Text(l.agentTunnelLeaveOriginal),
            ),
          if (c?.canStop == true)
            OutlinedButton(
              key: const ValueKey('agent-tunnel-stop'),
              onPressed: c!.stop,
              child: Text(l.agentTunnelStop),
            ),
          if (c != null && !c.awaitingCleanup && c.cleanup == null && !leaving)
            TextButton(
              key: const ValueKey('agent-tunnel-observe-close'),
              onPressed: () => c.close(),
              child: Text(l.agentTunnelAbortAndObserve),
            ),
        ],
      ),
    );
  }
}
