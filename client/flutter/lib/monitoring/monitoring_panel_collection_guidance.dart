import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:material_ui/material_ui.dart';

/// Fixed guidance for an accepted Native missing-series preparation only.
/// No collector discovery, endpoint inference, query or installation capability.
class MonitoringPanelCollectionGuidance extends StatelessWidget {
  const MonitoringPanelCollectionGuidance({super.key, required this.panel, this.enabled = true, this.onOpenKubernetes});

  final MonitoringPanel panel;
  final bool enabled;
  final VoidCallback? onOpenKubernetes;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final collector = switch (panel) {
      MonitoringPanel.hostOverview ||
      MonitoringPanel.cpu ||
      MonitoringPanel.memory ||
      MonitoringPanel.filesystem ||
      MonitoringPanel.diskIo ||
      MonitoringPanel.network => l.monitoringCollectorNode,
      MonitoringPanel.docker => l.monitoringCollectorContainers,
      MonitoringPanel.postgreSql => l.monitoringCollectorPostgres,
      MonitoringPanel.redis => l.monitoringCollectorRedis,
      MonitoringPanel.nginx => l.monitoringCollectorNginx,
    };
    return InfoBanner(
      key: const ValueKey('monitoring-panel-collection-guidance'),
      tone: BannerTone.warning,
      message: [
        l.monitoringCollectorMissingIntro,
        collector,
        if (panel == MonitoringPanel.docker) l.monitoringCollectorKubernetesUsage,
      ].join('\n\n'),
      action: panel == MonitoringPanel.docker
          ? TextButton(
              key: const ValueKey('monitoring-open-existing-kubernetes-metrics'),
              onPressed: enabled ? onOpenKubernetes : null,
              child: Text(l.monitoringCollectorOpenKubernetes),
            )
          : null,
    );
  }
}
