import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_metrics_installer.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_resource_table_view.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// First screen of a connected cluster: live node CPU/memory (metrics-server,
/// every 10 s with a short in-memory history), Pod phases, node readiness and
/// problem counts. Nothing is persisted; samples are dropped with the session.
class KubeClusterOverview extends ConsumerStatefulWidget {
  const KubeClusterOverview({
    required this.session,
    required this.service,
    required this.kinds,
    required this.namespaceCount,
    required this.onOpenKind,
    required this.onOpenProblems,
    super.key,
  });

  final KubeSession session;
  final KubeBrowserService service;
  final List<KubeResourceKind> kinds;
  final int namespaceCount;
  final ValueChanged<KubeResourceKind> onOpenKind;
  final VoidCallback onOpenProblems;

  @override
  ConsumerState<KubeClusterOverview> createState() => _KubeClusterOverviewState();
}

typedef _Sample = ({double cpu, double memory});

class _KubeClusterOverviewState extends ConsumerState<KubeClusterOverview> {
  static const _history = 30;
  late final KubeScope _scope;
  Timer? _timer;
  int _tick = 0;
  bool _loading = true, _metricsUnavailable = false;
  bool _insecureTls = false, _installing = false, _installFailed = false;
  String? _installMessage;
  List<KubeMetrics> _nodes = const [];
  List<KubeResourceRow> _nodeRows = const [];
  final _pods = <String, int>{};
  int _podTotal = 0;
  bool _podsLimited = false;
  int? _errors, _warnings;
  final _samples = <_Sample>[];

  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  KubeResourceKind? _kind(String kind) =>
      widget.kinds.where((k) => k.group.isEmpty && k.kind == kind && k.canList).firstOrNull;

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    unawaited(_load(full: true));
    _timer = Timer.periodic(const Duration(seconds: 10), (_) => unawaited(_load(full: ++_tick % 3 == 0)));
  }

  @override
  void dispose() {
    _timer?.cancel();
    _samples.clear();
    super.dispose();
  }

  Future<void> _installMetrics() async {
    final service = widget.service;
    if (!_current || _installing || service is! KubeManifestService) return;
    final l = context.l10n;
    final manifest = await MetricsServerManifest.load();
    if (!mounted || !_current) return;
    final approved = await showConfirmDialog(
      context,
      title: l.kubeMetricsInstall,
      message: l.kubeMetricsInstallHelp(manifest.version),
      confirmLabel: l.kubeMetricsInstall,
    );
    if (!approved || !mounted || !_current) return;
    setState(() {
      _installing = true;
      _installMessage = null;
    });
    final result = await installMetricsServer(
      l10n: l,
      service: service as KubeManifestService,
      session: widget.session,
      kinds: widget.kinds,
      objects: manifest.build(insecureTls: _insecureTls),
    );
    if (!mounted || !_current) return;
    setState(() {
      _installing = false;
      _installFailed = result.error != null;
      _installMessage = result.error ?? l.kubeMetricsInstalled(result.created, result.existing);
    });
  }

  Future<void> _load({required bool full}) async {
    if (!_current) return;
    final service = widget.service;
    final session = widget.session.id;
    try {
      if (service is KubeMetricsService) {
        final nodes = await (service as KubeMetricsService).metrics(session, nodes: true);
        if (!_current) return;
        final cpu = nodes.fold<double>(0, (sum, n) => sum + n.cpuCores);
        final cpuMax = nodes.fold<double>(0, (sum, n) => sum + (n.allocatable?.cpuCores ?? n.capacity?.cpuCores ?? 0));
        final memory = nodes.fold<double>(0, (sum, n) => sum + n.memoryBytes);
        final memoryMax = nodes.fold<double>(
          0,
          (sum, n) => sum + (n.allocatable?.memoryBytes ?? n.capacity?.memoryBytes ?? 0),
        );
        setState(() {
          _nodes = nodes;
          _metricsUnavailable = nodes.isEmpty;
          if (cpuMax > 0 && memoryMax > 0) {
            _samples.add((cpu: cpu / cpuMax, memory: memory / memoryMax));
            if (_samples.length > _history) _samples.removeAt(0);
          }
        });
      } else {
        _metricsUnavailable = true;
      }
    } catch (_) {
      if (_current) setState(() => _metricsUnavailable = true);
    }
    if (!full || !_current) {
      if (_current) setState(() => _loading = false);
      return;
    }
    try {
      final nodeKind = _kind('Node');
      if (nodeKind != null) {
        final page = await service.listResources(session, nodeKind, limit: 500);
        if (_current) setState(() => _nodeRows = page.items);
      }
      final podKind = _kind('Pod');
      if (podKind != null) {
        final phases = <String, int>{};
        var total = 0;
        String? token;
        var pages = 0;
        do {
          // The Table API carries the printed Pod status (Running, Completed,
          // CrashLoopBackOff); the plain list only has readiness.
          final table = service is KubeTableService
              ? await (service as KubeTableService).table(session, podKind, continueToken: token, limit: 500)
              : null;
          final page = table?.page ?? await service.listResources(session, podKind, continueToken: token, limit: 500);
          if (!_current) return;
          final statusColumn = table?.columns.indexWhere((column) => column.name.toLowerCase() == 'status') ?? -1;
          for (final pod in page.items) {
            total++;
            final printed = statusColumn >= 0 && statusColumn < pod.cells.length ? pod.cells[statusColumn] : '';
            phases.update(_phase(printed.isNotEmpty ? printed : pod.status), (n) => n + 1, ifAbsent: () => 1);
          }
          token = page.continueToken?.isEmpty == true ? null : page.continueToken;
        } while (token != null && ++pages < 10);
        setState(() {
          _pods
            ..clear()
            ..addAll(phases);
          _podTotal = total;
          _podsLimited = token != null;
        });
      }
      if (service is KubeInsightsService) {
        final insights = await (service as KubeInsightsService).insights(session);
        if (_current) {
          setState(() {
            _errors = insights.issues.where((i) => i.severity == 'error').length;
            _warnings = insights.issues.length - _errors!;
          });
        }
      }
    } catch (_) {
      /* Counts stay at their last value; metrics continue independently. */
    } finally {
      if (_current) setState(() => _loading = false);
    }
  }

  static String _phase(String status) {
    final value = status.toLowerCase();
    if (value == 'running' || value == 'ready') return 'Running';
    if (value == 'succeeded' || value == 'completed') return 'Succeeded';
    if (value == 'pending' || value.contains('creating') || value.contains('init') || value == 'notready') {
      return 'Pending';
    }
    if (value.contains('backoff') || value.contains('error') || value == 'failed' || value.contains('evicted')) {
      return 'Failed';
    }
    return 'Other';
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final readyNodes = _nodeRows.where((n) => n.status.toLowerCase() == 'ready').length;
    final latest = _samples.lastOrNull;
    final nodeKind = _kind('Node'), podKind = _kind('Pod'), namespaceKind = _kind('Namespace');
    final tiles = [
      _Tile(
        key: const ValueKey('kube-overview-nodes'),
        icon: Icons.dns_outlined,
        label: l.kubeOverviewNodes,
        value: _nodeRows.isEmpty ? '—' : '$readyNodes/${_nodeRows.length}',
        tone: _nodeRows.isNotEmpty && readyNodes < _nodeRows.length ? GlassTone.warning : GlassTone.success,
        onTap: nodeKind == null ? null : () => widget.onOpenKind(nodeKind),
      ),
      _Tile(
        key: const ValueKey('kube-overview-pods'),
        icon: Icons.view_in_ar_outlined,
        label: l.kubeOverviewPods,
        value: _podTotal == 0 ? '—' : '${_pods['Running'] ?? 0}/$_podTotal${_podsLimited ? '+' : ''}',
        tone: (_pods['Failed'] ?? 0) > 0 ? GlassTone.danger : GlassTone.success,
        onTap: podKind == null ? null : () => widget.onOpenKind(podKind),
      ),
      _Tile(
        key: const ValueKey('kube-overview-namespaces'),
        icon: Icons.layers_outlined,
        label: l.kubeOverviewNamespaces,
        value: '${widget.namespaceCount}',
        tone: GlassTone.neutral,
        onTap: namespaceKind == null ? null : () => widget.onOpenKind(namespaceKind),
      ),
      _Tile(
        key: const ValueKey('kube-overview-problems'),
        icon: Icons.warning_amber_rounded,
        label: l.kubeProblems,
        value: _errors == null ? '—' : '${_errors! + (_warnings ?? 0)}',
        tone: (_errors ?? 0) > 0
            ? GlassTone.danger
            : (_warnings ?? 0) > 0
            ? GlassTone.warning
            : GlassTone.success,
        onTap: widget.onOpenProblems,
      ),
    ];
    return ListView(
      key: const ValueKey('kube-cluster-overview'),
      children: [
        if (_loading) const LinearProgressIndicator(minHeight: 2),
        LayoutBuilder(
          builder: (context, constraints) {
            final columns = constraints.maxWidth >= 900 ? 4 : 2;
            final width = (constraints.maxWidth - (columns - 1) * GlassSpacing.s12) / columns;
            return Wrap(
              spacing: GlassSpacing.s12,
              runSpacing: GlassSpacing.s12,
              children: [for (final tile in tiles) SizedBox(width: width, child: tile)],
            );
          },
        ),
        const SizedBox(height: GlassSpacing.s12),
        if (_metricsUnavailable)
          ContentSurface(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(l.kubeMetricsUnavailable, style: tokens.typography.body.copyWith(color: tokens.secondaryLabel)),
                if (widget.service is KubeManifestService) ...[
                  const SizedBox(height: GlassSpacing.s12),
                  Row(
                    children: [
                      Switch.adaptive(
                        key: const ValueKey('kube-metrics-insecure-tls'),
                        value: _insecureTls,
                        onChanged: _installing ? null : (value) => setState(() => _insecureTls = value),
                      ),
                      const SizedBox(width: GlassSpacing.s8),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              l.kubeMetricsInsecureTls,
                              style: tokens.typography.body.copyWith(color: tokens.palette.label),
                            ),
                            Text(
                              l.kubeMetricsInsecureTlsHelp,
                              style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: GlassSpacing.s12),
                  GlassButton.prominent(
                    key: const ValueKey('kube-metrics-install'),
                    icon: Icons.download_rounded,
                    label: l.kubeMetricsInstall,
                    busy: _installing,
                    onPressed: _installing ? null : () => unawaited(_installMetrics()),
                  ),
                  if (_installMessage case final message?) ...[
                    const SizedBox(height: GlassSpacing.s8),
                    Text(
                      message,
                      key: const ValueKey('kube-metrics-install-result'),
                      style: tokens.typography.callout.copyWith(
                        color: _installFailed ? tokens.palette.danger : tokens.palette.success,
                      ),
                    ),
                  ],
                ],
              ],
            ),
          )
        else
          LayoutBuilder(
            builder: (context, constraints) {
              final wide = constraints.maxWidth >= 760;
              final cards = [
                _UsageCard(
                  key: const ValueKey('kube-overview-cpu'),
                  title: l.kubeOverviewCpu,
                  value: latest?.cpu,
                  detail: _cpuDetail(),
                  samples: [for (final s in _samples) s.cpu],
                  color: p.accent,
                ),
                _UsageCard(
                  key: const ValueKey('kube-overview-memory'),
                  title: l.kubeOverviewMemory,
                  value: latest?.memory,
                  detail: _memoryDetail(),
                  samples: [for (final s in _samples) s.memory],
                  color: p.success,
                ),
              ];
              return wide
                  ? Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Expanded(child: cards[0]),
                        const SizedBox(width: GlassSpacing.s12),
                        Expanded(child: cards[1]),
                      ],
                    )
                  : Column(
                      children: [
                        cards[0],
                        const SizedBox(height: GlassSpacing.s12),
                        cards[1],
                      ],
                    );
            },
          ),
        if (_podTotal > 0) ...[
          const SizedBox(height: GlassSpacing.s12),
          ContentSurface(
            key: const ValueKey('kube-overview-phases'),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.kubeOverviewPodPhases, style: tokens.typography.title3.copyWith(color: p.label)),
                const SizedBox(height: GlassSpacing.s12),
                _PhaseBar(counts: _pods, total: _podTotal),
              ],
            ),
          ),
        ],
        if (_nodes.isNotEmpty) ...[
          const SizedBox(height: GlassSpacing.s12),
          ContentSurface(
            key: const ValueKey('kube-overview-node-table'),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.kubeOverviewNodes, style: tokens.typography.title3.copyWith(color: p.label)),
                const SizedBox(height: GlassSpacing.s8),
                for (final node in _nodes)
                  _NodeRow(metrics: node, row: _nodeRows.where((r) => r.name == node.name).firstOrNull),
              ],
            ),
          ),
        ],
      ],
    );
  }

  String _cpuDetail() {
    final used = _nodes.fold<double>(0, (sum, n) => sum + n.cpuCores);
    final max = _nodes.fold<double>(0, (sum, n) => sum + (n.allocatable?.cpuCores ?? n.capacity?.cpuCores ?? 0));
    return '${used.toStringAsFixed(2)} / ${max.toStringAsFixed(max >= 10 ? 0 : 1)} CPU'; // l10n-ignore: Kubernetes unit.
  }

  String _memoryDetail() {
    final used = _nodes.fold<double>(0, (sum, n) => sum + n.memoryBytes);
    final max = _nodes.fold<double>(0, (sum, n) => sum + (n.allocatable?.memoryBytes ?? n.capacity?.memoryBytes ?? 0));
    return '${_gib(used)} / ${_gib(max)} GiB'; // l10n-ignore: binary unit.
  }
}

