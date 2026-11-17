import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_association_controller.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_dialog.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_exporter.dart';
import 'package:consolecrypt/monitoring/agent_exporter_dialog.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_dialog.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_port.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';
import 'package:consolecrypt/monitoring/agent_metrics_runtime_review.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap_dialog.dart';
import 'package:consolecrypt/monitoring/agent_revocation_dialog.dart';
import 'package:consolecrypt/monitoring/agent_rotation.dart';
import 'package:consolecrypt/monitoring/agent_rotation_dialog.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_dialog.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:material_ui/material_ui.dart';

class AgentAssociationDialog extends StatefulWidget {
  const AgentAssociationDialog({
    super.key,
    required this.port,
    required this.scope,
    required this.original,
    this.tunnelPort,
    this.metricsPort,
    this.exporterPort,
    this.outboundPort,
    this.collectorServicePort,
    this.enrollmentPurpose = AgentAssociationPurpose.tunnel,
  });
  final AgentAssociationPort port;
  final AgentTunnelRoutePort? tunnelPort;
  final AgentMetricsCollectorPort? metricsPort;
  final AgentExporterPort? exporterPort;
  final AgentOutboundBootstrapPort? outboundPort;
  final AgentCollectorServicePort? collectorServicePort;
  final AgentAssociationPurpose enrollmentPurpose;
  final RuntimeNoticeScope scope;
  final bool Function() original;
  @override
  State<AgentAssociationDialog> createState() => _AgentAssociationDialogState();
}

class _AgentAssociationDialogState extends State<AgentAssociationDialog> {
  late final AgentAssociationController owner;
  int lifetime = 86400;
  DialogRoute<void>? _actionRoute;
  @override
  void initState() {
    super.initState();
    owner = AgentAssociationController(
      port: widget.port,
      scope: widget.scope,
      original: () => mounted && widget.original(),
    )..addListener(refresh);
  }

  void refresh() {
    if (mounted) setState(() {});
  }

