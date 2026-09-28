import 'dart:async';

import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_inventory_service.dart';
import 'package:consolecrypt/core/mock/mock_sftp_fs.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

final class MockSftpConnection {
  MockSftpConnection(this.hostId, this.root, this.home);

  final ObjectId hostId;
  final MockFsNode root;
  final String home;
}

final class _TransferSpec {
  _TransferSpec(this.direction, this.session, this.sourcePath, this.destinationDirectory);

  final TransferDirection direction;
  final SftpSessionId session;
  final String sourcePath;
  final String destinationDirectory;
}

/// In-memory [SftpService] over [MockFsNode] trees (one per host, kept for
/// the app run) plus the helpers `MockSftpBrowserService` builds on.
final class MockSftpService implements SftpService {
  MockSftpService(this._config, this._inventory);

  final MockConfig _config;
  final MockInventoryService _inventory;
  final MockFsNode local = mockLocalTree();
  final Map<SftpSessionId, MockSftpConnection> _sessions = {};
  final Map<ObjectId, MockFsNode> _remoteByHost = {};
  final ValueStreamController<List<TransferJob>> _transfers = ValueStreamController(const []);
  final Map<TransferId, _TransferSpec> _specs = {};
  final Set<TransferId> _cancelled = {};
  final List<void Function(SftpSessionId)> _disconnectListeners = [];
  bool _failNextTransfer = false;

  static const localHomePath = '/Users/demo';

  /// Size used for files dropped from the real OS (the mock never reads the disk).
  static const syntheticUploadSize = 256 * 1024;

  MockConfig get config => _config;

  List<TransferJob> get currentTransfers => _transfers.value;

  MockSftpConnection connection(SftpSessionId id) {
    final s = _sessions[id];
    if (s == null) {
      throw const AppException(AppErrorCode.notFound, 'SFTP session closed', reason: AppErrorReason.sessionClosed);
    }
    return s;
  }

  bool isConnected(SftpSessionId id) => _sessions.containsKey(id);

  void addDisconnectListener(void Function(SftpSessionId) listener) => _disconnectListeners.add(listener);

  /// Test/demo hook: the next transfer fails half-way.
  void debugFailNextTransfer() => _failNextTransfer = true;

  static AppException _missingDirectory(String path) => AppException(
    AppErrorCode.notFound,
    'No such directory: $path',
    reason: AppErrorReason.directoryNotFound,
    args: {'path': path},
  );

  static List<FileEntry> _list(MockFsNode root, String path) {
    final dir = mockResolve(root, path);
    if (dir == null || !dir.isDir) throw _missingDirectory(path);
    return [for (final c in dir.children.values) mockEntry(root, c, joinRemotePath(path, c.name))]..sort((a, b) {
      if (a.isDirectory != b.isDirectory) return a.isDirectory ? -1 : 1;
      return a.name.toLowerCase().compareTo(b.name.toLowerCase());
    });
  }

  @override
  Future<SftpSessionId> connect(ObjectId hostId) async {
    final host = _inventory.hostById(hostId);
    if (host == null) {
      throw const AppException(AppErrorCode.notFound, 'Host not found', reason: AppErrorReason.hostNotFound);
    }
    final effective = await _inventory.resolveEffective(host);
    await mockDelay(_config.latency);
    final user = effective.username.value ?? 'root';
    final root = _remoteByHost.putIfAbsent(hostId, () => mockRemoteTree(user));
    final id = SftpSessionId.generate();
    _sessions[id] = MockSftpConnection(hostId, root, '/home/$user');
    return id;
  }

  @override
  Future<void> disconnect(SftpSessionId id) async {
    if (_sessions.remove(id) == null) return;
    for (final listener in List.of(_disconnectListeners)) {
      listener(id);
    }
  }

  @override
  Future<String> remoteHome(SftpSessionId id) async => connection(id).home;

  @override
  Future<List<FileEntry>> listRemote(SftpSessionId id, String path) async {
    await mockDelay(_config.latency);
    return _list(connection(id).root, path);
  }

