import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/native_drag_drop.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:material_ui/material_ui.dart';

typedef ProjectFileDrag = ({ProjectFiles project, String path});

class ProjectTree extends StatefulWidget {
  const ProjectTree({
    required this.project,
    required this.active,
    required this.onOpen,
    required this.revision,
    required this.enabled,
    required this.onMove,
    required this.onImport,
    super.key,
  });
  final ProjectFiles project;
  final String? active;
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
      for (final path in expanded.toList()) {
        _load(path);
      }
    }
  }

  Future<void> _load(String path) async {
    if (!loading.add(path)) return;
    final epoch = generation;
    try {
      final entries = await widget.project.children(path);
      if (!mounted || epoch != generation) return;
      contents[path] = entries;
      errors.remove(path);
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

  @override
  Widget build(BuildContext context) {
    final root = widget.project.root;
    final rows = _rows(root, 0).toList();
    final theme = Theme.of(context);
    final content = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
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
              final row = Material(
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
