import 'dart:convert';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:crypto/crypto.dart';
import 'package:path/path.dart' as p;

class SshProject extends ProjectFiles {
  SshProject(String root, this.host, this.session, this.sftp, this.browser) : super(root, p.posix);
  final Host host;
  final SftpSessionId session;
  final SftpService sftp;
  final SftpBrowserService browser;

  Future<void> _directory(String path) async {
    if (!(path == root || paths.isWithin(root, path)) ||
        paths.normalize(path) != path ||
        await browser.resolveDirectory(session, path, base: root) != path) {
      throw EditorIssue.outside;
    }
  }

  @override
  Future<List<ProjectEntry>> children(String path) async {
    await _directory(path);
    final entries =
        [
          for (final item in await browser.listDirectory(session, path))
            if ((item.kind == RemoteEntryKind.directory || item.kind == RemoteEntryKind.file) &&
                item.name != '.git' &&
                item.name != '.' &&
                item.name != '..' &&
                item.name.isNotEmpty &&
                !item.name.contains('/') &&
                paths.join(path, item.name) == item.path)
              (path: item.path, directory: item.kind == RemoteEntryKind.directory),
        ]..sort(
          (a, b) => a.directory == b.directory
              ? a.path.toLowerCase().compareTo(b.path.toLowerCase())
              : a.directory
              ? -1
              : 1,
        );
    return entries;
  }

  Future<void> _checkFilePath(String path) async {
    if (!paths.isWithin(root, path) || paths.normalize(path) != path) throw EditorIssue.outside;
    await _directory(paths.dirname(path));
  }

  @override
  Future<String> createPath(String path) async {
    await _checkFilePath(path);
    await browser.createFile(session, path);
    return path;
  }

  Future<void> _requireFree(String target) async {
    try {
      await browser.stat(session, target);
    } on AppException catch (error) {
      if (error.code == AppErrorCode.notFound) return;
      rethrow;
    }
    throw AppException(
      AppErrorCode.conflict,
      'Destination exists',
      reason: AppErrorReason.alreadyExists,
      args: {'name': paths.basename(target)},
    );
  }

  @override
  Future<void> createDirectory(String path) async {
    await _checkFilePath(path);
    await _requireFree(path);
    await sftp.makeRemoteDirectory(session, path);
  }

  Future<void> _checkEntry(ProjectEntry entry) async {
    await _checkFilePath(entry.path);
    final kind = (await browser.stat(session, entry.path)).kind;
    if (kind != (entry.directory ? RemoteEntryKind.directory : RemoteEntryKind.file)) throw EditorIssue.changed;
    if (entry.directory) await _directory(entry.path);
  }

  @override
  Future<void> relocate(ProjectEntry entry, String target) async {
    await _checkEntry(entry);
    await _checkFilePath(target);
    if (target == entry.path) return;
    if (entry.directory && paths.isWithin(entry.path, target)) throw EditorIssue.outside;
    await _requireFree(target);
    await sftp.renameRemote(session, entry.path, target);
  }

  @override
  Future<void> duplicate(ProjectEntry entry, String name) async {
    await _checkEntry(entry);
    final parent = paths.dirname(entry.path), target = namedChild(parent, name);
    await _requireFree(target);
    final staging = paths.join(parent, '.consolecrypt-copy-${generateUuidV4()}');
    await sftp.makeRemoteDirectory(session, staging);
    try {
      final staged = paths.join(staging, name);
      await browser.duplicate(session, entry.path, staged);
      await sftp.renameRemote(session, staged, target);
    } finally {
      await sftp.deleteRemote(session, staging, recursive: true);
    }
  }

  @override
  Future<void> remove(ProjectEntry entry) async {
    await _checkEntry(entry);
    await sftp.deleteRemote(session, entry.path, recursive: entry.directory);
  }

  @override
  Future<void> importFile(String localPath, String directory) async {
    await requireRegularFile(localPath);
    await _directory(directory);
    final name = p.basename(localPath);
    if (name.contains('\\')) throw EditorIssue.unsupported;
    final target = paths.join(directory, name);
    await _requireFree(target);
    final staging = paths.join(directory, '.consolecrypt-import-${generateUuidV4()}');
    await _requireFree(staging);
    await sftp.makeRemoteDirectory(session, staging);
    await _directory(staging);
    final stagedFile = paths.join(staging, name);
    try {
      final id = (await browser.uploadItems(session, [localPath], staging)).single;
      final jobs = await sftp.watchTransfers().firstWhere((jobs) => jobs.any((job) => job.id == id && !job.isActive));
      final job = jobs.firstWhere((job) => job.id == id);
      if (job.state != TransferState.completed) throw StateError(job.error ?? job.state.name);
      await sftp.renameRemote(session, stagedFile, target);
    } finally {
      try {
        await sftp.deleteRemote(session, stagedFile);
      } on AppException catch (error) {
        if (error.code != AppErrorCode.notFound) rethrow;
      }
      await sftp.deleteRemote(session, staging);
    }
  }

  @override
  Future<ProjectText> read(String path) async {
    await _checkFilePath(path);
    final entry = await browser.stat(session, path);
    if (entry.kind != RemoteEntryKind.file || entry.size > editorFileLimit) throw EditorIssue.unsupported;
    final preview = await browser.readPreview(session, path, maxBytes: editorFileLimit + 1);
    if (preview.truncated) throw EditorIssue.unsupported;
    return ProjectText.decode(path, preview.bytes);
  }

  @override
  Future<ProjectText> save(ProjectText original, String text) async {
    await _checkFilePath(original.path);
    final next = ProjectText.decode(original.path, original.encode(text));
    if (!await browser.saveEditorFile(session, original.path, sha256.convert(original.bytes).toString(), next.bytes)) {
      throw EditorIssue.changed;
    }
    return next;
  }

  @override
  Future<ProjectBranches?> branches() async {
    final raw = await browser.editorGitRefs(session, root);
    if (raw == null) return null;
    final lines = const LineSplitter().convert(raw);
    if (lines.isEmpty) return null;
    const heads = 'refs/heads/';
    final symbolic = lines.first.startsWith(heads);
    return ProjectBranches.parse(
      symbolic ? lines.first.substring(heads.length) : lines.first,
      lines.skip(1),
      symbolic: symbolic,
    );
  }

  @override
  Future<void> close() => sftp.disconnect(session);
}
