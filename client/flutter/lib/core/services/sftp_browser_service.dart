import 'dart:async';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/sftp_browser.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/sftp_service.dart';

export 'package:consolecrypt/core/models/sftp_browser.dart';
export 'package:consolecrypt/core/models/sftp_edit.dart';

/// Transmit-style SFTP browser and "edit in the default app" sessions
/// (docs/design/SFTP_BROWSER_SPEC.md, ADR-0108).
///
/// Additive companion of [SftpService]: connection, plain listing, mkdir,
/// rename, delete, single-file transfers and the transfer list stay there;
/// this interface adds what the new browser needs. Every [SftpSessionId]
/// comes from [SftpService.connect]. Errors are [AppException]s (codes as in
/// [SftpService]: `notFound` + `directoryNotFound`, `conflict` +
/// `alreadyExists`, `forbidden` for permission denied, `unsupported` when
/// the server or the backend cannot do it, `payloadTooLarge` with
/// `args = {size, limit}` for the size limits below).
///
/// Implementation notes for the bridge / app-core (`client/rust/app-core`
/// facade over `sftp_core::SftpClient` + `sftp_core::edit::EditManager`):
/// see each method. Streams follow the `watch*` contract (current value
/// first, then changes).
abstract interface class SftpBrowserService {
  /// Default limit of [readPreview] (Quick Look of text/code).
  static const defaultPreviewBytes = 256 * 1024;

  /// Largest image Quick Look loads.
  static const maxImagePreviewBytes = 8 * 1024 * 1024;

  /// Default limit of [openInEditor] (`EditConfig::max_file_size`).
  static const maxEditFileSize = 50 * 1024 * 1024;

  // Browsing -------------------------------------------------------------------

  /// Entries of [path] with full metadata, `.`/`..` excluded, in any order
  /// (the UI sorts). Symlinks are reported as such (`lstat`) with
  /// [RemoteFileInfo.linkTarget] (`readlink`) and
  /// [RemoteFileInfo.linkTargetKind] (`stat`, `null` if dangling).
  /// Core: `SftpClient::list` + `lstat`/`stat`/`readlink` for links.
  Future<List<RemoteFileInfo>> listDirectory(SftpSessionId session, String path);

  /// Metadata of one entry (`lstat`, link target filled as in [listDirectory]).
  Future<RemoteFileInfo> stat(SftpSessionId session, String path);

  /// Absolute, normalized form of a path typed by the user (`~`, `~/x`,
  /// relative to [base], `..`). Core: `SftpClient::canonicalize`
  /// (`realpath`) — must refer to an existing directory, else `notFound` +
  /// `directoryNotFound`.
  Future<String> resolveDirectory(SftpSessionId session, String input, {required String base});

  /// `chmod` to [mode] (`0o7777` mask). Core: `SftpClient::chmod`.
  Future<void> setPermissions(SftpSessionId session, String path, int mode);

  /// Creates an empty regular file (exclusive create, mode 0644 subject to
  /// the server umask). `conflict` + `alreadyExists` if the name is taken.
  Future<void> createFile(SftpSessionId session, String path);

  /// Server-side copy of a file or (recursively) a directory to [to] in the
  /// same session, keeping permissions. The core streams through the client
  /// (SFTP v3 has no copy) — bounded memory, cancellable in a later version.
  /// `conflict` + `alreadyExists` if [to] exists.
  Future<void> duplicate(SftpSessionId session, String from, String to);

  /// Reads at most [maxBytes] from the start of a regular file into memory
  /// (Quick Look). Never touches the local disk. Core: `SftpClient::download_to`
  /// with a bounded in-memory writer that stops after [maxBytes].
  Future<SftpPreview> readPreview(SftpSessionId session, String path, {int maxBytes = defaultPreviewBytes});

  // Transfers --------------------------------------------------------------------

  /// Queues uploads of local files **or folders** (folders recursively; the
  /// core walks the tree and creates remote directories) into
  /// [remoteDirectory]. One [TransferJob] per top-level item (a folder job
  /// counts all bytes below it). Paths come from the OS (drag & drop from
  /// Finder/Explorer, file dialogs) or from the local pane.
  Future<List<TransferId>> uploadItems(SftpSessionId session, List<String> localPaths, String remoteDirectory);

  /// Queues downloads of remote files or folders into [localDirectory]
  /// (folders recursively). One job per top-level item.
  Future<List<TransferId>> downloadItems(SftpSessionId session, List<String> remotePaths, String localDirectory);

  /// Re-queues a failed or cancelled transfer with the same source and
  /// destination. The old job is removed from [SftpService.watchTransfers];
  /// returns the id of the new job.
  Future<TransferId> retryTransfer(TransferId id);

  /// Removes completed, failed and cancelled jobs from the transfer list.
  Future<void> clearFinishedTransfers();

