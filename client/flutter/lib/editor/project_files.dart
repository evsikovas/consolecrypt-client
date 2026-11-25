import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path/path.dart' as p;

enum EditorIssue implements Exception { unsupported, changed, outside, gitMissing }

const _limit = 4 * 1024 * 1024;

class ProjectFiles {
  ProjectFiles._(this.root);
  final String root;

  static Future<ProjectFiles> open(String path) async => ProjectFiles._(await Directory(path).resolveSymbolicLinks());

  Future<void> _check(String path) async {
    if (!p.equals(path, root) && !p.isWithin(root, path)) throw EditorIssue.outside;
    final resolved = await File(path).resolveSymbolicLinks();
    if (!p.equals(path, resolved)) throw EditorIssue.outside;
  }

  Future<List<FileSystemEntity>> children(String path) async {
    await _check(path);
    final entries = await Directory(path)
        .list(followLinks: false)
        .where((entry) => (entry is Directory || entry is File) && p.basename(entry.path) != '.git')
        .toList();
    entries.sort((a, b) {
      if ((a is Directory) != (b is Directory)) return a is Directory ? -1 : 1;
      return p.basename(a.path).toLowerCase().compareTo(p.basename(b.path).toLowerCase());
    });
    return entries;
  }

  Future<File> create(String path) async {
    await _check(p.dirname(path));
    if (!p.isWithin(root, path) || p.basename(path).isEmpty) throw EditorIssue.outside;
    return File(path).create(exclusive: true);
  }

  Future<ProjectText> read(String path) async {
    await _check(path);
    final handle = await File(path).open();
    late Uint8List bytes;
    try {
      bytes = await handle.read(_limit + 1);
    } finally {
      await handle.close();
    }
    if (bytes.length > _limit || bytes.contains(0)) throw EditorIssue.unsupported;
    try {
      return ProjectText(path, bytes, utf8.decode(bytes));
    } on FormatException {
      throw EditorIssue.unsupported;
    }
  }

  Future<ProjectText> save(ProjectText original, String text) async {
    final file = File(original.path);
    await _check(file.path);
    final bytes = original.encode(text);
    if (bytes.length > _limit) throw EditorIssue.unsupported;
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

class ProjectText {
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
    final local = <String>[], remote = <String>[];
    for (final line in const LineSplitter().convert(refs.stdout.toString())) {
      final fields = line.split('\t');
      if (fields.length != 2 || fields[1].isNotEmpty) continue;
      final ref = fields.first;
      if (ref.startsWith('refs/heads/')) local.add(ref.substring('refs/heads/'.length));
      if (ref.startsWith('refs/remotes/')) remote.add(ref.substring('refs/remotes/'.length));
    }
    // A freshly initialized repository has a symbolic HEAD before its first commit.
    if (head.exitCode == 0 && !local.contains(current)) local.add(current);
    return ProjectBranches(current, local, remote);
  }
}
