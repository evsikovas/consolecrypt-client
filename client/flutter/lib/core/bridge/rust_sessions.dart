import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/core/bridge/effective_config.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:consolecrypt/src/rust/api/app.dart' as rs_app;
import 'package:consolecrypt/src/rust/api/inventory.dart' as rs_inv;
import 'package:consolecrypt/src/rust/api/sftp.dart' as rs_sftp;
import 'package:consolecrypt/src/rust/api/ssh.dart' as rs_ssh;
import 'package:flutter/services.dart';

// ---- terminals -------------------------------------------------------------------------

enum _PromptKind { hostKey, password, passphrase }

final class _Session {
  _Session(this.id, this.hostId, this.size);

  final TerminalSessionId id;
  final ObjectId hostId;
  TerminalSize size;

  /// Single-subscription: buffers output until the tab subscribes and stays
  /// the same across reconnects. Closed in [RustTerminalService.close].
  // ignore: close_sinks
  final output = StreamController<Uint8List>();
  // ignore: close_sinks
  final events = StreamController<TerminalEvent>();

  String? rustId;
  // Cancelled on reconnect / close.
  // ignore: cancel_subscriptions
  StreamSubscription<rs_ssh.TerminalFrame>? frames;
  bool connecting = false;
  bool connected = false;
  bool closed = false;
  String? promptRequest;
  _PromptKind? promptKind;
  int passwordAttempts = 0;

  void event(TerminalEvent e) {
    if (!closed) events.add(e);
  }
}

/// PTY shells over app-core terminals. Host-key and password/passphrase
/// prompts raised while a session connects are routed to that session's
/// event stream and answered through `answerHostKey` / `answerPassword`.
final class RustTerminalService implements TerminalService {
  RustTerminalService(this._hub) {
    _hub.onPrompt = _routePrompt;
  }

  final RustBackend _hub;
  final Map<TerminalSessionId, _Session> _sessions = {};

  _Session _session(TerminalSessionId id) =>
      _sessions[id] ??
      (throw const AppException(AppErrorCode.notFound, 'Session closed', reason: AppErrorReason.sessionClosed));

  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async {
    if (!_hub.hosts.value.any((h) => h.id == hostId)) {
      throw const AppException(AppErrorCode.notFound, 'Host not found', reason: AppErrorReason.hostNotFound);
    }
    final s = _Session(TerminalSessionId.generate(), hostId, size);
    _sessions[s.id] = s;
    unawaited(_connect(s));
    return TerminalSessionHandle(id: s.id, output: s.output.stream, events: s.events.stream);
  }

  Future<void> _connect(_Session s, {bool reconnect = false}) async {
    if (s.connecting || s.closed) return;
    s
      ..connecting = true
      ..connected = false
      ..passwordAttempts = 0;
    String? route;
    try {
      route =
          (decodeObject(await guard(() => rs_inv.hostsDescribeConnection(hostId: s.hostId.value))))['route'] as String?;
    } on AppException {
      route = null;
    }
    s.event(
      TerminalStateChanged(
        reconnect ? SessionConnectionState.reconnecting : SessionConnectionState.connecting,
        message: route,
      ),
    );
    try {
      final info = decodeObject(
        await guard(() => rs_ssh.terminalOpen(hostId: s.hostId.value, cols: s.size.columns, rows: s.size.rows)),
      );
      final rustId = info['id']! as String;
      if (s.closed) {
        await _closeRust(rustId);
        return;
      }
      s
        ..rustId = rustId
        ..connected = true;
      s.event(const TerminalStateChanged(SessionConnectionState.connected));
      final title = info['title'] as String?;
      if (title != null && title.isNotEmpty) s.event(TerminalTitleChanged(title));
      await s.frames?.cancel();
      s.frames = rs_ssh
          .terminalAttach(terminalId: rustId)
          .listen(
            (f) {
              if (!s.closed && s.rustId == rustId) _onFrame(s, f);
            },
            onError: (Object e) {
              if (!s.closed && s.rustId == rustId) _disconnected(s, toAppException(e).message);
            },
            onDone: () {
              if (!s.closed && s.rustId == rustId && s.connected) _disconnected(s, null);
            },
          );
    } on AppException catch (e) {
      if (e.code == AppErrorCode.hostKeyChanged) {
        // Possible MITM: a hard failure the UI never offers to accept.
        s.event(
          TerminalHostKeyPrompt(
            HostKeyInfo(hostPattern: _hostPattern(s), keyType: '', fingerprintSha256: '', changed: true),
          ),
        );
      }
      s.event(TerminalStateChanged(SessionConnectionState.disconnected, message: e.message));
    } finally {
      s
        ..connecting = false
        ..promptRequest = null
        ..promptKind = null;
    }
  }

