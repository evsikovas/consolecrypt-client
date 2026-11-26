import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass_menu.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/native_drag_drop.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

typedef ProjectFileDrag = ({ProjectFiles project, String path});

enum ProjectAction { open, newFile, newFolder, rename, duplicate, move, copyPath, copyRelativePath, delete }

class ProjectTree extends StatefulWidget {
  const ProjectTree({
    required this.project,
    required this.active,
    required this.onOpen,
    required this.revision,
    required this.enabled,
    required this.onMove,
    required this.onImport,
    required this.onAction,
    this.change,
    super.key,
  });
  final ProjectFiles project;
  final String? active;
  final ProjectChange? change;
  final void Function(ProjectEntry, ProjectAction) onAction;
  final ValueChanged<String> onOpen;
  final int revision;
  final bool enabled;
  final void Function(String path, String directory) onMove;
  final void Function(List<String> paths, String directory) onImport;
  @override
  State<ProjectTree> createState() => _ProjectTreeState();
}

class _ProjectTreeState extends State<ProjectTree> {
  final expanded = <String>{};
  final contents = <String, List<ProjectEntry>>{};
  final loading = <String>{};
  final errors = <String, String>{};
  int generation = 0;
  final viewport = GlobalKey();
  final scroll = ScrollController();
  String? hoveredDirectory;

  @override
  void dispose() {
    scroll.dispose();
    super.dispose();
  }

  void _hover(String? path) {
    if (mounted && path != hoveredDirectory) setState(() => hoveredDirectory = path);
  }

  String _destination(Offset global, List<(String, int, bool)> rows) {
    final box = viewport.currentContext?.findRenderObject() as RenderBox?;
    if (box == null || !scroll.hasClients) return widget.project.root;
    final y = box.globalToLocal(global).dy;
    final index = ((scroll.offset + y) / 32).floor();
    if (y < 0 || y >= box.size.height || index < 0 || index >= rows.length) return widget.project.root;
    final (path, _, folder) = rows[index];
    return folder ? path : widget.project.paths.dirname(path);
  }

  @override
  void initState() {
    super.initState();
    expanded.add(widget.project.root);
    _load(widget.project.root);
  }

  @override
  void didUpdateWidget(ProjectTree old) {
    super.didUpdateWidget(old);
    if (old.revision != widget.revision) {
      generation++;
      loading.clear();
      final change = widget.change;
      if (change != null && change != old.change) {
        final next = <String>{};
        for (final path in expanded) {
          final updated = change.remap(widget.project.paths, path);
          if (updated != null) next.add(updated);
        }
        expanded
          ..clear()
          ..addAll(next);
      }
      expanded.add(widget.project.root);
      contents.removeWhere((path, _) => !expanded.contains(path));
      errors.clear();
      _load(widget.project.root, refreshExpanded: true);
    }
  }

  Future<void> _load(String path, {bool refreshExpanded = false}) async {
    if (!loading.add(path)) return;
    final epoch = generation;
    try {
      final entries = await widget.project.children(path);
      if (!mounted || epoch != generation) return;
      contents[path] = entries;
      errors.remove(path);
      for (final entry in entries) {
        if (entry.directory &&
            expanded.contains(entry.path) &&
            (refreshExpanded || !contents.containsKey(entry.path))) {
          unawaited(_load(entry.path, refreshExpanded: refreshExpanded));
        }
      }
    } catch (error) {
      if (!mounted || epoch != generation) return;
      errors[path] = error.toString();
    } finally {
      if (mounted && epoch == generation) setState(() => loading.remove(path));
    }
  }

  Iterable<(String, int, bool)> _rows(String path, int depth) sync* {
    for (final entry in contents[path] ?? const <ProjectEntry>[]) {
      final folder = entry.directory;
      yield (entry.path, depth, folder);
      if (folder && expanded.contains(entry.path)) yield* _rows(entry.path, depth + 1);
    }
  }

  Future<void> _menu(ProjectEntry entry, Offset point) async {
    final l = context.l10n;
    final root = entry.path == widget.project.root;
    final action = await showGlassMenu<ProjectAction>(
      context: context,
      anchor: point & const Size(1, 1),
      entries: [
        if (!entry.directory)
          GlassMenuItem(value: ProjectAction.open, label: l.commonOpen, icon: Icons.description_outlined),
        GlassMenuItem(value: ProjectAction.newFile, label: l.sftpActionNewFile, icon: Icons.note_add_outlined),
        GlassMenuItem(value: ProjectAction.newFolder, label: l.sftpNewFolder, icon: Icons.create_new_folder_outlined),
        const GlassMenuDivider(),
        if (!root) ...[
          GlassMenuItem(value: ProjectAction.rename, label: l.commonRename, icon: Icons.edit_outlined),
          GlassMenuItem(value: ProjectAction.duplicate, label: l.sftpActionDuplicate, icon: Icons.file_copy_outlined),
          GlassMenuItem(value: ProjectAction.move, label: l.editorMoveTo, icon: Icons.drive_file_move_outline),
        ],
        GlassMenuItem(value: ProjectAction.copyPath, label: l.sftpCopyPath, icon: Icons.content_copy_outlined),
        if (!root)
          GlassMenuItem(value: ProjectAction.copyRelativePath, label: l.automationFileCopyPath, icon: Icons.link),
        if (!root) ...[
          const GlassMenuDivider(),
          GlassMenuItem(
            value: ProjectAction.delete,
            label: l.commonDelete,
            icon: Icons.delete_outline,
            destructive: true,
          ),
        ],
      ],
    );
    if (mounted && widget.enabled && action != null) widget.onAction(entry, action);
  }