String _gib(num bytes) => (bytes / (1024 * 1024 * 1024)).toStringAsFixed(1);

class _Tile extends StatelessWidget {
  const _Tile({
    required this.icon,
    required this.label,
    required this.value,
    required this.tone,
    required this.onTap,
    super.key,
  });
  final IconData icon;
  final String label, value;
  final GlassTone tone;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final color = tone == GlassTone.neutral ? p.accent : p.tone(tone);
    return GlassInteractive(
      semanticLabel: '$label $value',
      onPressed: onTap,
      builder: (context, state) => ContentSurface(
        color: state.hovered ? Color.alphaBlend(tokens.surfaces.fillHover, tokens.surfaces.content) : null,
        padding: const EdgeInsets.all(GlassSpacing.s16),
        child: Row(
          children: [
            Container(
              width: 38,
              height: 38,
              decoration: BoxDecoration(color: color.withValues(alpha: 0.15), borderRadius: BorderRadius.circular(10)),
              child: Icon(icon, size: 20, color: color),
            ),
            const SizedBox(width: GlassSpacing.s12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                  ),
                  const SizedBox(height: 2),
                  Text(value, style: tokens.typography.title2.copyWith(color: p.label)),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _UsageCard extends StatelessWidget {
  const _UsageCard({
    required this.title,
    required this.value,
    required this.detail,
    required this.samples,
    required this.color,
    super.key,
  });
  final String title, detail;
  final double? value;
  final List<double> samples;
  final Color color;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final percent = value == null ? '—' : '${(value! * 100).round()}%';
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Expanded(
                child: Text(title, style: tokens.typography.title3.copyWith(color: p.label)),
              ),
              Text(percent, style: tokens.typography.title1.copyWith(color: color)),
            ],
          ),
          const SizedBox(height: 2),
          Text(detail, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
          const SizedBox(height: GlassSpacing.s12),
          SizedBox(
            height: 90,
            child: CustomPaint(
              painter: _SparklinePainter(samples: samples, color: color, grid: tokens.surfaces.separator),
              size: Size.infinite,
            ),
          ),
        ],
      ),
    );
  }
}

