import 'dart:async';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/sftp/sftp_rows.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

const _keep = Object();

/// State of the SFTP browser: one remote session, the directory cache of
/// the current folder and its inline-expanded subfolders, selection, and
/// the optional local pane.
final class SftpState {
  const SftpState({
    this.host,
    this.session,
    this.connecting = false,
    this.error,
    this.home = '',
    this.remotePath = '',
    this.dirs = const {},
    this.expanded = const {},
    this.loading = const {},
    this.selection = const {},
    this.anchor,
    this.cursor,
    this.filter = '',
    this.renaming,
    this.prefs = const SftpBrowserPreferences(),
    this.localPath = '',
    this.local = const [],
    this.localSelection = const {},
  });

  final Host? host;
  final SftpSessionId? session;
  final bool connecting;

  /// Last connection / listing failure (shown as a banner).
  final AppException? error;
  final String home;
  final String remotePath;

  /// Directory cache: path → entries (unsorted, as listed).
  final Map<String, List<RemoteFileInfo>> dirs;

  /// Folders expanded inline (disclosure triangles).
  final Set<String> expanded;

  /// Folders being listed.
  final Set<String> loading;

  /// Selected remote paths.
  final Set<String> selection;

  /// Start of a Shift range.
  final String? anchor;

  /// Keyboard focus row.
  final String? cursor;

  /// Search field text (filters the current folder).
  final String filter;

  /// Path whose name is being edited inline.
  final String? renaming;
  final SftpBrowserPreferences prefs;
  final String localPath;
  final List<FileEntry> local;
  final Set<String> localSelection;

  bool get isConnected => session != null;

  List<RemoteFileInfo> get entries => dirs[remotePath] ?? const [];

  bool get isLoadingCurrent => loading.contains(remotePath) && !dirs.containsKey(remotePath);

  /// Every loaded entry by path (current folder + expanded subfolders).
  Map<String, RemoteFileInfo> get entriesByPath => {
    for (final list in dirs.values)
      for (final e in list) e.path: e,
  };

  List<RemoteFileInfo> get selectedEntries {
    final byPath = entriesByPath;
    return [for (final p in selection) ?byPath[p]];
  }

  SftpState copyWith({
    Object? host = _keep,
    Object? session = _keep,
    bool? connecting,
    Object? error = _keep,
    String? home,
    String? remotePath,
    Map<String, List<RemoteFileInfo>>? dirs,
    Set<String>? expanded,
    Set<String>? loading,
    Set<String>? selection,
    Object? anchor = _keep,
    Object? cursor = _keep,
    String? filter,
    Object? renaming = _keep,
    SftpBrowserPreferences? prefs,
    String? localPath,
    List<FileEntry>? local,
    Set<String>? localSelection,
  }) => SftpState(
    host: identical(host, _keep) ? this.host : host as Host?,
    session: identical(session, _keep) ? this.session : session as SftpSessionId?,
    connecting: connecting ?? this.connecting,
    error: identical(error, _keep) ? this.error : error as AppException?,
    home: home ?? this.home,
    remotePath: remotePath ?? this.remotePath,
    dirs: dirs ?? this.dirs,
    expanded: expanded ?? this.expanded,
    loading: loading ?? this.loading,
    selection: selection ?? this.selection,
    anchor: identical(anchor, _keep) ? this.anchor : anchor as String?,
    cursor: identical(cursor, _keep) ? this.cursor : cursor as String?,
    filter: filter ?? this.filter,
    renaming: identical(renaming, _keep) ? this.renaming : renaming as String?,
    prefs: prefs ?? this.prefs,
    localPath: localPath ?? this.localPath,
    local: local ?? this.local,
    localSelection: localSelection ?? this.localSelection,
  );
}

/// `name.ext` → (`name`, `.ext`); dot-files and `.tar.*` handled.
(String, String) splitFileName(String name) {
  final tar = RegExp(r'\.tar\.[a-z0-9]+$', caseSensitive: false).firstMatch(name);
  if (tar != null && tar.start > 0) return (name.substring(0, tar.start), name.substring(tar.start));
  final dot = name.lastIndexOf('.');
  if (dot <= 0) return (name, '');
  return (name.substring(0, dot), name.substring(dot));
}

