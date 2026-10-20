import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_relations_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeResourceRelations extends ConsumerStatefulWidget {
  const KubeResourceRelations({
    required this.session,
    required this.resource,
    required this.detail,
    required this.service,
    required this.openPod,
    required this.openResource,
    this.showEvents = true,
    this.showRelations = true,
    this.showMetrics = true,
    super.key,
  });
  final KubeSession session;
  final KubeResourceRef resource;
  final KubeResourceDetail detail;
  final KubeBrowserService service;
  final bool showEvents, showRelations, showMetrics;
  final void Function(KubeResourceRow row) openPod;
  final ValueChanged<KubeResourceRef> openResource;
  @override
  ConsumerState<KubeResourceRelations> createState() => _KubeResourceRelationsState();
}

class _KubeResourceRelationsState extends ConsumerState<KubeResourceRelations> with WidgetsBindingObserver {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  List<KubeResourceRow>? _events, _pods;
  KubeMetrics? _metrics;
  final _samples = <KubeMetrics>[];
  List<KubeJson>? _relations;
  bool _relationsPartial = false, _relationsFailed = false;
  bool _eventsFailed = false, _podsFailed = false, _metricsFailed = false, _loading = false;
  bool _foreground = true;
  Timer? _timer;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _workload => widget.resource.api.group == 'apps' || widget.resource.api.group == 'batch';
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    unawaited(_load());
    _timer = Timer.periodic(const Duration(seconds: 10), (_) {
      if (_current && _foreground && !_loading) unawaited(_load());
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) => _foreground = state == AppLifecycleState.resumed;
  Future<void> _load() async {
    if (!_current || _loading) return;
    setState(() => _loading = true);
    final service = widget.service;
    if (service is KubeOperationsService) {
      final ops = service as KubeOperationsService;
      final uid = (widget.detail.object['metadata'] as Map?)?['uid'] as String?;
      if (widget.showEvents && uid != null) {
        try {
          final events = await ops.objectEvents(widget.session.id, widget.resource, uid);
          if (_current) {
            setState(() {
              _events = events;
              _eventsFailed = false;
            });
          }
        } catch (_) {
          if (_current) setState(() => _eventsFailed = true);
        }
      }
      if (!_current) return;
      if (widget.showRelations && _workload) {
        try {
          final pods = await ops.workloadPods(widget.session.id, widget.resource);
          if (_current) {
            setState(() {
              _pods = pods;
              _podsFailed = false;
            });
          }
        } catch (_) {
          if (_current) setState(() => _podsFailed = true);
        }
      }
    }
    if (!_current) return;
    if (widget.showRelations && service is KubeRelationsService) {
      try {
        final related = await (service as KubeRelationsService).relatedObjects(widget.session.id, widget.resource);
        if (_current) {
          setState(() {
            _relations = (related['items'] as List? ?? const []).cast<KubeJson>();
            _relationsPartial = related['partial'] == true;
            _relationsFailed = false;
          });
        }
      } catch (_) {
        if (_current) setState(() => _relationsFailed = true);
      }
    }
    if (!_current) return;
    final isNode = widget.resource.api.group.isEmpty && widget.resource.api.resource == 'nodes';
    if (widget.showMetrics && service is KubeMetricsService && (widget.resource.api.isPod || isNode)) {
      try {
        final metrics = await (service as KubeMetricsService).metrics(
          widget.session.id,
          nodes: isNode,
          namespace: widget.resource.namespace,
        );
        if (_current) {
          setState(() {
            _metrics = metrics
                .where(
                  (metric) =>
                      metric.name == widget.resource.name && (isNode || metric.namespace == widget.resource.namespace),
                )
                .firstOrNull;
            _metricsFailed = _metrics == null;
            if (_metrics != null && (_samples.isEmpty || _samples.last.timestamp != _metrics!.timestamp)) {
              _samples.add(_metrics!);
              if (_samples.length > 60) _samples.removeAt(0);
            }
          });
        }
      } catch (_) {
        if (_current) setState(() => _metricsFailed = true);
      }
    }
    if (_current) setState(() => _loading = false);
  }

  @override
  Widget build(BuildContext context) {
    if (!_current) return const SizedBox.shrink();
    final l = context.l10n;
    final metrics = _metrics;
    Widget rows(List<KubeResourceRow> values, {required bool pods}) => SizedBox(
      height: values.isEmpty ? 40 : 220,
      child: values.isEmpty
          ? Text(l.kubeNoResources)
          : ListView.builder(
              itemCount: values.length,
              itemBuilder: (_, i) {
                final row = values[i];
                return ListTile(
                  title: Text(row.name),
                  subtitle: Text(
                    pods
                        ? '${row.namespace ?? ''} · ${row.status}'
                        : [
                            row.object['reason'],
                            row.object['message'],
                            row.object['lastTimestamp'],
                            row.object['eventTime'],
                          ].whereType<String>().join(' · '),
                  ),
                  onTap: pods ? () => widget.openPod(row) : null,
                );
              },
            ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (metrics != null) ...[
          Text(l.kubeMetrics, style: Theme.of(context).textTheme.titleSmall),
          Text(
            /* l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers. */ 'CPU: ${metrics.cpuCores.toStringAsFixed(3)} · RAM: ${formatBytes(l, metrics.memoryBytes)}',
          ), // l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers.
          if (metrics.timestamp != null) Text('${l.kubeMeasuredAt}: ${metrics.timestamp}'),
          if (_samples.length > 1)
            SizedBox(
              height: 60,
              child: Semantics(
                label: l.kubeMetricsTrend,
                child: CustomPaint(
                  painter: _MetricTrend(
                    _samples.map((m) => m.cpuCores).toList(),
                    Theme.of(context).colorScheme.primary,
                  ),
                ),
              ),
            ),
          if (_samples.length > 1) Text(l.kubeMetricsTrend),
          if (_samples.length > 1)
            SizedBox(
              height: 60,
              child: Semantics(
                label: l.kubeMemoryTrend,
                child: CustomPaint(
                  painter: _MetricTrend(
                    _samples.map((m) => m.memoryBytes.toDouble()).toList(),
                    Theme.of(context).colorScheme.tertiary,
                  ),
                ),
              ),
            ),
          if (_samples.length > 1) Text(l.kubeMemoryTrend),
          for (final budget in [
            (l.kubeRequests, metrics.requests),
            (l.kubeLimits, metrics.limits),
            (l.kubeCapacity, metrics.capacity),
            (l.kubeAllocatable, metrics.allocatable),
          ])
            if (budget.$2 != null)
              Text(
                // l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers.
                /* l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers. */ '${budget.$1}: CPU ${budget.$2!.cpuCores?.toStringAsFixed(3) ?? '—'} · RAM ${budget.$2!.memoryBytes == null ? '—' : formatBytes(l, budget.$2!.memoryBytes!)}',
              ),
          if (!metrics.resourcesAvailable) Text(l.kubeResourceBudgetsUnavailable),
          ...metrics.containers.map(
            (c) => Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  /* l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers. */ '${c.name}: CPU ${c.cpuCores.toStringAsFixed(3)} · RAM ${formatBytes(l, c.memoryBytes)}',
                ), // l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers.
                for (final budget in [(l.kubeRequests, c.requests), (l.kubeLimits, c.limits)])
                  if (budget.$2 != null)
                    Text(
                      // l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers.
                      /* l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers. */ '${budget.$1}: CPU ${budget.$2!.cpuCores?.toStringAsFixed(3) ?? '—'} · RAM ${budget.$2!.memoryBytes == null ? '—' : formatBytes(l, budget.$2!.memoryBytes!)}',
                    ),
              ],
            ),
          ),
        ],
        if (_metricsFailed) Text(l.kubeMetricsUnavailable),
        if (widget.showRelations && widget.service is KubeRelationsService)
          ExpansionTile(
            key: const ValueKey('kube-related-objects'),
            title: Text(l.kubeRelatedObjects),
            children: [
              if (_relationsFailed) Text(l.kubeRequestFailed),
              if (_relationsPartial) Text(l.kubeInsightsPartial),
              if (_relations != null)
                SizedBox(
                  height: _relations!.isEmpty ? 40 : 220,
                  child: _relations!.isEmpty
                      ? Text(l.kubeNoResources)
                      : ListView.builder(
                          itemCount: _relations!.length,
                          itemBuilder: (_, index) {
                            final row = _relations![index],
                                reference = KubeResourceRef.fromJson(_relations![index]['reference'] as KubeJson);
                            return ListTile(
                              title: Text('${reference.api.kind} · ${reference.name}'),
                              subtitle: Text(
                                // l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers.
                                /* l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers. */ '${_relationLabel(l, row['relation'] as String? ?? '')} · ${reference.namespace ?? ''} · ${row['status'] ?? ''}', // l10n-ignore: CPU/RAM technical units or localized relation labels with Kubernetes identifiers.
                              ),
                              onTap: () => widget.openResource(reference),
                            );
                          },
                        ),
                ),
            ],
          ),
        if (widget.showRelations && _workload && widget.service is KubeOperationsService)
          ExpansionTile(
            title: Text(l.kubeWorkloadPods),
            children: [if (_podsFailed) Text(l.kubeRequestFailed), if (_pods != null) rows(_pods!, pods: true)],
          ),
        if (widget.showEvents && widget.service is KubeOperationsService)
          ExpansionTile(
            initiallyExpanded: !widget.showRelations,
            title: Text(l.kubeEvents),
            children: [if (_eventsFailed) Text(l.kubeRequestFailed), if (_events != null) rows(_events!, pods: false)],
          ),
        if (_loading) const LinearProgressIndicator(),
      ],
    );
  }
}

