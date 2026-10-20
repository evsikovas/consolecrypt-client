import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:material_ui/material_ui.dart';

/// Bounded native inventory graph. The list alternative exposes every edge to
/// keyboard and screen-reader users without requiring canvas navigation.
class KubeGitOpsGraph extends StatefulWidget {
  const KubeGitOpsGraph({required this.flow, required this.open, required this.canOpen, super.key});
  final KubeGitOpsFlow flow;
  final ValueChanged<KubeObjectIdentity> open;
  final bool Function(KubeObjectIdentity) canOpen;
  @override
  State<KubeGitOpsGraph> createState() => _KubeGitOpsGraphState();
}

class _KubeGitOpsGraphState extends State<KubeGitOpsGraph> {
  final _transform = TransformationController();
  bool? _list;
  bool _problems = false, _fold = false;
  String _search = '';
  Size _viewport = Size.zero, _graph = Size.zero;

  void _fit() {
    if (_graph.isEmpty || _viewport.isEmpty) return;
    final scale = math.min(_viewport.width / _graph.width, _viewport.height / _graph.height).clamp(.2, 1.0);
    _transform.value = Matrix4.identity()..scaleByDouble(scale, scale, 1, 1);
  }

  static String _key(KubeObjectIdentity o) => '${o.kind}/${o.namespace}/${o.name}';
  @override
  void dispose() {
    _transform.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final flow = widget.flow;
    final list = _list ?? MediaQuery.sizeOf(context).width < 700;
    final visible = flow.nodes
        .where((node) => !_problems || node.ready == false)
        .map((node) => _key(node.object))
        .toSet();
    if (_problems) {
      // Expand ancestors to preserve the path from source to failed workload.
      for (var i = 0; i < flow.nodes.length; i++) {
        var changed = false;
        for (final edge in flow.edges) {
          if (visible.contains(_key(edge.to))) changed = visible.add(_key(edge.from)) || changed;
        }
        if (!changed) break;
      }
    }
    final parents = flow.edges.map((edge) => _key(edge.from)).toSet();
    final nodes = flow.nodes
        .where(
          (node) =>
              visible.contains(_key(node.object)) &&
              (!_fold || parents.contains(_key(node.object)) || node.ready == false) &&
              (_search.isEmpty || node.object.label.toLowerCase().contains(_search)),
        )
        .take(200)
        .toList();
    final keys = nodes.map((node) => _key(node.object)).toSet();
    final edges = flow.edges.where((edge) => keys.contains(_key(edge.from)) && keys.contains(_key(edge.to))).toList();
    final positions = <String, Offset>{};
    final levels = {for (final node in nodes) _key(node.object): 0};
    // Cycles remain bounded in the final column and are still represented by
    // their edges; cycle membership comes from the native dependency report.
    for (var iteration = 0; iteration < 8; iteration++) {
      var changed = false;
      for (final edge in edges) {
        final from = levels[_key(edge.from)], to = levels[_key(edge.to)];
        if (from == null || to == null || to > from || to >= 8) continue;
        levels[_key(edge.to)] = from + 1;
        changed = true;
      }
      if (!changed) break;
    }
    final counts = <int, int>{};
    for (final node in nodes) {
      final column = (levels[_key(node.object)] ?? 0).clamp(0, 8);
      final row = counts[column] ?? 0;
      counts[column] = row + 1;
      positions[_key(node.object)] = Offset(column * 260.0 + 16, row * 124.0 + 16);
    }
    final width = math.max(240.0, (counts.keys.fold(0, math.max) + 1) * 260.0);
    final height = math.max(120.0, counts.values.fold(0, math.max) * 124.0 + 16);
    _graph = Size(width, height);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(child: Text(l.kubeGitOpsFlow)),
            FilterChip(
              label: Text(l.kubeGraphList),
              selected: list,
              onSelected: (value) => setState(() => _list = value),
            ),
            if (!list) GlassIconButton(icon: Icons.center_focus_strong, tooltip: l.kubeGraphReset, onPressed: _fit),
          ],
        ),
        TextField(
          key: const ValueKey('kube-graph-search'),
          decoration: InputDecoration(labelText: l.kubeGraphSearch, prefixIcon: const Icon(Icons.search)),
          onChanged: (value) => setState(() => _search = value.trim().toLowerCase()),
        ),
        SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Row(
            children: [
              FilterChip(
                label: Text(l.kubeGraphProblems),
                selected: _problems,
                onSelected: (value) => setState(() => _problems = value),
              ),
              const SizedBox(width: 8),
              FilterChip(
                label: Text(l.kubeGraphFold),
                selected: _fold,
                onSelected: (value) => setState(() => _fold = value),
              ),
            ],
          ),
        ),
        if (flow.truncated || flow.nodes.length > 200) Text(l.kubeInsightsPartial),
        Expanded(
          child: nodes.isEmpty
              ? Center(child: Text(l.kubeNoResources))
              : list
              ? ListView.builder(
                  itemCount: edges.length + nodes.length,
                  itemBuilder: (_, index) {
                    if (index < edges.length) {
                      final edge = edges[index];
                      return ListTile(
                        leading: const Icon(Icons.account_tree_outlined),
                        title: Text(edge.from.label),
                        subtitle: Text('${edge.relation} → ${edge.to.label}'),
                        onTap: widget.canOpen(edge.to) ? () => widget.open(edge.to) : null,
                      );
                    }
                    final node = nodes[index - edges.length];
                    return ListTile(
                      title: Text(node.object.label),
                      leading: Icon(
                        node.ready == true
                            ? Icons.check_circle_outline
                            : node.ready == false
                            ? Icons.error_outline
                            : Icons.circle_outlined,
                      ),
                      onTap: widget.canOpen(node.object) ? () => widget.open(node.object) : null,
                    );
                  },
                )
              : LayoutBuilder(
                  builder: (context, bounds) {
                    _viewport = Size(bounds.maxWidth, bounds.maxHeight);
                    return InteractiveViewer(
                      key: const ValueKey('kube-gitops-flow'),
                      transformationController: _transform,
                      constrained: false,
                      boundaryMargin: const EdgeInsets.all(40),
                      minScale: .2,
                      maxScale: 2,
                      child: SizedBox(
                        width: width,
                        height: height,
                        child: Stack(
                          children: [
                            Positioned.fill(
                              child: CustomPaint(
                                painter: _Edges(edges, positions, Theme.of(context).colorScheme.outline),
                              ),
                            ),
                            for (final node in nodes)
                              Positioned(
                                left: positions[_key(node.object)]!.dx,
                                top: positions[_key(node.object)]!.dy,
                                width: 210,
                                height: 96,
                                child: ContentSurface(
                                  padding: const EdgeInsets.all(6),
                                  child: InkWell(
                                    onTap: widget.canOpen(node.object) ? () => widget.open(node.object) : null,
                                    child: Tooltip(
                                      message: node.object.label,
                                      child: Row(
                                        children: [
                                          Icon(
                                            node.ready == true
                                                ? Icons.check_circle_outline
                                                : node.ready == false
                                                ? Icons.error_outline
                                                : Icons.circle_outlined,
                                            size: 20,
                                          ),
                                          const SizedBox(width: 6),
                                          Expanded(
                                            child: Column(
                                              mainAxisAlignment: MainAxisAlignment.center,
                                              crossAxisAlignment: CrossAxisAlignment.start,
                                              children: [
                                                Text(node.object.kind, maxLines: 1, overflow: TextOverflow.ellipsis),
                                                Text(
                                                  node.object.name,
                                                  maxLines: 2,
                                                  overflow: TextOverflow.ellipsis,
                                                  style: Theme.of(context).textTheme.titleSmall,
                                                ),
                                                Text(
                                                  node.object.namespace,
                                                  maxLines: 1,
                                                  overflow: TextOverflow.ellipsis,
                                                ),
                                              ],
                                            ),
                                          ),
                                        ],
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                          ],
                        ),
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }
}

class _Edges extends CustomPainter {
  _Edges(this.edges, this.positions, this.color);
  final List<KubeFlowEdge> edges;
  final Map<String, Offset> positions;
  final Color color;
  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = color
      ..strokeWidth = 1.5
      ..style = PaintingStyle.stroke;
    for (final edge in edges) {
      final from = positions[_KubeGitOpsGraphState._key(edge.from)],
          to = positions[_KubeGitOpsGraphState._key(edge.to)];
      if (from == null || to == null) continue;
      final start = from + const Offset(210, 48), end = to + const Offset(0, 48);
      final bend = math.max(24.0, (end.dx - start.dx).abs() / 2);
      canvas.drawPath(
        Path()
          ..moveTo(start.dx, start.dy)
          ..cubicTo(start.dx + bend, start.dy, end.dx - bend, end.dy, end.dx, end.dy),
        paint,
      );
      canvas.drawPath(
        Path()
          ..moveTo(end.dx - 6, end.dy - 4)
          ..lineTo(end.dx, end.dy)
          ..lineTo(end.dx - 6, end.dy + 4),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _Edges old) => edges != old.edges || positions != old.positions || color != old.color;
}
