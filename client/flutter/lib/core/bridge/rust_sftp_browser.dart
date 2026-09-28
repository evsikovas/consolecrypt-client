import 'dart:async';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/bridge/rust_sessions.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:consolecrypt/src/rust/api/sftp.dart' as rs_sftp;

/// [SftpBrowserService] on app-core (SFTP_BROWSER_SPEC, ADR-0108): detailed
/// listings, stat / resolve / chmod / new file / duplicate / preview,
/// recursive transfer jobs (run in the core; listed by [RustSftpService])
/// and edit sessions (`EditManager` in the core; status via core events).
final class RustSftpBrowserService implements SftpBrowserService {
  RustSftpBrowserService(this._hub, this._sftp) {
    _hub
      ..eventListeners.add(_onEvent)
      ..onSessionReset.add(_onReset);
  }

  final RustBackend _hub;
  final RustSftpService _sftp;
  final _sessions = ValueStreamController<List<EditSessionInfo>>(const []);
  Future<void>? _refreshing;
  bool _refreshAgain = false;

  static const _preferencesKey = 'sftp_browser_preferences';

  void _onEvent(Json e) {
    switch (e['type']) {
      case 'edit_status' || 'edit_leftovers' || 'vault_unlocked' || 'lagged':
        unawaited(_refreshSessions());
    }
  }

  void _onReset() => _sessions.value = const [];

  /// Re-reads the session list (coalesced).
  Future<void> _refreshSessions() {
    final running = _refreshing;
    if (running != null) {
      _refreshAgain = true;
      return running;
    }
    return _refreshing = () async {
      try {
        do {
          _refreshAgain = false;
          try {
            _sessions.value = List.unmodifiable(decodeList(await guard(rs_sftp.editSessions)).map(editSessionFromJson));
          } on AppException {
            _sessions.value = const [];
          }
        } while (_refreshAgain);
      } finally {
        _refreshing = null;
      }
    }();
  }

  // Browsing -------------------------------------------------------------------

  @override
  Future<List<RemoteFileInfo>> listDirectory(SftpSessionId session, String path) async => [
    for (final j in decodeList(await guard(() => rs_sftp.sftpListDetailed(sftpId: session.value, path: path))))
      remoteFileInfoFromJson(j),
  ];

  @override
  Future<RemoteFileInfo> stat(SftpSessionId session, String path) async =>
      remoteFileInfoFromJson(decodeObject(await guard(() => rs_sftp.sftpStat(sftpId: session.value, path: path))));

  @override
  Future<String> resolveDirectory(SftpSessionId session, String input, {required String base}) =>
      guard(() => rs_sftp.sftpResolveDirectory(sftpId: session.value, input: input, base: base));

  @override
  Future<void> setPermissions(SftpSessionId session, String path, int mode) =>
      guard(() => rs_sftp.sftpChmod(sftpId: session.value, path: path, mode: mode & 0xFFF));

  @override
  Future<void> createFile(SftpSessionId session, String path) =>
      guard(() => rs_sftp.sftpCreateFile(sftpId: session.value, path: path));

  @override
  Future<void> duplicate(SftpSessionId session, String from, String to) =>
      guard(() => rs_sftp.sftpDuplicate(sftpId: session.value, from: from, to: to));

  @override
  Future<SftpPreview> readPreview(
    SftpSessionId session,
    String path, {
    int maxBytes = SftpBrowserService.defaultPreviewBytes,
  }) async {
    final p = await guard(() => rs_sftp.sftpReadPreview(sftpId: session.value, path: path, maxBytes: maxBytes));
    return SftpPreview(path: p.path, bytes: p.data, totalSize: p.totalSize);
  }

  // Transfers --------------------------------------------------------------------

  @override
  Future<List<TransferId>> uploadItems(SftpSessionId session, List<String> localPaths, String remoteDirectory) async =>
      [for (final p in localPaths) _sftp.startUploadItem(session, localPath: p, remoteDirectory: remoteDirectory)];

  @override
  Future<List<TransferId>> downloadItems(
    SftpSessionId session,
    List<String> remotePaths,
    String localDirectory,
  ) async => [
    for (final p in remotePaths) _sftp.startDownloadItem(session, remotePath: p, localDirectory: localDirectory),
  ];