class _SparklinePainter extends CustomPainter {
  const _SparklinePainter({required this.samples, required this.color, required this.grid});
  final List<double> samples;
  final Color color, grid;

  @override
  void paint(Canvas canvas, Size size) {
    final gridPaint = Paint()
      ..color = grid
      ..strokeWidth = 1;
    for (final level in [0.25, 0.5, 0.75]) {
      final y = size.height * (1 - level);
      canvas.drawLine(Offset(0, y), Offset(size.width, y), gridPaint);
    }
    if (samples.isEmpty) return;
    final points = <Offset>[];
    final step = samples.length == 1 ? 0.0 : size.width / (math.max(samples.length, 2) - 1);
    for (final (index, sample) in samples.indexed) {
      points.add(Offset(samples.length == 1 ? size.width : index * step, size.height * (1 - sample.clamp(0, 1))));
    }
    final line = Path()..moveTo(points.first.dx, points.first.dy);
    for (final point in points.skip(1)) {
      line.lineTo(point.dx, point.dy);
    }
    final area = Path.from(line)
      ..lineTo(points.last.dx, size.height)
      ..lineTo(points.first.dx, size.height)
      ..close();
    canvas.drawPath(
      area,
      Paint()
        ..shader = LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [color.withValues(alpha: 0.35), color.withValues(alpha: 0.02)],
        ).createShader(Offset.zero & size),
    );
    canvas.drawPath(
      line,
      Paint()
        ..color = color
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..strokeJoin = StrokeJoin.round,
    );
    canvas.drawCircle(points.last, 3.5, Paint()..color = color);
  }

  @override
  bool shouldRepaint(_SparklinePainter old) => old.samples != samples || old.color != color;
}

