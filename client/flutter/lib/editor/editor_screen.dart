import 'dart:async';

import 'package:consolecrypt/core/glass/glass_tab_strip.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/editor/editor_language.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:consolecrypt/editor/project_terminal.dart';
import 'package:consolecrypt/editor/project_tree.dart';
import 'package:consolecrypt/editor/ssh_folder_picker.dart';
import 'package:consolecrypt/editor/ssh_project.dart';
import 'package:consolecrypt/editor/workspace_code_editor.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final editorFolderPickerProvider = Provider<Future<String?> Function()>((ref) => getDirectoryPath);

class _Document {
  _Document(this.source, this.name) : controller = TextEditingController(text: source.text), saved = source.text;
  ProjectText source;
  final String name;
  final TextEditingController controller;
  String saved;
  bool dirty = false;
}

class _Folder {
  _Folder(this.project) : branches = project.branches();
  final ProjectFiles project;
  final documents = <String, _Document>{};
  _Document? active;
  int revision = 0;
  bool showTree = true;
  Future<ProjectBranches?> branches;

  String get name {
    final basename = project.paths.basename(project.root);
    return basename.isEmpty ? project.root : basename;
  }

  bool matches(ProjectFiles other) {
    final current = project;
    if (!current.paths.equals(current.root, other.root)) return false;
    if (current is SshProject) return other is SshProject && current.host.id == other.host.id;
    return other is! SshProject;
  }

  void dispose() {
    for (final doc in documents.values) {
      doc.controller.dispose();
    }
    unawaited(project.close());
  }
}

String _errorMessage(AppLocalizations l, Object error) => switch (error) {
  AppException() => errorMessage(l, error),
  EditorIssue.changed => l.editorChanged,
  EditorIssue.unsupported => l.editorUnsupported,
  EditorIssue.gitMissing => l.editorNoGitTool,
  EditorIssue.outside => l.editorOutside,
  _ => error.toString(),
};

class EditorScreen extends ConsumerStatefulWidget {
  const EditorScreen({super.key});
  @override
  ConsumerState<EditorScreen> createState() => _EditorScreenState();
}

class _EditorScreenState extends ConsumerState<EditorScreen> {
  final folders = <_Folder>[];
  _Folder? selected;
  bool busy = false;
  final _terminalOverrides = [terminalTabsProvider.overrideWith(TerminalTabsController.new)];

