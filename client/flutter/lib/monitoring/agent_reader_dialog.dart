import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_reader_controller.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:material_ui/material_ui.dart';

class AgentReaderDialog extends StatefulWidget {
  const AgentReaderDialog({
    super.key,
    required this.port,
    required this.profileId,
    required this.notificationScopeId,
    required this.sources,
    required this.bindings,
    required this.hosts,
    required this.knownHosts,
    required this.files,
    required this.original,
    this.onHeartbeat,
  });
  final AgentReaderNativePort port;
  final String profileId, notificationScopeId;
  final List<MonitoringSource> sources;
  final List<MonitoringBinding> bindings;
  final List<Host> hosts;
  final List<KnownHost> knownHosts;
  final FileDialogService files;
  final bool Function() original;
  final void Function(AgentReaderSelection, AgentReaderLocator, AgentReaderDataset)? onHeartbeat;
  @override
  State<AgentReaderDialog> createState() => _AgentReaderDialogState();
}

class _AgentReaderDialogState extends State<AgentReaderDialog> {
  AgentReaderController? owner;
  AgentReaderLocator? retainedLocator;
  AgentReaderDataset? presentedDataset;
  String? bindingId, knownHostId;
  AgentReaderAction action = AgentReaderAction.read;
  final selectedFiles = <String, String>{};
  int generation = 0, ttl = 600, page = 0;
  bool picking = false, originalDevice = false;
  bool get current => mounted && widget.original();
  List<MonitoringBinding> get bindings => widget.bindings
      .where(
        (b) =>
            widget.sources.any((s) => s.id == b.sourceId && s.revision == b.sourceRevision) &&
            widget.hosts.any(
              (h) => h.id.value == b.hostId && h.protocol == HostProtocol.ssh && h.backend == SshBackend.native,
            ),
      )
      .toList();
  List<KnownHost> get known => widget.knownHosts.where((k) => !k.revoked).toList();
  void discard({bool deferDisposal = false}) {
    generation++;
    final original = owner;
    original?.removeListener(refresh);
    owner = null;
    page = 0;
    presentedDataset = null;
    if (deferDisposal) {
      // Native cancellation stays synchronous. Only final ChangeNotifier disposal
      // waits until the original notification stack has unwound.
      original?.retire();
      WidgetsBinding.instance.addPostFrameCallback((_) => original?.dispose());
    } else {
      original?.dispose();
    }
  }

  void refresh() {
    if (!mounted) return;
    if (!current) {
      discard(deferDisposal: true);
      setState(() {});
      return;
    }
    retainedLocator = owner?.outcome?.locator ?? owner?.status?.locator ?? retainedLocator;
    final data = owner?.dataset;
    if (!identical(data, presentedDataset)) {
      presentedDataset = data;
      page = data == null || data.points.isEmpty ? 0 : (data.points.length - 1) ~/ 32;
    }
    setState(() {});
  }