class _PhaseBar extends StatelessWidget {
  const _PhaseBar({required this.counts, required this.total});
  final Map<String, int> counts;
  final int total;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final phases = [
      ('Running', p.success),
      ('Succeeded', p.tertiary),
      ('Pending', p.warning),
      ('Failed', p.danger),
      ('Other', p.secondary),
    ].where((phase) => (counts[phase.$1] ?? 0) > 0).toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ClipRRect(
          borderRadius: BorderRadius.circular(5),
          child: SizedBox(
            height: 10,
            child: Row(
              children: [
                for (final phase in phases)
                  Expanded(
                    flex: counts[phase.$1]!,
                    child: SizedBox.expand(child: ColoredBox(color: phase.$2)),
                  ),
              ],
            ),
          ),
        ),
        const SizedBox(height: GlassSpacing.s8),
        Wrap(
          spacing: GlassSpacing.s16,
          runSpacing: GlassSpacing.s6,
          children: [
            for (final phase in phases)
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Container(
                    width: 8,
                    height: 8,
                    decoration: BoxDecoration(color: phase.$2, shape: BoxShape.circle),
                  ),
                  const SizedBox(width: 6),
                  Text(
                    '${phase.$1} ${counts[phase.$1]}', // l10n-ignore: Kubernetes Pod phase and count.
                    style: tokens.typography.callout.copyWith(color: p.label),
                  ),
                ],
              ),
          ],
        ),
      ],
    );
  }
}

