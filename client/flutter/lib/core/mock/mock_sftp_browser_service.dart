import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_inventory_service.dart';
import 'package:consolecrypt/core/mock/mock_sftp_fs.dart';
import 'package:consolecrypt/core/mock/mock_sftp_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

/// State of one simulated edit session. The mock does not keep file bytes:
/// "local versions" count simulated saves, the base version is the remote
/// size/mtime at download or last upload (as in ADR-0108).
final class _EditState {
  _EditState(this.info, this.root, this.targetPath, this.baseSize, this.baseModified);

  EditSessionInfo info;
  final MockFsNode root;
  final String targetPath;
  int baseSize;
  DateTime baseModified;
  int localVersion = 0;
  int baseVersion = 0;
  bool failNext = false;
  bool uploading = false;

  bool get dirty => localVersion != baseVersion;
}

/// In-memory [SftpBrowserService] on top of [MockSftpService]'s trees.
final class MockSftpBrowserService implements SftpBrowserService, SftpBrowserDebugControls {
  MockSftpBrowserService(this._config, this._sftp, this._inventory) {
    _sftp.addDisconnectListener(_onDisconnect);
  }

  final MockConfig _config;
  final MockSftpService _sftp;
  final MockInventoryService _inventory;
  final Map<EditSessionId, _EditState> _edits = {};
  final ValueStreamController<List<EditSessionInfo>> _sessions = ValueStreamController(const []);
  List<EditLeftover>? _leftovers;
  SftpBrowserPreferences _preferences = const SftpBrowserPreferences();
  bool _disposed = false;

  static const _editRoot = '/Users/demo/Library/Caches/ConsoleCrypt/profiles/demo/edit';

  // Browsing -------------------------------------------------------------------

  MockFsNode _root(SftpSessionId session) => _sftp.connection(session).root;

  static AppException _notFound(String path) => AppException(AppErrorCode.notFound, 'No such file: $path');

  @override
  Future<List<RemoteFileInfo>> listDirectory(SftpSessionId session, String path) async {
    await mockDelay(_config.latency);
    final root = _root(session);
    final dir = mockResolve(root, path);
    if (dir == null || !dir.isDir) {
      throw AppException(
        AppErrorCode.notFound,
        'No such directory: $path',
        reason: AppErrorReason.directoryNotFound,
        args: {'path': path},
      );
    }
    return [for (final c in dir.children.values) mockInfo(root, c, joinRemotePath(path, c.name))];
  }

  @override
  Future<RemoteFileInfo> stat(SftpSessionId session, String path) async {
    await mockDelay(_config.latency);
    final root = _root(session);
    final node = mockResolve(root, path, followLast: false);
    if (node == null) throw _notFound(path);
    return mockInfo(root, node, path);
  }

  @override
  Future<String> resolveDirectory(SftpSessionId session, String input, {required String base}) async {
    await mockDelay(_config.latency);
    final connection = _sftp.connection(session);
    final path = normalizeRemotePath(input, base: base, home: connection.home);
    final node = mockResolve(connection.root, path);
    if (node == null || !node.isDir) {
      throw AppException(
        AppErrorCode.notFound,
        'No such directory: $path',
        reason: AppErrorReason.directoryNotFound,
        args: {'path': path},
      );
    }
    return path;
  }

  @override
  Future<void> setPermissions(SftpSessionId session, String path, int mode) async {
    await mockDelay(_config.latency);
    final node = mockResolve(_root(session), path);
    if (node == null) throw _notFound(path);
    node.mode = mode & 0xFFF;
  }

  @override
  Future<void> createFile(SftpSessionId session, String path) async {
    await mockDelay(_config.latency);
    final connection = _sftp.connection(session);
    final (parent, name) = MockSftpService.parentOf(connection.root, path);
    MockSftpService.ensureFree(parent, name);
    final home = mockResolve(connection.root, connection.home);
    parent.children[name] = MockFsNode.file(name, text: '', ageHours: 0)
      ..owner = home?.owner ?? 'root'
      ..group = home?.group ?? 'root';
    parent.touch();
  }

  @override
  Future<void> duplicate(SftpSessionId session, String from, String to) async {
    await mockDelay(_config.latency);
    final root = _root(session);
    final source = mockResolve(root, from, followLast: false);
    if (source == null) throw _notFound(from);
    final (parent, name) = MockSftpService.parentOf(root, to);
    MockSftpService.ensureFree(parent, name);
    parent.children[name] = source.copyNamed(name);
    parent.touch();
  }

  @override
  Future<SftpPreview> readPreview(
    SftpSessionId session,
    String path, {
    int maxBytes = SftpBrowserService.defaultPreviewBytes,
  }) async {
    await mockDelay(_config.latency);
    final node = mockResolve(_root(session), path);
    if (node == null) throw _notFound(path);
    if (!node.isFile) {
      throw const AppException(AppErrorCode.validation, 'Not a regular file', reason: AppErrorReason.selectFile);
    }
    return SftpPreview(path: path, bytes: mockGeneratedContent(node, maxBytes), totalSize: node.size);
  }

