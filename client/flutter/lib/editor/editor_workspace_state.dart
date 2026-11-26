part of 'editor_screen.dart';

T? _atIndex<T>(List<T> items, int index) => index >= 0 && index < items.length ? items[index] : null;

Map<String, Object?> _workspaceSnapshot(List<_Folder> folders, _Folder? selected) => {
  'selected': selected == null ? -1 : folders.indexOf(selected),
  'folders': [
    for (final folder in folders)
      {
        'root': folder.project.root,
        if (folder.project case final SshProject remote) 'host': hostToJson(remote.host),
        'tree': folder.showTree,
        'expanded': folder.expanded.toList(),
        'active': folder.active == null ? -1 : folder.documents.indexOf(folder.active!),
        'documents': [
          for (final doc in folder.documents)
            {
              'root': doc.files.root,
              'remote': doc.files is SshProject,
              'path': doc.source.path,
              // Keep the exact baseline for conflict checks, BOM and line endings.
              'original': base64Encode(doc.source.bytes),
              if (doc.controller.text != doc.saved) 'draft': doc.controller.text,
              'base': doc.controller.selection.baseOffset,
              'extent': doc.controller.selection.extentOffset,
              'preview': doc.markdownPreview,
            },
        ],
      },
  ],
};

Iterable<_Folder> _restoreFolders(Map<String, dynamic> state, WidgetRef ref) sync* {
  for (final json in (state['folders'] as List).cast<Map<String, dynamic>>()) {
    final root = json['root'] as String;
    final host = json['host'] as Map<String, dynamic>?;
    final project = host == null
        ? LocalProjectFiles(root)
        : SshProject.resume(
            root,
            hostFromJson(host),
            ref.read(sftpServiceProvider),
            ref.read(sftpBrowserServiceProvider),
          );
    final folder = _Folder(project)
      ..showTree = json['tree'] as bool
      ..expanded.addAll((json['expanded'] as List).cast<String>());
    for (final entry in (json['documents'] as List).cast<Map<String, dynamic>>()) {
      final origin = entry['root'] as String;
      final remote = entry['remote'] as bool;
      final files = remote == (project is SshProject) && origin == root
          ? project
          : remote
          ? (project as SshProject).at(origin)
          : LocalProjectFiles(origin);
      final source = ProjectText.decode(entry['path'] as String, base64Decode(entry['original'] as String));
      final draft = entry['draft'] as String?;
      final doc = _Document(files, source)
        ..markdownPreview = entry['preview'] as bool
        ..dirty = draft != null;
      doc.controller.value = TextEditingValue(
        text: draft ?? source.text,
        selection: TextSelection(baseOffset: entry['base'] as int, extentOffset: entry['extent'] as int),
      );
      folder.documents.add(doc);
    }
    folder.active = _atIndex(folder.documents, json['active'] as int);
    yield folder;
  }
}