class _NodeRow extends StatelessWidget {
  const _NodeRow({required this.metrics, required this.row});
  final KubeMetrics metrics;
  final KubeResourceRow? row;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final cpuMax = metrics.allocatable?.cpuCores ?? metrics.capacity?.cpuCores;
    final memMax = metrics.allocatable?.memoryBytes ?? metrics.capacity?.memoryBytes;
    Widget meter(double? ratio, String text, Color color) => Expanded(
      child: Row(
        children: [
          Expanded(
            child: ClipRRect(
              borderRadius: BorderRadius.circular(3),
              child: LinearProgressIndicator(
                value: ratio?.clamp(0, 1),
                minHeight: 6,
                color: color,
                backgroundColor: tokens.surfaces.fillField,
              ),
            ),
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 120,
            child: Text(text, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
          ),
        ],
      ),
    );
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        children: [
          SizedBox(
            width: 200,
            child: Row(
              children: [
                if (row != null) ...[
                  Container(
                    width: 8,
                    height: 8,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: KubeStatusLabel.tone(row!.status) == GlassTone.success ? p.success : p.danger,
                    ),
                  ),
                  const SizedBox(width: 10),
                ],
                Flexible(
                  child: Text(
                    metrics.name,
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.body.copyWith(color: p.label, fontWeight: FontWeight.w500),
                  ),
                ),
              ],
            ),
          ),
          meter(
            cpuMax == null || cpuMax == 0 ? null : metrics.cpuCores / cpuMax,
            '${metrics.cpuCores.toStringAsFixed(2)}${cpuMax == null ? '' : ' / ${cpuMax.toStringAsFixed(0)}'} CPU', // l10n-ignore: Kubernetes unit.
            p.accent,
          ),
          const SizedBox(width: 16),
          meter(
            memMax == null || memMax == 0 ? null : metrics.memoryBytes / memMax,
            '${_gib(metrics.memoryBytes)}${memMax == null ? '' : ' / ${_gib(memMax)}'} GiB', // l10n-ignore: binary unit.
            p.success,
          ),
        ],
      ),
    );
  }
}