String _relationLabel(AppLocalizations l, String relation) => switch (relation) {
  'owner' => l.kubeRelationOwner,
  'dependent' => l.kubeRelationDependent,
  'selected_pod' => l.kubeWorkloadPods,
  'service' => 'Service',
  'ingress' => 'Ingress',
  'volume' || 'uses_volume' => l.kubeRelationVolume,
  'configuration' || 'uses_configuration' => l.kubeRelationConfiguration,
  _ => l.kubeRelatedObjects,
};

class _MetricTrend extends CustomPainter {
  _MetricTrend(this.values, this.color);
  final List<double> values;
  final Color color;
  @override
  void paint(Canvas canvas, Size size) {
    if (values.length < 2) return;
    final maximum = values.fold<double>(0, (a, b) => a > b ? a : b);
    if (maximum <= 0) return;
    final path = Path();
    for (var i = 0; i < values.length; i++) {
      final x = i * size.width / (values.length - 1), y = size.height - (values[i] / maximum) * (size.height - 4) - 2;
      if (i == 0) {
        path.moveTo(x, y);
      } else {
        path.lineTo(x, y);
      }
    }
    canvas.drawPath(
      path,
      Paint()
        ..color = color
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2,
    );
  }

  @override
  bool shouldRepaint(covariant _MetricTrend old) => values != old.values || color != old.color;
}