  @override
  Future<TransferId> retryTransfer(TransferId id) async => _sftp.retry(id);

  @override
  Future<void> clearFinishedTransfers() => _sftp.clearFinished();

  // Edit sessions -------------------------------------------------------------------

  @override
  Future<EditSessionInfo> openInEditor(
    SftpSessionId session,
    String remotePath, {
    OpenWith openWith = const OpenWithDefault(),
  }) async {
    final info = editSessionFromJson(
      decodeObject(
        await guard(
          () => rs_sftp.editOpen(sftpId: session.value, remotePath: remotePath, openWithJson: openWithToJson(openWith)),
        ),
      ),
    );
    await _refreshSessions();
    return info;
  }

  @override
  Stream<List<EditSessionInfo>> watchEditSessions() {
    unawaited(_refreshSessions());
    return _sessions.stream;
  }

  @override
  Future<void> syncEditSession(EditSessionId id) async {
    await guard(() => rs_sftp.editSyncNow(sessionId: id.value));
    await _refreshSessions();
  }

  @override
  Future<void> reopenEditSession(EditSessionId id, {OpenWith? openWith}) => guard(
    () => rs_sftp.editReopen(sessionId: id.value, openWithJson: openWith == null ? null : openWithToJson(openWith)),
  );

  @override
  Future<void> revealEditSession(EditSessionId id) => guard(() => rs_sftp.editReveal(sessionId: id.value));

  @override
  Future<void> resolveEditConflict(EditSessionId id, EditConflictResolution resolution) async {
    await guard(
      () => rs_sftp.editResolve(
        sessionId: id.value,
        resolution: switch (resolution) {
          EditConflictResolution.overwriteRemote => rs_sftp.EditConflictChoice.overwriteRemote,
          EditConflictResolution.keepRemoteCopyLocally => rs_sftp.EditConflictChoice.keepRemoteCopyLocally,
          EditConflictResolution.discardLocal => rs_sftp.EditConflictChoice.discardLocal,
        },
      ),
    );
    await _refreshSessions();
  }

  @override
  Future<EditStopOutcome> stopEditing(EditSessionId id, {EditStopMode mode = EditStopMode.upload}) async {
    final outcome = editStopOutcomeFromJson(
      decodeObject(
        await guard(
          () => rs_sftp.editStop(
            sessionId: id.value,
            mode: switch (mode) {
              EditStopMode.upload => rs_sftp.EditStopChoice.upload,
              EditStopMode.uploadOrKeep => rs_sftp.EditStopChoice.uploadOrKeep,
              EditStopMode.keepFiles => rs_sftp.EditStopChoice.keepFiles,
              EditStopMode.discard => rs_sftp.EditStopChoice.discard,
            },
          ),
        ),
      ),
    );
    await _refreshSessions();
    return outcome;
  }

  @override
  Future<List<EditLeftover>> listEditLeftovers() async => [
    for (final j in decodeList(await guard(rs_sftp.editLeftovers))) editLeftoverFromJson(j),
  ];

  @override
  Future<EditSessionInfo> resumeEditLeftover(EditSessionId id, SftpSessionId session, {OpenWith? openWith}) async {
    final info = editSessionFromJson(
      decodeObject(
        await guard(
          () => rs_sftp.editResume(
            sessionId: id.value,
            sftpId: session.value,
            openWithJson: openWith == null ? null : openWithToJson(openWith),
          ),
        ),
      ),
    );
    await _refreshSessions();
    return info;
  }

  @override
  Future<void> discardEditLeftover(EditSessionId id) => guard(() => rs_sftp.editDiscardLeftover(sessionId: id.value));

  // Preferences ------------------------------------------------------------------

  @override
  Future<SftpBrowserPreferences> loadPreferences() async {
    final raw = await _hub.storeGet(_preferencesKey);
    if (raw == null) return const SftpBrowserPreferences();
    try {
      return SftpBrowserPreferences.fromJson(decodeObject(raw));
    } on FormatException {
      return const SftpBrowserPreferences();
    }
  }

  @override
  Future<void> savePreferences(SftpBrowserPreferences preferences) =>
      _hub.storeSet(_preferencesKey, encodeJson(preferences.toJson()));

  Future<void> dispose() async {
    _hub.eventListeners.remove(_onEvent);
    await _sessions.close();
  }
}