  @override
  Future<String> localHome() async => localHomePath;

  @override
  Future<List<FileEntry>> listLocal(String path) async {
    await mockDelay(Duration.zero);
    return _list(local, path);
  }

  /// Parent directory node of [path] and the last path component.
  static (MockFsNode, String) parentOf(MockFsNode root, String path) {
    final parentPath = parentRemotePath(path);
    final parent = mockResolve(root, parentPath);
    final name = path.split('/').where((p) => p.isNotEmpty).lastOrNull ?? '';
    if (parent == null || !parent.isDir || name.isEmpty) throw _missingDirectory(parentPath);
    return (parent, name);
  }

  static void ensureFree(MockFsNode dir, String name) {
    if (dir.children.containsKey(name)) {
      throw AppException(
        AppErrorCode.conflict,
        '"$name" already exists',
        reason: AppErrorReason.alreadyExists,
        args: {'name': name},
      );
    }
  }

  @override
  Future<void> makeRemoteDirectory(SftpSessionId id, String path) async {
    await mockDelay(_config.latency);
    final (parent, name) = parentOf(connection(id).root, path);
    ensureFree(parent, name);
    final session = connection(id);
    final home = mockResolve(session.root, session.home);
    parent.children[name] = MockFsNode.dir(name, ageHours: 0)
      ..owner = home?.owner ?? 'root'
      ..group = home?.group ?? 'root';
    parent.touch();
  }

  @override
  Future<void> renameRemote(SftpSessionId id, String from, String to) async {
    await mockDelay(_config.latency);
    final root = connection(id).root;
    final (source, oldName) = parentOf(root, from);
    final (target, newName) = parentOf(root, to);
    final node = source.children[oldName];
    if (node == null) throw AppException(AppErrorCode.notFound, 'Not found: $from');
    if (from == to) return;
    ensureFree(target, newName);
    source.children.remove(oldName);
    target.children[newName] = node..name = newName;
    source.touch();
    target.touch();
  }

  @override
  Future<void> deleteRemote(SftpSessionId id, String path, {bool recursive = false}) async {
    await mockDelay(_config.latency);
    final (parent, name) = parentOf(connection(id).root, path);
    final node = parent.children[name];
    if (node == null) throw AppException(AppErrorCode.notFound, 'Not found: $path');
    if (node.isDir && node.children.isNotEmpty && !recursive) {
      throw const AppException(
        AppErrorCode.validation,
        'Directory is not empty',
        reason: AppErrorReason.directoryNotEmpty,
      );
    }
    parent.children.remove(name);
    parent.touch();
  }

  @override
  Future<TransferId> upload(SftpSessionId id, {required String localPath, required String remoteDirectory}) async {
    final source = mockResolve(local, localPath);
    if (source == null || !source.isFile) {
      throw const AppException(AppErrorCode.validation, 'Select a file to upload', reason: AppErrorReason.selectFile);
    }
    return transfer(TransferDirection.upload, id, localPath, remoteDirectory);
  }

  @override
  Future<TransferId> download(SftpSessionId id, {required String remotePath, required String localDirectory}) async {
    final source = mockResolve(connection(id).root, remotePath);
    if (source == null || !source.isFile) {
      throw const AppException(AppErrorCode.validation, 'Select a file to download', reason: AppErrorReason.selectFile);
    }
    return transfer(TransferDirection.download, id, remotePath, localDirectory);
  }

  /// Queues a transfer of a file or folder. Upload sources missing from the
  /// demo disk (real paths dropped from Finder/Explorer) become synthetic
  /// files of [syntheticUploadSize] bytes.
  Future<TransferId> transfer(
    TransferDirection direction,
    SftpSessionId session,
    String sourcePath,
    String destinationDirectory,
  ) async {
    final spec = _TransferSpec(direction, session, sourcePath, destinationDirectory);
    final (source, destination) = _endpoints(spec);
    final job = TransferJob(
      id: TransferId.generate(),
      direction: direction,
      sourcePath: sourcePath,
      destinationPath: joinRemotePath(destinationDirectory, source.name),
      totalBytes: source.treeSize,
      state: TransferState.queued,
    );
    _specs[job.id] = spec;
    _transfers.value = [job, ..._transfers.value];
    unawaited(_run(job, source, destination));
    return job.id;
  }