  Future<void> _run(Future<void> Function() task) async {
    if (busy) return;
    setState(() => busy = true);
    try {
      await task();
    } catch (error) {
      if (mounted) showSnack(context, _errorMessage(context.l10n, error), error: true);
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<bool> _discard(Iterable<_Document> docs) async {
    final names = docs.where((d) => d.dirty).map((d) => d.name).toList();
    if (names.isEmpty) return true;
    final l = context.l10n;
    return showConfirmDialog(
      context,
      title: l.editorUnsaved,
      message: names.join('\n'),
      confirmLabel: l.editorDiscard,
      destructive: true,
    );
  }

  void _refresh(_Folder folder) {
    setState(() {
      folder.revision++;
      folder.branches = folder.project.branches();
    });
  }

  Future<void> _project() => _run(() async {
    final path = await ref.read(editorFolderPickerProvider)();
    if (!mounted || path == null) return;
    final next = await ProjectFiles.open(path);
    _openFolder(next);
  });

  Future<void> _remoteProject() => _run(() async {
    final host = await showHostPicker(context, title: context.l10n.editorOpenSsh);
    if (!mounted || host == null) return;
    final sftp = ref.read(sftpServiceProvider), browser = ref.read(sftpBrowserServiceProvider);
    final session = await sftp.connect(host.id);
    var adopted = false;
    try {
      final home = await sftp.remoteHome(session);
      if (!mounted) return;
      final root = await showDialog<String>(
        context: context,
        builder: (_) => SshFolderPicker(host: host.name, home: home, session: session, browser: browser),
      );
      if (!mounted || root == null) return;
      final next = SshProject(root, host, session, sftp, browser);
      adopted = _openFolder(next);
    } finally {
      if (!adopted) await sftp.disconnect(session);
    }
  });

  bool _openFolder(ProjectFiles project) {
    if (!mounted) return false;
    for (final folder in folders) {
      if (folder.matches(project)) {
        setState(() => selected = folder);
        return false;
      }
    }
    final folder = _Folder(project);
    setState(() {
      folders.add(folder);
      selected = folder;
    });
    return true;
  }

  Future<void> _closeFolder(_Folder folder) => _run(() async {
    if (!await _discard(folder.documents.values) || !mounted) return;
    setState(() {
      final index = folders.indexOf(folder);
      folders.removeAt(index);
      if (selected == folder) selected = folders.isEmpty ? null : folders[index.clamp(0, folders.length - 1)];
    });
    WidgetsBinding.instance.addPostFrameCallback((_) => folder.dispose());
  });

  Future<void> _open(_Folder folder, String path) async {
    final existing = folder.documents[path];
    final doc = existing ?? _Document(await folder.project.read(path), folder.project.paths.basename(path));
    if (!mounted) {
      if (existing == null) doc.controller.dispose();
      return;
    }
    setState(() {
      folder.documents[path] = doc;
      folder.active = doc;
      folder.showTree = false;
    });
  }

  Future<void> _newFile(_Folder folder) => _run(() async {
    final current = folder.project;
    final String path;
    if (current is SshProject) {
      final name = await showTextInputDialog(context, title: context.l10n.editorNew, label: context.l10n.commonName);
      if (!mounted || name == null) return;
      if (name.isEmpty || name.contains('/') || name == '.' || name == '..') throw EditorIssue.outside;
      path = current.paths.join(current.root, name);
    } else {
      final location = await getSaveLocation(initialDirectory: current.root);
      if (!mounted || location == null) return;
      path = location.path;
    }
    final file = await current.createPath(path);
    if (!mounted) return;
    _refresh(folder);
    await _open(folder, file);
  });

  Future<void> _save(_Folder folder) => _run(() async {
    final doc = folder.active;
    if (doc == null || !doc.dirty) return;
    final text = doc.controller.text;
    final result = await folder.project.save(doc.source, text);
    if (!mounted) return;
    setState(() {
      doc.source = result;
      doc.saved = text;
      doc.dirty = doc.controller.text != text;
    });
  });

  Future<void> _close(_Folder folder, _Document doc) => _run(() async {
    if (!await _discard([doc]) || !mounted) return;
    setState(() {
      folder.documents.remove(doc.source.path);
      if (folder.active == doc) folder.active = folder.documents.values.lastOrNull;
    });
    WidgetsBinding.instance.addPostFrameCallback((_) => doc.controller.dispose());
  });

  Future<void> _git(_Folder folder) async {
    _refresh(folder);
    final loaded = folder.branches;
    final l = context.l10n;
    await showDialog<void>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(l.editorBranches),
        content: SizedBox(
          width: 380,
          child: FutureBuilder<ProjectBranches?>(
            future: loaded,
            builder: (context, snap) {
              if (snap.hasError) return Text(_errorMessage(l, snap.error!));
              if (snap.connectionState != ConnectionState.done) return const LinearProgressIndicator();
              final git = snap.data;
              if (git == null) return Text(l.editorNoGit);
              return ListView(
                shrinkWrap: true,
                children: [
                  for (final (title, names) in [(l.editorLocal, git.local), (l.editorRemote, git.remote)])
                    if (names.isNotEmpty) ...[
                      Padding(
                        padding: const EdgeInsets.symmetric(vertical: 8),
                        child: Text(title, style: Theme.of(context).textTheme.labelLarge),
                      ),
                      for (final name in names)
                        ListTile(
                          dense: true,
                          title: SelectableText(name),
                          leading: Icon(
                            name == git.current && title == l.editorLocal ? Icons.check : Icons.account_tree_outlined,
                            size: 16,
                          ),
                        ),
                    ],
                  if (!git.local.contains(git.current)) SelectableText('HEAD · ${git.current}'),
                ],
              );
            },
          ),
        ),
        actions: [TextButton(onPressed: () => Navigator.pop(ctx), child: Text(l.commonClose))],
      ),
    );
  }

