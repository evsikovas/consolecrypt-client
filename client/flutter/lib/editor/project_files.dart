import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path/path.dart' as p;

enum EditorIssue implements Exception { unsupported, changed, outside, gitMissing, invalidName, sudoUnavailable }

const editorFileLimit = 4 * 1024 * 1024;

typedef ProjectEntry = ({String path, bool directory});

/// Maps paths in open buffers and expanded folders after a filesystem mutation.
class ProjectChange {
  const ProjectChange(this.entry, this.target);
  final ProjectEntry entry;
  final String? target;

  bool affects(p.Context paths, String path) =>
      paths.equals(entry.path, path) || (entry.directory && paths.isWithin(entry.path, path));

  String? remap(p.Context paths, String path) {
    if (!affects(paths, path)) return path;
    final dest = target;
    if (dest == null || paths.equals(entry.path, path)) return dest;
    return paths.join(dest, paths.relative(path, from: entry.path));
  }
}

String? resolveMarkdownFile(p.Context paths, String root, String source, String href) {
  final uri = Uri.tryParse(href);
  if (uri == null || uri.hasScheme || uri.hasAuthority || uri.path.isEmpty) return null;
  final parts = uri.pathSegments;
  if (parts.any((part) => part.contains('/') || part.contains('\\') || part.contains('\x00'))) {
    throw EditorIssue.outside;
  }
  final base = uri.hasAbsolutePath ? root : paths.dirname(source);
  final path = paths.normalize(paths.joinAll([base, ...parts]));
  if (!paths.isWithin(root, path)) throw EditorIssue.outside;
  return path;
}

abstract class ProjectFiles {
  ProjectFiles(this.root, this.paths);
  String root;
  final p.Context paths;
  static Future<LocalProjectFiles> open(String path) async =>
      LocalProjectFiles(await Directory(path).resolveSymbolicLinks());
  Future<List<ProjectEntry>> children(String path);
  Future<String> createPath(String path);
  Future<void> createDirectory(String path);
  Future<void> relocate(ProjectEntry entry, String target);
  Future<void> remove(ProjectEntry entry);
  Future<void> duplicate(ProjectEntry entry, String name);

  Future<String> moveFile(String path, String directory) async {
    final target = paths.join(directory, paths.basename(path));
    await relocate((path: path, directory: false), target);
    return target;
  }

  String namedChild(String parent, String name) {
    if (name.trim().isEmpty ||
        name == '.' ||
        name == '..' ||
        name == '.git' ||
        name.contains('/') ||
        name.contains('\x00') ||
        paths.basename(name) != name ||
        (paths.style == p.Style.windows &&
            (name.contains(RegExp(r'[<>:"\\|?*\x00-\x1f]')) ||
                name.endsWith('.') ||
                name.endsWith(' ') ||
                RegExp(r'^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\.|$)', caseSensitive: false).hasMatch(name)))) {
      throw EditorIssue.invalidName;
    }
    return paths.join(parent, name);
  }

  void checkChild(String path) {
    if (!paths.isWithin(root, path) || paths.normalize(path) != path) throw EditorIssue.outside;
  }

  Future<void> importFile(String localPath, String directory);
  Future<ProjectText> read(String path);
  Future<ProjectText> save(ProjectText original, String text);
  Future<ProjectBranches?> branches();
  Future<void> close() async {}
}

class LocalProjectFiles extends ProjectFiles {
  LocalProjectFiles(String root) : super(root, p.context);

  @override
  Future<ProjectBranches?> branches() => ProjectBranches.read(root);

  @override
  Future<String> createPath(String path) async => (await create(path)).path;

  @override
  Future<void> createDirectory(String path) async {
    checkChild(path);
    await _check(paths.dirname(path));
    await _requireFree(path);
    await Directory(path).create();
  }

  Future<void> _requireFree(String path) async {
    // ignore: avoid_slow_async_io
    if (await FileSystemEntity.type(path, followLinks: false) != FileSystemEntityType.notFound) {
      throw PathExistsException(path, const OSError('Destination exists'));
    }
  }