  @override
  void didUpdateWidget(covariant AgentReaderDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.port, widget.port) ||
        old.profileId != widget.profileId ||
        old.notificationScopeId != widget.notificationScopeId ||
        !identical(old.sources, widget.sources) ||
        !identical(old.bindings, widget.bindings) ||
        !identical(old.knownHosts, widget.knownHosts)) {
      discard();
    }
  }

  @override
  void dispose() {
    discard();
    selectedFiles.clear();
    super.dispose();
  }

  AgentReaderController? capture() {
    if (!current) return null;
    if (owner != null) return owner;
    final binding = bindings.where((b) => b.id == bindingId).firstOrNull;
    final key = known.where((k) => k.id.value == knownHostId).firstOrNull;
    if (binding == null || key == null) return null;
    final source = widget.sources
        .where((s) => s.id == binding.sourceId && s.revision == binding.sourceRevision)
        .firstOrNull;
    if (source == null) return null;
    final selected = AgentReaderSelection(
      profileId: widget.profileId,
      notificationScopeId: widget.notificationScopeId,
      source: source,
      binding: binding,
      knownHostId: key.id.value,
    );
    final created = AgentReaderController(
      port: widget.port,
      selection: selected,
      onHeartbeat: (data, association) => widget.onHeartbeat?.call(selected, association, data),
      isOriginal: () => current && bindings.any((b) => identical(b, binding)) && known.any((k) => identical(k, key)),
    );
    owner = created;
    created.addListener(refresh);
    return created;
  }

  Future<void> choose(String field) async {
    if (picking || owner?.busy == true || !current) return;
    discard();
    final epoch = generation;
    setState(() => picking = true);
    try {
      final path = await widget.files.chooseOpenFile(extensions: const ['json']);
      if (current && epoch == generation && path != null) setState(() => selectedFiles[field] = path);
    } finally {
      if (mounted && epoch == generation) setState(() => picking = false);
    }
  }

  Future<void> prepare() async {
    if (!current || picking) return;
    final controller = capture();
    if (controller == null) return;
    AgentReaderRequest request;
    switch (action) {
      case AgentReaderAction.nodeOffer:
        request = AgentReaderRequest.file(action, {
          for (final key in ['ownerPinFile', 'nodeHelloFile']) key: selectedFiles[key]!,
        }, expiresAt: DateTime.now().toUtc().millisecondsSinceEpoch ~/ 1000 + ttl);
      case AgentReaderAction.nodeCert:
        request = AgentReaderRequest.file(action, {
          for (final key in ['ownerPinFile', 'signedOfferFile', 'signedClaimFile']) key: selectedFiles[key]!,
        }, expiresAt: DateTime.now().toUtc().millisecondsSinceEpoch ~/ 1000 + ttl);
      case AgentReaderAction.association:
        request = AgentReaderRequest.file(action, {'selectionFile': selectedFiles['selectionFile']!});
      case AgentReaderAction.issuerInit:
        request = AgentReaderRequest.issuer();
      default:
        final locator = controller.status?.locator ?? controller.outcome?.locator ?? retainedLocator;
        if (locator == null) return;
        request = AgentReaderRequest.association(action, locator, originalDevice: originalDevice);
    }
    await controller.prepare(request);
  }

  Future<void> transfer() async {
    final controller = owner;
    if (controller == null || !current || picking) return;
    final epoch = generation, result = controller.outcome;
    final publicName = context.l10n.agentReaderPublicDest, invitationName = context.l10n.agentReaderInvitationDest;
    setState(() => picking = true);
    try {
      final public = await widget.files.chooseSaveFile(suggestedName: publicName, extensions: const ['json']);
      if (!current || epoch != generation || public == null || !identical(controller, owner)) return;
      String? invitation;
      if (result?.hasInvitation == true) {
        invitation = await widget.files.chooseSaveFile(suggestedName: invitationName, extensions: const ['hex']);
        if (!current || epoch != generation || invitation == null || !identical(controller, owner)) return;
      }
      await controller.transfer(publicDestination: public, invitationDestination: invitation);
    } finally {
      if (mounted && epoch == generation) setState(() => picking = false);
    }
  }

  String label(AgentReaderAction a) => switch (a) {
    AgentReaderAction.nodeOffer => context.l10n.agentReaderOffer,
    AgentReaderAction.nodeCert => context.l10n.agentReaderCertificate,
    AgentReaderAction.issuerInit => context.l10n.agentReaderIssuer,
    AgentReaderAction.association => context.l10n.agentReaderAssociation,
    AgentReaderAction.read => context.l10n.agentReaderRead,
    AgentReaderAction.revoke => context.l10n.agentReaderRevoke,
    AgentReaderAction.reconcile => context.l10n.agentReaderReconcile,
  };
  Widget collectionState(String? state) {
    final l = context.l10n;
    final label = switch (state) {
      'running' => l.agentReaderCollectionRunning,
      'disabled' => l.agentReaderCollectionDisabled,
      'stopped' => l.agentReaderCollectionStopped,
      'not_running' => l.agentReaderCollectionNotRunning,
      'starting' => l.agentReaderCollectionStarting,
      'degraded' => l.agentReaderCollectionDegraded,
      'blocked' => l.agentReaderCollectionBlocked,
      _ => l.commonUnknown,
    };
    final tone = switch (state) {
      'running' => GlassTone.success,
      'starting' => GlassTone.info,
      'degraded' => GlassTone.warning,
      'blocked' => GlassTone.danger,
      'disabled' || 'stopped' || 'not_running' => GlassTone.neutral,
      _ => GlassTone.unknown,
    };
    final tokens = GlassTokens.of(context);
    final foreground = tone == GlassTone.neutral ? tokens.secondaryLabel : tokens.palette.tone(tone);
    return Wrap(
      spacing: 8,
      runSpacing: 4,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        Text(l.agentReaderCollection),
        Tooltip(
          message: label,
          child: DecoratedBox(
            decoration: ShapeDecoration(
              color: tokens.palette.toneFill(tone).withValues(alpha: 0.16),
              shape: StadiumBorder(
                side: tokens.highContrast ? BorderSide(color: foreground.withValues(alpha: 0.5)) : BorderSide.none,
              ),
            ),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(
                    state == 'unknown' || state == null ? Icons.help_outline : Icons.circle_outlined,
                    size: 14,
                    color: foreground,
                  ),
                  const SizedBox(width: 4),
                  Flexible(
                    child: Text(
                      label,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: tokens.typography.caption.copyWith(color: foreground, fontWeight: FontWeight.w600),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }

  Widget facts(List<AgentReaderFact> facts) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      for (final f in facts)
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(f.label, style: Theme.of(context).textTheme.labelMedium),
              SelectableText(f.value),
            ],
          ),
        ),
    ],
  );
  Widget picker(String field, String label) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: OutlinedButton(
      key: ValueKey('agent-file-$field'),
      onPressed: picking ? null : () => choose(field),
      child: Wrap(
        spacing: 8,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [if (selectedFiles.containsKey(field)) const Icon(Icons.check, size: 16), Text(label)],
      ),
    ),
  );
  @override
  Widget build(BuildContext context) {
    if (!current) return const SizedBox.shrink();
    final l = context.l10n,
        controller = owner,
        review = controller?.review,
        outcome = controller?.outcome,
        status = controller?.status;
    final busy = picking || controller?.busy == true, locked = review != null || controller?.spent == true;
    final locator = status?.locator ?? outcome?.locator ?? retainedLocator;
    final requiredFiles = switch (action) {
      AgentReaderAction.nodeOffer => ['ownerPinFile', 'nodeHelloFile'],
      AgentReaderAction.nodeCert => ['ownerPinFile', 'signedOfferFile', 'signedClaimFile'],
      AgentReaderAction.association => ['selectionFile'],
      _ => const <String>[],
    };
    final allowed =
        bindingId != null &&
        knownHostId != null &&
        requiredFiles.every(selectedFiles.containsKey) &&
        (!{AgentReaderAction.read, AgentReaderAction.revoke, AgentReaderAction.reconcile}.contains(action) ||
            locator != null);
    return AlertDialog(
      title: Text(l.agentReaderTitle),
      content: SizedBox(
        width: 760,
        height: (MediaQuery.sizeOf(context).height - MediaQuery.viewInsetsOf(context).bottom - 220).clamp(80.0, 600.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoBanner(tone: BannerTone.info, message: l.agentReaderPurpose),
              if (action == AgentReaderAction.read)
                Text(l.agentReaderOnline, key: const ValueKey('agent-history-availability')),
              const SizedBox(height: 12),
              if (bindings.isEmpty || known.isEmpty)
                InfoBanner(tone: BannerTone.warning, message: l.agentReaderNoChoices),
              if (!locked) ...[
                GlassSelect<String?>(
                  value: bindingId,
                  expand: true,
                  semanticLabel: l.agentReaderBinding,
                  items: [
                    GlassSelectItem(value: null, label: l.agentReaderBinding),
                    for (final b in bindings)
                      GlassSelectItem(
                        value: b.id,
                        label:
                            '${b.name} / ${widget.sources.where((s) => s.id == b.sourceId).firstOrNull?.name ?? b.sourceId}',
                      ),
                  ],
                  onChanged: busy
                      ? null
                      : (id) {
                          discard();
                          retainedLocator = null;
                          setState(() => bindingId = id);
                        },
                ),
                const SizedBox(height: 12),
                GlassSelect<String?>(
                  value: knownHostId,
                  expand: true,
                  semanticLabel: l.agentReaderKnownHost,
                  items: [
                    GlassSelectItem(value: null, label: l.agentReaderKnownHost),
                    for (final k in known) GlassSelectItem(value: k.id.value, label: k.hostPattern),
                  ],
                  onChanged: busy
                      ? null
                      : (id) {
                          discard();
                          retainedLocator = null;
                          setState(() => knownHostId = id);
                        },
                ),
                if (knownHostId != null)
                  SelectableText(known.where((k) => k.id.value == knownHostId).firstOrNull?.fingerprintSha256 ?? '?'),
                const SizedBox(height: 12),
                GlassSelect<AgentReaderAction>(
                  value: action,
                  expand: true,
                  semanticLabel: l.agentReaderAction,
                  items: [for (final a in AgentReaderAction.values) GlassSelectItem(value: a, label: label(a))],
                  onChanged: busy
                      ? null
                      : (a) {
                          if (a == action) return;
                          setState(() {
                            action = a;
                            selectedFiles.clear();
                            ttl = a == AgentReaderAction.nodeCert ? 86400 : 600;
                          });
                        },
                ),
                if (requiredFiles.contains('ownerPinFile')) picker('ownerPinFile', l.agentReaderOwnerPin),
                if (requiredFiles.contains('nodeHelloFile')) picker('nodeHelloFile', l.agentReaderHello),
                if (requiredFiles.contains('signedOfferFile')) picker('signedOfferFile', l.agentReaderSignedOffer),
                if (requiredFiles.contains('signedClaimFile')) picker('signedClaimFile', l.agentReaderSignedClaim),
                if (requiredFiles.contains('selectionFile')) picker('selectionFile', l.agentReaderSelectionFile),
                if (action == AgentReaderAction.nodeOffer || action == AgentReaderAction.nodeCert)
                  GlassSelect<int>(
                    value: ttl,
                    expand: true,
                    semanticLabel: l.agentReaderExpiry,
                    items: [
                      for (final seconds
                          in action == AgentReaderAction.nodeCert ? [3600, 86400, 2592000] : [60, 300, 600])
                        GlassSelectItem(value: seconds, label: '$seconds s'),
                    ],
                    onChanged: busy ? null : (v) => setState(() => ttl = v),
                  ),
                if (action == AgentReaderAction.revoke)
                  CheckboxListTile(
                    value: originalDevice,
                    title: Text(l.agentReaderOriginalDevice),
                    onChanged: busy ? null : (v) => setState(() => originalDevice = v ?? false),
                  ),
              ],
              if (busy) const Padding(padding: EdgeInsets.symmetric(vertical: 12), child: LinearProgressIndicator()),
              if (controller?.failure != null)
                InfoBanner(
                  tone: BannerTone.danger,
                  message: controller?.unknown == true
                      ? l.agentReaderUnknown
                      : action == AgentReaderAction.read && controller?.readConsumed == true
                      ? switch (controller?.failure) {
                          'reader_unavailable' || 'reader_cancelled' => l.agentReaderReadUnavailable,
                          'reader_expired' => l.agentReaderReadExpired,
                          _ => l.agentReaderFailed,
                        }
                      : l.agentReaderFailed,
                ),
              if (!locked && status != null && status.choices.isNotEmpty)
                GlassSelect<int?>(
                  value:
                      retainedLocator == null ||
                          !status.choices.any(
                            (c) =>
                                c.locator.peerDigest == retainedLocator!.peerDigest &&
                                c.locator.associationDigest == retainedLocator!.associationDigest,
                          )
                      ? null
                      : status.choices.indexWhere(
                          (c) =>
                              c.locator.peerDigest == retainedLocator!.peerDigest &&
                              c.locator.associationDigest == retainedLocator!.associationDigest,
                        ),
                  expand: true,
                  semanticLabel: l.agentReaderAssociation,
                  items: [
                    GlassSelectItem(value: null, label: l.agentReaderAssociation),
                    for (var i = 0; i < status.choices.length; i++)
                      GlassSelectItem(
                        value: i,
                        label: '${status.choices[i].datasetKind} / ${status.choices[i].agentId}',
                      ),
                  ],
                  onChanged: busy
                      ? null
                      : (i) => setState(() => retainedLocator = i == null ? null : status.choices[i].locator),
                ),
              if (status != null) ...[
                Text(l.agentReaderStatusMeaning),
                if (status.choicesTruncated) Text(l.agentReaderPartial),
                ExpansionTile(title: Text(l.agentReaderStatus), children: [facts(status.facts)]),
              ],
              if (review != null) ...[
                const SizedBox(height: 12),
                Text(l.agentReaderDeadline),
                Text(
                  l.agentReaderReviewRemaining(controller!.remainingSeconds.toString()),
                  key: const ValueKey('agent-review-countdown'),
                ),
                ExpansionTile(
                  key: const ValueKey('agent-review-details'),
                  initiallyExpanded: true,
                  title: Text(l.agentReaderReviewDetails),
                  children: [facts(review.facts)],
                ),
              ],
              if (outcome != null) ...[
                if (outcome.publicPath != null) ...[
                  InfoBanner(tone: BannerTone.warning, message: l.agentReaderNoActorAck),
                  Text(l.agentReaderExports),
                  if (controller?.transferCompleted == true) Text(l.agentReaderTransferCompleted),

                  OutlinedButton(
                    key: const ValueKey('agent-transfer'),
                    onPressed: busy || controller?.transferAttempted == true ? null : transfer,
                    child: Text(l.agentReaderTransfer),
                  ),
                ],
                if (outcome.kind == 'issuerReady') InfoBanner(tone: BannerTone.info, message: l.agentReaderIssuerReady),
                if (outcome.kind == 'associationInstalled' || outcome.kind == 'readerRevocationApplied')
                  InfoBanner(tone: BannerTone.info, message: l.agentReaderObservedAck),
                if (outcome.kind == 'knownHeadReconciled') Text(l.agentReaderReconciled),
                if (outcome.kind == 'readReady' && controller?.readConsumed != true)
                  FilledButton(
                    key: const ValueKey('agent-consume'),
                    onPressed: busy ? null : controller!.consumeRead,
                    child: Text(l.agentReaderConsume),
                  ),
              ],
              if (controller?.dataset case final data?) ...[
                const SizedBox(height: 12),
                Text(l.agentReaderDataset, style: Theme.of(context).textTheme.titleMedium),
                Text(l.agentReaderClock),

                if (data.storageState == 'clock_rollback_paused')
                  InfoBanner(tone: BannerTone.warning, message: l.agentReaderClockRollback),
                if (data.kind == 'heartbeatV1') ...[
                  SelectableText('${data.observedAt?.toIso8601String()}'),
                  Text(
                    '${l.agentReaderVersion}: ${data.agentVersion ?? l.commonUnknown}',
                    key: const ValueKey('agent-heartbeat-version'),
                  ),
                  Text('${l.agentReaderUptime}: ${data.uptimeSeconds ?? l.commonUnknown}'),
                  collectionState(data.collectionState),
                ],
                if (data.kind == 'localAgentHistoryV1')
                  InfoBanner(tone: BannerTone.info, message: l.agentReaderCollectionRecorded),
                if (data.truncated || data.evicted > 0)
                  InfoBanner(tone: BannerTone.warning, message: l.agentReaderPartial),
                for (final point in data.points.skip(page * 32).take(32))
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 8),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Text(point.at.toIso8601String()),
                        if (point.gap) Text(l.commonUnknown),
                        if (point.collectionState != null) collectionState(point.collectionState),
                        for (final metric in data.metrics) Text('$metric: ${point.values[metric] ?? l.commonUnknown}'),
                      ],
                    ),
                  ),
                if (data.points.length > 32)
                  Wrap(
                    spacing: 8,
                    children: [
                      TextButton(
                        key: const ValueKey('agent-history-earlier'),
                        onPressed: page == 0 ? null : () => setState(() => page--),
                        child: Text(l.agentReaderEarlier),
                      ),
                      TextButton(
                        key: const ValueKey('agent-history-later'),
                        onPressed: (page + 1) * 32 >= data.points.length ? null : () => setState(() => page++),
                        child: Text(l.agentReaderLater),
                      ),
                    ],
                  ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('agent-close'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l.commonClose),
        ),
        if (controller != null)
          TextButton(
            key: const ValueKey('agent-cancel'),
            onPressed: () {
              discard();
              setState(() {});
            },
            child: Text(l.agentReaderCancel),
          ),
        if (controller?.spent == true && controller?.unknown != true && !busy && outcome?.publicPath == null)
          TextButton(
            key: const ValueKey('agent-new-action'),
            onPressed: () {
              discard();
              setState(() {});
            },
            child: Text(l.agentReaderNewAction),
          ),
        if (!locked)
          TextButton(
            key: const ValueKey('agent-status'),
            onPressed: busy || bindingId == null || knownHostId == null
                ? null
                : () => capture()?.readStatus(locator: locator),
            child: Text(l.agentReaderStatus),
          ),
        if (!locked)
          FilledButton(
            key: const ValueKey('agent-prepare'),
            onPressed: busy || !allowed ? null : prepare,
            child: Text(l.agentReaderReview),
          ),
        if (review != null && controller?.spent == false)
          FilledButton(
            key: const ValueKey('agent-confirm'),
            onPressed: busy || controller?.canConfirm != true ? null : controller!.confirm,
            child: Text(l.agentReaderConfirm),
          ),
      ],
    );
  }
}