  // Transfers --------------------------------------------------------------------

  @override
  Future<List<TransferId>> uploadItems(SftpSessionId session, List<String> localPaths, String remoteDirectory) async =>
      [for (final p in localPaths) await _sftp.transfer(TransferDirection.upload, session, p, remoteDirectory)];

  @override
  Future<List<TransferId>> downloadItems(
    SftpSessionId session,
    List<String> remotePaths,
    String localDirectory,
  ) async => [
    for (final p in remotePaths) await _sftp.transfer(TransferDirection.download, session, p, localDirectory),
  ];

  @override
  Future<TransferId> retryTransfer(TransferId id) => _sftp.retry(id);

  @override
  Future<void> clearFinishedTransfers() async => _sftp.clearFinished();

  // Edit sessions ------------------------------------------------------------------

  void _publish() {
    if (_disposed) return;
    _sessions.value = [
      for (final e in _edits.values)
        if (e.info.isActive) e.info,
    ];
  }

  void _set(_EditState state, EditStatus status, {DateTime? lastSyncedAt, int? uploads, List<String>? copies}) {
    state.info = state.info.copyWith(
      status: status,
      lastSyncedAt: lastSyncedAt,
      uploads: uploads,
      remoteCopies: copies,
    );
    _publish();
  }

  _EditState _edit(EditSessionId id) {
    final state = _edits[id];
    if (state == null || !state.info.isActive) {
      throw AppException(AppErrorCode.notFound, 'No such edit session: $id');
    }
    return state;
  }

  MockFsNode? _remoteNode(_EditState s) => mockResolve(s.root, s.targetPath);

  RemoteFileMeta? _meta(MockFsNode? node) =>
      node == null ? null : RemoteFileMeta(size: node.size, modifiedAt: node.modified, permissions: node.mode);

  bool _remoteChanged(_EditState s) {
    final node = _remoteNode(s);
    return node == null || node.size != s.baseSize || node.modified != s.baseModified;
  }

  void _rebase(_EditState s, MockFsNode node) {
    s
      ..baseSize = node.size
      ..baseModified = node.modified;
  }

  @override
  Future<EditSessionInfo> openInEditor(
    SftpSessionId session,
    String remotePath, {
    OpenWith openWith = const OpenWithDefault(),
  }) async {
    final connection = _sftp.connection(session);
    final target = mockCanonicalPath(connection.root, remotePath);
    final node = target == null ? null : mockResolve(connection.root, target);
    if (target == null || node == null) throw _notFound(remotePath);
    if (!node.isFile) {
      throw const AppException(AppErrorCode.validation, 'Not a regular file', reason: AppErrorReason.selectFile);
    }
    if (node.size > SftpBrowserService.maxEditFileSize) {
      throw AppException(
        AppErrorCode.payloadTooLarge,
        'File too large to edit',
        args: {'size': '${node.size}', 'limit': '${SftpBrowserService.maxEditFileSize}'},
      );
    }
    final existing = _edits.values
        .where((e) => e.info.isActive && e.info.hostId == connection.hostId && e.targetPath == target)
        .firstOrNull;
    if (existing != null) return existing.info; // reopened in the editor
    final id = EditSessionId.generate();
    final app = switch (openWith) {
      OpenWithApp(:final app) => app,
      OpenWithChoose() => const AppRef(AppRefKind.name, 'TextEdit'),
      OpenWithDefault() => null,
    };
    final state = _EditState(
      EditSessionInfo(
        id: id,
        hostId: connection.hostId,
        sftpSession: session,
        remotePath: remotePath,
        targetPath: target,
        localPath: '$_editRoot/${id.value}/${node.name}',
        status: const EditStatusOpening(),
        openedAt: DateTime.now(),
        app: app,
      ),
      connection.root,
      target,
      node.size,
      node.modified,
    );
    _edits[id] = state;
    _publish();
    await mockDelay(_config.latency);
    if (state.info.isActive) _set(state, const EditStatusSynced());
    return state.info;
  }

  @override
  Stream<List<EditSessionInfo>> watchEditSessions() => _sessions.stream;