  @override
  void dispose() {
    for (final folder in folders) {
      folder.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, theme = Theme.of(context);
    final folder = selected;
    Widget choose() => TextButton.icon(
      onPressed: busy ? null : _project,
      icon: const Icon(Icons.folder_open_outlined, size: 18),
      label: Text(l.editorOpen),
    );
    if (folder == null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.code_rounded, size: 36),
            const SizedBox(height: 12),
            Text(l.editorTitle, style: theme.textTheme.headlineSmall),
            const SizedBox(height: 8),
            Text(l.editorStart),
            const SizedBox(height: 16),
            Wrap(
              alignment: WrapAlignment.center,
              spacing: 12,
              children: [
                choose(),
                TextButton.icon(
                  onPressed: busy ? null : _remoteProject,
                  icon: const Icon(Icons.dns_outlined, size: 18),
                  label: Text(l.editorOpenSsh),
                ),
              ],
            ),
          ],
        ),
      );
    }
    return LayoutBuilder(
      builder: (context, box) {
        final compact = box.maxWidth < 650;
        return Column(
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
              child: GlassTabStrip(
                tabs: [
                  for (final item in folders)
                    GlassTab(
                      key: ObjectKey(item),
                      title: item.name,
                      subtitle: item.project is SshProject ? (item.project as SshProject).host.name : null,
                      tooltip: item.project.root,
                    ),
                ],
                activeIndex: folders.indexOf(folder),
                onSelect: (i) => setState(() => selected = folders[i]),
                onClose: busy ? null : (i) => _closeFolder(folders[i]),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (compact)
                      IconButton(
                        tooltip: l.editorFiles,
                        onPressed: () => setState(() => folder.showTree = !folder.showTree),
                        icon: const Icon(Icons.folder_outlined),
                      ),
                    if (!compact)
                      choose()
                    else
                      IconButton(
                        tooltip: l.editorOpen,
                        onPressed: busy ? null : _project,
                        icon: const Icon(Icons.folder_open_outlined),
                      ),
                    IconButton(
                      tooltip: l.editorOpenSsh,
                      onPressed: busy ? null : _remoteProject,
                      icon: const Icon(Icons.dns_outlined, size: 18),
                    ),
                    IconButton(
                      tooltip: l.editorNew,
                      onPressed: busy ? null : () => _newFile(folder),
                      icon: const Icon(Icons.note_add_outlined),
                    ),
                    IconButton(
                      tooltip: l.sftpRefresh,
                      onPressed: busy ? null : () => _refresh(folder),
                      icon: const Icon(Icons.refresh),
                    ),
                    IconButton(
                      tooltip: l.commonSave,
                      onPressed: busy || folder.active?.dirty != true ? null : () => _save(folder),
                      icon: const Icon(Icons.save_outlined),
                    ),
                  ],
                ),
              ),
            ),
            const Divider(height: 1),
            Expanded(
              child: IndexedStack(
                sizing: StackFit.expand,
                index: folders.indexOf(folder),
                children: [
                  for (final item in folders)
                    ExcludeFocus(
                      key: ObjectKey(item),
                      excluding: item != folder,
                      child: TickerMode(enabled: item == folder, child: _workspace(item, box, compact)),
                    ),
                ],
              ),
            ),
          ],
        );
      },
    );
  }

  Widget _workspace(_Folder folder, BoxConstraints box, bool compact) {
    final l = context.l10n;
    final current = folder.project;
    final active = folder.active;
    final tree = ProjectTree(
      project: current,
      active: active?.source.path,
      revision: folder.revision,
      onOpen: (path) => _run(() => _open(folder, path)),
    );
    return Column(
      children: [
        Expanded(
          child: Row(
            children: [
              if (!compact) ...[
                SizedBox(width: (box.maxWidth * 0.22).clamp(185, 300).toDouble(), child: tree),
                const VerticalDivider(width: 1),
              ],
              Expanded(
                child: Stack(
                  fit: StackFit.expand,
                  children: [
                    ProviderScope(
                      key: ObjectKey(current),
                      overrides: _terminalOverrides,
                      child: ProjectTerminal(
                        directory: current.root,
                        active: folder == selected,
                        host: current is SshProject ? current.host : null,
                        child: _codeArea(folder, l, Theme.of(context)),
                      ),
                    ),
                    if (compact)
                      Offstage(
                        offstage: !folder.showTree,
                        child: Material(color: Theme.of(context).colorScheme.surface, child: tree),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  active == null
                      ? current.paths.basename(current.root)
                      : current.paths.relative(active.source.path, from: current.root),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (!compact && active != null)
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Text(editorLanguage(active.name) ?? l.editorPlain),
                ),
              FutureBuilder<ProjectBranches?>(
                future: folder.branches,
                builder: (context, snap) => Tooltip(
                  message: l.editorBranches,
                  child: TextButton.icon(
                    onPressed: () => _git(folder),
                    icon: const Icon(Icons.account_tree_outlined, size: 16),
                    label: ConstrainedBox(
                      constraints: BoxConstraints(maxWidth: box.maxWidth * 0.32),
                      child: Text(snap.data?.current ?? l.editorBranches, overflow: TextOverflow.ellipsis),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _codeArea(_Folder folder, AppLocalizations l, ThemeData theme) {
    final doc = folder.active;
    final documents = folder.documents;
    if (doc == null) return Center(child: Text(l.editorPick));
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyS, control: true): () => _save(folder),
        const SingleActivator(LogicalKeyboardKey.keyS, meta: true): () => _save(folder),
      },
      child: Column(
        children: [
          SizedBox(
            height: 36,
            child: ListView(
              scrollDirection: Axis.horizontal,
              children: [
                for (final item in documents.values)
                  DecoratedBox(
                    decoration: BoxDecoration(
                      color: item == doc ? theme.colorScheme.surface : Colors.transparent,
                      border: Border(
                        bottom: BorderSide(
                          color: item == doc ? theme.colorScheme.primary : Colors.transparent,
                          width: 2,
                        ),
                      ),
                    ),
                    child: Row(
                      children: [
                        TextButton(
                          style: TextButton.styleFrom(
                            foregroundColor: item == doc
                                ? theme.colorScheme.onSurface
                                : theme.colorScheme.onSurfaceVariant,
                          ),
                          onPressed: () => setState(() => folder.active = item),
                          child: Tooltip(message: item.source.path, child: Text((item.dirty ? '• ' : '') + item.name)),
                        ),
                        IconButton(
                          icon: const Icon(Icons.close, size: 14),
                          tooltip: l.commonClose,
                          onPressed: busy ? null : () => _close(folder, item),
                        ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
          Expanded(
            child: IndexedStack(
              index: documents.values.toList().indexOf(doc),
              sizing: StackFit.expand,
              children: [
                for (final item in documents.values)
                  WorkspaceCodeEditor(
                    key: ObjectKey(item),
                    fieldKey: ValueKey(item.source.path),
                    path: item.name,
                    controller: item.controller,
                    maxBytes: editorFileLimit,
                    limitMessage: l.editorUnsupported,
                    onCopySelection: (value) async {
                      await Clipboard.setData(ClipboardData(text: value));
                      return true;
                    },
                    onChanged: (value) {
                      final dirty = value != item.saved;
                      if (dirty != item.dirty) setState(() => item.dirty = dirty);
                    },
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
