import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/agent_metrics_manifest.dart';
import 'package:material_ui/material_ui.dart';

/// Inert metadata from the exact v2 Native catalogue. A catalogue is not
/// observed execution, an installation result, or persistent-service authority.
class AgentMetricsRuntimeReview extends StatelessWidget {
  const AgentMetricsRuntimeReview({super.key, required this.runtime});
  final AgentMetricsRuntime runtime;

  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(label), SelectableText(value)]),
  );

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InfoBanner(
          key: ValueKey(
            runtime.persistentDisk ? 'agent-metrics-runtime-disk-metadata' : 'agent-metrics-runtime-volatile-only',
          ),
          message: runtime.persistentDisk ? l.agentMetricsRuntimeDiskMetadata : l.agentMetricsRuntimeVolatileOnly,
        ),
        ExpansionTile(
          key: const ValueKey('agent-metrics-runtime-details'),
          title: Text(l.agentMetricsRuntime),
          subtitle: Text('${runtime.kind} ${runtime.upstreamTag} / ${runtime.platformName}'),
          children: [
            fact(l.agentMetricsPlatform, runtime.platformName),
            fact(l.agentMetricsRuntimeBinarySha, runtime.binarySha256),
            fact(l.agentMetricsRuntimeBinaryBytes, runtime.binaryBytes.toString()),
            Text(l.agentMetricsRuntimeMetadataOnly),
          ],
        ),
      ],
    );
  }
}
