import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/editor/editor_language.dart';
import 'package:consolecrypt/editor/project_files.dart';
import 'package:consolecrypt/editor/ssh_project.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:path/path.dart' as p;

import '../helpers/test_app.dart';

void main() {
  late Directory root;
  late LocalProjectFiles project;
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

  test('moves files and imports binary data without replacing an existing file', () async {
    final dir = await Directory(p.join(project.root, 'sub')).create();
    final original = await File(p.join(project.root, 'sample.bin')).writeAsBytes([0, 255, 7]);
    await project.importFile(original.path, dir.path);
    final target = File(p.join(dir.path, 'sample.bin'));
    expect(await target.readAsBytes(), [0, 255, 7]);
    await original.writeAsBytes([4, 2]);
    await expectLater(project.importFile(original.path, dir.path), throwsA(isA<FileSystemException>()));
    await expectLater(project.moveFile(original.path, dir.path), throwsA(isA<FileSystemException>()));
    expect(await target.readAsBytes(), [0, 255, 7]);
    expect(await original.readAsBytes(), [4, 2]);
    await target.delete();
    expect(await project.moveFile(original.path, dir.path), target.path);
    expect(await target.readAsBytes(), [4, 2]);
    expect(original.existsSync(), isFalse);
    await expectLater(project.moveFile(target.path, root.parent.path), throwsA(EditorIssue.outside));
  });

  test('resolves document links within local and SSH projects', () {
    for (final paths in [p.windows, p.posix]) {
      final root = paths.style == p.Style.windows ? r'C:\project' : '/project';
      final source = paths.join(root, 'guide', 'readme.md');
      String? resolve(String href) => resolveMarkdownFile(paths, root, source, href);
      expect(resolve('../notes/My%20file.md#setup'), paths.join(root, 'notes', 'My file.md'));
      expect(resolve('/guide/setup.md'), paths.join(root, 'guide', 'setup.md'));
      expect(() => resolve('../../outside'), throwsA(EditorIssue.outside));
      expect(() => resolve('sub%2Foutside'), throwsA(EditorIssue.outside));
      expect(resolve('file:///etc/passwd'), isNull);
      expect(resolve('#heading'), isNull);
    }
  });

  test('soft wrapping is opt-in and survives settings serialization', () {
    final original = localSettingsFromJson({});
    expect(original.editorSoftWrap, isFalse);
    final saved = original.copyWith(editorSoftWrap: true).copyWith(editorFontSize: 16);
    expect(localSettingsFromJson(localSettingsToJson(saved)).editorSoftWrap, isTrue);
  });

  test('SSH moves, uploads and cleans failed imports without replacing files', () async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1');
    final session = await backend.sftp.connect(host.id);
    final home = await backend.sftp.remoteHome(session);
    final remote = SshProject(home, host, session, backend.sftp, backend.sftpBrowser);
    addTearDown(remote.close);
    final dir = '$home/target';
    await backend.sftp.makeRemoteDirectory(session, dir);
    expect(await remote.moveFile('$home/notes.txt', dir), '$dir/notes.txt');
    final file = await File(p.join(project.root, 'upload.bin')).writeAsBytes([0, 255]);
    await remote.importFile(file.path, dir);
    expect((await remote.children(dir)).map((entry) => entry.path), contains('$dir/upload.bin'));
    await expectLater(remote.importFile(file.path, dir), throwsA(isA<AppException>()));
    final next = await file.rename(p.join(project.root, 'fail.bin'));
    backend.sftp.debugFailNextTransfer();
    await expectLater(remote.importFile(next.path, dir), throwsStateError);
    expect((await remote.children(dir)).map((entry) => entry.path), ['$dir/notes.txt', '$dir/upload.bin']);
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
