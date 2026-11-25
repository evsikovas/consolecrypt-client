import 'dart:io';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path/path.dart' as p;

class ProjectTree extends StatefulWidget {
  const ProjectTree({
    required this.project,
    required this.active,
    required this.onOpen,
    required this.revision,
    super.key,
  });
  final ProjectFiles project;
  final String? active;
  final ValueChanged<String> onOpen;
  final int revision;
  @override
  State<ProjectTree> createState() => _ProjectTreeState();
}

class _ProjectTreeState extends State<ProjectTree> {
  final expanded = <String>{};
  final contents = <String, List<FileSystemEntity>>{};
  final loading = <String>{};
  final errors = <String, String>{};
  int generation = 0;

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
    for (final entry in contents[path] ?? const <FileSystemEntity>[]) {
      final folder = entry is Directory;
      yield (entry.path, depth, folder);
      if (folder && expanded.contains(entry.path)) yield* _rows(entry.path, depth + 1);
    }
  }

  @override
  Widget build(BuildContext context) {
    final root = widget.project.root;
    final rows = _rows(root, 0).toList();
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(14, 12, 8, 10),
          child: Tooltip(
            message: root,
            child: Text(
              p.basename(root).isEmpty ? root : p.basename(root),
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
            itemCount: rows.length,
            itemExtent: 32,
            itemBuilder: (context, index) {
              final (path, depth, folder) = rows[index];
              final selected = path == widget.active;
              return Material(
                color: selected ? theme.colorScheme.secondaryContainer : Colors.transparent,
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
                    padding: EdgeInsets.only(left: 8 + (depth.clamp(0, 8)) * 10.0, right: 8),
                    child: Row(
                      children: [
                        SizedBox(
                          width: 16,
                          child: folder
                              ? Icon(expanded.contains(path) ? Icons.expand_more : Icons.chevron_right, size: 15)
                              : null,
                        ),
                        Icon(
                          folder ? Icons.folder_outlined : Icons.description_outlined,
                          size: 16,
                          color: folder ? theme.colorScheme.primary : theme.colorScheme.onSurfaceVariant,
                        ),
                        const SizedBox(width: 7),
                        Expanded(child: Text(p.basename(path), maxLines: 1, overflow: TextOverflow.ellipsis)),
                      ],
                    ),
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
