import 'package:consolecrypt/core/models/models.dart';

/// SFTP browsing and transfers (sftp-core) plus local directory listing.
abstract interface class SftpService {
  Future<SftpSessionId> connect(ObjectId hostId);

  Future<void> disconnect(SftpSessionId id);

  Future<String> remoteHome(SftpSessionId id);

  /// Directories first, then files, each sorted by name.
  Future<List<FileEntry>> listRemote(SftpSessionId id, String path);

  Future<String> localHome();

  Future<List<FileEntry>> listLocal(String path);

  Future<void> makeRemoteDirectory(SftpSessionId id, String path);

  Future<void> renameRemote(SftpSessionId id, String from, String to);

  Future<void> deleteRemote(SftpSessionId id, String path, {bool recursive = false});

  /// Queues an upload; progress on [watchTransfers].
  Future<TransferId> upload(SftpSessionId id, {required String localPath, required String remoteDirectory});

  Future<TransferId> download(SftpSessionId id, {required String remotePath, required String localDirectory});

  Future<void> cancelTransfer(TransferId id);

  /// All transfers of this app run (most recent first), with live progress.
  Stream<List<TransferJob>> watchTransfers();
}
