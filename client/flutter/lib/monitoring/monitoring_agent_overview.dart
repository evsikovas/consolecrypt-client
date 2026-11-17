import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/monitoring/agent_heartbeat_observation.dart';
import 'package:material_ui/material_ui.dart';

/// The Agent's purpose and evidence, separate from Ansible and normal metric
/// setup. Saved configuration alone never changes the three unknown statuses.
class MonitoringAgentOverview extends StatelessWidget {
  const MonitoringAgentOverview({
    required this.isCurrent,
    this.onOpenAgentReader,
    this.onOpenAgentAssociation,
    this.observation,
    this.hostName,
    super.key,
  });

  final bool Function() isCurrent;
  final VoidCallback? onOpenAgentReader, onOpenAgentAssociation;

  /// Supply only an existing authenticated observation read for this exact
  /// profile, unlock epoch and unchanged Host through AgentHeartbeatObservations.
  final AgentHeartbeatObservation? observation;
  final String? hostName;

  Widget _status(BuildContext context, String label, IconData icon) =>
      ListTile(leading: Icon(icon), title: Text(label), subtitle: Text(context.l10n.monitoringAgentStatusUnknown));

  VoidCallback? _action(VoidCallback? callback) => callback == null
      ? null
      : () {
          if (isCurrent()) callback();
        };

  @override
  Widget build(BuildContext context) {
    if (!isCurrent()) return const SizedBox.shrink();
    final l = context.l10n, evidence = observation;
    final name = hostName ?? evidence?.host.name;
    return SingleChildScrollView(
      key: const ValueKey('monitoring-agent-overview'),
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.monitoringAgentOverviewTitle, style: Theme.of(context).textTheme.titleLarge),
          if (name != null) Padding(padding: const EdgeInsets.only(top: 4), child: Text(name)),
          const SizedBox(height: 12),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(l.monitoringAgentOverviewPurpose),
                  const SizedBox(height: 8),
                  Text(
                    l.monitoringAgentOverviewOptional,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600),
                  ),
                  const SizedBox(height: 8),
                  Text(l.monitoringAgentOverviewSetupHint),
                ],
              ),
            ),
          ),
          const SizedBox(height: 12),
          Text(l.monitoringAgentStatusTitle, style: Theme.of(context).textTheme.titleSmall),
          Card(
            child: Column(
              children: [
                _status(context, l.monitoringAgentSoftwareStatus, Icons.inventory_2_outlined),
                _status(context, l.monitoringAgentConnectionStatus, Icons.link),
                _status(context, l.monitoringAgentMetricsStatus, Icons.monitor_heart_outlined),
              ],
            ),
          ),
          Text(l.monitoringAgentStatusHint, style: Theme.of(context).textTheme.bodySmall),
          if (evidence != null && evidence.dataset.agentVersion != null) ...[
            const SizedBox(height: 12),
            Text(
              l.hostAgentLastObservedVersion(evidence.dataset.agentVersion!, formatDateTime(l, evidence.receivedAt)),
              key: const ValueKey('monitoring-agent-observed-version'),
            ),
            Text(l.hostAgentLastObservedHint, style: Theme.of(context).textTheme.bodySmall),
          ],
          if (onOpenAgentReader != null || onOpenAgentAssociation != null) ...[
            const SizedBox(height: 20),
            Text(l.monitoringAgentManagementTitle, style: Theme.of(context).textTheme.titleSmall),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                if (onOpenAgentReader != null)
                  OutlinedButton.icon(
                    key: const ValueKey('monitoring-agent-read'),
                    onPressed: _action(onOpenAgentReader),
                    icon: const Icon(Icons.fact_check_outlined),
                    label: Text(l.monitoringAgentReadStatus),
                  ),
                if (onOpenAgentAssociation != null)
                  OutlinedButton.icon(
                    key: const ValueKey('monitoring-agent-association'),
                    onPressed: _action(onOpenAgentAssociation),
                    icon: const Icon(Icons.link),
                    label: Text(l.monitoringAgentConnectionSettings),
                  ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}
