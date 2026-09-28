// Real Rust core + simulated native authentication, isolated ephemeral keys.
// No user keychain or real Touch ID prompt is accessed by this test.
// flutter test tool/device_unlock_integration.dart --dart-define=CC_TEST_RUST_LIB=/absolute/path/libcc_bridge.dylib
import 'dart:io';
import 'dart:math';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/app.dart' as api;
import 'package:consolecrypt/src/rust/api/profiles.dart' as profiles;
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter/services.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();
  test('native cancellation, enrollment, persisted opt-in and disabled FFI paths', () async {
    const library = String.fromEnvironment('CC_TEST_RUST_LIB');
    if (library.isEmpty) throw StateError('CC_TEST_RUST_LIB must point to the built bridge library');
    await RustLib.init(externalLibrary: ExternalLibrary.open(library));
    final root = await Directory.systemTemp.createTemp('cc-touchid-it-');
    var authenticated = false;
    var prompts = 0;
    const channel = MethodChannel('consolecrypt/local_auth');
    binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      if (call.method == 'availability') return {'kind': 'touch_id', 'not_enrolled': false};
      if (call.method == 'authenticate') {
        prompts++;
        return authenticated;
      }
      throw MissingPluginException();
    });
    RustBackend? hub;
    try {
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
      hub = RustBackend(info);
      await hub.start();
      final random = Random.secure();
      final pass = List<int>.generate(48, (_) => 33 + random.nextInt(90));
      await profiles.profilesCreateLocal(displayName: 'Device unlock test', passphrase: pass);
      pass.fillRange(0, pass.length, 0);
      await hub.refreshState();
      final service = RustVaultService(hub);
      expect(service.currentStatus.deviceUnlock.enabled, isFalse);
      await expectLater(
        service.setDeviceUnlockEnabled(true, reason: 'Test'),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.osAuthFailed)),
      );
      expect(decodeObject(await profiles.vaultDeviceUnlockInfo())['enabled'], isFalse);
      authenticated = true;
      await service.setDeviceUnlockEnabled(true, reason: 'Test');
      expect(service.currentStatus.deviceUnlockAvailable, isTrue);
      final id = await profiles.profilesLastActiveId();
      await profiles.profilesClose();
      await profiles.profilesOpen(profileId: id!);
      await hub.refreshState();
      expect(service.currentStatus.deviceUnlock.enabled, isTrue);
      await service.unlockWithDevice(reason: 'Test');
      expect(service.currentStatus.isUnlocked, isTrue);
      final beforeDisable = prompts;
      await service.setDeviceUnlockEnabled(false, reason: 'Test');
      expect(prompts, beforeDisable, reason: 'disabling does not authenticate');
      await service.lock();
      await expectLater(
        guard(profiles.vaultUnlockWithDeviceAttested),
        throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.unsupported)),
      );
      expect(service.currentStatus.isUnlocked, isFalse);
    } finally {
      await hub?.shutdown();
      await hub?.dispose();
      binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, null);
      await root.delete(recursive: true);
    }
  });
}
