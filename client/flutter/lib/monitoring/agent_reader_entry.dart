import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart' show DependencyOriginalGuard;
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/agent_heartbeat_observation.dart';
import 'package:consolecrypt/monitoring/agent_reader_dialog.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showAgentReader(
  BuildContext context,
  WidgetRef ref,
  MonitoringScope scope,
  MonitoringSession session,
  List<MonitoringSource> sources,
  List<MonitoringBinding> bindings,
  bool Function() workspaceCurrent,
) async {
  final service = session.service;
  if (service is! AgentReaderCapability) return;
  final port = (service as AgentReaderCapability).agentReaderPort, originalNative = scope.nativeScope;
  if (port == null || originalNative == null || !scope.current) return;
  final app = ref.read(appServicesProvider),
      events = ref.read(criticalRuntimeEventsProvider),
      router = ref.read(routerProvider),
      inventory = ref.read(inventoryServiceProvider),
      files = ref.read(fileDialogServiceProvider);
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
      identical(ref.read(monitoringScopeProvider), scope) &&
      identical(ref.read(monitoringSessionProvider), session) &&
      identical(ref.read(criticalRuntimeEventsProvider), events) &&
      events?.currentScope.epoch == epoch &&
      events?.currentScope.streamUnavailable == false &&
      originalNative.same(events?.currentScope.scope) &&
      identical(ref.read(inventoryServiceProvider), inventory) &&
      identical(ref.read(fileDialogServiceProvider), files) &&
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
    ref.listenManual(fileDialogServiceProvider, (_, _) => observe()),
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
        builder: (_) => AgentReaderDialog(
          port: port,
          profileId: originalNative.profileId,
          notificationScopeId: originalNative.nonce,
          sources: sources,
          bindings: bindings,
          hosts: hosts,
          knownHosts: known,
          files: files,
          original: original,
          onHeartbeat: (selection, association, dataset) {
            if (!original() || events == null || service is! AgentHeartbeatObservationCapability) return;
            final host = hosts.where((value) => value.id.value == selection.hostId).firstOrNull;
            if (host == null) return;
            (service as AgentHeartbeatObservationCapability).agentHeartbeatObservations.record(
              selection: selection,
              association: association,
              dataset: dataset,
              host: host,
              scopeOwner: events,
              epoch: epoch,
              receivedAt: DateTime.now().toUtc(),
            );
          },
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
