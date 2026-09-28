import 'package:consolecrypt/core/models/ids.dart';

/// Directory entry (local or remote).
final class FileEntry {
  const FileEntry({
    required this.name,
    required this.path,
    required this.isDirectory,
    this.size = 0,
    this.modifiedAt,
    this.permissions,
    this.isSymlink = false,
  });

  final String name;
  final String path;
  final bool isDirectory;
  final int size;
  final DateTime? modifiedAt;

  /// `drwxr-xr-x` style, remote only.
  final String? permissions;
  final bool isSymlink;
}

enum TransferDirection { upload, download }

enum TransferState { queued, running, completed, failed, cancelled }

/// A file transfer with progress (`SftpService.watchTransfers`).
final class TransferJob {
  const TransferJob({
    required this.id,
    required this.direction,
    required this.sourcePath,
    required this.destinationPath,
    required this.totalBytes,
    required this.state,
    this.transferredBytes = 0,
    this.bytesPerSecond = 0,
    this.error,
    this.isDirectory = false,
    this.filesDone = 0,
    this.filesTotal = 0,
    this.currentPath,
  });

  final TransferId id;
  final TransferDirection direction;
  final String sourcePath;
  final String destinationPath;
  final int totalBytes;
  final int transferredBytes;
  final int bytesPerSecond;
  final TransferState state;
  final String? error;

  /// The source is a folder (copied recursively by the core).
  final bool isDirectory;

  /// Files copied so far / in total (folder jobs; `0` = not reported).
  final int filesDone;
  final int filesTotal;

  /// File being copied right now (folder jobs).
  final String? currentPath;

  double get fraction => totalBytes == 0 ? 1 : (transferredBytes / totalBytes).clamp(0, 1);

  bool get isActive => state == TransferState.queued || state == TransferState.running;

  String get fileName => sourcePath.split(RegExp(r'[\\/]')).last;

  TransferJob copyWith({int? transferredBytes, int? bytesPerSecond, TransferState? state, String? error}) =>
      TransferJob(
        id: id,
        direction: direction,
        sourcePath: sourcePath,
        destinationPath: destinationPath,
        totalBytes: totalBytes,
        transferredBytes: transferredBytes ?? this.transferredBytes,
        bytesPerSecond: bytesPerSecond ?? this.bytesPerSecond,
        state: state ?? this.state,
        error: error ?? this.error,
        isDirectory: isDirectory,
        filesDone: filesDone,
        filesTotal: filesTotal,
        currentPath: currentPath,
      );
}

/// POSIX-style path helpers for remote paths.
String joinRemotePath(String dir, String name) => dir.endsWith('/') ? '$dir$name' : '$dir/$name';

String parentRemotePath(String path) {
  if (path == '/' || path.isEmpty) return '/';
  final trimmed = path.endsWith('/') ? path.substring(0, path.length - 1) : path;
  final idx = trimmed.lastIndexOf('/');
  return idx <= 0 ? '/' : trimmed.substring(0, idx);
}
