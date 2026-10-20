import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:material_ui/material_ui.dart';

/// Structural resource details deliberately omit environment values, probe
/// headers, exec arguments and volume contents, including in native-redacted
/// responses. They are not needed to understand scheduling or health.
class KubeResourceSummary extends StatelessWidget {
  const KubeResourceSummary({required this.resource, required this.detail, super.key});
  final KubeResourceRef resource;
  final KubeResourceDetail detail;

  static Map<Object?, Object?> _map(Object? value) => value is Map ? value : const {};
  static List<Map<Object?, Object?>> _maps(Object? value) =>
      value is List ? value.whereType<Map<Object?, Object?>>().toList() : const [];
  static String _text(Object? value) => value is String || value is num || value is bool ? value.toString() : '—';

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final spec = _map(detail.object['spec']), status = _map(detail.object['status']);
    var pod = spec;
    if (!resource.api.isPod) {
      pod = _map(_map(spec['template'])['spec']);
      if (resource.api.kind == 'CronJob') {
        pod = _map(_map(_map(_map(spec['jobTemplate'])['spec'])['template'])['spec']);
      }
    }
    final containers = [
      ..._maps(pod['initContainers']),
      ..._maps(pod['containers']),
      ..._maps(pod['ephemeralContainers']),
    ];
    final states = [
      ..._maps(status['initContainerStatuses']),
      ..._maps(status['containerStatuses']),
      ..._maps(status['ephemeralContainerStatuses']),
    ];
    Widget entry(String title, String text) => Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title, style: Theme.of(context).textTheme.labelLarge),
          SelectableText(text),
        ],
      ),
    );
    String quantities(Object? value) => _map(value).entries.map((e) => '${e.key}: ${_text(e.value)}').join(' · ');
    Widget probe(String name, Map<Object?, Object?> value) {
      final action = ['httpGet', 'tcpSocket', 'grpc', 'exec'].where(value.containsKey).firstOrNull;
      final endpoint = _map(value[action]);
      return entry(
        name,
        [
          ?action,
          if (endpoint['port'] != null) '${l.kubePort}: ${_text(endpoint['port'])}',
          ...[
            'initialDelaySeconds',
            'periodSeconds',
            'timeoutSeconds',
            'failureThreshold',
            'successThreshold',
          ].where(value.containsKey).map((key) => '$key: ${_text(value[key])}'),
        ].join(' · '),
      );
    }

    final node = resource.api.group.isEmpty && resource.api.kind == 'Node';
    final volumes = _maps(pod['volumes']);
    if (containers.isEmpty && !node && volumes.isEmpty && spec['replicas'] == null) return const SizedBox.shrink();
    return ExpansionTile(
      key: const ValueKey('kube-structural-summary'),
      tilePadding: EdgeInsets.zero,
      title: Text(l.kubeResourceConfiguration),
      childrenPadding: const EdgeInsets.only(bottom: 12),
      expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (spec['replicas'] != null)
          entry(l.kubeReplicaStatus, '${_text(status['readyReplicas'])} / ${_text(spec['replicas'])}'),
        if (pod['nodeName'] != null) entry(l.kubeNode, _text(pod['nodeName'])),
        if (pod['serviceAccountName'] != null) entry(l.kubeServiceAccount, _text(pod['serviceAccountName'])),
        if (_map(pod['nodeSelector']).isNotEmpty) entry(l.kubeNodeSelector, quantities(pod['nodeSelector'])),
        for (final container in containers)
          ExpansionTile(
            tilePadding: EdgeInsets.zero,
            title: Text(
              /* l10n-ignore: Localized container label followed by the Kubernetes container name. */ '${l.kubeContainer} · ${_text(container['name'])}',
            ), // l10n-ignore: Localized container label followed by the Kubernetes container name.
            subtitle: Text(_text(container['image'])),
            expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final state in states.where((state) => state['name'] == container['name']))
                entry(
                  l.kubeStatus,
                  [
                    '${l.kubeReady}: ${_text(state['ready'])}',
                    '${l.kubeRestarts}: ${_text(state['restartCount'])}',
                    for (final phase in _map(state['state']).entries)
                      '${phase.key}: ${_text(_map(phase.value)['reason'])}',
                    if (_map(_map(state['lastState'])['terminated'])['exitCode'] != null)
                      '${l.kubeIssueExitCode}: ${_text(_map(_map(state['lastState'])['terminated'])['exitCode'])}',
                  ].join(' · '),
                ),
              if (_map(_map(container['resources'])['requests']).isNotEmpty)
                entry(l.kubeRequests, quantities(_map(container['resources'])['requests'])),
              if (_map(_map(container['resources'])['limits']).isNotEmpty)
                entry(l.kubeLimits, quantities(_map(container['resources'])['limits'])),
              for (final name in ['livenessProbe', 'readinessProbe', 'startupProbe'])
                if (_map(container[name]).isNotEmpty) probe(name, _map(container[name])),
              if (_maps(container['ports']).isNotEmpty)
                entry(
                  l.kubePorts,
                  _maps(container['ports'])
                      .map(
                        (port) => '${_text(port['name'])}: ${_text(port['containerPort'])}/${_text(port['protocol'])}',
                      )
                      .join('\n'),
                ),
              if (_maps(container['volumeMounts']).isNotEmpty)
                entry(
                  l.kubeVolumeMounts,
                  _maps(container['volumeMounts'])
                      .map((mount) => '${_text(mount['name'])} → ${_text(mount['mountPath'])}')
                      .join('\n'),
                ),
            ],
          ),
        if (volumes.isNotEmpty)
          entry(
            l.kubeVolumes,
            volumes
                .map((volume) {
                  final type = volume.keys.where((key) => key != 'name').firstOrNull;
                  final source = _map(volume[type]);
                  final reference = source['claimName'] ?? source['secretName'] ?? source['name'];
                  return '${_text(volume['name'])} · $type${reference == null ? '' : ' · ${_text(reference)}'}';
                })
                .join('\n'),
          ),
        if (node) ...[
          entry(l.kubeCapacity, quantities(status['capacity'])),
          entry(l.kubeAllocatable, quantities(status['allocatable'])),
          entry(l.kubeUnschedulable, _text(spec['unschedulable'] ?? false)),
          if (_maps(spec['taints']).isNotEmpty)
            entry(
              l.kubeTaints,
              _maps(spec['taints'])
                  .map((taint) => '${_text(taint['key'])}=${_text(taint['value'])}:${_text(taint['effect'])}')
                  .join('\n'),
            ),
          if (_map(status['nodeInfo']).isNotEmpty)
            entry(
              l.kubeNodeInfo,
              ['kubeletVersion', 'containerRuntimeVersion', 'operatingSystem', 'architecture', 'osImage']
                  .where(_map(status['nodeInfo']).containsKey)
                  .map((key) => '$key: ${_text(_map(status['nodeInfo'])[key])}')
                  .join('\n'),
            ),
        ],
      ],
    );
  }
}
