// Real Rust core through the FFI bridge (S5: device unlock released only by
// the OS).
//
// Default: in-memory secure store, no OS protector — the UI cannot grant
// anything; enabling and unlocking refuse. No keychain, no prompt.
//   flutter test tool/device_unlock_integration.dart \
//     --dart-define=CC_TEST_RUST_LIB=/absolute/path/libcc_bridge.dylib
//
// Manual, macOS with Touch ID (or the login password): the real Secure
// Enclave protector with an isolated keychain service. Shows the system
// prompt three times: enable (approve), unlock (cancel), unlock (approve).
//   ... --dart-define=CC_TEST_REAL_DEVICE_UNLOCK=true
import 'dart:io';
import 'dart:math';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/src/rust/api/app.dart' as api;
import 'package:consolecrypt/src/rust/api/profiles.dart' as profiles;
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:flutter_test/flutter_test.dart';

const _real = bool.fromEnvironment('CC_TEST_REAL_DEVICE_UNLOCK');

Matcher _code(AppErrorCode code) => throwsA(isA<AppException>().having((e) => e.code, 'code', code));

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('device unlock is released by the OS only', () async {
    const library = String.fromEnvironment('CC_TEST_RUST_LIB');
    if (library.isEmpty) throw StateError('CC_TEST_RUST_LIB must point to the built bridge library');
    await RustLib.init(externalLibrary: ExternalLibrary.open(library));
    final root = await Directory.systemTemp.createTemp('cc-device-unlock-it-');
    RustBackend? hub;
    try {
      final info = await api.coreInit(
        config: api.CoreConfig(
          dataDir: root.path,
          secureStore: _real ? api.SecureStoreChoice.os : api.SecureStoreChoice.memory,
          keychainService: _real ? 'io.consolecrypt.ConsoleCrypt.DeviceUnlockTest' : null,
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
      await profiles.profilesCreateLocal(displayName: 'Device unlock test', passphrase: List.of(pass));
      await hub.refreshState();
      final service = RustVaultService(hub);
      expect(service.currentStatus.deviceUnlock.enabled, isFalse, reason: 'off by default');
      if (!_real) {
        await expectLater(service.setDeviceUnlockEnabled(true, reason: 'test'), _code(AppErrorCode.osAuthFailed));
        expect(decodeObject(await profiles.vaultDeviceUnlockInfo())['enabled'], isFalse);
        await service.lock();
        await expectLater(service.unlockWithDevice(reason: 'test'), _code(AppErrorCode.unsupported));
        expect(service.currentStatus.isUnlocked, isFalse);
        return;
      }
      // Approve the system prompt.
      await service.setDeviceUnlockEnabled(true, reason: 'enable the ConsoleCrypt test (approve)');
      expect(service.currentStatus.deviceUnlock.enabled, isTrue);
      final id = await profiles.profilesLastActiveId();
      await profiles.profilesClose();
      await profiles.profilesOpen(profileId: id!);
      await hub.refreshState();
      expect(service.currentStatus.isUnlocked, isFalse);
      // Cancel the system prompt: the vault stays locked.
      await expectLater(
        service.unlockWithDevice(reason: 'test cancelling (press Cancel)'),
        _code(AppErrorCode.osAuthFailed),
      );
      expect(service.currentStatus.isUnlocked, isFalse);
      // Approve the system prompt.
      await service.unlockWithDevice(reason: 'unlock the ConsoleCrypt test (approve)');
      expect(service.currentStatus.isUnlocked, isTrue);
      await service.setDeviceUnlockEnabled(false, reason: 'test');
      await profiles.profilesRemove(profileId: id);
    } finally {
      await hub?.shutdown();
      await hub?.dispose();
      await root.delete(recursive: true);
    }
  });
}