  // Edit sessions (ADR-0108) -------------------------------------------------------

  /// Downloads [remotePath] into a private working directory, opens it with
  /// [openWith] and uploads every save (conflict check + atomic replace).
  /// If the file is already being edited the existing session is returned
  /// and its working copy opened again (one session per host + resolved
  /// path). The host is the one of [session]; the edit session ends when
  /// [session] is disconnected. `payloadTooLarge` above [maxEditFileSize],
  /// `validation` for anything but a regular file (after resolving symlinks).
  /// Core: `EditManager::open(remote, host_id, path, with)`.
  Future<EditSessionInfo> openInEditor(
    SftpSessionId session,
    String remotePath, {
    OpenWith openWith = const OpenWithDefault(),
  });

  /// All active edit sessions of the profile (every host), oldest first.
  /// Closed sessions disappear from the list. Core: `EditManager::sessions`
  /// + the `EditEvent` broadcast.
  Stream<List<EditSessionInfo>> watchEditSessions();

  /// "Sync now" / "Retry": checks the working copy and uploads it if it
  /// changed (conflict check first). Core: `EditSession::sync_now`.
  Future<void> syncEditSession(EditSessionId id);

  /// Opens the working copy again ([openWith] `null` = the application the
  /// session used). Core: `EditSession::reopen`.
  Future<void> reopenEditSession(EditSessionId id, {OpenWith? openWith});

  /// Shows the working copy in Finder / Explorer (`open -R`,
  /// `explorer /select,`). Core: `platform_core::FileOpener::reveal`.
  Future<void> revealEditSession(EditSessionId id);

  /// Resolves [EditStatusConflict]. Core: `EditSession::resolve`.
  Future<void> resolveEditConflict(EditSessionId id, EditConflictResolution resolution);

  /// Ends a session. With [EditStopMode.upload] pending changes are uploaded
  /// first; on conflict / failure the session stays open and the outcome
  /// says why. Core: `EditSession::stop`.
  Future<EditStopOutcome> stopEditing(EditSessionId id, {EditStopMode mode = EditStopMode.upload});

  /// Edit directories left by a crash / quit (listed once per app start
  /// after unlock: "Recover unsaved edits?"). Core: `EditManager::leftovers`.
  Future<List<EditLeftover>> listEditLeftovers();

  /// Continues a leftover through [session] (must be connected to the
  /// leftover's host): the working copy is checked and uploaded if it
  /// differs from its recorded base (conflict check included).
  /// [openWith] non-null also opens it in an editor. Core: `EditManager::resume`.
  Future<EditSessionInfo> resumeEditLeftover(EditSessionId id, SftpSessionId session, {OpenWith? openWith});

  /// Securely removes one leftover. Core: `EditManager::discard_leftover`.
  Future<void> discardEditLeftover(EditSessionId id);

  // Preferences ------------------------------------------------------------------

  /// Device-local browser preferences (hidden files, sorting, columns, local
  /// pane, first-use hint). Never synced; the core stores
  /// [SftpBrowserPreferences.toJson] verbatim in local settings.
  Future<SftpBrowserPreferences> loadPreferences();

  Future<void> savePreferences(SftpBrowserPreferences preferences);
}

/// Demo hooks of the mock backend (the Editing panel shows them only when
/// `AppServices.developer` is present). The FRB backend never implements it.
abstract interface class SftpBrowserDebugControls {
  /// Pretends the external editor saved the working copy (→ upload).
  void debugSimulateSave(EditSessionId id);

  /// Pretends someone changed the file on the server (next save → conflict).
  void debugSimulateRemoteChange(EditSessionId id);

  /// Makes the next upload of the session fail (→ `Error` with Retry).
  void debugFailNextUpload(EditSessionId id);

  /// Makes the next transfer fail half-way (→ Retry in the transfer list).
  void debugFailNextTransfer();
}

/// Minimal [SftpBrowserService] on top of a plain [SftpService], used while
/// a backend does not provide the browser service yet
/// (`AppServices.sftpBrowser == null`). Listing, transfers and preferences
/// work; permissions, preview, new file, duplicate and editing report
/// `unsupported`.
final class SftpBrowserFallback implements SftpBrowserService {
  SftpBrowserFallback(this._sftp);

  final SftpService _sftp;
  SftpBrowserPreferences _preferences = const SftpBrowserPreferences();

  static Never _unsupported(String what) => throw AppException(AppErrorCode.unsupported, '$what is not available');

