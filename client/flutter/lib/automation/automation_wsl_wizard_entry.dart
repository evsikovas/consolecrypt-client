import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_windows_prerequisites.dart';
import 'package:consolecrypt/automation/automation_wsl_preparation.dart';
import 'package:consolecrypt/automation/automation_wsl_prerequisite_wizard.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Captures the existing owner before discovery, Native inspection or shell I/O.
Future<void> showAutomationWslWizard({
  required BuildContext context,
  required WidgetRef ref,
  required AutomationScope scope,
  required AutomationRunnerService service,
  required bool Function() originalCurrent,
  LocalShellProfiles? profiles,
  String? distribution,
  String? user,
  ValueChanged<WslPreparedSelection>? onPrepared,
}) async {
  final app = ref.read(appServicesProvider),
      terminal = ref.read(terminalServiceProvider),
      events = ref.read(criticalRuntimeEventsProvider);
  final frame = events?.currentScope,
      router = ref.read(routerProvider),
      tabs = ref.read(terminalTabsProvider.notifier);
  final uri = router.routeInformationProvider.value.uri,
      parent = ModalRoute.of(context);
  final live = ValueNotifier(true);
  var retired = false;
  bool matches() =>
      context.mounted &&
      originalCurrent() &&
      scope.unlocked &&
      identical(ref.read(automationScopeProvider), scope) &&
      identical(ref.read(appServicesProvider), app) &&
      identical(ref.read(automationRunnerServiceProvider), service) &&
      identical(ref.read(terminalServiceProvider), terminal) &&
      identical(ref.read(criticalRuntimeEventsProvider), events) &&
      events?.currentScope.epoch == frame?.epoch &&
      events?.currentScope.streamUnavailable != true &&
      events?.currentScope.scope?.profileId == frame?.scope?.profileId &&
      events?.currentScope.scope?.nonce == frame?.scope?.nonce &&
      identical(ref.read(routerProvider), router) &&
      router.routeInformationProvider.value.uri == uri;
  void observe() {
    if (!retired && !matches()) {
      retired = true;
      live.value = false;
    }
  }

  bool original() {
    observe();
    return !retired;
  }

  final subscriptions = <ProviderSubscription<dynamic>>[
    ref.listenManual(appServicesProvider, (_, _) => observe()),
    ref.listenManual(automationRunnerServiceProvider, (_, _) => observe()),
    ref.listenManual(automationScopeProvider, (_, _) => observe()),
    ref.listenManual(terminalServiceProvider, (_, _) => observe()),
    ref.listenManual(criticalRuntimeEventsProvider, (_, _) => observe()),
    ref.listenManual(routerProvider, (_, _) => observe()),
  ];
  final notices = events?.watchRuntimeNotices().listen(
    (_) => observe(),
    onError: (Object _, StackTrace _) {
      if (!retired) {
        retired = true;
        live.value = false;
      }
    },
    onDone: () {
      if (!retired) {
        retired = true;
        live.value = false;
      }
    },
  );
  router.routeInformationProvider.addListener(observe);
  final native = frame?.scope;
  final nativeScope =
      frame?.streamUnavailable != true &&
          native?.profileId == scope.profileId?.toString()
      ? native
      : null;
  try {
    if (!original()) return;
    await showGlassDialog<void>(
      context,
      builder: (dialogContext) => DependencyOriginalGuard(
        original: live,
        builder: (_) => AutomationWslPreparation(
          overview: AutomationWslPrerequisiteWizard(
            original: original,
            initialProfiles: profiles,
            initialDistribution: distribution,
            initialUser: user,
            nativeScope: nativeScope,
            windows: service is AutomationWindowsPrerequisiteService
                ? service as AutomationWindowsPrerequisiteService
                : null,
            setup: service is AutomationRunnerSetupService
                ? service as AutomationRunnerSetupService
                : null,
            discover: () => terminal.localProfiles(),
            onPrepared: (selection) {
              if (original()) onPrepared?.call(selection);
            },
            openTerminal: (selected) async {
              if (!original()) return;
              final tab = await tabs.openTarget(LocalTerminalTarget(selected));
              if (!original() || !dialogContext.mounted) {
                await tabs.close(tab);
                return;
              }
              // Only these owned routes are removed, never an unrelated top modal.
              final dialog = ModalRoute.of(dialogContext),
                  navigator = Navigator.of(dialogContext);
              if (dialog != null && dialog.isActive) {
                navigator.removeRoute(dialog);
              }
              if (parent is PopupRoute && parent.isActive) {
                navigator.removeRoute(parent);
              }
              router.go(AppRoutes.terminal);
            },
          ),
        ),
      ),
    );
  } finally {
    retired = true;
    for (final sub in subscriptions) {
      sub.close();
    }
    router.routeInformationProvider.removeListener(observe);
    await notices?.cancel();
    live.dispose();
  }
}