  /// Conflict check + simulated atomic replace. [force] skips the check
  /// (conflict resolved with "overwrite").
  Future<void> _upload(_EditState s, {bool force = false}) async {
    if (s.uploading || !s.info.isActive) return;
    if (!force && _remoteChanged(s)) {
      _set(s, EditStatusConflict(remote: _meta(_remoteNode(s))));
      return;
    }
    s.uploading = true;
    final total = _remoteNode(s)?.size ?? s.baseSize;
    const steps = 5;
    for (var i = 0; i <= steps; i++) {
      _set(s, EditStatusUploading(transferred: total * i ~/ steps, total: total));
      await mockDelay(_config.transferTick);
      if (!s.info.isActive) {
        s.uploading = false;
        return;
      }
      if (s.failNext && i == 2) {
        s
          ..failNext = false
          ..uploading = false;
        _set(s, const EditStatusError(message: 'Permission denied', retryable: true));
        return;
      }
    }
    final node = _remoteNode(s);
    if (node == null) {
      s.uploading = false;
      _set(s, const EditStatusConflict());
      return;
    }
    node.touch();
    _rebase(s, node);
    s
      ..baseVersion = s.localVersion
      ..uploading = false;
    _set(s, const EditStatusSynced(), lastSyncedAt: DateTime.now(), uploads: s.info.uploads + 1);
  }

  @override
  Future<void> syncEditSession(EditSessionId id) async {
    final s = _edit(id);
    if (s.dirty || s.info.status is EditStatusError) {
      await _upload(s);
    } else if (_remoteChanged(s)) {
      _set(s, EditStatusConflict(remote: _meta(_remoteNode(s))));
    } else {
      _set(s, const EditStatusSynced());
    }
  }

  @override
  Future<void> reopenEditSession(EditSessionId id, {OpenWith? openWith}) async {
    final s = _edit(id);
    if (openWith is OpenWithApp) s.info = s.info.copyWith(app: openWith.app);
    _publish();
  }

  @override
  Future<void> revealEditSession(EditSessionId id) async => _edit(id);

  @override
  Future<void> resolveEditConflict(EditSessionId id, EditConflictResolution resolution) async {
    final s = _edit(id);
    final node = _remoteNode(s);
    switch (resolution) {
      case EditConflictResolution.overwriteRemote:
        await _upload(s, force: true);
      case EditConflictResolution.keepRemoteCopyLocally:
        if (node != null) _rebase(s, node);
        final name = s.info.localPath.split('/').last; // the working copy's name
        final dot = name.lastIndexOf('.');
        final stamp = DateTime.now().toUtc().toIso8601String().replaceAll(RegExp('[-:]'), '').split('.').first;
        final copy = dot > 0
            ? '${name.substring(0, dot)}.remote-${stamp}Z${name.substring(dot)}'
            : '$name.remote-${stamp}Z';
        final dir = s.info.localPath.substring(0, s.info.localPath.lastIndexOf('/'));
        if (s.localVersion == s.baseVersion) s.localVersion++;
        _set(s, const EditStatusModified(), copies: [...s.info.remoteCopies, '$dir/$copy']);
      case EditConflictResolution.discardLocal:
        if (node != null) _rebase(s, node);
        s.localVersion = s.baseVersion;
        _set(s, const EditStatusSynced());
    }
  }

  void _close(_EditState s) => _set(s, const EditStatusClosed());

  EditLeftover _asLeftover(_EditState s) => EditLeftover(
    id: s.info.id,
    hostId: s.info.hostId,
    remotePath: s.info.remotePath,
    targetPath: s.targetPath,
    createdAt: s.info.openedAt,
    workingFile: s.info.localPath,
    locallyModified: s.dirty,
  );

  @override
  Future<EditStopOutcome> stopEditing(EditSessionId id, {EditStopMode mode = EditStopMode.upload}) async {
    final s = _edit(id);
    switch (mode) {
      case EditStopMode.discard:
        _close(s);
        return const EditStopClosed(uploaded: false);
      case EditStopMode.keepFiles:
        (_leftovers ??= []).add(_asLeftover(s));
        _close(s);
        return EditStopKeptFiles(s.info.localPath.substring(0, s.info.localPath.lastIndexOf('/')));
      case EditStopMode.upload || EditStopMode.uploadOrKeep:
        final wasDirty = s.dirty;
        if (s.info.status is! EditStatusConflict && wasDirty) await _upload(s);
        final status = s.info.status;
        final EditStopOutcome? failure = switch (status) {
          EditStatusConflict(:final remote) => EditStopConflict(remote: remote),
          EditStatusError(:final message) => EditStopUploadFailed(message),
          _ => null,
        };
        if (failure == null) {
          _close(s);
          return EditStopClosed(uploaded: wasDirty);
        }
        if (mode == EditStopMode.upload) return failure;
        (_leftovers ??= []).add(_asLeftover(s));
        _close(s);
        return EditStopKeptFiles(s.info.localPath.substring(0, s.info.localPath.lastIndexOf('/')));
    }
  }

  void _onDisconnect(SftpSessionId session) {
    for (final s in _edits.values.where((e) => e.info.isActive && e.info.sftpSession == session).toList()) {
      unawaited(stopEditing(s.info.id, mode: EditStopMode.uploadOrKeep));
    }
  }