  Widget _actions(ProjectEntry entry, Widget child) => Listener(
    onPointerDown: AppPlatform.isMacOS && widget.enabled
        ? (event) {
            if (event.buttons == kPrimaryButton && HardwareKeyboard.instance.isControlPressed) {
              GestureBinding.instance.cancelPointer(event.pointer);
              unawaited(_menu(entry, event.position));
            }
          }
        : null,
    child: GestureDetector(
      behavior: HitTestBehavior.opaque,
      onSecondaryTapUp: widget.enabled ? (event) => _menu(entry, event.globalPosition) : null,
      child: AppPlatform.isMobile
          ? Row(
              children: [
                Expanded(child: child),
                Builder(
                  builder: (context) => IconButton(
                    tooltip: context.l10n.glassMoreActions,
                    onPressed: widget.enabled ? () => _menu(entry, glassAnchorRect(context).bottomLeft) : null,
                    icon: const Icon(Icons.more_horiz, size: 16),
                  ),
                ),
              ],
            )
          : child,
    ),
  );

  @override
  Widget build(BuildContext context) {
    final root = widget.project.root;
    final rows = _rows(root, 0).toList();
    final theme = Theme.of(context);
    final content = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _actions(
          (path: root, directory: true),
          Container(
            color: hoveredDirectory == root ? theme.colorScheme.secondaryContainer : null,
            padding: const EdgeInsets.fromLTRB(16, 12, 8, 8),
            child: Tooltip(
              message: root,
              child: Text(
                widget.project.paths.basename(root).isEmpty ? root : widget.project.paths.basename(root),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.titleSmall,
              ),
            ),
          ),
        ),
        if (loading.isNotEmpty) const LinearProgressIndicator(minHeight: 2),
        for (final error in errors.entries.where((entry) => expanded.contains(entry.key)))
          TextButton(
            onPressed: () => _load(error.key),
            child: Text(error.value, maxLines: 2, overflow: TextOverflow.ellipsis),
          ),
        if (rows.isEmpty && loading.isEmpty && errors.isEmpty)
          Padding(padding: const EdgeInsets.all(12), child: Text(context.l10n.editorNoFiles)),
        Expanded(
          child: ListView.builder(
            key: viewport,
            controller: scroll,
            itemCount: rows.length,
            itemExtent: 32,
            itemBuilder: (context, index) {
              final (path, depth, folder) = rows[index];
              final selected = path == widget.active;
              final row = _actions(
                (path: path, directory: folder),
                Material(
                  color: (selected || folder && path == hoveredDirectory)
                      ? theme.colorScheme.secondaryContainer
                      : Colors.transparent,
                  child: InkWell(
                    onTap: () {
                      if (!folder) {
                        widget.onOpen(path);
                        return;
                      }
                      setState(() {
                        if (!expanded.remove(path)) {
                          expanded.add(path);
                          if (!contents.containsKey(path)) _load(path);
                        }
                      });
                    },
                    child: Padding(
                      padding: EdgeInsetsDirectional.only(start: 8 + (depth.clamp(0, 8)) * 12.0, end: 8),
                      child: Row(
                        children: [
                          SizedBox(
                            width: 16,
                            child: folder
                                ? Icon(expanded.contains(path) ? Icons.expand_more : Icons.chevron_right, size: 16)
                                : null,
                          ),
                          Icon(
                            folder ? Icons.folder_outlined : Icons.description_outlined,
                            size: 16,
                            color: selected ? theme.colorScheme.primary : theme.colorScheme.onSurfaceVariant,
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Text(
                              widget.project.paths.basename(path),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              );
              if (folder || !widget.enabled) return row;
              final ProjectFileDrag data = (project: widget.project, path: path);
              final feedback = Material(
                elevation: 3,
                child: Padding(padding: const EdgeInsets.all(8), child: Text(widget.project.paths.basename(path))),
              );
              return AppPlatform.isMobile
                  ? LongPressDraggable<ProjectFileDrag>(
                      data: data,
                      feedback: feedback,
                      dragAnchorStrategy: pointerDragAnchorStrategy,
                      child: row,
                    )
                  : Draggable<ProjectFileDrag>(
                      data: data,
                      feedback: feedback,
                      dragAnchorStrategy: pointerDragAnchorStrategy,
                      child: row,
                    );
            },
          ),
        ),
      ],
    );
    String destination(Offset local) {
      final box = context.findRenderObject()! as RenderBox;
      return _destination(box.localToGlobal(local), rows);
    }

    return OsFileDropTarget(
      key: const ValueKey('editor-tree-drop'),
      enabled: widget.enabled,
      onHover: (pos) => _hover(destination(pos)),
      onExit: () => _hover(null),
      onDrop: (paths, pos) {
        if (mounted && widget.enabled) widget.onImport(paths, destination(pos));
      },
      child: DragTarget<ProjectFileDrag>(
        onWillAcceptWithDetails: (d) => widget.enabled && identical(d.data.project, widget.project),
        onMove: (d) {
          if (widget.enabled && identical(d.data.project, widget.project)) _hover(_destination(d.offset, rows));
        },
        onLeave: (_) => _hover(null),
        onAcceptWithDetails: (d) {
          _hover(null);
          widget.onMove(d.data.path, _destination(d.offset, rows));
        },
        builder: (_, _, _) => content,
      ),
    );
  }
}