  (MockFsNode, MockFsNode) _endpoints(_TransferSpec spec) {
    final remote = connection(spec.session).root;
    final (sourceRoot, destinationRoot) = spec.direction == TransferDirection.upload
        ? (local, remote)
        : (remote, local);
    var source = mockResolve(sourceRoot, spec.sourcePath);
    if (source == null && spec.direction == TransferDirection.upload) {
      final name = spec.sourcePath.split(RegExp(r'[\\/]')).where((p) => p.isNotEmpty).lastOrNull ?? 'upload';
      source = MockFsNode.file(name, size: syntheticUploadSize, ageHours: 0);
    }
    if (source == null) throw AppException(AppErrorCode.notFound, 'Not found: ${spec.sourcePath}');
    final destination = mockResolve(destinationRoot, spec.destinationDirectory);
    if (destination == null || !destination.isDir) throw _missingDirectory(spec.destinationDirectory);
    return (source, destination);
  }

  void _update(TransferJob job) {
    _transfers.value = [for (final j in _transfers.value) j.id == job.id ? job : j];
  }

  Future<void> _run(TransferJob initial, MockFsNode source, MockFsNode destinationDir) async {
    var job = initial.copyWith(state: TransferState.running);
    _update(job);
    final fail = _failNextTransfer;
    _failNextTransfer = false;
    const steps = 20;
    final chunk = (job.totalBytes / steps).ceil();
    final tick = _config.transferTick;
    for (var i = 1; i <= steps; i++) {
      await mockDelay(tick);
      if (_cancelled.remove(job.id)) {
        _update(job.copyWith(state: TransferState.cancelled));
        return;
      }
      if (fail && i == steps ~/ 2) {
        _update(job.copyWith(state: TransferState.failed, error: 'Connection reset by peer'));
        return;
      }
      final done = (chunk * i).clamp(0, job.totalBytes);
      final perSecond = tick == Duration.zero ? 0 : (chunk * 1000 / tick.inMilliseconds).round();
      job = job.copyWith(transferredBytes: done, bytesPerSecond: perSecond);
      _update(job);
    }
    destinationDir.children[source.name] = source.copyNamed(source.name);
    destinationDir.touch();
    _update(job.copyWith(state: TransferState.completed, transferredBytes: job.totalBytes));
  }

  @override
  Future<void> cancelTransfer(TransferId id) async {
    final job = _transfers.value.where((j) => j.id == id).firstOrNull;
    if (job != null && job.isActive) _cancelled.add(id);
  }

  /// Re-queues a finished (failed / cancelled) job in place of the old one.
  Future<TransferId> retry(TransferId id) async {
    final spec = _specs[id];
    final job = _transfers.value.where((j) => j.id == id).firstOrNull;
    if (spec == null || job == null) throw AppException(AppErrorCode.notFound, 'No such transfer: $id');
    if (job.isActive) return id;
    _transfers.value = [
      for (final j in _transfers.value)
        if (j.id != id) j,
    ];
    _specs.remove(id);
    return transfer(spec.direction, spec.session, spec.sourcePath, spec.destinationDirectory);
  }

  void clearFinished() {
    final keep = [
      for (final j in _transfers.value)
        if (j.isActive) j,
    ];
    _specs.removeWhere((id, _) => !keep.any((j) => j.id == id));
    _transfers.value = keep;
  }

  @override
  Stream<List<TransferJob>> watchTransfers() => _transfers.stream;

  Future<void> dispose() async {
    _cancelled.addAll(_transfers.value.map((j) => j.id));
    await _transfers.close();
  }
}
