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