  List<EditLeftover> _seedLeftovers() {
    final hosts = _inventory.currentHosts;
    Host? named(String name) => hosts.where((h) => h.name == name).firstOrNull;
    final web = named('prod-web-1');
    final staging = named('staging-web');
    final day = DateTime.now().subtract(const Duration(hours: 20));
    return [
      if (web != null)
        EditLeftover(
          id: EditSessionId.generate(),
          hostId: web.id,
          remotePath: '/var/www/html/wp-config.php',
          targetPath: '/var/www/html/wp-config.php',
          createdAt: day,
          workingFile: '$_editRoot/leftover-1/wp-config.php',
          locallyModified: true,
        ),
      if (staging != null)
        EditLeftover(
          id: EditSessionId.generate(),
          hostId: staging.id,
          remotePath: '/etc/nginx/sites-available/default',
          targetPath: '/etc/nginx/sites-available/default',
          createdAt: day.subtract(const Duration(hours: 3)),
          workingFile: '$_editRoot/leftover-2/default',
          locallyModified: false,
        ),
      EditLeftover(id: EditSessionId.generate(), createdAt: day, workingFile: '$_editRoot/leftover-3/settings.json'),
    ];
  }

  @override
  Future<List<EditLeftover>> listEditLeftovers() async {
    await mockDelay(Duration.zero);
    if (_leftovers == null) {
      // Seed once the vault is open (the leftovers name hosts of the demo inventory).
      if (_config.seedEditLeftovers && _inventory.currentHosts.isEmpty) return const [];
      _leftovers = _config.seedEditLeftovers ? _seedLeftovers() : [];
    }
    return List.unmodifiable(_leftovers!);
  }

  @override
  Future<EditSessionInfo> resumeEditLeftover(EditSessionId id, SftpSessionId session, {OpenWith? openWith}) async {
    final leftover = _leftovers?.where((l) => l.id == id).firstOrNull;
    if (leftover == null || !leftover.canResume) throw AppException(AppErrorCode.notFound, 'No such leftover: $id');
    final connection = _sftp.connection(session);
    if (connection.hostId != leftover.hostId) {
      throw const AppException(AppErrorCode.validation, 'SFTP session is connected to another host');
    }
    _leftovers!.remove(leftover);
    final target = leftover.targetPath ?? leftover.remotePath!;
    final node = mockResolve(connection.root, target);
    final state = _EditState(
      EditSessionInfo(
        id: id,
        hostId: connection.hostId,
        sftpSession: session,
        remotePath: leftover.remotePath!,
        targetPath: target,
        localPath: leftover.workingFile!,
        status: const EditStatusOpening(),
        openedAt: leftover.createdAt ?? DateTime.now(),
      ),
      connection.root,
      target,
      node?.size ?? 0,
      node?.modified ?? DateTime.now(),
    );
    if (leftover.locallyModified ?? false) state.localVersion = 1;
    _edits[id] = state;
    _publish();
    await mockDelay(_config.latency);
    if (node == null) {
      _set(state, const EditStatusConflict());
    } else if (state.dirty) {
      _set(state, const EditStatusModified());
      await _upload(state);
    } else {
      _set(state, const EditStatusSynced());
    }
    return state.info;
  }

  @override
  Future<void> discardEditLeftover(EditSessionId id) async {
    await mockDelay(Duration.zero);
    _leftovers?.removeWhere((l) => l.id == id);
  }

  // Preferences -------------------------------------------------------------------

  @override
  Future<SftpBrowserPreferences> loadPreferences() async => _preferences;

  @override
  Future<void> savePreferences(SftpBrowserPreferences preferences) async => _preferences = preferences;

  // SftpBrowserDebugControls ------------------------------------------------------------

  @override
  void debugSimulateSave(EditSessionId id) {
    final s = _edits[id];
    if (s == null || !s.info.isActive) return;
    s.localVersion++;
    if (s.info.status is EditStatusConflict) return; // stays in conflict until resolved
    _set(s, const EditStatusModified());
    unawaited(_upload(s));
  }

  @override
  void debugSimulateRemoteChange(EditSessionId id) {
    final s = _edits[id];
    final node = s == null ? null : _remoteNode(s);
    if (node == null) return;
    final content = node.content;
    if (content == null) {
      node.size = node.size + 17;
    } else {
      node.content = Uint8List.fromList([...content, ...utf8.encode('\n# changed on the server\n')]);
    }
    node.modified = DateTime.now().add(const Duration(seconds: 1));
  }

  @override
  void debugFailNextUpload(EditSessionId id) => _edits[id]?.failNext = true;

  @override
  void debugFailNextTransfer() => _sftp.debugFailNextTransfer();

  Future<void> dispose() async {
    _disposed = true;
    await _sessions.close();
  }
}