  Future<FileSystemEntity> _entry(ProjectEntry entry) async {
    checkChild(entry.path);
    await _check(entry.path);
    // ignore: avoid_slow_async_io
    final type = await FileSystemEntity.type(entry.path, followLinks: false);
    if (entry.directory && type == FileSystemEntityType.directory) return Directory(entry.path);
    if (!entry.directory && type == FileSystemEntityType.file) return File(entry.path);
    throw EditorIssue.changed;
  }

  @override
  Future<void> relocate(ProjectEntry entry, String target) async {
    final source = await _entry(entry);
    checkChild(target);
    await _check(paths.dirname(target));
    if (entry.path == target) return;
    if (entry.directory && paths.isWithin(entry.path, target)) throw EditorIssue.outside;
    final parent = source.parent;
    final name = paths.basename(target);
    if (paths.equals(parent.path, paths.dirname(target)) &&
        paths.basename(entry.path).toLowerCase() == name.toLowerCase() &&
        // A project may be on a network share. Keep the check off the UI thread.
        // ignore: avoid_slow_async_io
        await (entry.directory ? Directory(target) : File(target)).exists() &&
        await FileSystemEntity.identical(entry.path, target)) {
      // Case-insensitive volumes resolve both spellings to one entry. Distinct hard links also
      // share an inode, so only use a direct rename if the new spelling is absent from the listing.
      final listed = await parent.list(followLinks: false).any((child) => paths.basename(child.path) == name);
      if (!listed) {
        await source.rename(target);
        return;
      }
    }
    if (entry.directory) {
      await _requireFree(target);
      await source.rename(target);
    } else {
      final reserved = await create(target);
      try {
        await source.rename(target);
      } catch (_) {
        await reserved.delete();
        rethrow;
      }
    }
  }

  @override
  Future<void> duplicate(ProjectEntry entry, String name) async {
    final source = await _entry(entry);
    final target = namedChild(paths.dirname(entry.path), name);
    await _requireFree(target);
    final staging = await source.parent.createTemp('.consolecrypt-copy-');
    final staged = paths.join(staging.path, name);
    try {
      await _copyEntry(source, staged);
      await relocate((path: staged, directory: entry.directory), target);
    } finally {
      await staging.delete(recursive: true);
    }
  }

  Future<void> _copyEntry(FileSystemEntity source, String dest) async {
    if (source is Link) {
      await Link(dest).create(await source.target());
    } else if (source is Directory) {
      await Directory(dest).create();
      await for (final child in source.list(followLinks: false)) {
        await _copyEntry(child, paths.join(dest, paths.basename(child.path)));
      }
    } else if (source is File) {
      await requireRegularFile(source.path);
      await source.copy(dest);
    } else {
      throw EditorIssue.unsupported;
    }
  }

  @override
  Future<void> remove(ProjectEntry entry) async {
    final source = await _entry(entry);
    await source.delete(recursive: entry.directory);
  }

  @override
  Future<void> importFile(String localPath, String directory) async {
    await requireRegularFile(localPath);
    await _check(directory);
    final target = p.join(directory, p.basename(localPath));
    final reserved = await create(target);
    try {
      await File(localPath).copy(target);
    } catch (_) {
      await reserved.delete();
      rethrow;
    }
  }

  Future<void> _check(String path) async {
    if (!p.equals(path, root) && !p.isWithin(root, path)) throw EditorIssue.outside;
    final resolved = await File(path).resolveSymbolicLinks();
    if (!p.equals(path, resolved)) throw EditorIssue.outside;
  }

  @override
  Future<List<ProjectEntry>> children(String path) async {
    await _check(path);
    final entries = await Directory(path)
        .list(followLinks: false)
        .where((entry) => (entry is Directory || entry is File) && p.basename(entry.path) != '.git')
        .toList();
    entries.sort((a, b) {
      if ((a is Directory) != (b is Directory)) return a is Directory ? -1 : 1;
      return p.basename(a.path).toLowerCase().compareTo(p.basename(b.path).toLowerCase());
    });
    return [for (final entry in entries) (path: entry.path, directory: entry is Directory)];
  }

  Future<File> create(String path) async {
    await _check(p.dirname(path));
    if (!p.isWithin(root, path) || p.basename(path).isEmpty) throw EditorIssue.outside;
    return File(path).create(exclusive: true);
  }