  String _hostPattern(_Session s) {
    final host = _hub.hosts.value.where((h) => h.id == s.hostId).firstOrNull;
    if (host == null) return '';
    final effective = _resolver.resolve(host);
    return hostPattern(host.address, effective.port.value ?? defaultSshPort);
  }

  EffectiveConfigResolver get _resolver => EffectiveConfigResolver(
    hosts: _hub.hosts.value,
    groups: _hub.groups.value,
    credentials: _hub.credentials.value,
    jumpProfiles: _hub.jumpProfiles.value,
  );

  void _onFrame(_Session s, rs_ssh.TerminalFrame f) {
    switch (f.kind) {
      case rs_ssh.TerminalFrameKind.data:
        if (f.data.isNotEmpty && !s.closed) s.output.add(f.data);
      case rs_ssh.TerminalFrameKind.lagged:
        break;
      case rs_ssh.TerminalFrameKind.closed:
        s.event(TerminalExited(f.exitStatus));
        _disconnected(s, f.message);
      case rs_ssh.TerminalFrameKind.failed:
        _disconnected(s, f.message);
    }
  }

  void _disconnected(_Session s, String? message) {
    if (!s.connected) return;
    s.connected = false;
    s.event(TerminalStateChanged(SessionConnectionState.disconnected, message: message));
  }

  /// Claims a core prompt for the connecting session it belongs to.
  Future<bool> _routePrompt(Json prompt) async {
    final (kind, body) = switch (prompt) {
      {'HostKey': final Map<Object?, Object?> b} => (_PromptKind.hostKey, b),
      {'Password': final Map<Object?, Object?> b} => (_PromptKind.password, b),
      {'Passphrase': final Map<Object?, Object?> b} => (_PromptKind.passphrase, b),
      _ => (null, const <Object?, Object?>{}),
    };
    if (kind == null) return false;
    final connecting = _sessions.values.where((s) => s.connecting && !s.closed).toList();
    if (connecting.isEmpty) return false;
    final hostId = body['host_id'] as String?;
    final s = connecting.where((s) => s.hostId.value == hostId).firstOrNull ?? connecting.first;
    s
      ..promptRequest = body['request_id']! as String
      ..promptKind = kind;
    switch (kind) {
      case _PromptKind.hostKey:
        s
          ..event(const TerminalStateChanged(SessionConnectionState.awaitingHostKey))
          ..event(
            TerminalHostKeyPrompt(
              HostKeyInfo(
                hostPattern: body['host_pattern'] as String? ?? '',
                keyType: body['key_type'] as String? ?? '',
                fingerprintSha256: body['fingerprint_sha256'] as String? ?? '',
              ),
            ),
          );
      case _PromptKind.password:
        final host = _hub.hosts.value.where((h) => h.id == s.hostId).firstOrNull;
        final username = host == null ? '' : _resolver.resolve(host).username.value ?? '';
        s
          ..event(const TerminalStateChanged(SessionConnectionState.awaitingPassword))
          ..event(
            TerminalPasswordPrompt(
              username: username,
              hostLabel: body['host_name'] as String? ?? host?.name ?? '',
              retry: s.passwordAttempts++ > 0,
            ),
          );
      case _PromptKind.passphrase:
        // TODO(ui): dedicated key-passphrase prompt — shown as a password
        // prompt labelled with the key's name; next: TerminalPassphrasePrompt.
        s
          ..event(const TerminalStateChanged(SessionConnectionState.awaitingPassword))
          ..event(
            TerminalPasswordPrompt(
              username: '',
              hostLabel: body['credential_name'] as String? ?? '',
              retry: ((body['attempt'] as num?)?.toInt() ?? 0) > 0,
            ),
          );
    }
    return true;
  }

  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {
    final s = _sessions[id];
    final rustId = s?.rustId;
    if (s == null || rustId == null || !s.connected) return;
    await guard(() => rs_ssh.terminalWrite(terminalId: rustId, data: data));
  }

  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {
    final s = _sessions[id];
    if (s == null) return;
    s.size = size;
    final rustId = s.rustId;
    if (rustId == null || !s.connected) return;
    try {
      await guard(() => rs_ssh.terminalResize(terminalId: rustId, cols: size.columns, rows: size.rows));
    } on AppException {
      // Session ending; ignore.
    }
  }

  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {
    final s = _session(id);
    final request = s.promptRequest;
    if (request == null || s.promptKind != _PromptKind.hostKey) return;
    s
      ..promptRequest = null
      ..promptKind = null;
    final answer = switch (decision) {
      HostKeyDecision.acceptAndSave => rs_app.HostKeyAnswer.acceptAndSave,
      HostKeyDecision.acceptOnce => rs_app.HostKeyAnswer.acceptOnce,
      HostKeyDecision.reject => rs_app.HostKeyAnswer.reject,
    };
    await guard(() => rs_app.promptAnswerHostKey(requestId: request, answer: answer));
    if (decision != HostKeyDecision.reject) s.event(const TerminalStateChanged(SessionConnectionState.connecting));
  }

  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async {
    final s = _sessions[id];
    final request = s?.promptRequest;
    final kind = s?.promptKind;
    if (s == null || request == null || kind == null || kind == _PromptKind.hostKey) {
      password?.wipe();
      return;
    }
    s
      ..promptRequest = null
      ..promptKind = null;
    final bytes = password?.exposeBytes();
    password?.wipe();
    try {
      if (kind == _PromptKind.password) {
        await guard(() => rs_app.promptAnswerPassword(requestId: request, password: bytes));
      } else {
        await guard(() => rs_app.promptAnswerPassphrase(requestId: request, passphrase: bytes));
      }
    } finally {
      bytes?.fillRange(0, bytes.length, 0);
    }
    if (bytes != null) s.event(const TerminalStateChanged(SessionConnectionState.connecting));
  }

