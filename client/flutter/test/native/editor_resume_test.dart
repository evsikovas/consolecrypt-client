import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/src/rust/api/app.dart' as app;
import 'package:consolecrypt/src/rust/api/profiles.dart' as profiles;
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart' show ExternalLibrary;
import 'package:flutter_test/flutter_test.dart';

const _library = String.fromEnvironment('CC_NATIVE_LIBRARY');

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('editor resume key requires the original unlocked vault', () async {
    final root = await Directory.systemTemp.createTemp('cc-editor-resume-');
    try {
      await RustLib.init(externalLibrary: ExternalLibrary.open(_library));
      await app.coreInit(
        config: app.CoreConfig(
          dataDir: root.path,
          secureStore: app.SecureStoreChoice.memory,
          kdf: app.KdfChoice.floor,
          clientVersion: '1.2.5',
          backgroundSync: false,
          autoStartTunnels: false,
        ),
      );
      try {
        final pass = utf8.encode('synthetic editor resume test passphrase');
        final record =
            jsonDecode(await profiles.profilesCreateLocal(displayName: 'Editor test', passphrase: pass)) as Map;
        final owner = (record['profile'] as Map)['id'] as String;
        final nonce = List.filled(32, 42);
        final key = await profiles.vaultWorkspaceResumeKey(profileId: owner, nonce: nonce);
        expect(key.length, 32);
        await expectLater(profiles.vaultWorkspaceResumeKey(profileId: 'other', nonce: nonce), throwsA(anything));
        await profiles.vaultLock();
        await expectLater(profiles.vaultWorkspaceResumeKey(profileId: owner, nonce: nonce), throwsA(anything));
        await profiles.vaultUnlockWithPassphrase(passphrase: pass);
        expect(await profiles.vaultWorkspaceResumeKey(profileId: owner, nonce: nonce), orderedEquals(key));
        await profiles.profilesCreateLocal(displayName: 'Second profile', passphrase: pass);
        await expectLater(profiles.vaultWorkspaceResumeKey(profileId: owner, nonce: nonce), throwsA(anything));
      } finally {
        await app.coreShutdown();
      }
    } finally {
      await root.delete(recursive: true);
    }
  }, skip: _library.isEmpty);
}
