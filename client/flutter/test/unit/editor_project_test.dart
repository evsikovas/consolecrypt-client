import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/editor/editor_language.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:path/path.dart' as p;

void main() {
  late Directory root;
  late ProjectFiles project;
  setUp(() async {
    root = await Directory.systemTemp.createTemp('consolecrypt editor ');
    project = await ProjectFiles.open(root.path);
  });
  tearDown(() => root.delete(recursive: true));

  test('creates and saves UTF-8 BOM and CRLF, detects another writer and rejects binary files', () async {
    final f = await project.create(p.join(project.root, 'пример.py'));
    await f.writeAsBytes([0xef, 0xbb, 0xbf, ...utf8.encode('# Привет\r\n')]);
    final doc = await project.read(f.path);
    expect(doc.text, '# Привет\n');
    final saved = await project.save(doc, '# Привет!\n');
    expect(await f.readAsBytes(), [0xef, 0xbb, 0xbf, ...utf8.encode('# Привет!\r\n')]);
    await f.writeAsString('edited elsewhere');
    await expectLater(project.save(saved, 'stale'), throwsA(EditorIssue.changed));
    expect(await f.readAsString(), 'edited elsewhere');
    await expectLater(project.create(f.path), throwsA(isA<FileSystemException>()));
    await f.writeAsBytes([0, 1, 2]);
    await expectLater(project.read(f.path), throwsA(EditorIssue.unsupported));
    await expectLater(project.create(p.join(root.parent.path, 'outside.txt')), throwsA(EditorIssue.outside));
  });

  test('lists local and remote branches without checking out or fetching', () async {
    Future<void> git(List<String> args) async {
      final result = await Process.run('git', ['-C', root.path, ...args]);
      expect(result.exitCode, 0, reason: result.stderr.toString());
    }

    expect(await ProjectBranches.read(root.path), isNull);
    await git(['init', '-b', 'main']);
    expect((await ProjectBranches.read(root.path))!.current, 'main');
    await git([
      '-c',
      'user.name=Test',
      '-c',
      'user.email=test@example.org',
      '-c',
      'commit.gpgSign=false',
      'commit',
      '--allow-empty',
      '-m',
      'fixture',
    ]);
    await git(['branch', 'feature/editor']);
    await git(['update-ref', 'refs/remotes/origin/feature/editor', 'HEAD']);
    final branch = await ProjectBranches.read(root.path);
    expect(branch!.local, containsAll(['main', 'feature/editor']));
    expect(branch.remote, ['origin/feature/editor']);
    await git(['checkout', '--detach', 'HEAD']);
    expect((await ProjectBranches.read(root.path))!.current, matches(RegExp(r'^[0-9a-f]+$')));
  });

  test('chooses grammars by extension and project filenames', () {
    for (final file in [
      'app.dart',
      'module.py',
      'Main.java',
      'MainActivity.kt',
      'App.swift',
      'index.tsx',
      'main.rs',
      'app.go',
      'Makefile',
      'Dockerfile',
      'main.cpp',
      'main.cs',
      'config.yaml',
      'data.json',
      'docs.md',
      'app.sql',
      'run.sh',
      'script.ps1',
      'index.html',
      'style.css',
    ]) {
      expect(editorLanguage(file), isNotNull, reason: file);
    }
    expect(editorLanguage('notes.txt'), isNull);
  });
}
