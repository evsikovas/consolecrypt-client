// Explicit opt-in: exercises the actual compiled DLL in a new isolated vault.
// No container mutations, registry login, user profile or OS keychain writes.
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/src/rust/api/app.dart' as app;
import 'package:consolecrypt/src/rust/api/docker.dart' as docker;
import 'package:consolecrypt/src/rust/api/profiles.dart' as profiles;
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:consolecrypt/wslc/src/rust/api/desk.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart' show ExternalLibrary;
import 'package:flutter_test/flutter_test.dart';

const _library = String.fromEnvironment('CC_NATIVE_LIBRARY');
const _probe = bool.fromEnvironment('CC_WSLC_READONLY_PROBE');

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test(
    'real DLL preserves typed WSLc settings, engine identities and lock boundaries',
    () async {
      final parent = Directory.systemTemp.absolute;
      final root = await parent.createTemp('cc-wslc-native-');
      expect(root.parent.path, parent.path);
      String? profile;
      WslcApi? api;
      var initialized = false;
      try {
        await RustLib.init(externalLibrary: ExternalLibrary.open(_library));
        await app.coreInit(
          config: app.CoreConfig(
            dataDir: root.path,
            secureStore: app.SecureStoreChoice.memory,
            kdf: app.KdfChoice.floor,
            clientVersion: '1.2.1',
            backgroundSync: false,
            autoStartTunnels: false,
          ),
        );
        initialized = true;
        final created = jsonDecode(
          await profiles.profilesCreateLocal(
            displayName: 'WSLc native smoke fixture',
            passphrase: utf8.encode('synthetic-native-smoke-only-passphrase'),
          ),
        ) as Map;
        profile = (created['profile'] as Map)['id'] as String;
        api = WslcApi(profileId: profile, reveal: (_, _) => throw StateError('No secret reads in this smoke'));
        final settings = await api.loadSettings();
        expect(settings.templates, isEmpty);
        final changed = settings.toJson()..['refresh_seconds'] = 9;
        await api.request('save_settings', {'settings': changed});
        expect((await api.loadSettings()).refreshSeconds, 9);
        final engines = (jsonDecode(await docker.dockerHosts()) as List).cast<Map<String, dynamic>>();
        final local = engines.where((engine) => engine['kind'] == 'local').toList();
        expect(local, isNotEmpty);
        expect(local.map((engine) => engine['id']).toSet(), hasLength(local.length));
        for (final engine in local) {
          expect(engine['id'], matches(r'^local/endpoint/[a-f0-9]{64}$'));
        }
        if (_probe) {
          final hostId = local.first['id'] as String;
          final connection = jsonDecode(await docker.dockerConnect(hostId: hostId)) as Map;
          expect(connection['outcome'], 'connected', reason: 'The local Docker engine must accept a connection');
          expect(connection['host_id'], hostId);
          final sessionId = connection['id'] as String;
          try {
            expect(jsonDecode(await docker.dockerContainers(sessionId: sessionId, all: true)), isA<List>());
          } finally {
            await docker.dockerDisconnect(sessionId: sessionId);
          }
          final runtime = await api.probeRuntime();
          expect(runtime.versionSupported, isTrue);
          expect(runtime.readable, isTrue, reason: 'Installed WSLc must permit the read-only container listing');
          // Exercise the real JSON codecs without changing runtime resources
          // or printing private inventory names into a test log.
          await Future.wait([api.listContainers(), api.listImages(), api.listVolumes(), api.listNetworks()]);
        }
        await profiles.vaultLock();
        await expectLater(api.loadSettings(), throwsA(anything));
      } finally {
        await api?.close();
        if (initialized) {
          if (profile != null) await profiles.profilesRemove(profileId: profile);
          await app.coreShutdown();
        }
        RustLib.dispose();
        // This test owns the exact fresh temp directory and nothing else.
        expect(root.parent.path, parent.path);
        expect(root.path.split(Platform.pathSeparator).last, startsWith('cc-wslc-native-'));
        await root.delete(recursive: true);
      }
    },
    skip: !Platform.isWindows || _library.isEmpty,
    timeout: const Timeout(Duration(minutes: 3)),
  );
}