  @override
  Future<void> reconnect(TerminalSessionId id) async {
    final s = _session(id);
    if (s.connecting) return;
    final old = s.rustId;
    final oldFrames = s.frames;
    s
      ..rustId = null
      ..frames = null
      ..connected = false;
    // FRB's async stream cancellation waits for a producer frame. End the
    // old Rust attachment before awaiting cancellation of an idle stream.
    if (old != null) await _closeRust(old);
    await oldFrames?.cancel();
    if (s.closed || s.rustId != null) return;
    await _connect(s, reconnect: true);
  }

  Future<void> _closeRust(String rustId) async {
    try {
      await rs_ssh.terminalClose(terminalId: rustId);
    } on Object {
      // Already gone (locked / closed remotely).
    }
  }

  @override
  Future<void> close(TerminalSessionId id) async {
    final s = _sessions.remove(id);
    if (s == null) return;
    final rustId = s.rustId;
    final oldFrames = s.frames;
    s
      ..closed = true
      ..connected = false
      ..rustId = null
      ..frames = null;
    // Close the producer first so a pending FRB cancellation can finish.
    if (rustId != null) await _closeRust(rustId);
    await oldFrames?.cancel();
    if (s.promptRequest case final request?) {
      try {
        if (s.promptKind == _PromptKind.hostKey) {
          await rs_app.promptAnswerHostKey(requestId: request, answer: rs_app.HostKeyAnswer.reject);
        } else if (s.promptKind == _PromptKind.password) {
          await rs_app.promptAnswerPassword(requestId: request);
        } else {
          await rs_app.promptAnswerPassphrase(requestId: request);
        }
      } on Object {
        // Timed out already.
      }
    }
    await s.output.close();
    await s.events.close();
  }

  Future<void> dispose() async {
    for (final id in [..._sessions.keys]) {
      await close(id);
    }
  }
}

// ---- SFTP --------------------------------------------------------------------------------

/// SFTP browsing over app-core sessions; transfers are queued in the core
/// bridge (one at a time) with progress and cancel. Local listing uses
/// `dart:io`.
final class RustSftpService implements SftpService {
  RustSftpService(this._hub) {
    _hub.onSessionReset.add(_onReset);
  }

  final RustBackend _hub;
  final _transfers = <TransferJob>[];
  final _transferStream = ValueStreamController<List<TransferJob>>(const []);
  final Map<TransferId, StreamSubscription<rs_ssh.TransferUpdate>> _running = {};

  void _onReset() {
    for (final sub in _running.values) {
      unawaited(sub.cancel());
    }
    _running.clear();
  }

  @override
  Future<SftpSessionId> connect(ObjectId hostId) async =>
      SftpSessionId(await guard(() => rs_ssh.sftpOpen(hostId: hostId.value)));

  @override
  Future<void> disconnect(SftpSessionId id) async {
    try {
      await guard(() => rs_ssh.sftpClose(sftpId: id.value));
    } on AppException {
      // Already closed (lock / network drop).
    }
  }

  @override
  Future<String> remoteHome(SftpSessionId id) => guard(() => rs_ssh.sftpHome(sftpId: id.value));