  static RemoteFileInfo _fromEntry(FileEntry e) => RemoteFileInfo(
    name: e.name,
    path: e.path,
    kind: e.isSymlink ? RemoteEntryKind.symlink : (e.isDirectory ? RemoteEntryKind.directory : RemoteEntryKind.file),
    size: e.size,
    permissions: e.permissions == null ? null : parseModeString(e.permissions!),
    modifiedAt: e.modifiedAt,
    linkTargetKind: e.isSymlink ? (e.isDirectory ? RemoteEntryKind.directory : RemoteEntryKind.file) : null,
  );

  @override
  Future<List<RemoteFileInfo>> listDirectory(SftpSessionId session, String path) async => [
    for (final e in await _sftp.listRemote(session, path)) _fromEntry(e),
  ];

  @override
  Future<RemoteFileInfo> stat(SftpSessionId session, String path) async {
    final parent = parentRemotePath(path);
    final entries = await listDirectory(session, parent);
    final match = entries.where((e) => e.path == path || joinRemotePath(parent, e.name) == path).firstOrNull;
    if (match == null) throw AppException(AppErrorCode.notFound, 'No such file: $path');
    return match;
  }

  @override
  Future<String> resolveDirectory(SftpSessionId session, String input, {required String base}) async {
    final home = await _sftp.remoteHome(session);
    final path = normalizeRemotePath(input, base: base, home: home);
    await _sftp.listRemote(session, path); // throws directoryNotFound
    return path;
  }

  @override
  Future<void> setPermissions(SftpSessionId session, String path, int mode) async => _unsupported('chmod');

  @override
  Future<void> createFile(SftpSessionId session, String path) async => _unsupported('createFile');

  @override
  Future<void> duplicate(SftpSessionId session, String from, String to) async => _unsupported('duplicate');

  @override
  Future<SftpPreview> readPreview(
    SftpSessionId session,
    String path, {
    int maxBytes = SftpBrowserService.defaultPreviewBytes,
  }) async => _unsupported('preview');

  @override
  Future<List<TransferId>> uploadItems(SftpSessionId session, List<String> localPaths, String remoteDirectory) async =>
      [for (final p in localPaths) await _sftp.upload(session, localPath: p, remoteDirectory: remoteDirectory)];

  @override
  Future<List<TransferId>> downloadItems(
    SftpSessionId session,
    List<String> remotePaths,
    String localDirectory,
  ) async => [
    for (final p in remotePaths) await _sftp.download(session, remotePath: p, localDirectory: localDirectory),
  ];

  @override
  Future<TransferId> retryTransfer(TransferId id) async => _unsupported('retry');

  @override
  Future<void> clearFinishedTransfers() async {}

  @override
  Future<EditSessionInfo> openInEditor(
    SftpSessionId session,
    String remotePath, {
    OpenWith openWith = const OpenWithDefault(),
  }) async => _unsupported('editSessions');

  @override
  Stream<List<EditSessionInfo>> watchEditSessions() => Stream.value(const []);

  @override
  Future<void> syncEditSession(EditSessionId id) async => _unsupported('editSessions');

  @override
  Future<void> reopenEditSession(EditSessionId id, {OpenWith? openWith}) async => _unsupported('editSessions');

  @override
  Future<void> revealEditSession(EditSessionId id) async => _unsupported('editSessions');

  @override
  Future<void> resolveEditConflict(EditSessionId id, EditConflictResolution resolution) async =>
      _unsupported('editSessions');

  @override
  Future<EditStopOutcome> stopEditing(EditSessionId id, {EditStopMode mode = EditStopMode.upload}) async =>
      _unsupported('editSessions');

  @override
  Future<List<EditLeftover>> listEditLeftovers() async => const [];

  @override
  Future<EditSessionInfo> resumeEditLeftover(EditSessionId id, SftpSessionId session, {OpenWith? openWith}) async =>
      _unsupported('editSessions');

  @override
  Future<void> discardEditLeftover(EditSessionId id) async => _unsupported('editSessions');

  @override
  Future<SftpBrowserPreferences> loadPreferences() async => _preferences;

  @override
  Future<void> savePreferences(SftpBrowserPreferences preferences) async => _preferences = preferences;
}

/// Normalizes a typed remote path: `~` / `~/x` → [home], relative → under
/// [base], `.`/`..`/duplicate slashes collapsed. Pure (no server round trip).
String normalizeRemotePath(String input, {required String base, required String home}) {
  var text = input.trim();
  if (text.isEmpty) return base;
  if (text == '~') {
    text = home;
  } else if (text.startsWith('~/')) {
    text = joinRemotePath(home, text.substring(2));
  } else if (!text.startsWith('/')) {
    text = joinRemotePath(base, text);
  }
  final parts = <String>[];
  for (final part in text.split('/')) {
    if (part.isEmpty || part == '.') continue;
    if (part == '..') {
      if (parts.isNotEmpty) parts.removeLast();
    } else {
      parts.add(part);
    }
  }
  return '/${parts.join('/')}';
}