/// Parent of a local path (POSIX or Windows separators).
String parentLocalPath(String path) {
  final trimmed = path.length > 1 && (path.endsWith('/') || path.endsWith(r'\'))
      ? path.substring(0, path.length - 1)
      : path;
  final idx = trimmed.lastIndexOf(RegExp(r'[\\/]'));
  if (idx < 0) return trimmed;
  if (idx == 0) return trimmed.substring(0, 1);
  if (idx == 2 && trimmed[1] == ':') return trimmed.substring(0, 3); // C:\
  return trimmed.substring(0, idx);
}

bool _isUnder(String path, String dir) => dir == '/' ? path.startsWith('/') : (path == dir || path.startsWith('$dir/'));

/// SFTP browser controller (one remote session at a time). Screens open a
/// host with [connect]; the hosts and terminal screens call it too.
class SftpController extends Notifier<SftpState> {
  final Set<TransferId> _seenCompleted = {};
  int _generation = 0;

  @override
  SftpState build() {
    // Refresh the destination when a transfer finishes.
    ref.listen(transfersProvider, (_, next) {
      for (final job in next.value ?? const <TransferJob>[]) {
        if (job.state != TransferState.completed || !_seenCompleted.add(job.id)) continue;
        if (job.direction == TransferDirection.upload) {
          final dir = parentRemotePath(job.destinationPath);
          if (state.dirs.containsKey(dir)) unawaited(_load(dir));
        } else if (parentLocalPath(job.destinationPath) == state.localPath) {
          unawaited(refreshLocal());
        }
      }
    });
    ref.listen(activeProfileProvider.select((p) => p?.id), (prev, next) {
      if (prev != next) unawaited(disconnect());
    });
    return const SftpState();
  }

  SftpService get _sftp => ref.read(sftpServiceProvider);

  SftpBrowserService get _browser => ref.read(sftpBrowserServiceProvider);

  SftpSessionId _requireSession() {
    final session = state.session;
    if (session == null) {
      throw const AppException(AppErrorCode.notFound, 'Not connected', reason: AppErrorReason.sessionClosed);
    }
    return session;
  }

  // Connection -------------------------------------------------------------------

  Future<void> connect(Host host) async {
    await disconnect();
    final generation = ++_generation;
    state = SftpState(host: host, connecting: true, prefs: state.prefs);
    try {
      final session = await _sftp.connect(host.id);
      if (generation != _generation) {
        await _sftp.disconnect(session);
        return;
      }
      final home = await _sftp.remoteHome(session);
      final localHome = await _sftp.localHome();
      final prefs = await _browser.loadPreferences();
      if (generation != _generation) return;
      state = state.copyWith(
        session: session,
        home: home,
        remotePath: home,
        localPath: localHome,
        prefs: prefs,
        connecting: false,
      );
      await Future.wait([_load(home), refreshLocal()]);
    } on AppException catch (e) {
      if (generation == _generation) state = state.copyWith(connecting: false, error: e);
    }
  }

  Future<void> disconnect() async {
    _generation++;
    final session = state.session;
    state = SftpState(prefs: state.prefs);
    if (session != null) await _sftp.disconnect(session);
  }

  void clearError() => state = state.copyWith(error: null);

  // Listing ------------------------------------------------------------------------

  Future<void> _load(String path) async {
    final session = state.session;
    if (session == null) return;
    final generation = _generation;
    state = state.copyWith(loading: {...state.loading, path});
    try {
      final entries = await _browser.listDirectory(session, path);
      if (generation != _generation) return;
      state = state.copyWith(
        dirs: {...state.dirs, path: entries},
        loading: {...state.loading}..remove(path),
        error: path == state.remotePath ? null : state.error,
      );
    } on AppException catch (e) {
      if (generation != _generation) return;
      state = state.copyWith(
        dirs: {...state.dirs}..remove(path),
        loading: {...state.loading}..remove(path),
        expanded: {...state.expanded}..remove(path),
        error: e,
      );
    }
  }

  /// Reloads the current folder and its expanded subfolders.
  Future<void> refresh() async {
    if (!state.isConnected) return;
    final targets = {
      state.remotePath,
      for (final p in state.expanded)
        if (state.dirs.containsKey(p) && _isUnder(p, state.remotePath)) p,
    };
    // Drop folders outside the view so they are listed afresh when visited.
    state = state.copyWith(
      dirs: {
        for (final e in state.dirs.entries)
          if (targets.contains(e.key)) e.key: e.value,
      },
    );
    await Future.wait(targets.map(_load));
    _pruneSelection();
  }

  void _pruneSelection() {
    final known = state.entriesByPath;
    final kept = state.selection.where(known.containsKey).toSet();
    if (kept.length != state.selection.length) {
      state = state.copyWith(
        selection: kept,
        cursor: known.containsKey(state.cursor) ? state.cursor : null,
        anchor: known.containsKey(state.anchor) ? state.anchor : null,
      );
    }
  }

  Future<void> _reloadParents(Iterable<String> paths) async {
    final dirs = {for (final p in paths) parentRemotePath(p)}.where(state.dirs.containsKey);
    await Future.wait(dirs.map(_load));
    _pruneSelection();
  }

  Future<void> navigateTo(String path, {String? select}) async {
    if (!state.isConnected) return;
    state = state.copyWith(
      remotePath: path,
      selection: select == null ? {} : {select},
      anchor: select,
      cursor: select,
      filter: '',
      renaming: null,
    );
    await _load(path);
  }

  /// Parent folder; the folder we came from stays selected (like Finder).
  Future<void> goUp() async {
    final current = state.remotePath;
    if (current == '/' || current.isEmpty) return;
    await navigateTo(parentRemotePath(current), select: current);
  }

  /// Resolves a typed path (`~`, relative, `..`) and opens it.
  Future<void> goToTypedPath(String input) async {
    final session = _requireSession();
    final path = await _browser.resolveDirectory(session, input, base: state.remotePath);
    await navigateTo(path);
  }

  /// Folder names under the directory part of [input] (path autocomplete).
  Future<List<String>> completePath(String input) async {
    final session = state.session;
    if (session == null || input.isEmpty) return const [];
    final slash = input.lastIndexOf('/');
    final dir = slash <= 0 ? '/' : input.substring(0, slash);
    final prefix = input.substring(slash + 1).toLowerCase();
    if (!dir.startsWith('/')) return const [];
    try {
      final entries = state.dirs[dir] ?? await _browser.listDirectory(session, dir);
      return [
        for (final e in entries)
          if (e.isDirectory && e.name.toLowerCase().startsWith(prefix) && (prefix.startsWith('.') || !e.isHidden))
            joinRemotePath(dir, e.name),
      ]..sort(compareNatural);
    } on AppException {
      return const [];
    }
  }

  void toggleExpanded(RemoteFileInfo entry) {
    if (!entry.isDirectory) return;
    if (state.expanded.contains(entry.path)) {
      state = state.copyWith(expanded: {...state.expanded}..remove(entry.path));
    } else {
      state = state.copyWith(expanded: {...state.expanded, entry.path});
      unawaited(_load(entry.path));
    }
  }

  void setExpanded(RemoteFileInfo entry, {required bool expanded}) {
    if (state.expanded.contains(entry.path) != expanded) toggleExpanded(entry);
  }

  // Selection ------------------------------------------------------------------------

  void selectOnly(String path) =>
      state = state.copyWith(selection: {path}, anchor: path, cursor: path, localSelection: const {});

  void toggleSelection(String path) {
    final next = {...state.selection};
    if (!next.remove(path)) next.add(path);
    state = state.copyWith(selection: next, anchor: path, cursor: path);
  }

  /// Shift-click / Shift-arrow: everything between the anchor and [path] in
  /// the visible [order].
  void selectRange(String path, List<String> order) {
    final anchor = state.anchor ?? state.cursor ?? path;
    final a = order.indexOf(anchor);
    final b = order.indexOf(path);
    if (a < 0 || b < 0) return selectOnly(path);
    final (from, to) = a <= b ? (a, b) : (b, a);
    state = state.copyWith(selection: order.sublist(from, to + 1).toSet(), anchor: anchor, cursor: path);
  }

  void selectAll(List<String> order) =>
      state = state.copyWith(selection: order.toSet(), cursor: state.cursor ?? order.firstOrNull);

  void clearSelection() => state = state.copyWith(selection: {}, anchor: null, cursor: null);

  void setFilter(String filter) => state = state.copyWith(filter: filter);

  // Preferences ------------------------------------------------------------------------

  void _setPrefs(SftpBrowserPreferences prefs) {
    state = state.copyWith(prefs: prefs);
    unawaited(_browser.savePreferences(prefs));
  }

  void setSort(SftpListColumn column) {
    final p = state.prefs;
    _setPrefs(
      p.sortColumn == column
          ? p.copyWith(sortAscending: !p.sortAscending)
          : p.copyWith(sortColumn: column, sortAscending: true),
    );
  }

  void toggleFoldersFirst() => _setPrefs(state.prefs.copyWith(foldersFirst: !state.prefs.foldersFirst));

  void toggleShowHidden() {
    _setPrefs(state.prefs.copyWith(showHidden: !state.prefs.showHidden));
    if (!state.prefs.showHidden) {
      final hidden = state.entriesByPath.values.where((e) => e.isHidden).map((e) => e.path).toSet();
      state = state.copyWith(selection: state.selection.difference(hidden));
    }
  }

  void toggleLocalPane() => _setPrefs(state.prefs.copyWith(showLocalPane: !state.prefs.showLocalPane));

  void acknowledgeEditHint() => _setPrefs(state.prefs.copyWith(editHintAcknowledged: true));

  void toggleColumn(SftpListColumn column) {
    if (column == SftpListColumn.name) return;
    final visible = {...state.prefs.visibleColumns};
    if (!visible.remove(column)) visible.add(column);
    _setPrefs(state.prefs.copyWith(visibleColumns: visible));
  }

  void resetColumns() => _setPrefs(
    state.prefs.copyWith(
      columnOrder: SftpBrowserPreferences.defaultColumnOrder,
      visibleColumns: SftpBrowserPreferences.defaultVisibleColumns,
      columnWidths: const {},
    ),
  );

  void setColumnWidth(SftpListColumn column, double width) {
    if (column == SftpListColumn.name) return;
    final clamped = width.clamp(SftpBrowserPreferences.minColumnWidth, SftpBrowserPreferences.maxColumnWidth);
    _setPrefs(state.prefs.copyWith(columnWidths: {...state.prefs.columnWidths, column: clamped.toDouble()}));
  }

  /// Moves [column] in front of [before] (Name stays first).
  void moveColumn(SftpListColumn column, SftpListColumn before) {
    if (column == SftpListColumn.name || column == before) return;
    final order = [...state.prefs.columnOrder]..remove(column);
    final index = before == SftpListColumn.name ? 1 : order.indexOf(before);
    order.insert(index < 1 ? 1 : index, column);
    _setPrefs(state.prefs.copyWith(columnOrder: order));
  }

  // File operations (throw AppException; the UI shows feedback) ---------------------------

  /// A free name in [dir] based on [base]: `base`, `base 2`, `base 3`… (the
  /// extension is kept).
  String uniqueName(String dir, String base) {
    final taken = {for (final e in state.dirs[dir] ?? const <RemoteFileInfo>[]) e.name};
    if (!taken.contains(base)) return base;
    final (stem, ext) = splitFileName(base);
    for (var i = 2; ; i++) {
      final candidate = '$stem $i$ext';
      if (!taken.contains(candidate)) return candidate;
    }
  }

  /// Creates a folder named like [baseName] in the current folder and
  /// starts renaming it inline. Returns its path.
  Future<String> createFolder(String baseName) async {
    final session = _requireSession();
    final dir = state.remotePath;
    final path = joinRemotePath(dir, uniqueName(dir, baseName));
    await _sftp.makeRemoteDirectory(session, path);
    await _load(dir);
    selectOnly(path);
    startRename(path);
    return path;
  }

  Future<String> createFile(String baseName) async {
    final session = _requireSession();
    final dir = state.remotePath;
    final path = joinRemotePath(dir, uniqueName(dir, baseName));
    await _browser.createFile(session, path);
    await _load(dir);
    selectOnly(path);
    startRename(path);
    return path;
  }

  void startRename(String path) =>
      state = state.copyWith(renaming: path, selection: {path}, anchor: path, cursor: path);

  void cancelRename() => state = state.copyWith(renaming: null);

  /// Validates and applies an inline rename; returns the new path. Throws
  /// `validation` (+ `invalidField` name/format) for an empty name or one
  /// containing `/`, `conflict` + `alreadyExists` if taken.
  Future<String> commitRename(String path, String newName) async {
    final name = newName.trim();
    state = state.copyWith(renaming: null);
    final oldName = path.split('/').last;
    if (name == oldName) return path;
    if (name.isEmpty || name.contains('/') || name == '.' || name == '..') {
      throw const AppException(
        AppErrorCode.validation,
        'invalid_file_name',
        reason: AppErrorReason.invalidField,
        args: {'field': 'name', 'rule': 'format'},
      );
    }
    final dir = parentRemotePath(path);
    if ((state.dirs[dir] ?? const <RemoteFileInfo>[]).any((e) => e.name == name)) {
      throw AppException(
        AppErrorCode.conflict,
        'name_taken',
        reason: AppErrorReason.alreadyExists,
        args: {'name': name},
      );
    }
    final target = joinRemotePath(dir, name);
    await _sftp.renameRemote(_requireSession(), path, target);
    final expanded = {for (final p in state.expanded) _isUnder(p, path) ? target + p.substring(path.length) : p};
    state = state.copyWith(expanded: expanded);
    await _load(dir);
    selectOnly(target);
    return target;
  }

  /// Copies each path next to itself as `<stem> <suffix><ext>` (unique).
  Future<List<String>> duplicate(List<String> paths, String suffix) async {
    final session = _requireSession();
    final created = <String>[];
    for (final path in paths) {
      final dir = parentRemotePath(path);
      final (stem, ext) = splitFileName(path.split('/').last);
      final target = joinRemotePath(dir, uniqueName(dir, '$stem $suffix$ext'));
      await _browser.duplicate(session, path, target);
      created.add(target);
      await _load(dir);
    }
    if (created.isNotEmpty) {
      state = state.copyWith(selection: created.toSet(), anchor: created.last, cursor: created.last);
    }
    return created;
  }

  Future<void> delete(List<RemoteFileInfo> entries) async {
    final session = _requireSession();
    try {
      for (final e in entries) {
        await _sftp.deleteRemote(session, e.path, recursive: e.kind == RemoteEntryKind.directory);
      }
    } finally {
      state = state.copyWith(
        selection: state.selection.difference({for (final e in entries) e.path}),
        expanded: state.expanded.where((p) => !entries.any((e) => _isUnder(p, e.path))).toSet(),
      );
      await _reloadParents(entries.map((e) => e.path));
    }
  }

  Future<void> setPermissions(String path, int mode) async {
    await _browser.setPermissions(_requireSession(), path, mode);
    await _reloadParents([path]);
  }

  /// In-list drag: moves [paths] into [targetDirectory] (skips no-ops and
  /// moves of a folder into itself). Returns the number moved.
  Future<int> move(List<String> paths, String targetDirectory) async {
    final session = _requireSession();
    var moved = 0;
    final touched = <String>{targetDirectory};
    for (final path in paths) {
      if (parentRemotePath(path) == targetDirectory || _isUnder(targetDirectory, path)) continue;
      await _sftp.renameRemote(session, path, joinRemotePath(targetDirectory, path.split('/').last));
      touched.add(parentRemotePath(path));
      moved++;
    }
    await Future.wait(touched.where(state.dirs.containsKey).map(_load));
    _pruneSelection();
    return moved;
  }

  Future<List<TransferId>> upload(List<String> localPaths, String remoteDirectory) =>
      _browser.uploadItems(_requireSession(), localPaths, remoteDirectory);

  Future<List<TransferId>> download(List<String> remotePaths, String localDirectory) =>
      _browser.downloadItems(_requireSession(), remotePaths, localDirectory);

  // Local pane -------------------------------------------------------------------------

  Future<void> refreshLocal() async {
    if (state.localPath.isEmpty) return;
    try {
      final entries = await _sftp.listLocal(state.localPath);
      state = state.copyWith(local: entries);
    } on AppException catch (e) {
      state = state.copyWith(error: e);
    }
  }

  Future<void> openLocal(String path) async {
    state = state.copyWith(localPath: path, localSelection: const {});
    await refreshLocal();
  }

  void selectLocal(String path, {bool toggle = false}) {
    final next = toggle ? ({...state.localSelection}..toggle(path)) : {path};
    state = state.copyWith(localSelection: next, selection: toggle ? state.selection : const {});
  }

  Future<List<TransferId>> uploadLocalSelection() => upload(state.localSelection.toList(), state.remotePath);
}

extension on Set<String> {
  void toggle(String value) {
    if (!remove(value)) add(value);
  }
}

final sftpControllerProvider = NotifierProvider<SftpController, SftpState>(SftpController.new);

/// Visible rows of the remote list (sorted, filtered, inline tree).
final sftpRowsProvider = Provider<List<SftpRow>>((ref) {
  final s = ref.watch(sftpControllerProvider);
  return buildSftpRows(
    root: s.remotePath,
    dirs: s.dirs,
    expanded: s.expanded,
    loading: s.loading,
    prefs: s.prefs,
    filter: s.filter,
  );
});
