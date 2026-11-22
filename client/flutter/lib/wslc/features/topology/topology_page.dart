import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

enum NodeKind { image, container, volume, folder, network }

/// List rows carry a short image ID and Inspect the full `sha256:` one;
/// both reduce to the same key.
String imageKey(String id) {
  final hex = id.startsWith('sha256:') ? id.substring(7) : id;
  return hex.length > 12 ? hex.substring(0, 12) : hex;
}

class MapNode {
  MapNode(this.kind, this.key, this.label, {this.detail, this.containerId});
  final NodeKind kind;
  final String key;
  final String label;
  final String? detail;
  final String? containerId;

  /// Nothing uses it, so it is a cleanup candidate. Built-in networks cannot be removed.
  bool get unused =>
      kind != NodeKind.container &&
      degree == 0 &&
      !(kind == NodeKind.network && const {'bridge', 'host', 'none'}.contains(key));
  int degree = 0;
}

/// Columns and edges of the relations map; pure so it can be tested.
class MapModel {
  MapModel(this.columns, this.edges);
  final List<List<MapNode>> columns;
  final List<(MapNode, MapNode)> edges;

  factory MapModel.build({
    required List<ContainerLinks> links,
    required List<ImageSummary> images,
    required List<VolumeSummary> volumes,
    required List<NetworkSummary> networks,
  }) {
    final imageNodes = <String, MapNode>{};
    for (final i in images) {
      // One image may carry several tags; it is one node.
      final node = imageNodes.putIfAbsent(
        imageKey(i.id),
        () => MapNode(NodeKind.image, i.id, i.repository ?? shortId(i.id)),
      );
      if (i.tag != null && node.detail == null) {
        imageNodes[imageKey(i.id)] = MapNode(NodeKind.image, i.id, node.label, detail: i.tag);
      }
    }
    final byName = {
      for (final i in images)
        if (i.repository != null && i.tag != null) '${i.repository}:${i.tag}': imageNodes[imageKey(i.id)]!,
    };
    final volumeNodes = {for (final v in volumes) v.name: MapNode(NodeKind.volume, v.name, v.name)};
    final networkNodes = {
      for (final n in networks)
        if (n.name != null) n.name!: MapNode(NodeKind.network, n.name!, n.name!, detail: n.driver),
    };
    final folderNodes = <String, MapNode>{};
    final containerNodes = <MapNode>[];
    final edges = <(MapNode, MapNode)>[];
    void link(MapNode a, MapNode b) {
      a.degree++;
      b.degree++;
      edges.add((a, b));
    }

    for (final c in links) {
      final node = MapNode(NodeKind.container, c.id, c.name ?? shortId(c.id), detail: c.state.name, containerId: c.id);
      containerNodes.add(node);
      // Match by ID first, then by the `repository:tag` the container was created from.
      final image =
          (c.imageId == null ? null : imageNodes[imageKey(c.imageId!)]) ??
          (c.imageName == null ? null : byName[c.imageName]);
      if (image != null) {
        link(image, node);
      } else if (c.imageName != null) {
        link(
          imageNodes.putIfAbsent(
            'name:${c.imageName}',
            () => MapNode(NodeKind.image, 'name:${c.imageName}', c.imageName!),
          ),
          node,
        );
      }
      for (final v in c.volumes) {
        link(node, volumeNodes.putIfAbsent(v, () => MapNode(NodeKind.volume, v, v)));
      }
      for (final f in c.binds) {
        link(node, folderNodes.putIfAbsent(f, () => MapNode(NodeKind.folder, f, f.split(r'\').last, detail: f)));
      }
      for (final n in c.networks) {
        link(node, networkNodes.putIfAbsent(n, () => MapNode(NodeKind.network, n, n)));
      }
    }
    // Used resources first, so lines stay short; unused ones gather at the bottom.
    List<MapNode> ordered(Iterable<MapNode> nodes) => nodes.toList()..sort((a, b) => b.degree.compareTo(a.degree));
    // Every resource sits in the column right after the containers, so no line
    // has to cross another column's cards.
    return MapModel([
      ordered(imageNodes.values),
      containerNodes,
      [...ordered(volumeNodes.values), ...ordered(folderNodes.values), ...ordered(networkNodes.values)],
    ], edges);
  }
}

class TopologyPage extends StatefulWidget {
  const TopologyPage({super.key, required this.controller});
  final DeskController controller;
  @override
  State<TopologyPage> createState() => _TopologyPageState();
}

class _TopologyPageState extends State<TopologyPage> {
  late final api = WslcApiScope.of(context);

  List<ContainerLinks>? links;
  String? error;
  String? hovered;
  List<ContainerSummary> _seen = const [];

  @override
  void initState() {
    super.initState();
    unawaited(load());
  }

  @override
  void didUpdateWidget(covariant TopologyPage old) {
    super.didUpdateWidget(old);
    // Re-read relations when the container list itself changed.
    if (!identical(_seen, widget.controller.containers)) unawaited(load());
  }

  Future<void> load() async {
    _seen = widget.controller.containers;
    try {
      final result = await api.containerLinks();
      if (mounted) {
        setState(() {
          links = result;
          error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => error = errorText(e));
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    final data = links;
    if (data == null) {
      return Panel(
        child: EmptyState(
          icon: Icons.account_tree_outlined,
          loading: error == null,
          title: error == null ? context.l10n.wslcBuildingMap : context.l10n.wslcCouldNotBuildMap,
          hint: error,
        ),
      );
    }
    final model = MapModel.build(links: data, images: c.images, volumes: c.volumes, networks: c.networks);
    final unused = [
      for (final col in model.columns)
        for (final n in col)
          if (n.unused) n,
    ];
    final p = DeskPalette.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            for (final (kind, label) in [
              (NodeKind.image, context.l10n.dockerImage),
              (NodeKind.container, context.l10n.kubeContainer),
              (NodeKind.volume, context.l10n.wslcVolumeRunWizard),
              (NodeKind.folder, context.l10n.wslcWindowsFolderTopologyPage),
              (NodeKind.network, context.l10n.hostMetricsNetwork),
            ]) ...[
              Icon(_icon(kind), size: 16, color: _color(p, kind)),
              const SizedBox(width: 4),
              Text(label, style: TextStyle(color: p.muted, fontSize: 12.5)),
              const SizedBox(width: 14),
            ],
            const Spacer(),
            if (unused.isNotEmpty)
              Tooltip(
                message: context.l10n.wslcOpenCleanup,
                child: InkWell(
                  borderRadius: BorderRadius.circular(20),
                  onTap: () => c.selectSection(DeskSection.cleanup),
                  child: Pill(
                    context.l10n.wslcUnused(unused.length),
                    color: p.warning,
                    icon: Icons.cleaning_services_outlined,
                  ),
                ),
              ),
          ],
        ),
        const SizedBox(height: 12),
        Expanded(
          child: Panel(
            child: data.isEmpty && unused.isEmpty
                ? EmptyState(icon: Icons.account_tree_outlined, title: context.l10n.wslcNoConnectionsYet)
                : LayoutBuilder(
                    builder: (context, box) => _MapCanvas(
                      model: model,
                      width: box.maxWidth,
                      hovered: hovered,
                      onHover: (key) => setState(() => hovered = key),
                      onOpen: c.openContainer,
                    ),
                  ),
          ),
        ),
      ],
    );
  }
}

IconData _icon(NodeKind k) => switch (k) {
  NodeKind.image => Icons.layers_rounded,
  NodeKind.container => Icons.view_in_ar_rounded,
  NodeKind.volume => Icons.storage_rounded,
  NodeKind.folder => Icons.folder_rounded,
  NodeKind.network => Icons.hub_rounded,
};

Color _color(DeskPalette p, NodeKind k) => switch (k) {
  NodeKind.image => p.accentEnd,
  NodeKind.container => p.success,
  NodeKind.volume => p.accent,
  NodeKind.folder => p.warning,
  NodeKind.network => const Color(0xffc084fc),
};

const _nodeHeight = 46.0;
const _gap = 12.0;
const _groupGap = 16.0;
const _pad = 20.0;
const _maxNodeWidth = 340.0;

class _MapCanvas extends StatelessWidget {
  const _MapCanvas({
    required this.model,
    required this.width,
    required this.hovered,
    required this.onHover,
    required this.onOpen,
  });
  final MapModel model;
  final double width;
  final String? hovered;
  final ValueChanged<String?> onHover;
  final ValueChanged<String> onOpen;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final colWidth = (width - _pad * 2) / model.columns.length;
    final nodeWidth = (colWidth - 48).clamp(120.0, _maxNodeWidth);
    double left(int c) => _pad + c * colWidth + (colWidth - nodeWidth) / 2;
    final positions = <MapNode, Rect>{};
    var height = 0.0;
    for (var c = 0; c < model.columns.length; c++) {
      final col = model.columns[c];
      var y = _pad + 28;
      for (var r = 0; r < col.length; r++) {
        // Volumes, folders and networks share a column; a gap separates them.
        if (r > 0 && col[r].kind != col[r - 1].kind) y += _groupGap;
        positions[col[r]] = Rect.fromLTWH(left(c), y, nodeWidth, _nodeHeight);
        y += _nodeHeight + _gap;
      }
      if (y + _pad > height) height = y + _pad;
    }
    final focus = <MapNode>{};
    if (hovered != null) {
      for (final (a, b) in model.edges) {
        if (a.key == hovered || b.key == hovered) focus.addAll([a, b]);
      }
    }
    final titles = [context.l10n.kubeImages, context.l10n.kubeContainers, context.l10n.wslcVolumesFoldersAndNetworks];
    return SingleChildScrollView(
      child: SizedBox(
        height: height,
        child: Stack(
          children: [
            for (var c = 0; c < titles.length; c++)
              Positioned(
                left: left(c),
                top: _pad,
                child: Text(
                  titles[c].toUpperCase(),
                  style: TextStyle(color: p.faint, fontSize: 10.5, fontWeight: FontWeight.w700, letterSpacing: .7),
                ),
              ),
            Positioned.fill(child: CustomPaint(painter: _EdgePainter(model.edges, positions, hovered, p))),
            for (final entry in positions.entries)
              Positioned.fromRect(
                rect: entry.value,
                child: _NodeCard(
                  node: entry.key,
                  dimmed: hovered != null && entry.key.key != hovered && !focus.contains(entry.key),
                  onHover: onHover,
                  onOpen: onOpen,
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _EdgePainter extends CustomPainter {
  _EdgePainter(this.edges, this.positions, this.hovered, this.p);
  final List<(MapNode, MapNode)> edges;
  final Map<MapNode, Rect> positions;
  final String? hovered;
  final DeskPalette p;

  @override
  void paint(Canvas canvas, Size size) {
    for (final (a, b) in edges) {
      final ra = positions[a], rb = positions[b];
      if (ra == null || rb == null) continue;
      final active = hovered != null && (a.key == hovered || b.key == hovered);
      final start = ra.centerRight;
      final end = rb.centerLeft;
      final dx = (end.dx - start.dx) / 2;
      final path = Path()
        ..moveTo(start.dx, start.dy)
        ..cubicTo(start.dx + dx, start.dy, end.dx - dx, end.dy, end.dx, end.dy);
      final color = _color(p, b.kind == NodeKind.container ? a.kind : b.kind);
      canvas.drawPath(
        path,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = active ? 2.4 : 1.4
          ..color = color.withValues(
            alpha: hovered == null
                ? .45
                : active
                ? .95
                : .08,
          ),
      );
    }
  }

  @override
  bool shouldRepaint(_EdgePainter old) => old.hovered != hovered || old.edges != edges || old.positions != positions;
}

class _NodeCard extends StatelessWidget {
  const _NodeCard({required this.node, required this.dimmed, required this.onHover, required this.onOpen});
  final MapNode node;
  final bool dimmed;
  final ValueChanged<String?> onHover;
  final ValueChanged<String> onOpen;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final color = _color(p, node.kind);
    final running = node.kind == NodeKind.container && node.detail == 'running';
    return MouseRegion(
      onEnter: (_) => onHover(node.key),
      onExit: (_) => onHover(null),
      cursor: node.containerId != null ? SystemMouseCursors.click : MouseCursor.defer,
      child: GestureDetector(
        onTap: node.containerId == null ? null : () => onOpen(node.containerId!),
        child: AnimatedOpacity(
          duration: const Duration(milliseconds: 150),
          opacity: dimmed ? .3 : 1,
          child: Tooltip(
            message: [
              node.key.startsWith('name:') ? node.key.substring(5) : node.key,
              if (node.unused) context.l10n.dockerVolumeUnused,
            ].join('\n'),
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 10),
              decoration: BoxDecoration(
                color: p.raised,
                borderRadius: BorderRadius.circular(10),
                border: Border.all(color: node.unused ? p.warning.withValues(alpha: .6) : color.withValues(alpha: .45)),
              ),
              child: Row(
                children: [
                  Icon(_icon(node.kind), size: 17, color: color),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          node.label,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontSize: 12.5, fontWeight: FontWeight.w600),
                        ),
                        if (node.detail != null || node.unused)
                          Text(
                            node.unused ? context.l10n.dockerVolumeUnused : node.detail!,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(fontSize: 11, color: node.unused ? p.warning : p.faint),
                          ),
                      ],
                    ),
                  ),
                  if (node.kind == NodeKind.container) StatusDot(color: running ? p.success : p.faint, glow: running),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
