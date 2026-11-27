import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/bridge/mapping.dart' show hostFromJson, hostToJson;
import 'package:consolecrypt/core/glass/glass_tab_strip.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/native_drag_drop.dart';
import 'package:consolecrypt/core/widgets/sudo_password_dialog.dart';
import 'package:consolecrypt/editor/editor_language.dart';
import 'package:consolecrypt/editor/editor_resume.dart';
import 'package:consolecrypt/editor/editor_sftp_requests.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:consolecrypt/editor/project_move_dialog.dart';
import 'package:consolecrypt/editor/project_terminal.dart';
import 'package:consolecrypt/editor/project_tree.dart';
import 'package:consolecrypt/editor/ssh_folder_picker.dart';
import 'package:consolecrypt/editor/ssh_project.dart';
import 'package:consolecrypt/editor/workspace_code_editor.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:cryptography/cryptography.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/gestures.dart' show DragStartBehavior;
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path/path.dart' as p;

part 'editor_workspace_state.dart';

final editorFolderPickerProvider = Provider<Future<String?> Function()>((ref) => getDirectoryPath);

class _Document {
  _Document(this.files, this.source) : controller = TextEditingController(text: source.text), saved = source.text;
  ProjectFiles files;
  ProjectText source;
  String get name => files.paths.basename(source.path);
  final TextEditingController controller;
  String saved;
  bool dirty = false;
  bool markdownPreview = true;
}

class _Folder {
  _Folder(this.project) : branches = project.branches();
  final ProjectFiles project;
  final documents = <_Document>[];
  final expanded = <String>{};
  _Document? active;
  int revision = 0;
  ProjectChange? change;
  bool showTree = true;
  Future<ProjectBranches?> branches;

  String get name {
    final basename = project.paths.basename(project.root);
    return basename.isEmpty ? project.root : basename;
  }

  _Document? findDocument(ProjectFiles files, String path) =>
      documents.where((doc) => _sameHost(doc.files, files) && files.paths.equals(doc.source.path, path)).firstOrNull;

  bool matches(ProjectFiles other) => _sameHost(project, other) && project.paths.equals(project.root, other.root);

  void dispose() {
    for (final doc in documents) {
      doc.controller.dispose();
    }
    unawaited(project.close());
  }
}

bool _sameHost(ProjectFiles a, ProjectFiles b) =>
    a is SshProject ? b is SshProject && a.host.id == b.host.id : b is! SshProject;

