import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:material_ui/material_ui.dart';

/// Bounded native inventory graph. The list alternative exposes every edge to
/// keyboard and screen-reader users without requiring canvas navigation.
class KubeGitOpsGraph extends StatefulWidget {
  const KubeGitOpsGraph({
    required this.flow,
    required this.open,
    required this.canOpen,
    this.resources = const [],
    this.dependencyLayout = false,
    this.statusLabel,
    super.key,
  });
  final KubeGitOpsFlow flow;
  final ValueChanged<KubeObjectIdentity> open;
  final bool Function(KubeObjectIdentity) canOpen;
  final List<KubeInsightResource> resources;
  final bool dependencyLayout;
  final String? Function(KubeObjectIdentity)? statusLabel;
  @override
  State<KubeGitOpsGraph> createState() => _KubeGitOpsGraphState();
}

class _KubeGitOpsGraphState extends State<KubeGitOpsGraph> {
  final _transform = TransformationController();
  bool? _list;
  bool _problems = false, _fold = false;
  String _search = '';
  final _collapsed = <String>{};
  String? _namespace, _label;
  Map<String, KubeInsightResource> _metadata = {};
  Size _viewport = Size.zero, _graph = Size.zero;

  void _fit() {
    if (_graph.isEmpty || _viewport.isEmpty) return;
    final scale = math.min(_viewport.width / _graph.width, _viewport.height / _graph.height).clamp(.2, 1.0);
    _transform.value = Matrix4.identity()..scaleByDouble(scale, scale, 1, 1);
  }

  static String _key(KubeObjectIdentity o) =>
      '${o.group.isEmpty ? '' : '${o.group}/'}${o.kind}/${o.namespace}/${o.name}';
  Map<String, dynamic> _labels(KubeFlowNode node) =>
      (_metadata[_key(node.object)]?.object['metadata'] as Map?)?['labels'] as Map<String, dynamic>? ?? const {};

  ({IconData icon, Color color}) _appearance(KubeFlowNode node) {
    final resource = _metadata[_key(node.object)];
    final palette = GlassTokens.of(context).palette;
    final conditions = ((resource?.object['status'] as Map?)?['conditions'] as List? ?? const []);
    bool condition(String type) =>
        conditions.whereType<Map<String, dynamic>>().any((c) => c['type'] == type && c['status'] == 'True');
    if (resource?.suspended == true) {
      return (icon: Icons.pause_circle_outline, color: GlassTokens.of(context).secondaryLabel);
    }
    if (condition('Stalled')) return (icon: Icons.error_outline, color: palette.tone(GlassTone.danger));
    if (condition('Reconciling')) return (icon: Icons.sync, color: palette.tone(GlassTone.warning));
    return (
      icon: node.ready == true
          ? Icons.check_circle_outline
          : node.ready == false
          ? Icons.error_outline
          : Icons.circle_outlined,
      color: node.ready == true
          ? palette.tone(GlassTone.success)
          : node.ready == false
          ? palette.tone(GlassTone.danger)
          : GlassTokens.of(context).secondaryLabel,
    );
  }

  /// Columns by longest path from the roots. Cycles are common in Flux (the
  /// bootstrap Kustomization manages the GitRepository it is built from, and
  /// itself), so back edges found by a depth-first walk that starts at
  /// sources, then Kustomizations, then releases are ignored for the layout
  /// (they are still drawn). Unused columns are dropped.
  static Map<String, int> _levels(List<KubeFlowNode> nodes, List<KubeFlowEdge> edges) {
    int rank(KubeObjectIdentity o) => switch (o.kind) {
      'GitRepository' || 'OCIRepository' || 'HelmRepository' || 'Bucket' || 'ExternalArtifact' => 0,
      'Kustomization' || 'Application' || 'ApplicationSet' => 1,
      'HelmRelease' || 'HelmChart' => 2,
      _ => 3,
    };
    final outgoing = <String, List<String>>{};
    final indegree = <String, int>{};
    for (final edge in edges) {
      final from = _key(edge.from), to = _key(edge.to);
      if (from == to) continue;
      outgoing.putIfAbsent(from, () => []).add(to);
      indegree[to] = (indegree[to] ?? 0) + 1;
    }
    final order = [...nodes]
      ..sort((a, b) {
        final roots = ((indegree[_key(a.object)] ?? 0) == 0 ? 0 : 1) - ((indegree[_key(b.object)] ?? 0) == 0 ? 0 : 1);
        return roots != 0 ? roots : rank(a.object) - rank(b.object);
      });
    final dag = <String, List<String>>{};
    final state = <String, int>{}; // 1 on stack, 2 done
    void visit(String key) {
      state[key] = 1;
      for (final to in outgoing[key] ?? const <String>[]) {
        if (state[to] == 1) continue; // back edge closes a cycle
        dag.putIfAbsent(key, () => []).add(to);
        if (state[to] == null) visit(to);
      }
      state[key] = 2;
    }

    for (final node in order) {
      if (state[_key(node.object)] == null) visit(_key(node.object));
    }
    final incoming = {for (final node in nodes) _key(node.object): 0};
    for (final targets in dag.values) {
      for (final to in targets) {
        incoming[to] = (incoming[to] ?? 0) + 1;
      }
    }
    final levels = {for (final key in incoming.keys) key: 0};
    final queue = [
      for (final entry in incoming.entries)
        if (entry.value == 0) entry.key,
    ];
    for (var i = 0; i < queue.length; i++) {
      final from = queue[i];
      for (final to in dag[from] ?? const <String>[]) {
        levels[to] = math.max(levels[to] ?? 0, math.min(levels[from]! + 1, 8));
        incoming[to] = incoming[to]! - 1;
        if (incoming[to] == 0) queue.add(to);
      }
    }
    final used = levels.values.toSet().toList()..sort();
    final column = {for (final (i, level) in used.indexed) level: i};
    return {for (final entry in levels.entries) entry.key: column[entry.value]!};
  }

