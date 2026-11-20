import 'dart:math' as math;

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:material_ui/material_ui.dart';

/// Internal tree operations carry references only.
final class AutomationFileDrag {
  const AutomationFileDrag({
    required this.scope,
    required this.projectId,
    required this.digest,
    required this.path,
    this.directory = false,
  });
  final Object scope;
  final String projectId, digest, path;
  final bool directory;
}

final class AutomationTreeNode {
  AutomationTreeNode(this.path, {required this.directory});
  final String path;
  bool directory;
  final children = <String, AutomationTreeNode>{};
  String get name => path.split('/').last;
  AutomationTreeNode? at(String path) {
    if (path.isEmpty) return this;
    AutomationTreeNode? node = this;
    for (final part in path.split('/')) {
      node = node?.children[part];
    }
    return node;
  }

  List<AutomationTreeNode> get sortedChildren => children.values.toList()
    ..sort((a, b) {
      if (a.directory != b.directory) return a.directory ? -1 : 1;
      return a.name.toLowerCase().compareTo(b.name.toLowerCase());
    });
}

AutomationTreeNode automationFileHierarchy(List<AutomationFileEntry> entries) {
  final root = AutomationTreeNode('', directory: true);
  for (final entry in entries) {
    if (entry.path == automationGitSourceFile) continue;
    final parts = entry.path.split('/');
    if (parts.any((part) => part.isEmpty || part == '.' || part == '..' || part.contains(r'\'))) continue;
    var parent = root;
    for (var index = 0; index < parts.length; index++) {
      final path = parts.take(index + 1).join('/'), directory = index < parts.length - 1 || entry.directory;
      final node = parent.children.putIfAbsent(parts[index], () => AutomationTreeNode(path, directory: directory));
      if (directory) node.directory = true;
      parent = node;
    }
  }
  return root;
}

class AutomationFileTree extends StatefulWidget {
  const AutomationFileTree({
    required this.scope,
    required this.projectId,
    required this.digest,
    required this.entries,
    required this.rootLabel,
    required this.emptyLabel,
    required this.onRead,
    this.selectedPath,
    this.enabled = true,
    this.onMove,
    this.onContextMenu,
    super.key,
  });
  final Object scope;
  final String projectId, digest, rootLabel, emptyLabel;
  final List<AutomationFileEntry> entries;
  final String? selectedPath;
  final bool enabled;
  final ValueChanged<String> onRead;
  final void Function(AutomationFileDrag source, String destinationDirectory)? onMove;
  final void Function(AutomationFileDrag source, Offset position)? onContextMenu;
  @override
  State<AutomationFileTree> createState() => _AutomationFileTreeState();
}

class _AutomationFileTreeState extends State<AutomationFileTree> {
  final expanded = <String>{''};
  @override
  void initState() {
    super.initState();
    _revealSelection();
  }

  @override
  void didUpdateWidget(covariant AutomationFileTree oldWidget) {
    super.didUpdateWidget(oldWidget);
    final changed = oldWidget.projectId != widget.projectId || !identical(oldWidget.scope, widget.scope);
    if (changed) {
      expanded
        ..clear()
        ..add('');
    }
    if (changed || oldWidget.selectedPath != widget.selectedPath) _revealSelection();
  }

  void _revealSelection() {
    final parts = widget.selectedPath?.split('/') ?? const <String>[];
    for (var index = 0; index < parts.length; index++) {
      expanded.add(parts.take(index).join('/'));
    }
  }

  bool _accept(AutomationFileDrag data, String directory) {
    final root = automationFileHierarchy(widget.entries), parts = data.path.split('/');
    final target = directory.isEmpty ? parts.last : '$directory/${parts.last}';
    return widget.enabled &&
        widget.onMove != null &&
        identical(data.scope, widget.scope) &&
        data.projectId == widget.projectId &&
        data.digest == widget.digest &&
        widget.entries.any((e) => !e.directory && e.path == data.path) &&
        root.at(directory)?.directory == true &&
        root.at(target) == null &&
        parts.take(parts.length - 1).join('/') != directory;
  }

  Widget _row(AutomationTreeNode node, int depth, {bool root = false}) {
    final open = expanded.contains(node.path), selected = node.path == widget.selectedPath;
    final name = root ? widget.rootLabel : node.name;
    final colors = Theme.of(context).colorScheme;
    final fileRef = AutomationFileDrag(
      scope: widget.scope,
      projectId: widget.projectId,
      digest: widget.digest,
      path: node.path,
      directory: node.directory,
    );
    final touch = AppPlatform.isMobile && MediaQuery.sizeOf(context).width < 600;
    final height = math.max(touch ? 44.0 : 28.0, MediaQuery.textScalerOf(context).scale(13) * 1.4 + 8);
    Widget content = Semantics(
      button: true,
      selected: selected,
      expanded: node.directory ? open : null,
      child: Tooltip(
        message: root ? widget.rootLabel : node.path,
        triggerMode: touch ? TooltipTriggerMode.manual : null,
        child: InkWell(
          key: root ? const ValueKey('automation-file-root') : ValueKey('automation-file-${node.path}'),
          onSecondaryTapDown: widget.enabled && widget.onContextMenu != null
              ? (details) => widget.onContextMenu!(fileRef, details.globalPosition)
              : null,
          onTap: node.directory
              ? () => setState(() => open ? expanded.remove(node.path) : expanded.add(node.path))
              : widget.enabled
              ? () => widget.onRead(node.path)
              : null,
          child: LayoutBuilder(
            builder: (context, constraints) => Container(
              constraints: BoxConstraints(minHeight: height),
              padding: EdgeInsets.only(
                left: math.min(6 + depth * 12.0, math.max(0.0, constraints.maxWidth - 84)),
                right: 8,
              ),
              color: selected ? colors.primary.withValues(alpha: .14) : null,
              child: Row(
                children: [
                  SizedBox(
                    width: 20,
                    child: node.directory ? Icon(open ? Icons.expand_more : Icons.chevron_right, size: 16) : null,
                  ),
                  Icon(
                    node.directory
                        ? (open ? Icons.folder_open_outlined : Icons.folder_outlined)
                        : Icons.description_outlined,
                    size: 16,
                  ),
                  const SizedBox(width: 6),
                  Expanded(
                    child: Text(
                      name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
    if (touch && widget.enabled && widget.onContextMenu != null) {
      content = GestureDetector(
        onLongPressStart: (details) => widget.onContextMenu!(fileRef, details.globalPosition),
        child: content,
      );
    }
    if (node.directory) {
      final row = content;
      content = DragTarget<AutomationFileDrag>(
        key: ValueKey('automation-drop-${node.path}'),
        onWillAcceptWithDetails: (details) => _accept(details.data, node.path),
        onAcceptWithDetails: (details) {
          if (!_accept(details.data, node.path)) return;
          setState(() => expanded.add(node.path));
          widget.onMove!(details.data, node.path);
        },
        builder: (_, candidates, _) => DecoratedBox(
          decoration: BoxDecoration(
            border: candidates.isEmpty ? null : Border.all(color: colors.primary),
            color: candidates.isEmpty ? null : colors.primary.withValues(alpha: .1),
          ),
          child: row,
        ),
      );
    } else if (!touch && widget.enabled && widget.onMove != null) {
      content = Draggable<AutomationFileDrag>(
        data: fileRef,
        dragAnchorStrategy: pointerDragAnchorStrategy,
        feedback: Material(
          elevation: 4,
          borderRadius: BorderRadius.circular(6),
          child: Padding(padding: const EdgeInsets.all(8), child: Text(node.name)),
        ),
        childWhenDragging: Opacity(opacity: .35, child: content),
        child: content,
      );
    }
    if (!node.directory || !open) return content;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        content,
        for (final child in node.sortedChildren) _row(child, depth + 1),
        if (root && node.children.isEmpty) Padding(padding: const EdgeInsets.all(12), child: Text(widget.emptyLabel)),
      ],
    );
  }

  @override
  Widget build(BuildContext context) => _row(automationFileHierarchy(widget.entries), 0, root: true);
}
