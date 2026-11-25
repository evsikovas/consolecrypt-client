import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/editor/editor_language.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:consolecrypt/editor/project_terminal.dart';
import 'package:consolecrypt/editor/project_tree.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path/path.dart' as p;
import 'package:re_editor/re_editor.dart';
import 'package:re_highlight/languages/all.dart';
import 'package:re_highlight/styles/atom-one-dark.dart';
import 'package:re_highlight/styles/atom-one-light.dart';

final editorFolderPickerProvider = Provider<Future<String?> Function()>((ref) => getDirectoryPath);

class _Document {
  _Document(this.source) : controller = CodeLineEditingController.fromText(source.text) {
    saved = controller.codeLines;
  }
  ProjectText source;
  final CodeLineEditingController controller;
  late CodeLines saved;
  bool dirty = false;
  String get name => p.basename(source.path);
}

String _errorMessage(AppLocalizations l, Object error) => switch (error) {
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
      branches = ProjectBranches.read(project!.root);
    });
  }

  Future<void> _project() => _run(() async {
    final path = await ref.read(editorFolderPickerProvider)();
    if (!mounted || path == null) return;
    final next = await ProjectFiles.open(path);
    if (!mounted || (project != null && p.equals(project!.root, next.root))) return;
    if (!await _discard(documents.values) || !mounted) return;
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
  });

  Future<void> _open(String path) async {
    final doc = documents[path] ?? _Document(await project!.read(path));
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
    final location = await getSaveLocation(initialDirectory: project!.root);
    if (!mounted || location == null) return;
    final file = await project!.create(location.path);
    if (!mounted) return;
    _refresh();
    await _open(file.path);
  });

  Future<void> _save() => _run(() async {
    final doc = active;
    if (doc == null || !doc.dirty) return;
    final lines = doc.controller.codeLines;
    final result = await project!.save(doc.source, doc.controller.text);
    if (!mounted) return;
    setState(() {
      doc.source = result;
      doc.saved = lines;
      doc.dirty = !doc.controller.codeLines.equals(lines);
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
      icon: const Icon(Icons.folder_open_outlined, size: 19),
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
            const SizedBox(height: 14),
            choose(),
          ],
        ),
      );
    }
    return LayoutBuilder(
      builder: (context, box) {
        final compact = box.maxWidth < 650;
        final tree = ProjectTree(
          key: ValueKey(current.root),
          project: current,
          active: active?.source.path,
          revision: revision,
          onOpen: (path) => _run(() => _open(path)),
        );
        return Column(
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
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
                      l.editorTitle,
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
                          key: ValueKey(current.root),
                          overrides: _terminalOverrides,
                          child: ProjectTerminal(directory: current.root, child: _codeArea(l, theme)),
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
              padding: const EdgeInsets.symmetric(horizontal: 10),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      active == null ? p.basename(current.root) : p.relative(active!.source.path, from: current.root),
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
    final lang = editorLanguage(doc.name), mode = builtinAllLanguages[lang];
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyS, control: true): _save,
        const SingleActivator(LogicalKeyboardKey.keyS, meta: true): _save,
      },
      child: Column(
        children: [
          SizedBox(
            height: 38,
            child: ListView(
              scrollDirection: Axis.horizontal,
              children: [
                for (final item in documents.values)
                  DecoratedBox(
                    decoration: BoxDecoration(
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
            child: CodeEditor(
              key: ValueKey(doc.source.path),
              controller: doc.controller,
              wordWrap: false,
              padding: const EdgeInsets.fromLTRB(8, 10, 12, 12),
              style: CodeEditorStyle(
                fontFamily: AppPlatform.monospaceFamily,
                fontFamilyFallback: AppPlatform.monospaceFallback,
                fontSize: 14,
                fontHeight: 1.45,
                textColor: theme.colorScheme.onSurface,
                backgroundColor: theme.colorScheme.surface,
                codeTheme: mode == null || lang == null
                    ? null
                    : CodeHighlightTheme(
                        languages: {lang: CodeHighlightThemeMode(mode: mode)},
                        theme: theme.brightness == Brightness.dark ? atomOneDarkTheme : atomOneLightTheme,
                      ),
              ),
              indicatorBuilder: (context, controller, chunks, notifier) =>
                  DefaultCodeLineNumber(controller: controller, notifier: notifier),
              onChanged: (value) {
                final dirty = !value.codeLines.equals(doc.saved);
                if (dirty != doc.dirty) setState(() => doc.dirty = dirty);
              },
            ),
          ),
        ],
      ),
    );
  }
}