  @override
  Future<List<FileEntry>> listRemote(SftpSessionId id, String path) async => sortEntries(
    decodeList(await guard(() => rs_ssh.sftpList(sftpId: id.value, path: path))).map(remoteEntryFromJson),
  );

  @override
  Future<String> localHome() async {
    if (Platform.isAndroid || Platform.isIOS) {
      final root = await MethodChannel(Platform.isIOS ? 'consolecrypt/ios' : 'consolecrypt/android')
          .invokeMethod<String>('dataDirectory');
      if (root == null || root.isEmpty) throw StateError('Mobile private storage unavailable');
      return (await Directory('$root/files').create(recursive: true)).path;
    }
    return Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'] ?? Directory.current.path;
  }

  @override
  Future<List<FileEntry>> listLocal(String path) async {
    final dir = Directory(path);
    if (!dir.existsSync()) {
      throw AppException(
        AppErrorCode.notFound,
        'Directory not found', // l10n-ignore: diagnostic
        reason: AppErrorReason.directoryNotFound,
        args: {'path': path},
      );
    }
    final entries = <FileEntry>[];
    await for (final e in dir.list(followLinks: false)) {
      final name = e.path.split(Platform.pathSeparator).last;
      if (name.isEmpty) continue;
      try {
        final stat = e.statSync();
        final isLink = e is Link;
        final isDir = e is Directory || (isLink && FileSystemEntity.isDirectorySync(e.path));
        entries.add(
          FileEntry(
            name: name,
            path: e.path,
            isDirectory: isDir,
            size: isDir ? 0 : stat.size,
            modifiedAt: stat.modified,
            isSymlink: isLink,
          ),
        );
      } on FileSystemException {
        // Unreadable entry: skip.
      }
    }
    return sortEntries(entries);
  }

  @override
  Future<void> makeRemoteDirectory(SftpSessionId id, String path) =>
      guard(() => rs_ssh.sftpMkdir(sftpId: id.value, path: path));

  @override
  Future<void> renameRemote(SftpSessionId id, String from, String to) =>
      guard(() => rs_ssh.sftpRename(sftpId: id.value, from: from, to: to));

  @override
  Future<void> deleteRemote(SftpSessionId id, String path, {bool recursive = false}) =>
      guard(() => rs_ssh.sftpRemove(sftpId: id.value, path: path, recursive: recursive));

  static String _baseName(String path) => path.split(RegExp(r'[\\/]')).where((p) => p.isNotEmpty).lastOrNull ?? path;

  @override
  Future<TransferId> upload(SftpSessionId id, {required String localPath, required String remoteDirectory}) async {
    final file = File(localPath);
    if (!file.existsSync()) {
      throw const AppException(
        AppErrorCode.validation,
        'Select a file', // l10n-ignore: diagnostic
        reason: AppErrorReason.selectFile,
      );
    }
    return _start(
      id,
      direction: TransferDirection.upload,
      source: localPath,
      destination: joinRemotePath(remoteDirectory, _baseName(localPath)),
      total: file.lengthSync(),
    );
  }

  /// Upload a local file **or folder** (recursive, done by the core) into
  /// [remoteDirectory] (SFTP browser, drag & drop).
  TransferId startUploadItem(SftpSessionId id, {required String localPath, required String remoteDirectory}) {
    final isFile = FileSystemEntity.isFileSync(localPath);
    if (!isFile && !FileSystemEntity.isDirectorySync(localPath)) {
      throw AppException(
        AppErrorCode.notFound,
        'No such file or folder', // l10n-ignore: diagnostic
        reason: AppErrorReason.directoryNotFound,
        args: {'path': localPath},
      );
    }
    return _start(
      id,
      direction: TransferDirection.upload,
      source: localPath,
      destination: joinRemotePath(remoteDirectory, _baseName(localPath)),
      total: isFile ? File(localPath).lengthSync() : 0,
    );
  }

  /// Download a remote file **or folder** (recursive) into [localDirectory].
  TransferId startDownloadItem(SftpSessionId id, {required String remotePath, required String localDirectory}) {
    final sep = Platform.pathSeparator;
    final dir = localDirectory.endsWith(sep) ? localDirectory : '$localDirectory$sep';
    return _start(
      id,
      direction: TransferDirection.download,
      source: remotePath,
      destination: '$dir${_baseName(remotePath)}',
      total: 0,
    );
  }