  @override
  void initState() {
    super.initState();
    // Large inventories are mostly leaves; start folded so sources and
    // controllers are visible first.
    _fold = widget.flow.nodes.length > 40;
  }

  @override
  void dispose() {
    _transform.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final flow = widget.flow;
    _metadata = {
      for (final resource in widget.resources)
        _key(
          KubeObjectIdentity(
            group: resource.api.group,
            kind: resource.api.kind,
            namespace: resource.resource.namespace ?? '',
            name: resource.resource.name,
          ),
        ): resource,
    };
    final namespaces = flow.nodes.map((n) => n.object.namespace).where((n) => n.isNotEmpty).toSet().toList()..sort();
    final labels =
        flow.nodes.expand((node) => _labels(node).entries.map((e) => '${e.key}=${e.value}')).toSet().take(150).toList()
          ..sort();
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
          if (widget.dependencyLayout && visible.contains(_key(edge.from))) {
            changed = visible.add(_key(edge.to)) || changed;
          }
        }
        if (!changed) break;
      }
    }
    final parents = flow.edges.map((edge) => _key(edge.from)).toSet();
    final nodes = flow.nodes
        .where(
          (node) =>
              visible.contains(_key(node.object)) &&
              (_namespace == null || node.object.namespace == _namespace) &&
              (_label == null || _labels(node).entries.any((e) => '${e.key}=${e.value}' == _label)) &&
              (!_fold || parents.contains(_key(node.object)) || node.ready == false) &&
              (_search.isEmpty || node.object.label.toLowerCase().contains(_search)),
        )
        .take(200)
        .toList();
    final keys = nodes.map((node) => _key(node.object)).toSet();
    final edges = flow.edges.where((edge) => keys.contains(_key(edge.from)) && keys.contains(_key(edge.to))).toList();
    final positions = <String, Offset>{};
    final levels = _levels(nodes, edges);
    final counts = <int, int>{};
    for (final node in nodes) {
      final column = (levels[_key(node.object)] ?? 0).clamp(0, 8);
      final row = counts[column] ?? 0;
      counts[column] = row + 1;
      positions[_key(node.object)] = widget.dependencyLayout
          ? Offset(row * 260.0 + 16, column * 124.0 + 16)
          : Offset(column * 260.0 + 16, row * 124.0 + 16);
    }
    final columns = counts.keys.fold(0, math.max) + 1, rows = counts.values.fold(0, math.max);
    final width = math.max(240.0, (widget.dependencyLayout ? rows : columns) * 260.0);
    final height = math.max(120.0, (widget.dependencyLayout ? columns : rows) * 124.0 + 16);
    _graph = Size(width, height);
    return LayoutBuilder(
      builder: (context, bounds) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(maxHeight: bounds.maxHeight * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      Expanded(child: Text(widget.dependencyLayout ? l.kubeDependencies : l.kubeGitOpsFlow)),
                      FilterChip(
                        label: Text(l.kubeGraphList),
                        selected: list,
                        onSelected: (value) => setState(() => _list = value),
                      ),
                      if (!list)
                        GlassIconButton(icon: Icons.center_focus_strong, tooltip: l.kubeGraphReset, onPressed: _fit),
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
                        PopupMenuButton<String>(
                          key: const ValueKey('kube-graph-namespace'),
                          tooltip: l.kubeNamespace,
                          onSelected: (value) => setState(() => _namespace = value.isEmpty ? null : value),
                          itemBuilder: (_) => [
                            PopupMenuItem(value: '', child: Text(l.kubeAllNamespaces)),
                            for (final namespace in namespaces) PopupMenuItem(value: namespace, child: Text(namespace)),
                          ],
                          child: Padding(
                            padding: const EdgeInsets.all(8),
                            child: Text(_namespace ?? l.kubeAllNamespaces),
                          ),
                        ),
                        PopupMenuButton<String>(
                          key: const ValueKey('kube-graph-label'),
                          tooltip: l.kubeLabelSelector,
                          onSelected: (value) => setState(() => _label = value.isEmpty ? null : value),
                          itemBuilder: (_) => [
                            PopupMenuItem(value: '', child: Text(l.kubeGitOpsAll)),
                            for (final label in labels) PopupMenuItem(value: label, child: Text(label)),
                          ],
                          child: Padding(padding: const EdgeInsets.all(8), child: Text(_label ?? l.kubeLabelSelector)),
                        ),
                      ],
                    ),
                  ),
                  if (flow.truncated || flow.nodes.length > 200) Text(l.kubeInsightsPartial),
                ],
              ),
            ),
          ),
          Expanded(
            child: nodes.isEmpty
                ? Center(child: Text(l.kubeNoResources))
                : list
                ? _tree(nodes, edges)
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
                                  painter: _Edges(
                                    edges,
                                    positions,
                                    Theme.of(context).colorScheme.outline,
                                    widget.dependencyLayout,
                                  ),
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
                                        message: [node.object.label, ?widget.statusLabel?.call(node.object)].join('\n'),
                                        child: Row(
                                          mainAxisAlignment: MainAxisAlignment.center,
                                          children: [
                                            Icon(_appearance(node).icon, color: _appearance(node).color, size: 20),
                                            const SizedBox(width: 8),
                                            Flexible(
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
      ),
    );
  }

  Widget _tree(List<KubeFlowNode> nodes, List<KubeFlowEdge> edges) {
    final byId = {for (final node in nodes) _key(node.object): node};
    final children = <String, Set<String>>{};
    final incoming = <String>{};
    for (final edge in edges) {
      children.putIfAbsent(_key(edge.from), () => {}).add(_key(edge.to));
      incoming.add(_key(edge.to));
    }
    final rows = <({KubeFlowNode node, int depth})>[];
    final visited = <String>{};
    void visit(String id, int depth) {
      final node = byId[id];
      if (node == null || !visited.add(id) || rows.length >= 200) return;
      rows.add((node: node, depth: depth.clamp(0, 6)));
      if (_collapsed.contains(id)) {
        void hide(String child) {
          if (!visited.add(child)) return;
          for (final next in children[child] ?? <String>{}) {
            hide(next);
          }
        }

        for (final child in children[id] ?? <String>{}) {
          hide(child);
        }
        return;
      }
      for (final child in children[id] ?? <String>{}) {
        visit(child, depth + 1);
      }
    }

    for (final node in nodes.where((node) => !incoming.contains(_key(node.object)))) {
      visit(_key(node.object), 0);
    }
    // Cycles have no root. Each still appears once, without recursive looping.
    for (final node in nodes) {
      visit(_key(node.object), 0);
    }
    return ListView.builder(
      key: const ValueKey('kube-gitops-tree'),
      itemCount: rows.length,
      itemBuilder: (_, index) {
        final row = rows[index], node = row.node;
        final id = _key(node.object), count = children[_key(node.object)]?.length ?? 0;
        return Padding(
          padding: EdgeInsetsDirectional.only(start: row.depth * 12),
          child: ListTile(
            key: ValueKey('kube-gitops-tree-$id'),
            leading: count == 0
                ? Icon(_appearance(node).icon, color: _appearance(node).color)
                : IconButton(
                    tooltip: _collapsed.contains(id) ? context.l10n.kubeGraphExpand : context.l10n.kubeGraphFold,
                    icon: Icon(_collapsed.contains(id) ? Icons.chevron_right : Icons.expand_more),
                    onPressed: () => setState(() {
                      if (!_collapsed.add(id)) _collapsed.remove(id);
                    }),
                  ),
            title: Text(node.object.name, style: TextStyle(color: _appearance(node).color)),
            subtitle: Text(
              ['${node.object.kind} · ${node.object.namespace}', ?widget.statusLabel?.call(node.object)].join('\n'),
            ),
            trailing: count == 0 ? null : Text('+$count'),
            onTap: widget.canOpen(node.object) ? () => widget.open(node.object) : null,
          ),
        );
      },
    );
  }
}

class _Edges extends CustomPainter {
  _Edges(this.edges, this.positions, this.color, this.vertical);
  final bool vertical;
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
      final start = from + (vertical ? const Offset(105, 96) : const Offset(210, 48));
      final end = to + (vertical ? const Offset(105, 0) : const Offset(0, 48));
      final bend = math.max(24.0, (vertical ? end.dy - start.dy : end.dx - start.dx).abs() / 2);
      canvas.drawPath(
        Path()
          ..moveTo(start.dx, start.dy)
          ..cubicTo(
            start.dx + (vertical ? 0 : bend),
            start.dy + (vertical ? bend : 0),
            end.dx - (vertical ? 0 : bend),
            end.dy - (vertical ? bend : 0),
            end.dx,
            end.dy,
          ),
        paint,
      );
      canvas.drawPath(
        Path()
          ..moveTo(end.dx - (vertical ? 4 : 6), end.dy - (vertical ? 6 : 4))
          ..lineTo(end.dx, end.dy)
          ..lineTo(end.dx + (vertical ? 4 : -6), end.dy + (vertical ? -6 : 4)),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _Edges old) =>
      edges != old.edges || positions != old.positions || color != old.color || vertical != old.vertical;
}
