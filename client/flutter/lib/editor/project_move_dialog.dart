import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:material_ui/material_ui.dart';

class ProjectMoveDialog extends StatefulWidget {
  const ProjectMoveDialog({required this.project, required this.entry, super.key});
  final ProjectFiles project;
  final ProjectEntry entry;

  @override
  State<ProjectMoveDialog> createState() => _ProjectMoveDialogState();
}

class _ProjectMoveDialogState extends State<ProjectMoveDialog> {
  late String current = widget.project.paths.dirname(widget.entry.path);
  late Future<List<ProjectEntry>> listing = widget.project.children(current);

  void navigate(String directory) => setState(() {
    current = directory;
    listing = widget.project.children(directory);
  });

  @override
  Widget build(BuildContext context) {
    final project = widget.project, l = context.l10n;
    final paths = project.paths;
    return FutureBuilder<List<ProjectEntry>>(
      future: listing,
      builder: (context, snap) {
        final ready = snap.connectionState == ConnectionState.done && snap.hasData;
        final entries = ready
            ? snap.data!.where((entry) => entry.directory && entry.path != widget.entry.path)
            : const <ProjectEntry>[];
        return AlertDialog(
          title: Text(l.editorMoveTo),
          content: SizedBox(
            width: 420,
            height: 300,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  children: [
                    IconButton(
                      icon: const Icon(Icons.arrow_upward),
                      tooltip: paths.dirname(current),
                      onPressed: current == project.root ? null : () => navigate(paths.dirname(current)),
                    ),
                    Expanded(child: Text(current, maxLines: 2, overflow: TextOverflow.ellipsis)),
                  ],
                ),
                if (snap.hasError) Text(errorMessage(l, snap.error!)),
                if (snap.connectionState != ConnectionState.done) const LinearProgressIndicator(minHeight: 2),
                Expanded(
                  child: ListView(
                    children: [
                      for (final dir in entries)
                        ListTile(
                          leading: const Icon(Icons.folder_outlined),
                          title: Text(paths.basename(dir.path)),
                          onTap: () => navigate(dir.path),
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonCancel)),
            FilledButton(
              onPressed: ready && !paths.equals(current, paths.dirname(widget.entry.path))
                  ? () => Navigator.pop(context, current)
                  : null,
              child: Text(l.editorMoveHere),
            ),
          ],
        );
      },
    );
  }
}
