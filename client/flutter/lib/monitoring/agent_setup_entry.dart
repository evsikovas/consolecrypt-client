import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_port.dart';
import 'package:consolecrypt/monitoring/agent_setup_wizard.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// One-use navigation to graphs after a Native-verified setup, never a command.
final class AgentSetupFocus {
  AgentSetupFocus(this.result);
  final AgentSetupVerification result;
}

Future<AgentSetupVerification?> showAgentSetup(BuildContext context, WidgetRef ref, {String? hostId}) async {
  final session = ref.read(monitoringSessionProvider), scope = ref.read(monitoringScopeProvider);
  final service = session?.service;
  final port = service is AgentSetupCapability ? (service as AgentSetupCapability).agentSetupPort : null;
  if (session == null || port == null || !scope.current) return null;
  final app = ref.read(appServicesProvider), router = ref.read(routerProvider);
  String hostSignature() {
    final rows = ref.read(hostsProvider).value;
    if (rows == null) return '';
    final values = [
      for (final host in rows)
        if (hostId == null || host.id.value == hostId)
          '${host.id.value}:${host.updatedAt.toUtc().toIso8601String()}:${host.protocol.name}',
    ];
    values.sort();
    return values.join('|');
  }

  final originalHosts = hostSignature();
  final uri = router.routeInformationProvider.value.uri;
  var open = true, retired = false;
  bool current() {
    if (!open || retired) return false;
    final valid =
        context.mounted &&
        scope.current &&
        identical(ref.read(monitoringScopeProvider), scope) &&
        identical(ref.read(monitoringSessionProvider), session) &&
        identical(ref.read(appServicesProvider), app) &&
        hostSignature() == originalHosts &&
        router.routeInformationProvider.value.uri == uri;
    if (!valid) retired = true;
    return valid;
  }

  void observe() {
    current();
  }

  final subscriptions = <ProviderSubscription<dynamic>>[
    ref.listenManual(monitoringScopeProvider, (_, _) => observe()),
    ref.listenManual(monitoringSessionProvider, (_, _) => observe()),
    ref.listenManual(appServicesProvider, (_, _) => observe()),
    ref.listenManual(hostsProvider, (_, _) => observe()),
  ];
  router.routeInformationProvider.addListener(observe);
  try {
    final result = await showAppDialog<AgentSetupVerification>(
      context,
      secure: true,
      builder: (dialogContext) => Consumer(
        builder: (context, watch, _) {
          watch.watch(monitoringScopeProvider);
          watch.watch(monitoringSessionProvider);
          watch.watch(appServicesProvider);
          watch.watch(hostsProvider);
          return ValueListenableBuilder<RouteInformation>(
            valueListenable: router.routeInformationProvider,
            builder: (context, _, _) {
              if (!current()) {
                WidgetsBinding.instance.addPostFrameCallback((_) {
                  if (dialogContext.mounted && ModalRoute.of(dialogContext)?.isCurrent == true) {
                    Navigator.of(dialogContext).pop();
                  }
                });
                return const SizedBox.shrink();
              }
              return AgentSetupWizard(
                port: port,
                initialHostId: hostId,
                isCurrent: current,
                onOpenGraphs: (value) {
                  if (current()) Navigator.of(dialogContext).pop(value);
                },
              );
            },
          );
        },
      ),
    );
    return current() ? result : null;
  } finally {
    open = false;
    router.routeInformationProvider.removeListener(observe);
    for (final subscription in subscriptions) {
      subscription.close();
    }
  }
}