  /// Re-queue a failed / cancelled job under a new id (the old one is
  /// removed from the list).
  TransferId retry(TransferId old) {
    final i = _transfers.indexWhere((t) => t.id == old);
    if (i < 0) {
      throw const AppException(AppErrorCode.notFound, 'Unknown transfer'); // l10n-ignore: diagnostic
    }
    final t = _transfers.removeAt(i);
    final next = TransferId.generate();
    _transfers.insert(
      0,
      TransferJob(
        id: next,
        direction: t.direction,
        sourcePath: t.sourcePath,
        destinationPath: t.destinationPath,
        totalBytes: t.totalBytes,
        state: TransferState.queued,
      ),
    );
    _publish();
    _running[next] = rs_ssh
        .sftpRetryTransfer(transferId: old.value, newTransferId: next.value)
        .listen(
          (u) => _update(next, u),
          onError: (Object e) => _fail(next, toAppException(e).message),
          onDone: () => _running.remove(next),
        );
    return next;
  }

  /// Drop completed, failed and cancelled jobs (here and in the core).
  Future<void> clearFinished() async {
    _transfers.removeWhere((t) => !t.isActive);
    _publish();
    try {
      await guard(rs_sftp.sftpClearFinishedTransfers);
    } on AppException {
      // Locked meanwhile: nothing to clear in the core.
    }
  }

  @override
  Future<TransferId> download(SftpSessionId id, {required String remotePath, required String localDirectory}) async {
    final sep = Platform.pathSeparator;
    final dir = localDirectory.endsWith(sep) ? localDirectory : '$localDirectory$sep';
    return _start(
      id,
      direction: TransferDirection.download,
      source: remotePath,
      destination: '$dir${_baseName(remotePath)}',
      total: 0,
    );
  }

  TransferId _start(
    SftpSessionId id, {
    required TransferDirection direction,
    required String source,
    required String destination,
    required int total,
  }) {
    final transferId = TransferId.generate();
    final job = TransferJob(
      id: transferId,
      direction: direction,
      sourcePath: source,
      destinationPath: destination,
      totalBytes: total,
      state: TransferState.queued,
    );
    _transfers.insert(0, job);
    _publish();
    final upload = direction == TransferDirection.upload;
    _running[transferId] = rs_ssh
        .sftpTransfer(
          transferId: transferId.value,
          sftpId: id.value,
          direction: upload ? rs_ssh.TransferDirection.upload : rs_ssh.TransferDirection.download,
          localPath: upload ? source : destination,
          remotePath: upload ? destination : source,
        )
        .listen(
          (u) => _update(transferId, u),
          onError: (Object e) => _fail(transferId, toAppException(e).message),
          onDone: () => _running.remove(transferId),
        );
    return transferId;
  }

  void _update(TransferId id, rs_ssh.TransferUpdate u) {
    final i = _transfers.indexWhere((t) => t.id == id);
    if (i < 0) return;
    final t = _transfers[i];
    final total = u.total ?? (t.totalBytes > 0 ? t.totalBytes : u.transferred);
    final seconds = u.elapsedMs / 1000;
    final next = TransferJob(
      id: t.id,
      direction: t.direction,
      sourcePath: t.sourcePath,
      destinationPath: t.destinationPath,
      totalBytes: total,
      transferredBytes: u.transferred,
      bytesPerSecond: seconds > 0 ? (u.transferred / seconds).round() : 0,
      state: switch (u.phase) {
        rs_ssh.TransferPhase.queued => TransferState.queued,
        rs_ssh.TransferPhase.running => TransferState.running,
        rs_ssh.TransferPhase.completed => TransferState.completed,
        rs_ssh.TransferPhase.failed => TransferState.failed,
        rs_ssh.TransferPhase.cancelled => TransferState.cancelled,
      },
      error: u.error == null ? null : mapBridgeError(u.error!).message,
      isDirectory: u.isDirectory,
      filesDone: u.filesDone,
      filesTotal: u.filesTotal,
      currentPath: u.currentPath,
    );
    _transfers[i] = next;
    _publish();
  }

  void _fail(TransferId id, String message) {
    final i = _transfers.indexWhere((t) => t.id == id);
    if (i < 0) return;
    _transfers[i] = _transfers[i].copyWith(state: TransferState.failed, error: message);
    _publish();
  }

  void _publish() => _transferStream.value = List.unmodifiable(_transfers);

  @override
  Future<void> cancelTransfer(TransferId id) async {
    await guard(() => rs_ssh.sftpCancelTransfer(transferId: id.value));
  }

  @override
  Stream<List<TransferJob>> watchTransfers() => _transferStream.stream;

  Future<void> dispose() async {
    _onReset();
    await _transferStream.close();
  }
}
