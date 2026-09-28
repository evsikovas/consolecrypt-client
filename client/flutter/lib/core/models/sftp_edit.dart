import 'package:consolecrypt/core/models/ids.dart';

/// Dart mirror of the edit-session engine (`sftp_core::edit`, ADR-0108):
/// a remote file opened in an external application whose saves are
/// uploaded automatically.

/// `sftp_core::edit::EditSessionInfo::id` (UUID) — also the leftover id.
extension type const EditSessionId(String value) implements Object {
  factory EditSessionId.generate() => EditSessionId(generateUuidV4());
}

/// `platform_core::AppRef` kinds.
enum AppRefKind {
  /// macOS: `.app` bundle path; Windows/Linux: executable path.
  path('path'),

  /// macOS only: application name resolved by LaunchServices.
  name('name'),

  /// macOS only: bundle identifier.
  bundleId('bundle_id');

  const AppRefKind(this.wireName);

  final String wireName;
}

/// A specific application (`platform_core::AppRef`).
final class AppRef {
  const AppRef(this.kind, this.value);

  final AppRefKind kind;
  final String value;

  /// Short display name (`/Applications/TextEdit.app` → `TextEdit`).
  String get displayName {
    if (kind != AppRefKind.path) return value;
    final last = value.split(RegExp(r'[\\/]')).where((p) => p.isNotEmpty).lastOrNull ?? value;
    return last.replaceAll(RegExp(r'\.(app|exe)$', caseSensitive: false), '');
  }

  @override
  bool operator ==(Object other) => other is AppRef && other.kind == kind && other.value == value;

  @override
  int get hashCode => Object.hash(kind, value);
}

/// Which application opens a working copy (`sftp_core::edit::OpenWith`).
sealed class OpenWith {
  const OpenWith();
}

/// "Open": the OS default application for the file type.
final class OpenWithDefault extends OpenWith {
  const OpenWithDefault();
}

/// A specific application (e.g. the one used last time).
final class OpenWithApp extends OpenWith {
  const OpenWithApp(this.app);

  final AppRef app;
}

/// "Open With…": the UI resolves macOS choices through a native application
/// bundle panel before passing [OpenWithApp] to the core. Other platforms
/// use the core chooser (Windows `OpenAs_RunDLL`). The application is reused
/// for re-opens of the session.
final class OpenWithChoose extends OpenWith {
  const OpenWithChoose();
}

/// Remote metadata shown with a conflict (`sftp_core::edit::RemoteMeta`).
final class RemoteFileMeta {
  const RemoteFileMeta({required this.size, this.modifiedAt, this.permissions});

  final int size;
  final DateTime? modifiedAt;
  final int? permissions;
}

/// State of an edit session (`sftp_core::edit::EditStatus`).
sealed class EditStatus {
  const EditStatus();

  /// Needs the user (conflict or failed upload).
  bool get needsAttention => false;
}

/// Downloading and opening the editor.
final class EditStatusOpening extends EditStatus {
  const EditStatusOpening();
}

/// The remote file equals the working copy.
final class EditStatusSynced extends EditStatus {
  const EditStatusSynced();
}

/// The working copy has changes that are not uploaded yet.
final class EditStatusModified extends EditStatus {
  const EditStatusModified();
}

/// Uploading a save.
final class EditStatusUploading extends EditStatus {
  const EditStatusUploading({required this.transferred, this.total});

  final int transferred;

  /// Total size when known.
  final int? total;

  /// 0…1, or `null` when the total is unknown.
  double? get fraction => total == null || total == 0 ? null : (transferred / total!).clamp(0, 1).toDouble();
}

/// The remote file changed since it was downloaded / last uploaded
/// ([remote] `null`: it was deleted). Nothing is uploaded until resolved.
final class EditStatusConflict extends EditStatus {
  const EditStatusConflict({this.remote});

  final RemoteFileMeta? remote;

  @override
  bool get needsAttention => true;
}

/// The last upload failed; the local copy is kept. [message] is the core's
/// English diagnostic (shown as detail only).
final class EditStatusError extends EditStatus {
  const EditStatusError({required this.message, this.retryable = true});

  final String message;
  final bool retryable;

  @override
  bool get needsAttention => true;
}

/// Session ended.
final class EditStatusClosed extends EditStatus {
  const EditStatusClosed();
}

/// Snapshot of an edit session (`sftp_core::edit::EditSessionInfo`).
final class EditSessionInfo {
  const EditSessionInfo({
    required this.id,
    required this.hostId,
    required this.remotePath,
    required this.localPath,
    required this.status,
    required this.openedAt,
    String? targetPath,
    this.sftpSession,
    this.lastSyncedAt,
    this.uploads = 0,
    this.remoteCopies = const [],
    this.app,
  }) : targetPath = targetPath ?? remotePath;