String _errorMessage(AppLocalizations l, Object error) => switch (error) {
  AppException() => errorMessage(l, error),
  PathExistsException(:final path) => l.errorReasonAlreadyExists(p.basename(path ?? '')),
  EditorIssue.invalidName => l.editorInvalidName,
  EditorIssue.changed => l.editorChanged,
  EditorIssue.unsupported => l.editorUnsupported,
  EditorIssue.gitMissing => l.editorNoGitTool,
  EditorIssue.outside => l.editorOutside,
  EditorIssue.sudoUnavailable => l.editorSudoUnavailable,
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
  double? _treeWidth;
  final _terminalOverrides = [terminalTabsProvider.overrideWith(TerminalTabsController.new)];

  late final EditorResume resume;
  SecretKeyData? _resumeKey;
  Object? _resumeError;

  @override
  void initState() {
    super.initState();
    resume = ref.read(editorResumeProvider);
    ref.listenManual(editorSftpRequestsProvider, (_, requests) {
      if (requests.isNotEmpty) scheduleMicrotask(_handleSftpRequest);
    });
    ref.listenManual(editorCloseTabRequestsProvider, (_, _) {
      final folder = selected;
      final doc = folder?.active;
      if (folder != null && doc != null) unawaited(_close(folder, doc));
    });
    ref.listenManual(appStageProvider, (previous, next) {
      if (next != AppStage.unlocked) {
        final key = _resumeKey;
        _resumeKey = null;
        if (key == null) return;
        try {
          if (next == AppStage.locked && ref.read(activeProfileProvider)?.id == resume.owner) {
            if (folders.isEmpty) {
              resume.clear();
            } else {
              resume.save(key, _workspaceSnapshot(folders, selected));
            }
          }
        } finally {
          key.destroy();
        }
      }
    });
    busy = true;
    unawaited(_restoreWorkspace());
  }

  Future<void> _restoreWorkspace() async {
    final profile = resume.owner;
    if (profile == null) return;
    SecretKeyData? key;
    try {
      final vault = ref.read(vaultServiceProvider);
      final bytes = await vault.editorResumeKey(profile, resume.nonce);
      key = SecretKeyData(bytes, overwriteWhenDestroyed: true);
      if (!mounted ||
          !vault.currentStatus.isUnlocked ||
          ref.read(activeProfileProvider)?.id != profile ||
          ref.read(appStageProvider) != AppStage.unlocked) {
        return;
      }
      final state = resume.open(key);
      if (state != null) {
        folders.addAll(_restoreFolders(state, ref));
        selected = _atIndex(folders, state['selected'] as int);
        resume.clear();
      }
      _resumeKey = key;
      key = null;
      _resumeError = null;
    } catch (error) {
      if (mounted) _resumeError = error;
    } finally {
      key?.destroy();
      if (mounted) {
        setState(() => busy = false);
        _handleSftpRequest();
      }
    }
  }

  Future<void> _run(Future<void> Function() task) async {
    if (busy) return;
    setState(() => busy = true);
    try {
      await task();
    } catch (error) {
      if (mounted) showSnack(context, _errorMessage(context.l10n, error), error: true);
    } finally {
      if (mounted) {
        setState(() => busy = false);
        _handleSftpRequest();
      }
    }
  }

  void _handleSftpRequest() {
    if (!mounted || busy || _resumeKey == null || ref.read(editorSftpRequestsProvider).isEmpty) return;
    final request = ref.read(editorSftpRequestsProvider.notifier).takeFirst();
    unawaited(
      _run(() async {
        final path = request.path;
        _Folder? target;
        for (final folder in folders) {
          final project = folder.project;
          if (project is! SshProject ||
              project.host.id != request.host.id ||
              !project.paths.isWithin(project.root, path)) {
            continue;
          }
          if (folder.findDocument(project, path) != null) {
            target = folder;
            break;
          }
          if (target == null || project.root.length > target.project.root.length) target = folder;
        }
        if (target == null) {
          final project = SshProject.resume(
            p.posix.dirname(path),
            request.host,
            ref.read(sftpServiceProvider),
            ref.read(sftpBrowserServiceProvider),
          );
          _openFolder(project);
          target = selected!;
        } else {
          setState(() => selected = target);
        }
        await _open(target, path);
      }),
    );
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
    if (!await _discard(folder.documents) || !mounted) return;
    setState(() {
      final index = folders.indexOf(folder);
      folders.removeAt(index);
      if (selected == folder) selected = folders.isEmpty ? null : folders[index.clamp(0, folders.length - 1)];
    });
    WidgetsBinding.instance.addPostFrameCallback((_) => folder.dispose());
  });

  Future<void> _open(_Folder folder, String path, {ProjectFiles? origin}) async {
    final files = origin ?? folder.project;
    final existing = folder.findDocument(files, path);
    final doc = existing ?? _Document(files, await files.read(path));
    if (!mounted) {
      if (existing == null) doc.controller.dispose();
      return;
    }
    setState(() {
      if (existing == null) folder.documents.add(doc);
      folder.active = doc;
      folder.showTree = false;
    });
  }

  Future<void> _eachDroppedFile(List<String> paths, Future<void> Function(String) action) async {
    final errors = <String>[];
    for (final path in paths) {
      if (!mounted) return;
      try {
        await action(path);
      } catch (error) {
        if (mounted) errors.add('${p.basename(path)} — ${_errorMessage(context.l10n, error)}');
      }
    }
    if (errors.isNotEmpty && mounted) showSnack(context, errors.join('\n'), error: true);
  }

  Future<void> _openDropped(_Folder? workspace, List<String> paths) => _run(
    () => _eachDroppedFile(paths, (path) async {
      await requireRegularFile(path);
      final canonical = await File(path).resolveSymbolicLinks();
      if (!mounted) return;
      final parent = LocalProjectFiles(p.dirname(canonical));
      if (workspace == null) {
        _openFolder(parent);
        workspace = selected!;
      }
      final folder = workspace!, project = folder.project;
      final origin = project is LocalProjectFiles && project.paths.isWithin(project.root, canonical) ? project : parent;
      await _open(folder, canonical, origin: origin);
    }),
  );

  Future<void> _relocate(ProjectFiles project, ProjectEntry entry, String to) async {
    if (entry.path == to) return;
    final change = ProjectChange(entry, to);
    for (final tab in folders) {
      for (final doc in tab.documents.where((d) => _sameHost(d.files, project))) {
        final path = doc.source.path;
        if (!change.affects(project.paths, path) &&
            (project.paths.equals(to, path) || entry.directory && project.paths.isWithin(to, path))) {
          throw AppException(
            AppErrorCode.conflict,
            'Destination already open',
            reason: AppErrorReason.alreadyExists,
            args: {'name': project.paths.basename(path)},
          );
        }
      }
    }
    await project.relocate(entry, to);
    if (!mounted) return;
    for (final tab in folders) {
      final other = tab.project;
      if (_sameHost(project, other)) {
        other.root = change.remap(other.paths, other.root)!;
      }
      for (final doc in tab.documents.where((d) => _sameHost(d.files, project))) {
        if (!change.affects(project.paths, doc.source.path)) continue;
        final result = change.remap(project.paths, doc.source.path)!;
        doc.source = ProjectText(result, doc.source.bytes, doc.source.original);
        final original = doc.files;
        if (!original.paths.isWithin(original.root, result)) {
          final parent = original.paths.dirname(result);
          doc.files = original is SshProject ? original.at(parent) : LocalProjectFiles(parent);
        }
      }
      if (_sameHost(project, other)) {
        tab.change = change;
        _refresh(tab);
      }
    }
  }

  Future<void> _delete(ProjectFiles project, ProjectEntry entry) async {
    final change = ProjectChange(entry, null);
    final closing = folders
        .where((f) => _sameHost(f.project, project) && change.affects(project.paths, f.project.root))
        .toSet();
    final removing = <_Document>{
      for (final tab in folders)
        for (final doc in tab.documents)
          if (closing.contains(tab) || _sameHost(doc.files, project) && change.affects(project.paths, doc.source.path))
            doc,
    };
    final l = context.l10n;
    final dirty = removing.where((doc) => doc.dirty).map((doc) => doc.source.path).toSet();
    final confirmed = await showConfirmDialog(
      context,
      title: l.sftpDeleteTitle(project.paths.basename(entry.path)),
      message: [
        entry.directory ? l.editorDeleteFolder : l.editorDeleteFile,
        entry.path,
        if (dirty.isNotEmpty) ...[l.editorUnsaved, ...dirty],
      ].join('\n'),
      confirmLabel: l.commonDelete,
      destructive: true,
    );
    if (!mounted || !confirmed) return;
    await project.remove(entry);
    if (!mounted) return;
    folders.removeWhere(closing.contains);
    if (closing.contains(selected)) selected = folders.lastOrNull;
    final detached = <_Document>[];
    for (final tab in folders) {
      detached.addAll(tab.documents.where(removing.contains));
      tab.documents.removeWhere(removing.contains);
      if (removing.contains(tab.active)) tab.active = tab.documents.lastOrNull;
      if (_sameHost(project, tab.project)) {
        tab.change = change;
        _refresh(tab);
      }
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      for (final doc in detached) {
        doc.controller.dispose();
      }
      for (final tab in closing) {
        tab.dispose();
      }
    });
  }

  Future<void> _treeAction(_Folder folder, ProjectEntry entry, ProjectAction action) => _run(() async {
    final project = folder.project, paths = project.paths, l = context.l10n;
    final parent = paths.dirname(entry.path);
    switch (action) {
      case ProjectAction.open:
        await _open(folder, entry.path);
      case ProjectAction.copyPath:
      case ProjectAction.copyRelativePath:
        final text = action == ProjectAction.copyPath ? entry.path : paths.relative(entry.path, from: project.root);
        await copyPlainWithNotice(context, ref, text, what: l.sftpCopyWhatPath);
      case ProjectAction.delete:
        await _delete(project, entry);
      case ProjectAction.move:
        final directory = await showDialog<String>(
          context: context,
          builder: (_) => ProjectMoveDialog(project: project, entry: entry),
        );
        if (mounted && directory != null) {
          await _relocate(project, entry, paths.join(directory, paths.basename(entry.path)));
        }
      case ProjectAction.rename:
      case ProjectAction.duplicate:
      case ProjectAction.newFile:
      case ProjectAction.newFolder:
        final title = switch (action) {
          ProjectAction.rename => l.commonRename,
          ProjectAction.duplicate => l.sftpActionDuplicate,
          ProjectAction.newFolder => l.sftpNewFolder,
          _ => l.sftpActionNewFile,
        };
        final sibling = action == ProjectAction.rename || action == ProjectAction.duplicate;
        final basename = paths.basename(entry.path);
        final suffix = entry.directory ? '' : paths.extension(basename);
        final initial = action == ProjectAction.duplicate
            ? '${basename.substring(0, basename.length - suffix.length)}-copy$suffix'
            : basename;
        final name = await showTextInputDialog(
          context,
          title: title,
          label: l.commonName,
          initial: sibling ? initial : '',
          confirmLabel: sibling ? title : l.commonCreate,
        );
        if (!mounted || name == null) return;
        final target = project.namedChild(sibling || !entry.directory ? parent : entry.path, name);
        if (action == ProjectAction.rename) {
          await _relocate(project, entry, target);
        } else {
          if (action == ProjectAction.duplicate) {
            await project.duplicate(entry, name);
          } else if (action == ProjectAction.newFolder) {
            await project.createDirectory(target);
          } else {
            await project.createPath(target);
          }
          if (!mounted) return;
          for (final tab in folders.where(
            (f) => _sameHost(f.project, project) && paths.isWithin(f.project.root, target),
          )) {
            _refresh(tab);
          }
          if (action == ProjectAction.newFile) await _open(folder, target);
        }
    }
  });

  Future<void> _import(_Folder folder, List<String> paths, String directory) => _run(() async {
    await _eachDroppedFile(paths, (path) => folder.project.importFile(path, directory));
    if (mounted) _refresh(folder);
  });

  Widget _openDropTarget(_Folder? folder, Widget child) => OsFileDropTarget(
    key: const ValueKey('editor-open-drop'),
    enabled: !busy && selected == folder,
    onDrop: (paths, _) {
      if (mounted && selected == folder) _openDropped(folder, paths);
    },
    child: DragTarget<ProjectFileDrag>(
      onWillAcceptWithDetails: (d) => !busy && folder != null && identical(d.data.project, folder.project),
      onAcceptWithDetails: (d) => _run(() => _open(folder!, d.data.path)),
      builder: (_, _, _) => child,
    ),
  );

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
    if (doc != null && doc.dirty) await _writeDocument(doc);
  });

  Future<void> _writeDocument(_Document doc) async {
    final text = doc.controller.text;
    final files = doc.files;
    late final ProjectText result;
    try {
      result = await files.save(doc.source, text);
    } on AppException catch (error) {
      if (error.code != AppErrorCode.forbidden || files is! SshProject) rethrow;
      final saved = await _retrySudo(files, doc.source, text);
      if (saved == null) return;
      result = saved;
    }
    if (!mounted) return;
    setState(() {
      doc.source = result;
      doc.saved = text;
      doc.dirty = doc.controller.text != text;
    });
  }

  Future<ProjectText?> _retrySudo(SshProject files, ProjectText source, String text) async {
    bool current() =>
        mounted &&
        _resumeKey != null &&
        ref.read(activeProfileProvider)?.id == resume.owner &&
        ref.read(appStageProvider) == AppStage.unlocked;
    var retry = false;
    while (current()) {
      final secret = await showAppDialog<SecretText>(
        context,
        secure: true,
        builder: (_) => SudoPasswordDialog(host: files.host.name, path: source.path, retry: retry),
      );
      if (secret == null) return null;
      try {
        if (!current()) return null;
        return await files.save(source, text, sudoPassword: secret);
      } on AppException catch (error) {
        if (error.code == AppErrorCode.unsupported) throw EditorIssue.sudoUnavailable;
        if (error.code != AppErrorCode.forbidden) rethrow;
        retry = true;
      } finally {
        secret.wipe();
      }
    }
    return null;
  }

  Future<void> _close(_Folder folder, _Document doc) => _run(() async {
    if (doc.dirty) {
      final l = context.l10n;
      final save = await showConfirmDialog(
        context,
        title: l.automationUnsaved,
        message: doc.source.path,
        confirmLabel: l.commonSave,
      );
      if (!mounted || !save) return;
      await _writeDocument(doc);
      // The buffer can still be edited while a remote save is in flight.
      if (!mounted || doc.dirty) return;
    }
    setState(() {
      folder.documents.remove(doc);
      if (folder.active == doc) folder.active = folder.documents.lastOrNull;
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
    _resumeKey?.destroy();
    for (final folder in folders) {
      folder.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, theme = Theme.of(context);
    final folder = selected;
    if (_resumeError case final error?) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(_errorMessage(l, error)),
            TextButton.icon(
              onPressed: busy
                  ? null
                  : () {
                      setState(() => busy = true);
                      unawaited(_restoreWorkspace());
                    },
              icon: const Icon(Icons.refresh),
              label: Text(l.commonRetry),
            ),
          ],
        ),
      );
    }
    Widget choose() => TextButton.icon(
      onPressed: busy ? null : _project,
      icon: const Icon(Icons.folder_open_outlined, size: 18),
      label: Text(l.editorOpen),
    );
    if (folder == null) {
      return _openDropTarget(
        null,
        Center(
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
    const minTree = 140.0;
    final maxTree = (box.maxWidth - 320 - 12).clamp(minTree, double.infinity);
    final defaultTree = (box.maxWidth * .22).clamp(185.0, 300.0);
    double treeWidth() => (_treeWidth ?? defaultTree).clamp(minTree, maxTree);
    void resize(double delta) => setState(() => _treeWidth = (treeWidth() + delta).clamp(minTree, maxTree));
    final tree = ProjectTree(
      project: current,
      expandedPaths: folder.expanded,
      active: active != null && (active.files is SshProject) == (current is SshProject) ? active.source.path : null,
      enabled: !busy && folder == selected,
      change: folder.change,
      onAction: (entry, action) => _treeAction(folder, entry, action),
      onMove: (path, directory) => _run(
        () => _relocate(current, (
          path: path,
          directory: false,
        ), current.paths.join(directory, current.paths.basename(path))),
      ),
      onImport: (paths, directory) => _import(folder, paths, directory),
      revision: folder.revision,
      onOpen: (path) => _run(() => _open(folder, path)),
    );
    return Column(
      children: [
        Expanded(
          child: Row(
            children: [
              if (!compact) ...[
                SizedBox(width: treeWidth(), child: tree),
                Semantics(
                  label: l.keepassResizeGroups,
                  onIncrease: () => resize(24),
                  onDecrease: () => resize(-24),
                  child: MouseRegion(
                    cursor: SystemMouseCursors.resizeColumn,
                    child: GestureDetector(
                      key: const ValueKey('editor-tree-divider'),
                      behavior: HitTestBehavior.opaque,
                      dragStartBehavior: DragStartBehavior.down,
                      onHorizontalDragUpdate: (d) => resize(d.delta.dx),
                      child: const VerticalDivider(width: 12, thickness: 1),
                    ),
                  ),
                ),
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
                        child: _openDropTarget(folder, _codeArea(folder, l, Theme.of(context))),
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
                      : identical(active.files, current)
                      ? current.paths.relative(active.source.path, from: current.root)
                      : active.source.path,
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
                for (final item in documents)
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
              index: documents.indexOf(doc),
              sizing: StackFit.expand,
              children: [
                for (final item in documents)
                  WorkspaceCodeEditor(
                    key: ObjectKey(item),
                    fieldKey: ValueKey(item.source.path),
                    path: item.name,
                    initialMarkdownPreview: item.markdownPreview,
                    onMarkdownPreview: (value) => item.markdownPreview = value,
                    onOpenFileLink: (href) => _run(() async {
                      final origin = item.files;
                      final path = resolveMarkdownFile(origin.paths, origin.root, item.source.path, href);
                      if (path != null) await _open(folder, path, origin: origin);
                    }),
                    controller: item.controller,
                    maxBytes: editorFileLimit,
                    limitMessage: l.editorUnsupported,
                    onCopySelection: (value) async {
                      try {
                        await Clipboard.setData(ClipboardData(text: value));
                        return true;
                      } on PlatformException catch (error) {
                        if (mounted) showSnack(context, _errorMessage(l, error), error: true);
                        return false;
                      }
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