  @override
  Future<ProjectText> read(String path) async {
    await _check(path);
    await requireRegularFile(path);
    final handle = await File(path).open();
    late Uint8List bytes;
    try {
      bytes = await handle.read(editorFileLimit + 1);
    } finally {
      await handle.close();
    }
    return ProjectText.decode(path, bytes);
  }

  @override
  Future<ProjectText> save(ProjectText original, String text) async {
    final file = File(original.path);
    await _check(file.path);
    final bytes = original.encode(text);
    if (bytes.length > editorFileLimit) throw EditorIssue.unsupported;
    final temporary = await file.parent.createTemp('.consolecrypt-editor-');
    try {
      final staged = await file.copy(p.join(temporary.path, 'content'));
      await staged.writeAsBytes(bytes, flush: true);
      final disk = await read(file.path);
      if (!listEquals(original.bytes, disk.bytes)) throw EditorIssue.changed;
      await staged.rename(file.path);
      return ProjectText(file.path, bytes, utf8.decode(bytes));
    } finally {
      await temporary.delete(recursive: true);
    }
  }
}

Future<void> requireRegularFile(String path) async {
  // Network-backed local projects must not block the UI thread.
  // ignore: avoid_slow_async_io
  if (await FileSystemEntity.type(path, followLinks: false) != FileSystemEntityType.file) throw EditorIssue.unsupported;
}

class ProjectText {
  factory ProjectText.decode(String path, Uint8List bytes) {
    if (bytes.length > editorFileLimit || bytes.contains(0)) throw EditorIssue.unsupported;
    try {
      return ProjectText(path, bytes, utf8.decode(bytes));
    } on FormatException {
      throw EditorIssue.unsupported;
    }
  }
  ProjectText(this.path, this.bytes, this.original);
  final String path, original;
  final Uint8List bytes;
  String get text => original.replaceAll('\r\n', '\n');

  Uint8List encode(String text) {
    final newline = original.contains('\r\n') ? '\r\n' : '\n';
    return Uint8List.fromList([
      if (bytes.length >= 3 && bytes[0] == 0xef && bytes[1] == 0xbb && bytes[2] == 0xbf) ...[0xef, 0xbb, 0xbf],
      ...utf8.encode(text.replaceAll('\r\n', '\n').replaceAll('\n', newline)),
    ]);
  }
}

class ProjectBranches {
  const ProjectBranches(this.current, this.local, this.remote);
  final String current;
  final List<String> local, remote;

  static Future<ProjectBranches?> read(String directory) async {
    Future<ProcessResult> git(List<String> args) async {
      try {
        return await Process.run(
          'git',
          ['-C', directory, ...args],
          environment: {'GIT_OPTIONAL_LOCKS': '0', 'LC_ALL': 'C'},
          stdoutEncoding: const Utf8Codec(allowMalformed: true),
          stderrEncoding: const Utf8Codec(allowMalformed: true),
        );
      } on ProcessException {
        throw EditorIssue.gitMissing;
      }
    }

    final refs = await git(['for-each-ref', '--format=%(refname)%09%(symref)', 'refs/heads', 'refs/remotes']);
    if (refs.exitCode != 0) {
      if (refs.stderr.toString().contains('not a git repository')) return null;
      throw FileSystemException(refs.stderr.toString().trim(), directory);
    }
    final head = await git(['symbolic-ref', '--quiet', '--short', 'HEAD']);
    final current = head.exitCode == 0
        ? head.stdout.toString().trim()
        : (await git(['rev-parse', '--short', 'HEAD'])).stdout.toString().trim();
    return parse(current, const LineSplitter().convert(refs.stdout.toString()), symbolic: head.exitCode == 0);
  }

  static ProjectBranches parse(String current, Iterable<String> refs, {required bool symbolic}) {
    final local = <String>[], remote = <String>[];
    for (final line in refs) {
      final fields = line.split('\t');
      if (fields.length != 2 || fields[1].isNotEmpty) continue;
      final ref = fields.first;
      if (ref.startsWith('refs/heads/')) local.add(ref.substring('refs/heads/'.length));
      if (ref.startsWith('refs/remotes/')) remote.add(ref.substring('refs/remotes/'.length));
    }
    // A freshly initialized repository has a symbolic HEAD before its first commit.
    if (symbolic && !local.contains(current)) local.add(current);
    return ProjectBranches(current, local, remote);
  }
}