  final EditSessionId id;
  final ObjectId hostId;

  /// The SFTP connection the session uploads through; the session ends when
  /// it is disconnected (final upload attempted first).
  final SftpSessionId? sftpSession;

  /// Remote path as opened.
  final String remotePath;

  /// Remote path that is actually replaced (symlinks resolved).
  final String targetPath;

  /// Private local working copy (`<profile cache>/edit/<uuid>/<name>`).
  final String localPath;
  final EditStatus status;
  final DateTime openedAt;
  final DateTime? lastSyncedAt;

  /// Successful uploads so far.
  final int uploads;

  /// Remote versions saved by [EditConflictResolution.keepRemoteCopyLocally].
  final List<String> remoteCopies;

  /// Application chosen with "Open With…" (reused by "Reopen"), if any.
  final AppRef? app;

  String get fileName => remotePath.split('/').where((p) => p.isNotEmpty).lastOrNull ?? remotePath;

  bool get isActive => status is! EditStatusClosed;

  EditSessionInfo copyWith({
    EditStatus? status,
    DateTime? lastSyncedAt,
    int? uploads,
    List<String>? remoteCopies,
    AppRef? app,
  }) => EditSessionInfo(
    id: id,
    hostId: hostId,
    sftpSession: sftpSession,
    remotePath: remotePath,
    targetPath: targetPath,
    localPath: localPath,
    status: status ?? this.status,
    openedAt: openedAt,
    lastSyncedAt: lastSyncedAt ?? this.lastSyncedAt,
    uploads: uploads ?? this.uploads,
    remoteCopies: remoteCopies ?? this.remoteCopies,
    app: app ?? this.app,
  );
}

/// How to resolve [EditStatusConflict] (`sftp_core::edit::ConflictResolution`).
enum EditConflictResolution {
  /// Upload the local copy over the changed remote file.
  overwriteRemote('overwrite_remote'),

  /// Download the remote version next to the working copy as
  /// `<stem>.remote-<UTC ts>[.<ext>]`, open it for comparison and make it the
  /// new base; the next save of the working copy uploads normally.
  keepRemoteCopyLocally('keep_remote_copy_locally'),

  /// Replace the working copy with the remote version.
  discardLocal('discard_local');

  const EditConflictResolution(this.wireName);

  final String wireName;
}

/// How a session ends (`sftp_core::edit::StopMode`).
enum EditStopMode {
  /// "Stop editing": upload pending changes, then remove the working copy.
  /// On conflict / failure the session stays open ([EditStopOutcome] says why).
  upload('upload'),

  /// Unattended end (vault lock, app quit): like [upload], but on conflict /
  /// failure the files are kept for recovery on the next start.
  uploadOrKeep('upload_or_keep'),

  /// Stop watching, keep the working copy (recoverable leftover).
  keepFiles('keep_files'),

  /// Remove the working copy without uploading.
  discard('discard');

  const EditStopMode(this.wireName);

  final String wireName;
}

/// Result of `stopEditing` (`sftp_core::edit::StopOutcome`).
sealed class EditStopOutcome {
  const EditStopOutcome();
}

/// Closed and the working copy removed.
final class EditStopClosed extends EditStopOutcome {
  const EditStopClosed({required this.uploaded});

  final bool uploaded;
}

/// Closed, the working copy kept (listed as a leftover next time).
final class EditStopKeptFiles extends EditStopOutcome {
  const EditStopKeptFiles(this.directory);

  final String directory;
}

/// Not stopped: the remote changed meanwhile — resolve first.
final class EditStopConflict extends EditStopOutcome {
  const EditStopConflict({this.remote});

  final RemoteFileMeta? remote;
}

/// Not stopped: the final upload failed ([message]: English diagnostic).
final class EditStopUploadFailed extends EditStopOutcome {
  const EditStopUploadFailed(this.message);

  final String message;
}

/// An edit directory left behind by a crash / disconnect / app quit
/// (`sftp_core::edit::Leftover`). Fields are `null` when the manifest is
/// missing or unreadable (such a leftover can only be discarded).
final class EditLeftover {
  const EditLeftover({
    required this.id,
    this.hostId,
    this.remotePath,
    this.targetPath,
    this.createdAt,
    this.workingFile,
    this.locallyModified,
  });

  final EditSessionId id;
  final ObjectId? hostId;
  final String? remotePath;
  final String? targetPath;
  final DateTime? createdAt;
  final String? workingFile;

  /// `true`: the working copy differs from the version last downloaded /
  /// uploaded (unsaved edits that never reached the server).
  final bool? locallyModified;

  String? get fileName => (remotePath ?? workingFile)?.split(RegExp(r'[\\/]')).where((p) => p.isNotEmpty).lastOrNull;

  bool get canResume => hostId != null && remotePath != null && workingFile != null;
}
