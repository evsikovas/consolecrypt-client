import 'dart:async';

import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter/foundation.dart';

/// In-memory, profile-scoped sessions. No credentials or certificate decisions
/// are persisted. Native code owns independent transport and lock guards.
final class RdpTab extends ChangeNotifier {
  RdpTab(
    this.info,
    this.options, {
    String? title,
    this.hostId,
    this.permissions = const RdpSessionPermissions(),
    this.directory,
  }) : title = title ?? options.endpoint,
       folderState = permissions.directoryGrantId == null ? RdpFolderState.disabled : RdpFolderState.pending;
  final RdpSessionInfo info;
  final RdpConnectionOptions options;
  final String title;
  final String? hostId;
  RdpSessionPermissions permissions;
  RdpDirectoryGrant? directory;
  RdpFolderState folderState;
  int _permissionEpoch = 0;
  Future<void> _permissionTail = Future.value();
  RdpStatus status = const RdpStatus(RdpPhase.connecting);
  RdpFrame? frame;
  String? noticeCode;
  bool closed = false;
  bool polling = false;
  int _lastSequence = -1;
  Future<void> _inputTail = Future.value();
  int _queuedInputs = 0;
}

final class RdpWorkspaceController extends ChangeNotifier {
  RdpWorkspaceController(this.service) {
    _timer = Timer.periodic(const Duration(milliseconds: 67), (_) => _pollActive());
  }
  final RdpService service;
  final List<RdpTab> _tabs = [];
  List<RdpTab> get tabs => List.unmodifiable(_tabs);
  RdpTab? get active => _active;
  RdpTab? _active;
  late final Timer _timer;
  int _generation = 0;
  int _connecting = 0;
  bool _disposed = false;
  bool _visible = true;
  bool get canConnect => !_disposed && _tabs.length + _connecting < rdpMaxSessions;

  /// Indexed-shell branches remain mounted while offstage. Polling pauses
  /// while another section is selected; the native connection stays open.
  void setVisible(bool visible) {
    _visible = visible;
  }

  /// Caller keeps the mutable credential buffer until this Future settles.
  /// A late connect after cancellation is disconnected, never adopted.
  Future<RdpTab?> connect(
    RdpConnectionOptions options,
    Uint8List passwordBytes,
    String certificateSha256, {
    required bool Function() isCurrent,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
    RdpDirectoryGrant? directory,
  }) => _connect(
    options,
    () => service.connect(
      options,
      passwordBytes: passwordBytes,
      certificateSha256: certificateSha256,
      permissions: permissions,
    ),
    isCurrent: isCurrent,
    permissions: permissions,
    directory: directory,
  );

  Future<RdpTab?> connectSavedHost(
    RdpSavedHostTicket ticket, {
    Uint8List? passwordBytes,
    required bool Function() isCurrent,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
    RdpDirectoryGrant? directory,
  }) => _connect(
    ticket.options,
    () => service.connectSavedHost(ticket, passwordBytes: passwordBytes, permissions: permissions),
    isCurrent: isCurrent,
    permissions: permissions,
    directory: directory,
    title: ticket.name,
    hostId: ticket.hostId,
  );

  Future<RdpTab?> _connect(
    RdpConnectionOptions options,
    Future<RdpSessionInfo> Function() create, {
    required bool Function() isCurrent,
    required RdpSessionPermissions permissions,
    RdpDirectoryGrant? directory,
    String? title,
    String? hostId,
  }) async {
    if (!canConnect || !isCurrent()) throw const RdpFailure('cancelled');
    final generation = _generation;
    _connecting++;
    try {
      final info = await create();
      if (_disposed || generation != _generation || !isCurrent()) {
        await _disconnect(info.id);
        return null;
      }
      if (_tabs.any((tab) => tab.info.id == info.id)) {
        await _disconnect(info.id);
        throw const RdpFailure('invalid_session');
      }
      final tab = RdpTab(info, options, title: title, hostId: hostId, permissions: permissions, directory: directory);
      _tabs.add(tab);
      _active = tab;
      notifyListeners();
      return tab;
    } finally {
      _connecting--;
    }
  }

  Future<void> setPermissions(RdpTab tab, RdpSessionPermissions next, {RdpDirectoryGrant? directory}) {
    final generation = _generation;
    final result = tab._permissionTail.then((_) async {
      if (_disposed || tab.closed || generation != _generation) throw const RdpFailure('cancelled');
      tab._permissionEpoch++;
      await service.setPermissions(tab.info.id, next);
      if (_disposed || tab.closed || generation != _generation) throw const RdpFailure('cancelled');
      final old = tab.directory;
      final folderChanged =
          tab.permissions.directoryGrantId != next.directoryGrantId ||
          tab.permissions.directoryWritable != next.directoryWritable;
      tab.permissions = next;
      tab.directory = next.directoryGrantId == null ? null : directory ?? tab.directory;
      if (folderChanged) {
        tab.folderState = next.directoryGrantId == null ? RdpFolderState.disabled : RdpFolderState.pending;
      }
      // A poll initiated while the native update was awaiting acknowledgement
      // may still contain the old drive's acceptance. Never apply it to this one.
      tab._permissionEpoch++;
      tab.notifyListeners();
      if (old != null && old.id != next.directoryGrantId) await releaseDirectory(old.id);
    });
    tab._permissionTail = result.then<void>((_) {}, onError: (Object _, StackTrace _) {});
    return result;
  }

