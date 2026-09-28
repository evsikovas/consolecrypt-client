// Headless integration test: real Rust store, isolated temp directory, no
// user vault, keychain entries or native app window.
// flutter test tool/appearance_persistence.dart --dart-define=CC_TEST_RUST_LIB=/absolute/path/libcc_bridge.dylib
import 'dart:io';
import 'dart:math';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/theme/terminal_palettes.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/bridge/rust_inventory.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:consolecrypt/src/rust/api/app.dart' as api;
import 'package:consolecrypt/src/rust/api/profiles.dart' as profiles;
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('rapid appearance edits survive a real core shutdown and restart', () async {
    const library = String.fromEnvironment('CC_TEST_RUST_LIB');
    if (library.isEmpty) throw StateError('CC_TEST_RUST_LIB must point to the built bridge library');
    await RustLib.init(externalLibrary: ExternalLibrary.open(library));
    final root = await Directory.systemTemp.createTemp('cc-appearance-it-');
    RustBackend? hub;
    Future<RustBackend> open() async {
      final info = await api.coreInit(
        config: api.CoreConfig(
          dataDir: root.path,
          secureStore: api.SecureStoreChoice.memory,
          kdf: api.KdfChoice.floor,
          clientVersion: '0.1.0',
          backgroundSync: false,
          autoStartTunnels: false,
        ),
      );
      final backend = RustBackend(info);
      await backend.start();
      return backend;
    }

    try {
      hub = await open();
      // A persisted profile with ephemeral test keys: startup must list it
      // without attempting to open its database or request secure storage.
      final random = Random.secure();
      final passphrase = List<int>.generate(48, (_) => 33 + random.nextInt(90));
      await profiles.profilesCreateLocal(displayName: 'Startup test', passphrase: passphrase);
      passphrase.fillRange(0, passphrase.length, 0);
      final lastProfile = await profiles.profilesLastActiveId();
      expect(lastProfile, isNotNull);
      await hub.refreshState();
      final settings = RustSettingsService(hub);
      final writes = <Future<void>>[];
      for (var i = 0; i < 20; i++) {
        writes.add(
          settings.updateLocal(
            settings.currentLocal.copyWith(
              uiFontScale: .8 + i * .02,
              uiAccentColor: 0x7C5CFC + i,
              uiBackgroundColor: 0x16324F,
              terminalColorScheme: TerminalColorScheme.custom,
              customTerminalColors: colorsFromTerminalTheme(AppTheme.terminalTheme(Brightness.dark))
                  .withColor('background', 0x102030 + i),
              reopenLastProfile: false,
              terminalFontSize: 17,
              sidebarStyle: SidebarStyle.edgeToEdge,
              sftpDefaultEditor: const AppRef(AppRefKind.path, '/Applications/Visual Studio Code.app'),
            ),
          ),
        );
      }
      final expected = localSettingsToJson(settings.currentLocal);
      // Same exit path as the desktop app: no caller waits for individual edits.
      await settings.flushLocalWrites();
      await hub.shutdown();
      await Future.wait(writes);
      await hub.dispose();
      hub = null;
      hub = await open();
      expect(localSettingsToJson(RustSettingsService(hub).currentLocal), expected);
      expect(hub.activeInfo, isNull, reason: 'No automatic keychain access at startup');
      expect(hub.profiles.value.profiles.single.id.value, lastProfile);
      expect(await profiles.profilesLastActiveId(), lastProfile);
    } finally {
      await hub?.shutdown();
      await hub?.dispose();
      await root.delete(recursive: true);
    }
  });
}
