import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_sftp_fs.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/sftp/file_kind.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_rows.dart';
import 'package:flutter_test/flutter_test.dart';

RemoteFileInfo _f(String name, {int size = 0, bool dir = false, String dirPath = '/d', DateTime? at}) => RemoteFileInfo(
  name: name,
  path: '$dirPath/$name',
  kind: dir ? RemoteEntryKind.directory : RemoteEntryKind.file,
  size: size,
  permissions: dir ? octal('755') : octal('644'),
  modifiedAt: at,
);

Future<(MockBackend, SftpSessionId)> _connected() async {
  final backend = MockBackend(config: const MockConfig.test());
  await backend.debugSignInDemoAndUnlock();
  final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1');
  final session = await backend.sftp.connect(host.id);
  return (backend, session);
}

void main() {
  group('permissions', () {
    test('mode strings round-trip, including setuid/setgid/sticky', () {
      expect(formatModeString(RemoteEntryKind.directory, octal('755')), 'drwxr-xr-x');
      expect(formatModeString(RemoteEntryKind.file, octal('4755')), '-rwsr-xr-x');
      expect(formatModeString(RemoteEntryKind.file, octal('2640')), '-rw-r-S---');
      expect(formatModeString(RemoteEntryKind.directory, octal('1777')), 'drwxrwxrwt');
      expect(formatModeString(RemoteEntryKind.symlink, octal('777')), 'lrwxrwxrwx');
      for (final m in ['0', '644', '755', '4755', '2640', '1777', '7000']) {
        final mode = octal(m);
        expect(parseModeString(formatModeString(RemoteEntryKind.file, mode)), mode, reason: m);
      }
      expect(parseModeString('bad'), isNull);
    });

    test('octal text', () {
      expect(formatOctalMode(octal('755')), '0755');
      expect(formatOctalMode(octal('4755')), '4755');
      expect(parseOctalMode('750'), octal('750'));
      expect(parseOctalMode('0644'), octal('644'));
      expect(parseOctalMode('8'), isNull);
      expect(parseOctalMode('12345'), isNull);
    });
  });

  test('typed paths are normalized (~, relative, ..)', () {
    expect(normalizeRemotePath('~', base: '/var', home: '/home/u'), '/home/u');
    expect(normalizeRemotePath('~/logs/', base: '/var', home: '/home/u'), '/home/u/logs');
    expect(normalizeRemotePath('www/../log', base: '/var', home: '/home/u'), '/var/log');
    expect(normalizeRemotePath('/a//b/./c', base: '/', home: '/'), '/a/b/c');
    expect(normalizeRemotePath('../../..', base: '/a', home: '/'), '/');
  });

  test('preferences survive JSON and tolerate garbage', () {
    const prefs = SftpBrowserPreferences(
      showHidden: true,
      foldersFirst: false,
      showLocalPane: true,
      editHintAcknowledged: true,
      sortColumn: SftpListColumn.size,
      sortAscending: false,
      visibleColumns: {SftpListColumn.name, SftpListColumn.owner},
      columnWidths: {SftpListColumn.size: 120},
    );
    final back = SftpBrowserPreferences.fromJson(jsonDecode(jsonEncode(prefs.toJson())) as Map<String, Object?>);
    expect(back.showHidden, isTrue);
    expect(back.foldersFirst, isFalse);
    expect(back.showLocalPane, isTrue);
    expect(back.editHintAcknowledged, isTrue);
    expect(back.sortColumn, SftpListColumn.size);
    expect(back.sortAscending, isFalse);
    expect(back.visibleColumns, {SftpListColumn.name, SftpListColumn.owner});
    expect(back.widthOf(SftpListColumn.size), 120);
    final junk = SftpBrowserPreferences.fromJson({'sort_column': 'nope', 'column_order': 'x', 'visible_columns': []});
    expect(junk.sortColumn, SftpListColumn.name);
    expect(junk.columnOrder, SftpBrowserPreferences.defaultColumnOrder);
    expect(junk.visibleColumns, SftpBrowserPreferences.defaultVisibleColumns);
  });

  group('rows', () {
    final t0 = DateTime(2026);
    final dirs = {
      '/d': [
        _f('b10.txt', size: 5, at: t0),
        _f('b2.txt', size: 50, at: t0.add(const Duration(days: 1))),
        _f('Zeta', dir: true),
        _f('alpha', dir: true),
        _f('.hidden', size: 1),
      ],
      '/d/alpha': [_f('match-me.log', dirPath: '/d/alpha', size: 3)],
    };

    List<String> names(List<SftpRow> rows) => [for (final r in rows) r.entry.name];

    test('natural order, folders first, hidden files dropped', () {
      final rows = buildSftpRows(
        root: '/d',
        dirs: dirs,
        expanded: const {},
        loading: const {},
        prefs: const SftpBrowserPreferences(),
      );
      expect(names(rows), ['alpha', 'Zeta', 'b2.txt', 'b10.txt']);
      expect(compareNatural('file2', 'file10'), lessThan(0));
    });

    test('sort by size descending, folders stay first; hidden shown on request', () {
      final rows = buildSftpRows(
        root: '/d',
        dirs: dirs,
        expanded: const {},
        loading: const {},
        prefs: const SftpBrowserPreferences(sortColumn: SftpListColumn.size, sortAscending: false, showHidden: true),
      );
      expect(names(rows), ['alpha', 'Zeta', 'b2.txt', 'b10.txt', '.hidden'], reason: 'folders tie on size → by name');
    });

    test('expanded folders are inlined; the filter keeps ancestors of matches', () {
      final expanded = buildSftpRows(
        root: '/d',
        dirs: dirs,
        expanded: const {'/d/alpha', '/d/Zeta'},
        loading: const {'/d/Zeta'},
        prefs: const SftpBrowserPreferences(),
      );
      expect(names(expanded), ['alpha', 'match-me.log', 'Zeta', 'b2.txt', 'b10.txt']);
      expect(expanded[1].depth, 1);
      expect(expanded[2].loading, isTrue, reason: 'Zeta is being listed');
      final filtered = buildSftpRows(
        root: '/d',
        dirs: dirs,
        expanded: const {'/d/alpha'},
        loading: const {},
        prefs: const SftpBrowserPreferences(),
        filter: 'MATCH',
      );
      expect(names(filtered), ['alpha', 'match-me.log']);
    });

    test('counts and column fitting', () {
      final counts = SftpCounts.of(dirs['/d']!);
      expect((counts.folders, counts.files, counts.bytes), (2, 3, 56));
      const prefs = SftpBrowserPreferences();
      expect(fitColumns(prefs, 1200), [
        SftpListColumn.name,
        SftpListColumn.size,
        SftpListColumn.kind,
        SftpListColumn.modified,
        SftpListColumn.permissions,
      ]);
      expect(fitColumns(prefs, 520), [SftpListColumn.name, SftpListColumn.size, SftpListColumn.modified]);
    });

    test('file names and local parents', () {
      expect(splitFileName('index.php'), ('index', '.php'));
      expect(splitFileName('.bashrc'), ('.bashrc', ''));
      expect(splitFileName('v1.8.2.tar.gz'), ('v1.8.2', '.tar.gz'));
      expect(parentLocalPath('/Users/demo/Downloads'), '/Users/demo');
      expect(parentLocalPath('/Users'), '/');
      expect(parentLocalPath(r'C:\Users\demo'), r'C:\Users');
      expect(parentLocalPath(r'C:\Users'), r'C:\');
    });
  });

  test('file kinds from names', () {
    FileKindGroup g(String name, {bool x = false}) => fileKindForName(name, executable: x).group;
    expect(fileKindForName('index.php').format, 'PHP');
    expect(fileKindForName('logo.png').format, 'PNG');
    expect(fileKindForName('backup.tar.gz').format, 'TAR.GZ');
    expect(g('deploy.sh'), FileKindGroup.script);
    expect(g('.bashrc'), FileKindGroup.script);
    expect(g('.htaccess'), FileKindGroup.config);
    expect(g('app.log'), FileKindGroup.log);
    expect(g('authorized_keys'), FileKindGroup.key);
    expect(g('README'), FileKindGroup.text);
    expect(g('run', x: true), FileKindGroup.executable);
    expect(g('mystery'), FileKindGroup.generic);
    expect(fileKindForName('logo.svg').isTextual, isTrue);
    expect(fileKindForName('logo.png').isImage, isTrue);
  });

  group('mock browser service', () {
    test('lists with full metadata, stat, chmod, create, duplicate, preview', () async {
      final (backend, session) = await _connected();
      addTearDown(backend.dispose);
      final browser = backend.sftpBrowser;
      final home = await browser.listDirectory(session, '/home/deploy');
      final current = home.firstWhere((e) => e.name == 'current');
      expect(current.isSymlink, isTrue);
      expect(current.isDirectory, isTrue);
      expect(current.linkTarget, 'app/releases/v1.8.3');
      expect(home.firstWhere((e) => e.name == 'old-backup').linkTargetKind, isNull);
      expect(home.firstWhere((e) => e.name == 'deploy.sh').owner, 'deploy');

      await browser.setPermissions(session, '/home/deploy/deploy.sh', octal('700'));
      expect((await browser.stat(session, '/home/deploy/deploy.sh')).modeString, '-rwx------');

      await browser.createFile(session, '/home/deploy/new.txt');
      await expectLater(
        browser.createFile(session, '/home/deploy/new.txt'),
        throwsA(isA<AppException>().having((e) => e.reason, 'reason', AppErrorReason.alreadyExists)),
      );
      await browser.duplicate(session, '/home/deploy/app', '/home/deploy/app copy');
      final copy = await browser.listDirectory(session, '/home/deploy/app copy');
      expect(copy.map((e) => e.name), containsAll(['config.yml', 'releases']));

      final preview = await browser.readPreview(session, '/home/deploy/logs/app.log', maxBytes: 1000);
      expect(preview.bytes.length, 1000);
      expect(preview.truncated, isTrue);
      expect(utf8.decode(preview.bytes, allowMalformed: true), contains('INFO'));
      expect(await browser.resolveDirectory(session, '~/app/../logs', base: '/'), '/home/deploy/logs');
    });

    test('edit session lifecycle: open → save → synced; conflict + resolutions; stop outcomes', () async {
      final (backend, session) = await _connected();
      addTearDown(backend.dispose);
      final browser = backend.sftpBrowser;
      final seen = <List<EditSessionInfo>>[];
      final sub = browser.watchEditSessions().listen(seen.add);
      addTearDown(sub.cancel);

      final info = await browser.openInEditor(session, '/home/deploy/latest.log');
      expect(info.targetPath, '/home/deploy/logs/app.log', reason: 'symlinks resolved');
      EditSessionInfo now() => seen.last.single;
      await pumpEventQueue();
      expect(now().status, isA<EditStatusSynced>());
      expect((await browser.openInEditor(session, '/home/deploy/logs/app.log')).id, info.id, reason: 'one per path');

      browser.debugSimulateSave(info.id);
      await pumpEventQueue();
      expect(now().status, isA<EditStatusSynced>());
      expect(now().uploads, 1);
      expect(seen.any((list) => list.isNotEmpty && list.single.status is EditStatusUploading), isTrue);

      browser
        ..debugSimulateRemoteChange(info.id)
        ..debugSimulateSave(info.id);
      await pumpEventQueue();
      expect(now().status, isA<EditStatusConflict>());
      expect(await browser.stopEditing(info.id), isA<EditStopConflict>());

      await browser.resolveEditConflict(info.id, EditConflictResolution.keepRemoteCopyLocally);
      await pumpEventQueue();
      expect(now().status, isA<EditStatusModified>());
      expect(now().remoteCopies.single, contains('app.remote-'));
      final outcome = await browser.stopEditing(info.id);
      expect(outcome, isA<EditStopClosed>().having((o) => o.uploaded, 'uploaded', isTrue));
      await pumpEventQueue();
      expect(seen.last, isEmpty);

      await expectLater(
        browser.openInEditor(session, '/home/deploy/backups/db-2026-09-25.sql.gz'),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.payloadTooLarge)),
      );
      await expectLater(browser.openInEditor(session, '/home/deploy/app'), throwsA(isA<AppException>()));
    });

    test('disconnect ends sessions; unresolved ones become leftovers that can be resumed', () async {
      final (backend, session) = await _connected();
      addTearDown(backend.dispose);
      final browser = backend.sftpBrowser;
      expect(await browser.listEditLeftovers(), isEmpty);
      final info = await browser.openInEditor(session, '/home/deploy/notes.txt');
      browser
        ..debugSimulateRemoteChange(info.id)
        ..debugSimulateSave(info.id);
      await pumpEventQueue();
      await backend.sftp.disconnect(session);
      await pumpEventQueue();
      final leftovers = await browser.listEditLeftovers();
      expect(leftovers.single.remotePath, '/home/deploy/notes.txt');
      expect(leftovers.single.locallyModified, isTrue);

      final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1');
      final again = await backend.sftp.connect(host.id);
      final resumed = await browser.resumeEditLeftover(leftovers.single.id, again);
      await pumpEventQueue();
      expect(resumed.remotePath, '/home/deploy/notes.txt');
      expect(await browser.listEditLeftovers(), isEmpty);
    });

    test('seeded leftovers (demo) name demo hosts; discard removes them', () async {
      final backend = MockBackend(config: const MockConfig.test(seedEditLeftovers: true));
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      final leftovers = await backend.sftpBrowser.listEditLeftovers();
      expect(leftovers.length, 3);
      expect(leftovers.where((l) => !l.canResume).length, 1);
      for (final l in leftovers) {
        await backend.sftpBrowser.discardEditLeftover(l.id);
      }
      expect(await backend.sftpBrowser.listEditLeftovers(), isEmpty);
    });

    test('transfers: folder upload, failure and retry, clear finished', () async {
      final (backend, session) = await _connected();
      addTearDown(backend.dispose);
      final browser = backend.sftpBrowser;
      final ids = await browser.uploadItems(session, ['/Users/demo/Projects/website', '/tmp/dropped.bin'], '/tmp');
      await pumpEventQueue();
      expect(ids.length, 2);
      final names = (await browser.listDirectory(session, '/tmp')).map((e) => e.name);
      expect(names, containsAll(['website', 'dropped.bin']));
      browser.debugFailNextTransfer();
      final failed = (await browser.downloadItems(session, ['/home/deploy/notes.txt'], '/Users/demo')).single;
      await pumpEventQueue();
      expect(backend.sftp.currentTransfers.firstWhere((j) => j.id == failed).state, TransferState.failed);
      final retried = await browser.retryTransfer(failed);
      await pumpEventQueue();
      expect(backend.sftp.currentTransfers.firstWhere((j) => j.id == retried).state, TransferState.completed);
      expect(backend.sftp.currentTransfers.any((j) => j.id == failed), isFalse);
      await browser.clearFinishedTransfers();
      expect(backend.sftp.currentTransfers, isEmpty);
    });
  });

  test('fallback adapter lists through SftpService and reports unsupported features', () async {
    final (backend, session) = await _connected();
    addTearDown(backend.dispose);
    final fallback = SftpBrowserFallback(backend.sftp);
    final entries = await fallback.listDirectory(session, '/home/deploy');
    final deploy = entries.firstWhere((e) => e.name == 'deploy.sh');
    expect(deploy.permissions, octal('755'));
    expect(entries.firstWhere((e) => e.name == 'current').isDirectory, isTrue);
    expect((await fallback.stat(session, '/home/deploy/notes.txt')).name, 'notes.txt');
    await expectLater(
      fallback.openInEditor(session, '/home/deploy/notes.txt'),
      throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.unsupported)),
    );
    expect(await fallback.listEditLeftovers(), isEmpty);
    expect(await fallback.watchEditSessions().first, isEmpty);
    unawaited(fallback.savePreferences(const SftpBrowserPreferences(showHidden: true)));
    expect((await fallback.loadPreferences()).showHidden, isTrue);
  });
}