  Future<void> releaseDirectory(String id) async {
    try {
      await service.releaseDirectoryGrant(id);
    } catch (_) {
      /* Profile/native teardown already owns revocation. */
    }
  }

  void activate(RdpTab tab) {
    if (_disposed || !_tabs.contains(tab) || tab.closed) return;
    _active = tab;
    notifyListeners();
  }

  Future<void> _pollActive() async {
    final tab = _active;
    if (_disposed ||
        !_visible ||
        tab == null ||
        tab.closed ||
        tab.polling ||
        (tab.status.phase != RdpPhase.connected && tab.status.phase != RdpPhase.connecting)) {
      return;
    }
    tab.polling = true;
    final permissionEpoch = tab._permissionEpoch;
    try {
      final result = await service.pollFrame(tab.info.id);
      if (_disposed || tab.closed || !_tabs.contains(tab)) return;
      final statusChanged = tab.status.phase != result.status.phase || tab.status.errorCode != result.status.errorCode;
      var changed = statusChanged;
      tab.status = result.status;
      if (permissionEpoch == tab._permissionEpoch && tab.folderState != result.folderState) {
        tab.folderState = result.folderState;
        changed = true;
      }
      final frame = result.frame;
      if (result.status.phase != RdpPhase.connected) {
        changed = changed || tab.frame != null;
        tab.frame = null;
      } else if (frame != null && frame.sequence > tab._lastSequence) {
        tab._lastSequence = frame.sequence;
        tab.frame = frame;
        changed = true;
      }
      if (changed) tab.notifyListeners();
      if (statusChanged) notifyListeners();
    } catch (error) {
      if (!_disposed && !tab.closed) {
        tab.status = RdpStatus(RdpPhase.failed, errorCode: error is RdpFailure ? error.code : 'transport');
        tab.frame = null;
        tab.notifyListeners();
        notifyListeners();
        await _disconnect(tab.info.id);
      }
    } finally {
      tab.polling = false;
    }
  }

  /// A bounded FIFO: a delayed native write cannot reorder keys/buttons.
  /// Queue overflow fails the session instead of silently dropping key-up.
  void send(RdpTab tab, List<RdpInput> inputs) {
    if (_disposed || tab.closed || tab.status.phase != RdpPhase.connected || inputs.isEmpty) return;
    if (inputs.length > 64 ||
        tab._queuedInputs + inputs.length > 256 ||
        inputs.any((input) => input is RdpUnicodeInput && input.text.length > 4096)) {
      tab.status = const RdpStatus(RdpPhase.failed, errorCode: 'input_limit');
      tab.frame = null;
      tab.notifyListeners();
      notifyListeners();
      unawaited(_disconnect(tab.info.id));
      return;
    }
    final batch = List<RdpInput>.unmodifiable(inputs);
    tab._queuedInputs += batch.length;
    tab._inputTail = tab._inputTail.then((_) async {
      try {
        if (!_disposed && !tab.closed && tab.status.phase == RdpPhase.connected) {
          await service.sendInput(tab.info.id, batch);
          if (!_disposed && !tab.closed && tab.noticeCode != null && batch.any((event) => event is RdpResizeInput)) {
            tab.noticeCode = null;
            tab.notifyListeners();
          }
        }
      } catch (error) {
        if (!_disposed && !tab.closed) {
          if (error is RdpFailure &&
              (error.code == 'resize_unavailable' || error.code == 'frame_limit') &&
              batch.every((event) => event is RdpResizeInput)) {
            tab.noticeCode = error.code;
            tab.notifyListeners();
            return;
          }
          tab.status = RdpStatus(RdpPhase.failed, errorCode: error is RdpFailure ? error.code : 'transport');
          tab.frame = null;
          tab.notifyListeners();
          notifyListeners();
          await _disconnect(tab.info.id);
        }
      } finally {
        tab._queuedInputs -= batch.length;
      }
    });
  }

  Future<void> close(RdpTab tab) async {
    if (tab.closed) return;
    tab.closed = true;
    tab.frame = null;
    _tabs.remove(tab);
    if (identical(_active, tab)) _active = _tabs.lastOrNull;
    if (!_disposed) notifyListeners();
    // Tab listeners detach on the next widget rebuild. Do not dispose the
    // notifier before their detach; tab itself owns no native resources.
    await _disconnect(tab.info.id);
    final directory = tab.directory;
    tab.directory = null;
    if (directory != null) await releaseDirectory(directory.id);
  }

  Future<void> closeAll() async {
    _generation++;
    final old = _tabs.toList();
    for (final tab in old) {
      tab.closed = true;
      tab.frame = null;
    }
    _tabs.clear();
    _active = null;
    if (!_disposed) notifyListeners();
    await Future.wait(
      old.map((tab) async {
        await _disconnect(tab.info.id);
        final directory = tab.directory;
        tab.directory = null;
        if (directory != null) await releaseDirectory(directory.id);
      }),
    );
  }

  Future<void> _disconnect(String id) async {
    try {
      await service.disconnect(id);
    } catch (_) {
      // Native disconnect is idempotent. Never log transport/account payloads.
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _timer.cancel();
    unawaited(closeAll());
    super.dispose();
  }
}
