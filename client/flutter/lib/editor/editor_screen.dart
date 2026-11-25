import 'dart:async';

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
  ProjectFiles? project;
  final documents = <String, _Document>{};
  _Document? active;
  int revision = 0;
  bool busy = false, showTree = false;
  Future<ProjectBranches?>? branches;
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

  void _refresh() {
    setState(() {
      revision++;
      branches = project!.branches();
    });
  }

  Future<void> _project() => _run(() async {
    final path = await ref.read(editorFolderPickerProvider)();
    if (!mounted || path == null) return;
    final next = await ProjectFiles.open(path);
    await _switchProject(next);
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
      adopted = await _switchProject(next);
    } finally {
      if (!adopted) await sftp.disconnect(session);
    }
  });

  Future<bool> _switchProject(ProjectFiles next) async {
    final previous = project;
    if (!mounted ||
        previous != null &&
            previous.runtimeType == next.runtimeType &&
            next.paths.equals(previous.root, next.root) &&
            (next is! SshProject || previous is SshProject && previous.host.id == next.host.id)) {
      return false;
    }
    if (!await _discard(documents.values) || !mounted) return false;
    final old = documents.values.toList();
    setState(() {
      documents.clear();
      active = null;
      project = next;
      showTree = true;
    });
    _refresh();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      for (final doc in old) {
        doc.controller.dispose();
      }
    });
    if (previous != null) unawaited(previous.close());
    return true;
  }

  Future<void> _open(String path) async {
    final doc = documents[path] ?? _Document(await project!.read(path), project!.paths.basename(path));
    if (!mounted) {
      if (!documents.containsKey(path)) doc.controller.dispose();
      return;
    }
    setState(() {
      documents[path] = doc;
      active = doc;
      showTree = false;
    });
  }

  Future<void> _newFile() => _run(() async {
    final current = project!;
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
    _refresh();
    await _open(file);
  });

  Future<void> _save() => _run(() async {
    final doc = active;
    if (doc == null || !doc.dirty) return;
    final text = doc.controller.text;
    final result = await project!.save(doc.source, text);
    if (!mounted) return;
    setState(() {
      doc.source = result;
      doc.saved = text;
      doc.dirty = doc.controller.text != text;
    });
  });

  Future<void> _close(_Document doc) => _run(() async {
    if (!await _discard([doc]) || !mounted) return;
    setState(() {
      documents.remove(doc.source.path);
      if (active == doc) active = documents.values.lastOrNull;
    });
    WidgetsBinding.instance.addPostFrameCallback((_) => doc.controller.dispose());
  });

  Future<void> _git() async {
    _refresh();
    final loaded = branches!;
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
    final current = project;
    if (current != null) unawaited(current.close());
    for (final doc in documents.values) {
      doc.controller.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, theme = Theme.of(context);
    final current = project;
    Widget choose() => TextButton.icon(
      onPressed: busy ? null : _project,
      icon: const Icon(Icons.folder_open_outlined, size: 18),
      label: Text(l.editorOpen),
    );
    if (current == null) {
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
        final tree = ProjectTree(
          key: ObjectKey(current),
          project: current,
          active: active?.source.path,
          revision: revision,
          onOpen: (path) => _run(() => _open(path)),
        );
        return Column(
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
              child: Row(
                children: [
                  if (compact)
                    IconButton(
                      tooltip: l.editorFiles,
                      onPressed: () => setState(() => showTree = !showTree),
                      icon: const Icon(Icons.folder_outlined),
                    ),
                  Expanded(
                    child: Text(
                      current is SshProject ? current.host.name : l.editorTitle,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.titleMedium,
                    ),
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
                    onPressed: busy ? null : _newFile,
                    icon: const Icon(Icons.note_add_outlined),
                  ),
                  IconButton(
                    tooltip: l.sftpRefresh,
                    onPressed: busy ? null : _refresh,
                    icon: const Icon(Icons.refresh),
                  ),
                  IconButton(
                    tooltip: l.commonSave,
                    onPressed: busy || active?.dirty != true ? null : _save,
                    icon: const Icon(Icons.save_outlined),
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
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
                            host: current is SshProject ? current.host : null,
                            child: _codeArea(l, theme),
                          ),
                        ),
                        if (compact && showTree) Material(color: theme.colorScheme.surface, child: tree),
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
                          : current.paths.relative(active!.source.path, from: current.root),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  if (!compact && active != null)
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 16),
                      child: Text(editorLanguage(active!.name) ?? l.editorPlain),
                    ),
                  FutureBuilder<ProjectBranches?>(
                    future: branches,
                    builder: (context, snap) => Tooltip(
                      message: l.editorBranches,
                      child: TextButton.icon(
                        onPressed: _git,
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
      },
    );
  }

  Widget _codeArea(AppLocalizations l, ThemeData theme) {
    final doc = active;
    if (doc == null) return Center(child: Text(l.editorPick));
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyS, control: true): _save,
        const SingleActivator(LogicalKeyboardKey.keyS, meta: true): _save,
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
                          onPressed: () => setState(() => active = item),
                          child: Tooltip(message: item.source.path, child: Text((item.dirty ? '• ' : '') + item.name)),
                        ),
                        IconButton(
                          icon: const Icon(Icons.close, size: 14),
                          tooltip: l.commonClose,
                          onPressed: busy ? null : () => _close(item),
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
