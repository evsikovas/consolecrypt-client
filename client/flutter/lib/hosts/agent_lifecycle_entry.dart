import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/agent_installer_project_focus.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_agent_installer_dialog.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart' show DependencyOriginalGuard;
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/agent_heartbeat_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Fixed existing source factory only; no connection, installation or execution.
Future<void> showHostAgentLifecycle(BuildContext context, WidgetRef ref, Host host) async {
  final service = ref.read(automationServiceProvider), scope = ref.read(automationScopeProvider);
  if (service == null || service is! AutomationAgentInstallerService || !scope.unlocked || host.isRdp) return;
  final installer = service as AutomationAgentInstallerService;
  final app = ref.read(appServicesProvider),
      events = ref.read(criticalRuntimeEventsProvider),
      router = ref.read(routerProvider),
      files = ref.read(fileDialogServiceProvider),
      monitoring = ref.read(monitoringSessionProvider)?.service;
  final uri = router.routeInformationProvider.value.uri,
      frame = events?.currentScope.scope,
      epoch = events?.currentScope.epoch;
  if (uri.path != AppRoutes.hosts || frame == null || frame.profileId != scope.profileId?.toString()) return;
  var sourceRetired = false, retired = false, finished = false;
  final live = ValueNotifier(true);
  bool sourceCurrent() {
    if (sourceRetired || !context.mounted) return false;
    final currentHost = ref.read(hostsProvider).value?.where((h) => h.id == host.id).firstOrNull;
    final same =
        context.mounted &&
        identical(currentHost, host) &&
        identical(ref.read(appServicesProvider), app) &&
        identical(ref.read(automationServiceProvider), service) &&
        identical(ref.read(automationScopeProvider), scope) &&
        identical(ref.read(criticalRuntimeEventsProvider), events) &&
        events?.currentScope.epoch == epoch &&
        events?.currentScope.streamUnavailable == false &&
        events?.currentScope.unsupported == false &&
        frame.same(events?.currentScope.scope) &&
        identical(ref.read(fileDialogServiceProvider), files) &&
        identical(ref.read(monitoringSessionProvider)?.service, monitoring) &&
        identical(ref.read(routerProvider), router);
    if (!same) sourceRetired = true;
    return same;
  }

  void retire() {
    if (retired || finished) return;
    retired = true;
    // Synchronous original() checks refuse work immediately; the notifier only
    // removes the dialog after the current provider/render callback returns.
    scheduleMicrotask(() {
      if (!finished) live.value = false;
    });
  }

  bool original() {
    if (!sourceCurrent() || router.routeInformationProvider.value.uri != uri) retire();
    return !retired && !finished;
  }

  void observe() {
    original();
  }

  final subscriptions = <ProviderSubscription<dynamic>>[
    ref.listenManual(appServicesProvider, (_, _) => observe()),
    ref.listenManual(automationServiceProvider, (_, _) => observe()),
    ref.listenManual(automationScopeProvider, (_, _) => observe()),
    ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ref.listenManual(hostsProvider, (_, _) => observe()),
    ref.listenManual(fileDialogServiceProvider, (_, _) => observe()),
    ref.listenManual(monitoringSessionProvider, (_, _) => observe()),
  ];
  final notices = events?.watchRuntimeNotices().listen(
    (_) => observe(),
    onError: (Object _, StackTrace _) {
      sourceRetired = true;
      retire();
    },
    onDone: () {
      sourceRetired = true;
      retire();
    },
  );
  router.routeInformationProvider.addListener(observe);
  try {
    if (!original()) return;
    AgentCadvisorComposerFocus? cadvisorFocus;
    final observed = events != null && monitoring is AgentHeartbeatObservationCapability
        ? (monitoring as AgentHeartbeatObservationCapability).agentHeartbeatObservations.read(
            host: host,
            profileId: frame.profileId,
            notificationScopeId: frame.nonce,
            scopeOwner: events,
            epoch: epoch,
          )
        : null;
    final receipt = await showAppDialog<AgentInstallerCreated>(
      context,
      secure: true,
      builder: (_) => DependencyOriginalGuard(
        original: live,
        builder: (dialogContext) => AutomationAgentInstallerDialog(
          scope: scope,
          service: installer,
          fixedHostId: host.id.value,
          observedHeartbeat: observed,
          originalCurrent: original,
          originalLive: live,
          onCadvisorAnsible: service is AutomationJobService
              ? (nativeContext, selectedHost) {
                  if (!original() ||
                      selectedHost.id != host.id.value ||
                      nativeContext.frame.profileId != frame.profileId ||
                      nativeContext.frame.notificationScopeId != frame.nonce ||
                      !nativeContext.hosts.any((value) => identical(value, selectedHost))) {
                    return;
                  }
                  cadvisorFocus = AgentCadvisorComposerFocus(
                    host: selectedHost,
                    frame: nativeContext.frame,
                    returnUri: uri,
                    scope: scope,
                    service: service,
                    original: sourceCurrent,
                  );
                  Navigator.of(dialogContext).pop();
                }
              : null,
          onOpenCreated: (created) {
            if (original()) Navigator.of(dialogContext).pop(created);
          },
        ),
      ),
    );
    if (!original()) return;
    if (cadvisorFocus case final requested?) {
      router.go(AppRoutes.automation, extra: requested);
      return;
    }
    if (receipt == null) return;
    final focus = AgentInstallerProjectFocus.fromCreated(
      receipt: receipt,
      hostId: host.id.value,
      hostName: host.name,
      returnUri: uri,
      scope: scope,
      service: service,
      original: sourceCurrent,
    );
    ref.invalidate(automationProjectsProvider);
    if (!original()) return;
    router.go(AppRoutes.automation, extra: focus);
  } finally {
    finished = true;
    for (final s in subscriptions) {
      s.close();
    }
    await notices?.cancel();
    router.routeInformationProvider.removeListener(observe);
    live.dispose();
  }
}
