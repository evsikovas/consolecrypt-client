// Native iOS Simulator smoke test. Uses app-scoped Keychain and SQLCipher,
// generated temporary identities, and no network/account/real credentials.
import 'dart:io';

import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('iOS native storage, encrypted vault, restart and native channels', (tester) async {
    if (!Platform.isIOS) return;
    const channel = MethodChannel('consolecrypt/ios');
    final privateRoot = await channel.invokeMethod<String>('dataDirectory');
    expect(privateRoot, contains('/Library/Application Support/ConsoleCrypt'));
    expect(Directory(privateRoot!).existsSync(), isTrue);
    await expectLater(
      channel.invokeMethod<bool>('exportFile', {'path': '/etc/passwd'}),
      throwsA(isA<PlatformException>().having((error) => error.code, 'code', 'export_path')),
    );

    final auth = await const MethodChannel('consolecrypt/local_auth').invokeMapMethod<String, Object?>('availability');
    expect(auth, contains('kind'));
    expect(auth?['not_enrolled'], isA<bool>());
    final root = await Directory(privateRoot).createTemp('ios-it-');
    final options = RustCoreOptions(
      dataDir: '${root.path}/data',
      keychainService: 'io.consolecrypt.ios.it-${DateTime.now().microsecondsSinceEpoch}',
      fastKdfForTests: true,
      backgroundSync: false,
      autoStartTunnels: false,
      hookAppExit: false,
    );
    final passphrase = 'ios-${ObjectId.generate().value}-Vault-Local';
    RustAppServices? rust;
    try {
      rust = await RustAppServices.open(options);
      var services = rust.services;
      await services.profiles.createLocalProfile(name: 'iOS smoke');
      await services.vault.createVault(name: 'iOS vault', passphrase: SecretText(passphrase));
      await services.vault.confirmRecoveryKitSaved();
      // Onboarding replaces the profile draft with its persisted vault ID.
      final profile = services.profiles.currentProfiles.active!;
      final host = await services.inventory.saveHostWithAuth(
        Host.create(name: 'simulator-host', address: '192.0.2.1'),
        HostAuthInlinePassword(password: SecretText('generated-${ObjectId.generate().value}')),
      );
      await services.settings.updateLocal(services.settings.currentLocal.copyWith(reopenLastProfile: true));
      await rust.close();
      rust = await RustAppServices.open(options);
      services = rust.services;
      expect(services.profiles.currentProfiles.activeId, profile.id);
      expect(services.vault.currentStatus.phase, VaultPhase.locked);
      await services.vault.unlockWithPassphrase(SecretText(passphrase));
      expect((await services.inventory.watchHosts().first).single.id, host.id);
      await services.vault.lock();
      expect(await services.inventory.watchHosts().first, isEmpty);
    } finally {
      if (rust != null) {
        for (final profile in rust.services.profiles.currentProfiles.profiles) {
          await rust.services.profiles.delete(profile.id);
        }
        await rust.close();
      }
      await root.delete(recursive: true);
    }
  });
}
