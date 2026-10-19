import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/storage/app_storage_paths.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late Directory home;
  late Directory currentCore, legacyCore, currentAi, legacyAi;

  setUp(() {
    home = Directory.systemTemp.createTempSync('cc-storage-paths-');
    currentCore = Directory('${home.path}/Library/Application Support/io.consolecrypt.ConsoleCrypt');
    currentAi = Directory('${home.path}/Library/Application Support/ConsoleCrypt/local-ai');
    final legacyHome = '${home.path}/Library/Containers/io.consolecrypt.consolecrypt/Data';
    legacyCore = Directory('$legacyHome/Library/Application Support/io.consolecrypt.ConsoleCrypt');
    legacyAi = Directory('$legacyHome/Library/Application Support/ConsoleCrypt/local-ai');
  });
  tearDown(() => home.deleteSync(recursive: true));

  Future<AppStoragePaths> resolve({Map<String, String> extra = const {}}) =>
      AppStoragePaths.resolve(isMacOS: true, environment: {'HOME': home.path, ...extra});

  File fixture(Directory root, String name, String contents) {
    final file = File('${root.path}/$name');
    file.parent.createSync(recursive: true);
    return file..writeAsStringSync(contents);
  }

  Matcher failure(AppStorageFailure value) =>
      throwsA(isA<AppStorageException>().having((error) => error.failure, 'failure', value));

  // These exercise the macOS migration policy against real POSIX filesystem
  // fixtures. Windows temporary paths cannot stand in for an absolute macOS
  // HOME, and symlink/chmod semantics differ. Keep explicit-root and non-Mac
  // policy checks below cross-platform; report these fixtures as skipped.
  void macStorageTest(String description, Future<void> Function() body) => test(
    description,
    body,
    skip: Platform.isWindows ? 'macOS storage migration requires POSIX filesystem fixtures' : false,
  );

  macStorageTest('new install with no container resolves canonical roots without creating them', () async {
    final paths = await resolve();
    expect(paths.coreRoot!.path, currentCore.path);
    expect(paths.localAiRoot!.path, currentAi.path);
    expect(home.listSync(), isEmpty);
  });

  macStorageTest('sandbox upgrade preserves vault, model and explicit false preference in place', () async {
    final vault = fixture(legacyCore, 'profiles/fixture/vault.db', 'synthetic encrypted vault');
    final settings = fixture(
      legacyCore,
      'ui-state.json',
      jsonEncode({
        'local_settings': jsonEncode({'prefer_device_local_ai': false}),
      }),
    );
    final model = fixture(legacyAi, 'models/legacy.gguf', 'synthetic GGUF');
    final paths = await resolve();
    expect(paths.coreRoot!.path, legacyCore.path);
    expect(paths.localAiRoot!.path, legacyAi.path);
    expect(vault.readAsStringSync(), 'synthetic encrypted vault');
    final store = jsonDecode(settings.readAsStringSync()) as Map<String, dynamic>;
    expect((jsonDecode(store['local_settings'] as String) as Map<String, dynamic>)['prefer_device_local_ai'], false);
    expect(model.readAsStringSync(), 'synthetic GGUF');
    expect(currentCore.existsSync(), false);
    expect(currentAi.existsSync(), false);
    expect((await resolve()).coreRoot!.path, legacyCore.path); // Stable on restart.
  });

  macStorageTest('core and AI choose existing roots independently in either mixed layout', () async {
    fixture(legacyCore, 'profiles.json', '{}');
    fixture(currentAi, 'models/model.gguf', 'synthetic model');
    var paths = await resolve();
    expect(paths.coreRoot!.path, legacyCore.path);
    expect(paths.localAiRoot!.path, currentAi.path);
    legacyCore.renameSync(currentCore.path);
    legacyAi.parent.createSync(recursive: true);
    currentAi.renameSync(legacyAi.path);
    paths = await resolve();
    expect(paths.coreRoot!.path, currentCore.path);
    expect(paths.localAiRoot!.path, legacyAi.path);
  });

  for (final component in ['core', 'AI']) {
    macStorageTest('two populated $component roots fail without merging or removing either', () async {
      final current = component == 'core' ? currentCore : currentAi;
      final legacy = component == 'core' ? legacyCore : legacyAi;
      final a = fixture(current, 'owned.txt', 'current');
      final b = fixture(legacy, 'owned.txt', 'legacy');
      await expectLater(
        resolve(),
        failure(component == 'core' ? AppStorageFailure.coreConflict : AppStorageFailure.localAiConflict),
      );
      expect(a.readAsStringSync(), 'current');
      expect(b.readAsStringSync(), 'legacy');
      expect(current.listSync().length, 1);
      expect(legacy.listSync().length, 1);
    });
  }

  macStorageTest('empty canonical directories do not hide populated legacy storage', () async {
    currentCore.createSync(recursive: true);
    currentAi.createSync(recursive: true);
    fixture(legacyCore, 'ui-state.json', '{}');
    fixture(legacyAi, 'selection.json', '{}');
    final paths = await resolve();
    expect(paths.coreRoot!.path, legacyCore.path);
    expect(paths.localAiRoot!.path, legacyAi.path);
  });

  macStorageTest('two paths pointing to the same existing root are not divergent copies', () async {
    fixture(legacyCore, 'ui-state.json', '{}');
    currentCore.parent.createSync(recursive: true);
    Link(currentCore.path).createSync(legacyCore.path);
    final paths = await resolve();
    expect(paths.coreRoot!.resolveSymbolicLinksSync(), legacyCore.resolveSymbolicLinksSync());
    expect(File('${legacyCore.path}/ui-state.json').readAsStringSync(), '{}');
  });

  macStorageTest('explicit portable core root is respected while AI still reuses legacy storage', () async {
    fixture(currentCore, 'ui-state.json', '{}');
    fixture(legacyCore, 'ui-state.json', '{}');
    fixture(legacyAi, 'models/model.gguf', 'synthetic model');
    final portable = Directory('${home.path}/portable');
    final paths = await resolve(extra: {'CONSOLECRYPT_DATA_DIR': portable.path});
    expect(paths.coreRoot!.path, portable.path);
    expect(paths.localAiRoot!.path, legacyAi.path);
    expect(portable.existsSync(), false);
  });

  test('explicit native test root isolates AI and does not inspect the real home', () {
    final paths = AppStoragePaths.explicit('${home.path}/native-test');
    expect(paths.coreRoot!.path, '${home.path}/native-test');
    expect(paths.localAiRoot!.path, '${home.path}/native-test/local-ai');
    expect(home.listSync(), isEmpty);
  });

  macStorageTest('launcher carrying former container HOME resolves only one container level', () async {
    fixture(legacyCore, 'ui-state.json', '{}');
    final paths = await resolve(extra: {'HOME': '${home.path}/Library/Containers/io.consolecrypt.consolecrypt/Data'});
    expect(paths.coreRoot!.path, legacyCore.path);
    expect(paths.localAiRoot!.path, currentAi.path);
  });

  macStorageTest('non-directory root is an error, not a new empty profile', () async {
    currentCore.parent.createSync(recursive: true);
    File(currentCore.path).writeAsStringSync('unexpected file');
    await expectLater(resolve(), failure(AppStorageFailure.unavailable));
    expect(File(currentCore.path).readAsStringSync(), 'unexpected file');
    expect(legacyCore.existsSync(), false);
  });

  test('missing home is explicit startup failure; other platforms keep existing defaults', () async {
    await expectLater(
      AppStoragePaths.resolve(isMacOS: true, environment: {}),
      failure(AppStorageFailure.homeUnavailable),
    );
    await expectLater(
      AppStoragePaths.resolve(isMacOS: true, environment: {'HOME': r'C:\Users\fixture'}),
      failure(AppStorageFailure.homeUnavailable),
    );
    // Windows keeps its native AppData defaults even if HOME or a macOS-style
    // portable override is present; this branch must not inspect those paths.
    final paths = await AppStoragePaths.resolve(
      isMacOS: false,
      environment: {'HOME': r'C:\Users\fixture', 'CONSOLECRYPT_DATA_DIR': r'C:\fixture-vault'},
    );
    expect(paths.coreRoot, null);
    expect(paths.localAiRoot, null);
  });

  macStorageTest('denied previous container must not be treated as a missing root', () async {
    fixture(legacyCore, 'ui-state.json', '{}');
    final container = Directory('${home.path}/Library/Containers/io.consolecrypt.consolecrypt');
    expect(Process.runSync('chmod', ['000', container.path]).exitCode, 0);
    try {
      await expectLater(resolve(), failure(AppStorageFailure.unavailable));
      expect(currentCore.existsSync(), false);
      expect(currentAi.existsSync(), false);
    } finally {
      expect(Process.runSync('chmod', ['700', container.path]).exitCode, 0);
    }
  });

  macStorageTest('denied existing UI state cannot silently reset a disabled local AI preference', () async {
    final state = fixture(
      legacyCore,
      'ui-state.json',
      jsonEncode({
        'local_settings': jsonEncode({'prefer_device_local_ai': false}),
      }),
    );
    expect(Process.runSync('chmod', ['000', state.path]).exitCode, 0);
    try {
      await expectLater(resolve(), failure(AppStorageFailure.unavailable));
      expect(currentCore.existsSync(), false);
    } finally {
      expect(Process.runSync('chmod', ['600', state.path]).exitCode, 0);
    }
  });
}