  @override
  void didUpdateWidget(covariant AgentAssociationDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.port, widget.port) ||
        !identical(old.tunnelPort, widget.tunnelPort) ||
        !identical(old.metricsPort, widget.metricsPort) ||
        !identical(old.exporterPort, widget.exporterPort) ||
        old.enrollmentPurpose != widget.enrollmentPurpose ||
        !old.scope.same(widget.scope) ||
        !widget.original()) {
      owner.retire();
      _closeAction();
    }
  }

  @override
  void dispose() {
    owner.removeListener(refresh);
    owner.retire(); // Synchronously invalidates the exact child review first.
    _closeAction();
    owner.dispose();
    super.dispose();
  }

  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(label), SelectableText(value)]),
  );
  void _closeAction() {
    final route = _actionRoute;
    _actionRoute = null;
    if (route == null) return;
    final navigator = route.navigator;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (navigator?.mounted == true && route.isActive && identical(route.navigator, navigator)) {
        navigator!.removeRoute(route);
      }
    });
  }

  AgentAssociationRevocationPort? revocationPort(AgentAssociationRecord row) {
    final port = widget.port;
    if (port is AgentExporterEnrollmentAssociationPort && row.purpose != AgentAssociationPurpose.exporters) return null;
    if (row.purpose == AgentAssociationPurpose.exporters) {
      if (port is AgentExporterEnrollmentAssociationPort) return port;
      final exporters = widget.exporterPort;
      return exporters is AgentExporterEnrollmentPort
          ? AgentExporterEnrollmentAssociationPort(port, exporters as AgentExporterEnrollmentPort)
          : null;
    }
    if (port is AgentMetricsEnrollmentAssociationPort && row.purpose != AgentAssociationPurpose.metrics) return null;
    if (row.purpose == AgentAssociationPurpose.metrics) {
      if (port is AgentMetricsEnrollmentAssociationPort) return port;
      if (port is AgentMetricsEnrollmentPort) {
        return AgentMetricsEnrollmentAssociationPort(port, port as AgentMetricsEnrollmentPort);
      }
      return null;
    }
    return port is AgentAssociationRevocationPort ? port as AgentAssociationRevocationPort : null;
  }

  Future<void> revoke(AgentAssociationRecord row) async {
    final revoker = revocationPort(row), status = owner.status;
    if (revoker == null || status == null || !owner.current || _actionRoute != null) {
      return;
    }
    final originalPort = widget.port, originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final route = DialogRoute<void>(
      context: context,
      builder: (_) => AgentRevocationDialog(
        port: revoker,
        scope: originalScope,
        record: row,
        originalChanges: owner,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, originalPort) &&
            widget.scope.same(originalScope) &&
            identical(owner.status, status) &&
            status.records.any((r) => identical(r, row)) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  Future<void> rotate(AgentAssociationRecord row, AgentRotationMode mode) async {
    final port = widget.port, status = owner.status;
    if (port is! AgentAssociationRotationPort || status == null || !owner.current || _actionRoute != null) return;
    final originalPort = widget.port, originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final route = DialogRoute<void>(
      context: context,
      builder: (_) => AgentRotationDialog(
        port: port as AgentAssociationRotationPort,
        scope: originalScope,
        record: row,
        mode: mode,
        originalChanges: owner,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, originalPort) &&
            widget.scope.same(originalScope) &&
            identical(owner.status, status) &&
            status.records.any((r) => identical(r, row)) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  bool tunnelHostMatches(AgentAssociationStatus status, AgentAssociationRecord row) =>
      status.hosts.any((host) => host.id == row.hostId && host.revision == row.hostRevision);

  Future<void> tunnel(AgentAssociationRecord row) async {
    final port = widget.tunnelPort, status = owner.status;
    if (port == null ||
        status == null ||
        !owner.current ||
        !row.canRequestTunnel ||
        _actionRoute != null ||
        !status.records.any((saved) => identical(saved, row)) ||
        !tunnelHostMatches(status, row)) {
      return;
    }
    final originalPort = widget.port, originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final route = DialogRoute<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => AgentTunnelRouteDialog(
        port: port,
        scope: originalScope,
        record: row,
        originalChanges: owner,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, originalPort) &&
            identical(widget.tunnelPort, port) &&
            widget.scope.same(originalScope) &&
            identical(owner.status, status) &&
            status.records.any((r) => identical(r, row)) &&
            tunnelHostMatches(status, row) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  Future<void> collectorService(AgentAssociationRecord row) async {
    final port = widget.collectorServicePort, status = owner.status;
    if (port == null ||
        status == null ||
        !owner.current ||
        row.phase != AgentAssociationPhase.associated ||
        row.purpose != AgentAssociationPurpose.metrics ||
        row.hostRevision == null ||
        row.certificateDigest == null ||
        row.nodeKeyEpoch == null ||
        row.nodeKeyFingerprint == null ||
        _actionRoute != null ||
        !status.hosts.any((h) => h.id == row.hostId && h.revision == row.hostRevision)) {
      return;
    }
    final originalPort = widget.port, originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final route = DialogRoute<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => AgentCollectorServiceDialog(
        port: port,
        scope: originalScope,
        record: row,
        originalChanges: owner,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, originalPort) &&
            identical(widget.collectorServicePort, port) &&
            widget.scope.same(originalScope) &&
            identical(owner.status, status) &&
            status.records.any((r) => identical(r, row)) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  Future<void> outbound(AgentAssociationRecord row) async {
    final port = widget.outboundPort, status = owner.status;
    if (port == null ||
        status == null ||
        !owner.current ||
        !row.canRequestTunnel ||
        _actionRoute != null ||
        !status.hosts.any((h) => h.id == row.hostId && h.revision == row.hostRevision)) {
      return;
    }
    final originalPort = widget.port, originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final route = DialogRoute<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => AgentOutboundBootstrapDialog(
        port: port,
        scope: originalScope,
        record: row,
        originalChanges: owner,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, originalPort) &&
            identical(widget.outboundPort, port) &&
            widget.scope.same(originalScope) &&
            identical(owner.status, status) &&
            status.records.any((r) => identical(r, row)) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  Future<void> exporterEnrollment() async {
    final exporters = widget.exporterPort, port = widget.port;
    if (exporters is! AgentExporterEnrollmentPort || !owner.current || _actionRoute != null) return;
    final originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final route = DialogRoute<void>(
      context: context,
      builder: (_) => AgentAssociationDialog(
        port: AgentExporterEnrollmentAssociationPort(port, exporters as AgentExporterEnrollmentPort),
        scope: originalScope,
        enrollmentPurpose: AgentAssociationPurpose.exporters,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, port) &&
            identical(widget.exporterPort, exporters) &&
            widget.scope.same(originalScope) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  Future<void> exporter(AgentAssociationRecord row) async {
    final port = widget.exporterPort, status = owner.status;
    if (port == null ||
        status == null ||
        !owner.current ||
        !canRequestAgentExporter(row) ||
        _actionRoute != null ||
        !status.hosts.any((h) => h.id == row.hostId && h.revision == row.hostRevision)) {
      return;
    }
    final originalPort = widget.port, originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final route = DialogRoute<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => AgentExporterDialog(
        port: port,
        scope: originalScope,
        record: row,
        originalChanges: owner,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, originalPort) &&
            identical(widget.exporterPort, port) &&
            widget.scope.same(originalScope) &&
            identical(owner.status, status) &&
            status.records.any((r) => identical(r, row)) &&
            status.hosts.any((h) => h.id == row.hostId && h.revision == row.hostRevision) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  Future<void> metricsEnrollment() async {
    final port = widget.port;
    if (port is! AgentMetricsEnrollmentPort || !owner.current || _actionRoute != null) return;
    final originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final wrapped = AgentMetricsEnrollmentAssociationPort(port, port as AgentMetricsEnrollmentPort);
    final route = DialogRoute<void>(
      context: context,
      builder: (_) => AgentAssociationDialog(
        port: wrapped,
        scope: originalScope,
        enrollmentPurpose: AgentAssociationPurpose.metrics,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, port) &&
            widget.scope.same(originalScope) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  Future<void> collectMetrics(AgentAssociationRecord row) async {
    final port = widget.metricsPort, status = owner.status;
    if (port == null || status == null || !row.canRequestMetricsCollection || !owner.current || _actionRoute != null) {
      return;
    }
    final originalPort = widget.port, originalScope = widget.scope;
    final navigator = Navigator.of(context, rootNavigator: true);
    final route = DialogRoute<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => AgentMetricsCollectorDialog(
        port: port,
        scope: originalScope,
        record: row,
        originalChanges: owner,
        original: () =>
            mounted &&
            owner.current &&
            identical(widget.port, originalPort) &&
            identical(widget.metricsPort, port) &&
            widget.scope.same(originalScope) &&
            identical(owner.status, status) &&
            status.records.any((r) => identical(r, row)) &&
            widget.original(),
      ),
    );
    _actionRoute = route;
    try {
      await navigator.push(route);
    } finally {
      if (identical(_actionRoute, route)) _actionRoute = null;
    }
  }

  List<Widget> catalogFacts(AgentMetricsCollectorCatalog catalog) {
    final l = context.l10n, m = catalog.manifest;
    return [
      fact(l.agentMetricsCollectorId, catalog.collectorId),
      fact(l.agentMetricsManifestSha, catalog.manifestSha256),
      fact(l.agentMetricsMode, m.mode),
      fact(l.agentMetricsLabels, '${m.site} / ${m.environment}'),
      fact(l.agentMetricsInterval, m.scrapeIntervalSeconds.toString()),
      fact(l.agentMetricsReceiver, m.receiverUrl),
      fact(l.agentMetricsBuffer, '${m.maxBufferBytes} / ${m.maxBufferAgeSeconds}'),
      if (m.runtime case final runtime?) AgentMetricsRuntimeReview(runtime: runtime),
      for (final t in m.targets) ...[
        fact(l.agentMetricsTarget, '${t.id} / ${t.kind} / ${t.url}'),
        fact(l.agentMetricsTargetLimits, '${t.sampleLimit} / ${t.seriesLimit} / ${t.responseBytes} / ${t.labelLimit}'),
      ],
    ];
  }

  String phase(AgentAssociationPhase value) => switch (value) {
    AgentAssociationPhase.unenrolled => context.l10n.agentAssociationUnenrolled,
    AgentAssociationPhase.associated => context.l10n.agentAssociationAssociated,
    AgentAssociationPhase.revoked => context.l10n.agentAssociationUnknown,
    _ => context.l10n.agentAssociationUnknown,
  };
  @override
  Widget build(BuildContext context) {
    if (!owner.current) return const SizedBox.shrink();
    final l = context.l10n,
        status = owner.status,
        review = owner.review,
        result = owner.result,
        identityReview = owner.identityReview,
        identityResult = owner.identityResult;
    final maximum = review?.maxCertificateLifetimeSeconds ?? 0;
    final chosenLifetime = maximum > 0 ? lifetime.clamp(1, maximum) : lifetime;
    final lifetimeChoices = maximum > 0
        ? <int>{
            for (final requested in [3600, 86400, 2592000]) requested.clamp(1, maximum),
          }.toList()
        : <int>[];
    return AlertDialog(
      title: Text(
        widget.enrollmentPurpose == AgentAssociationPurpose.exporters
            ? l.agentExporterEnroll
            : widget.enrollmentPurpose == AgentAssociationPurpose.metrics
            ? l.agentMetricsEnrollmentTitle
            : l.agentAssociationOpen,
      ),
      content: SizedBox(
        width: 680,
        height: (MediaQuery.sizeOf(context).height - 240).clamp(120.0, 580.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoBanner(tone: BannerTone.info, message: l.agentAssociationDisclosure),
              if (widget.enrollmentPurpose == AgentAssociationPurpose.metrics)
                InfoBanner(tone: BannerTone.warning, message: l.agentMetricsEnrollmentDisclosure),
              if (owner.busy) const LinearProgressIndicator(),
              if (owner.error != null) InfoBanner(tone: BannerTone.danger, message: errorMessage(l, owner.error!)),
              if (owner.unknown)
                InfoBanner(
                  key: const ValueKey('agent-association-unknown'),
                  tone: BannerTone.warning,
                  message: l.agentAssociationUnknownDetail,
                ),
              if (owner.expired)
                InfoBanner(
                  key: const ValueKey('agent-association-expired'),
                  tone: BannerTone.warning,
                  message: l.agentAssociationExpired,
                ),
              if (owner.cancelled)
                InfoBanner(
                  key: const ValueKey('agent-association-cancel-requested'),
                  tone: BannerTone.info,
                  message: l.agentAssociationCancelRequested,
                ),
              if (owner.awaitingCleanup) Text(l.agentAssociationCleanupWaiting),
              if (owner.cleanupUnavailable || owner.cleanup?.state == 'unknown')
                InfoBanner(tone: BannerTone.warning, message: l.agentAssociationCleanupUnknown),
              if (owner.cleanup?.state == 'closedObserved') Text(l.agentAssociationCleanupObserved),
              if (identityReview != null) ...[
                InfoBanner(tone: BannerTone.warning, message: l.agentAssociationIdentityCreateDisclosure),
                Text(l.agentAssociationRemaining(owner.remainingSeconds.toString())),
                fact(l.agentAssociationOwner, '${identityReview.ownerKind}: ${identityReview.ownerId}'),
                fact(l.agentAssociationDevice, identityReview.deviceId),
                fact(l.agentAssociationSigningKey, identityReview.signingKeySha256),
              ],
              if (identityResult != null) ...[
                InfoBanner(
                  key: const ValueKey('agent-identity-stored'),
                  tone: BannerTone.info,
                  message: l.agentAssociationIdentityCreated,
                ),
                fact(l.agentAssociationTlsIdentity, identityResult.identityId),
                fact(l.agentAssociationTlsCertificate, identityResult.certificateSha256),
              ],
              if (status != null) ...[
                InfoBanner(
                  key: const ValueKey('agent-association-client-identity'),
                  tone: BannerTone.info,
                  message: status.clientIdentityState == AgentClientIdentityState.storedMetadata
                      ? l.agentAssociationIdentityStored
                      : l.agentAssociationIdentityUnavailable,
                ),
                const SizedBox(height: 12),
                GlassSelect<String?>(
                  key: const ValueKey('agent-association-host'),
                  value: owner.host?.id,
                  expand: true,
                  semanticLabel: l.agentAssociationSavedHost,
                  items: [
                    GlassSelectItem(value: null, label: l.agentAssociationChooseHost),
                    for (final h in status.hosts) GlassSelectItem(value: h.id, label: h.name),
                  ],
                  onChanged: owner.busy || review != null
                      ? null
                      : (id) {
                          for (final h in status.hosts) {
                            if (h.id == id) {
                              owner.select(h);
                              break;
                            }
                          }
                        },
                ),
                if (status.records.isEmpty) Text(l.agentAssociationUnenrolled),
                for (final row in status.records) ...[
                  fact(
                    row.agentId,
                    row.remoteRevocationObserved == true &&
                            row.revocationState == 'observedRevoked' &&
                            row.phase == AgentAssociationPhase.revoked
                        ? l.agentRevokeObserved
                        : row.revocationState == 'unknown'
                        ? l.agentRevokeUnknown
                        : phase(row.phase),
                  ),
                  if (widget.collectorServicePort != null &&
                      row.phase == AgentAssociationPhase.associated &&
                      row.purpose == AgentAssociationPurpose.metrics &&
                      row.hostRevision != null &&
                      row.certificateDigest != null &&
                      row.nodeKeyEpoch != null &&
                      row.nodeKeyFingerprint != null &&
                      status.hosts.any((h) => h.id == row.hostId && h.revision == row.hostRevision))
                    TextButton(
                      key: ValueKey('agent-collector-service-row-${row.id}'),
                      onPressed: owner.busy || review != null || identityReview != null
                          ? null
                          : () => collectorService(row),
                      child: Text(l.agentCollectorServiceTitle),
                    ),
                  if (widget.outboundPort != null &&
                      row.canRequestTunnel &&
                      status.hosts.any((h) => h.id == row.hostId && h.revision == row.hostRevision))
                    TextButton(
                      key: ValueKey('agent-outbound-row-${row.id}'),
                      onPressed: owner.busy || review != null || identityReview != null ? null : () => outbound(row),
                      child: Text(l.agentOutboundTitle),
                    ),
                  if (widget.exporterPort != null &&
                      canRequestAgentExporter(row) &&
                      status.hosts.any((h) => h.id == row.hostId && h.revision == row.hostRevision))
                    OutlinedButton(
                      key: ValueKey('agent-exporter-row-${row.id}'),
                      onPressed: owner.busy || review != null || identityReview != null ? null : () => exporter(row),
                      child: Text(l.agentExporterTitle),
                    ),
                  if (widget.tunnelPort != null && row.canRequestTunnel)
                    OutlinedButton(
                      key: ValueKey('agent-tunnel-row-${row.id}'),
                      onPressed:
                          owner.busy || review != null || identityReview != null || !tunnelHostMatches(status, row)
                          ? null
                          : () => tunnel(row),
                      child: Text(l.agentTunnelTitle),
                    ),
                  if (widget.tunnelPort != null && row.canRequestTunnel && !tunnelHostMatches(status, row))
                    Text(l.agentTunnelHostChanged, key: ValueKey('agent-tunnel-host-stale-${row.id}')),
                  if (row.scopeName != null) fact(l.agentMetricsScope, row.scopeName!),
                  if (widget.metricsPort != null && row.canRequestMetricsCollection)
                    OutlinedButton(
                      key: ValueKey('agent-metrics-row-${row.id}'),
                      onPressed: owner.busy || review != null || identityReview != null
                          ? null
                          : () => collectMetrics(row),
                      child: Text(l.agentMetricsTitle),
                    ),
                  if (row.nodeKeyEpoch != null) fact(l.agentAssociationNodeEpoch, row.nodeKeyEpoch!),
                  if (row.rotationState == 'unknown') Text(l.agentRotationUnknown),
                  if (row.rotationState == 'selectedObserved') Text(l.agentRotationObserved),
                  if ((row.purpose == null || row.purpose == AgentAssociationPurpose.tunnel) &&
                      widget.port is AgentAssociationRotationPort &&
                      row.hostRevision != null &&
                      row.certificateDigest != null &&
                      row.nodeKeyEpoch == '1' &&
                      row.revocationState == 'notRequested') ...[
                    if (row.phase == AgentAssociationPhase.associated && row.rotationState == 'notRequested')
                      OutlinedButton(
                        key: ValueKey('agent-rotation-row-${row.id}'),
                        onPressed: owner.busy || review != null || identityReview != null
                            ? null
                            : () => rotate(row, AgentRotationMode.rotate),
                        child: Text(l.agentRotationTitle),
                      ),
                    if (row.phase == AgentAssociationPhase.blocked && row.rotationState == 'unknown')
                      OutlinedButton(
                        key: ValueKey('agent-reconcile-row-${row.id}'),
                        onPressed: owner.busy || review != null || identityReview != null
                            ? null
                            : () => rotate(row, AgentRotationMode.reconcile),
                        child: Text(l.agentRotationReconcileTitle),
                      ),
                  ],
                  if (revocationPort(row) != null &&
                      row.phase == AgentAssociationPhase.associated &&
                      row.revocationState != 'unknown' &&
                      row.certificateDigest != null &&
                      row.hostRevision != null)
                    OutlinedButton(
                      key: ValueKey('agent-revoke-row-${row.id}'),
                      onPressed: owner.busy || review != null || identityReview != null ? null : () => revoke(row),
                      child: Text(l.agentRevokeAccess),
                    ),
                ],
              ],
              if (review != null) ...[
                const SizedBox(height: 12),
                Text(
                  review.stage == AgentAssociationReviewStage.offer
                      ? l.agentAssociationOfferReview
                      : l.agentAssociationCertificateReview,
                ),
                Text(
                  l.agentAssociationRemaining(owner.remainingSeconds.toString()),
                  key: const ValueKey('agent-association-countdown'),
                ),
                fact(l.agentMetricsScope, review.scopeName),
                if (review.exporterCatalog case final catalog?) ...[
                  Text('${catalog.policy.address} · cpu, meminfo'), // l10n-ignore: endpoint and exporter collector identifiers.
                  Text(catalog.policy.artifactVersion),
                  Text(l.agentExporterEnrollmentDisclosure),
                ],
                if (review.metricsCollector != null) ...catalogFacts(review.metricsCollector!),
                fact(l.agentAssociationSavedHost, owner.host!.name),
                fact(l.agentAssociationNode, review.agentId),
                fact(l.agentAssociationNodeFingerprint, review.nodeFingerprint),
                fact(l.agentAssociationOwner, '${review.ownerKind}: ${review.ownerId}'),
                if (review.ownerDeviceId != null) fact(l.agentAssociationDevice, review.ownerDeviceId!),
                if (review.ownerSigningKeySha256 != null)
                  fact(l.agentAssociationSigningKey, review.ownerSigningKeySha256!),
                if (review.nodePublicKey != null) fact(l.agentAssociationPublicKey, review.nodePublicKey!),
                if (review.nodeKeyEpoch != null) fact(l.agentAssociationNodeEpoch, review.nodeKeyEpoch!),
                if (review.nodeContextId != null)
                  fact(l.agentAssociationNodeContext, '${review.nodeContextKind}: ${review.nodeContextId}'),
                fact(l.agentAssociationOwnerEpoch, review.ownerEpoch),
                fact(l.agentAssociationCheckpoint, review.checkpoint),
                fact(l.agentAssociationTlsCertificate, review.certificateSha256),
                if (review.claimDigest != null) fact(l.agentAssociationClaim, review.claimDigest!),
                if (review.stage == AgentAssociationReviewStage.certificate)
                  GlassSelect<int>(
                    key: const ValueKey('agent-association-lifetime'),
                    value: chosenLifetime,
                    expand: true,
                    semanticLabel: l.agentAssociationLifetime,
                    items: [
                      for (final seconds in lifetimeChoices)
                        GlassSelectItem(value: seconds, label: l.agentAssociationLifetimeSeconds(seconds.toString())),
                    ],
                    onChanged: owner.busy ? null : (value) => setState(() => lifetime = value),
                  ),
              ],
              if (result != null) ...[
                InfoBanner(
                  key: const ValueKey('agent-association-associated'),
                  tone: BannerTone.info,
                  message: l.agentAssociationAssociatedDetail,
                ),
                fact(l.agentMetricsScope, switch (result.purpose) {
                  AgentAssociationPurpose.metrics => 'metrics.collect',
                  AgentAssociationPurpose.exporters => 'exporters.manage',
                  AgentAssociationPurpose.tunnel => 'tunnel.open',
                }),
                if (result.metricsCollector != null) ...catalogFacts(result.metricsCollector!),
                fact(l.agentAssociationNode, result.agentId),
                if (result.certificateExpiresUtc != null)
                  fact(l.agentAssociationActualExpiry, result.certificateExpiresUtc!.toIso8601String()),
                fact(l.agentAssociationCheckpoint, result.checkpoint),
                fact(l.agentAssociationTlsCertificate, result.certificateDigest),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('agent-association-close'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l.commonClose),
        ),
        if (widget.exporterPort is AgentExporterEnrollmentPort &&
            review == null &&
            identityReview == null &&
            !owner.busy)
          OutlinedButton(
            key: const ValueKey('agent-exporter-enrollment'),
            onPressed: exporterEnrollment,
            child: Text(l.agentExporterEnroll),
          ),
        if (widget.port is AgentMetricsEnrollmentPort && review == null && identityReview == null && !owner.busy)
          OutlinedButton(
            key: const ValueKey('agent-metrics-enrollment'),
            onPressed: metricsEnrollment,
            child: Text(l.agentMetricsEnrollmentTitle),
          ),
        if (review == null && identityReview == null && !owner.busy)
          TextButton(
            key: const ValueKey('agent-association-status'),
            onPressed: owner.readStatus,
            child: Text(l.agentAssociationReadStatus),
          ),
        if (status != null && review == null && identityReview == null && result == null)
          OutlinedButton(
            key: const ValueKey('agent-association-probe'),
            onPressed:
                owner.busy ||
                    owner.host == null ||
                    status.clientIdentityState != AgentClientIdentityState.storedMetadata
                ? null
                : owner.probe,
            child: Text(l.agentAssociationProbe),
          ),
        if (owner.busy || review != null || identityReview != null)
          TextButton(
            key: const ValueKey('agent-association-cancel'),
            onPressed: owner.cancel,
            child: Text(l.commonCancel),
          ),
        if (status?.clientIdentityState == AgentClientIdentityState.missing &&
            identityReview == null &&
            identityResult == null &&
            review == null)
          OutlinedButton(
            key: const ValueKey('agent-identity-prepare'),
            onPressed: owner.busy ? null : owner.prepareIdentity,
            child: Text(l.agentAssociationIdentityPrepare),
          ),
        if (identityReview != null)
          FilledButton(
            key: const ValueKey('agent-identity-confirm'),
            onPressed: owner.canCreateIdentity ? owner.createIdentity : null,
            child: Text(l.agentAssociationIdentityConfirm),
          ),
        if (review != null)
          FilledButton(
            key: const ValueKey('agent-association-approve'),
            onPressed: owner.canApprove && review.purpose == widget.enrollmentPurpose
                ? () => owner.approve(lifetimeSeconds: chosenLifetime)
                : null,
            child: Text(
              review.stage == AgentAssociationReviewStage.offer
                  ? l.agentAssociationApproveOffer
                  : l.agentAssociationApproveCertificate,
            ),
          ),
      ],
    );
  }
}
