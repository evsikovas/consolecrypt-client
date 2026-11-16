import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart' show DependencyOriginalGuard;
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_exporter.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_port.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showAgentAssociation(
  BuildContext context,
  WidgetRef ref,
  MonitoringScope scope,
  MonitoringSession session,
  bool Function() workspaceCurrent,
) async {
  final service = session.service;
  if (service is! AgentAssociationCapability) return;
  final port = (service as AgentAssociationCapability).agentAssociationPort, originalNative = scope.nativeScope;
  if (port == null || originalNative == null || !scope.current) return;
  final tunnelPort = service is AgentTunnelRouteCapability
      ? (service as AgentTunnelRouteCapability).agentTunnelRoutePort
      : null;
  final exporterPort = service is AgentExporterCapability
      ? (service as AgentExporterCapability).agentExporterPort
      : null;
  final metricsPort = service is AgentMetricsCollectorCapability
      ? (service as AgentMetricsCollectorCapability).agentMetricsCollectorPort
      : null;
  final outboundPort = service is AgentOutboundBootstrapCapability
      ? (service as AgentOutboundBootstrapCapability).agentOutboundBootstrapPort
      : null;
  final collectorServicePort = service is AgentCollectorServiceCapability
      ? (service as AgentCollectorServiceCapability).agentCollectorServicePort
      : null;
  final app = ref.read(appServicesProvider),
      events = ref.read(criticalRuntimeEventsProvider),
      router = ref.read(routerProvider),
      inventory = ref.read(inventoryServiceProvider);
  final epoch = events?.currentScope.epoch, uri = router.routeInformationProvider.value.uri;
  final hosts = ref.read(hostsProvider).value ?? const <Host>[],
      known = ref.read(knownHostsProvider).value ?? const <KnownHost>[];
  var retired = false, finished = false;
  final live = ValueNotifier(true);
  bool same() =>
      context.mounted &&
      scope.current &&
      workspaceCurrent() &&
      identical(ref.read(appServicesProvider), app) &&
      identical(
        service is AgentExporterCapability ? (service as AgentExporterCapability).agentExporterPort : null,
        exporterPort,
      ) &&
      identical(
        service is AgentCollectorServiceCapability
            ? (service as AgentCollectorServiceCapability).agentCollectorServicePort
            : null,
        collectorServicePort,
      ) &&
      identical(
        service is AgentOutboundBootstrapCapability
            ? (service as AgentOutboundBootstrapCapability).agentOutboundBootstrapPort
            : null,
        outboundPort,
      ) &&
      identical(ref.read(monitoringScopeProvider), scope) &&
      identical(ref.read(monitoringSessionProvider), session) &&
      identical(
        service is AgentMetricsCollectorCapability
            ? (service as AgentMetricsCollectorCapability).agentMetricsCollectorPort
            : null,
        metricsPort,
      ) &&
      identical(
        service is AgentTunnelRouteCapability ? (service as AgentTunnelRouteCapability).agentTunnelRoutePort : null,
        tunnelPort,
      ) &&
      identical(ref.read(criticalRuntimeEventsProvider), events) &&
      events?.currentScope.epoch == epoch &&
      events?.currentScope.streamUnavailable == false &&
      originalNative.same(events?.currentScope.scope) &&
      identical(ref.read(inventoryServiceProvider), inventory) &&
      identical(ref.read(routerProvider), router) &&
      uri.path == AppRoutes.monitoring &&
      router.routeInformationProvider.value.uri == uri &&
      identical(ref.read(hostsProvider).value, hosts) &&
      identical(ref.read(knownHostsProvider).value, known);
  void retire() {
    if (retired || finished) return;
    retired = true;
    scheduleMicrotask(() {
      if (!finished) live.value = false;
    });
  }

  bool original() {
    if (!same()) retire();
    return !retired && !finished;
  }

  void observe() {
    if (!same()) retire();
  }

  final subscriptions = <ProviderSubscription<dynamic>>[
    ref.listenManual(appServicesProvider, (_, _) => observe()),
    ref.listenManual(monitoringScopeProvider, (_, _) => observe()),
    ref.listenManual(monitoringSessionProvider, (_, _) => observe()),
    ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ref.listenManual(hostsProvider, (_, _) => observe()),
    ref.listenManual(knownHostsProvider, (_, _) => observe()),
    ref.listenManual(inventoryServiceProvider, (_, _) => observe()),
  ];
  final notices = events?.watchRuntimeNotices().listen(
    (_) => observe(),
    onError: (Object _, StackTrace _) => retire(),
    onDone: retire,
  );
  router.routeInformationProvider.addListener(observe);
  try {
    if (!original()) return;
    await showAppDialog<void>(
      context,
      secure: true,
      builder: (_) => DependencyOriginalGuard(
        original: live,
        builder: (_) => AgentAssociationDialog(
          port: port,
          tunnelPort: tunnelPort,
          metricsPort: metricsPort,
          exporterPort: exporterPort,
          outboundPort: outboundPort,
          collectorServicePort: collectorServicePort,
          scope: originalNative,
          original: original,
        ),
      ),
    );
  } finally {
    finished = true;
    for (final subscription in subscriptions) {
      subscription.close();
    }
    await notices?.cancel();
    router.routeInformationProvider.removeListener(observe);
    live.dispose();
  }
}
