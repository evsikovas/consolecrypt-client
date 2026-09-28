// The owner's scenario through the real UI on the real core: create a local
// profile + vault in the app, "restart" (tear the UI down, shut the core down,
// start both again) → the app shows Vault Unlock (not onboarding) and the
// data is intact. Isolated data dir + keychain service.
//
//   flutter test integration_test/app_persistence_test.dart -d macos
import 'dart:io';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/vault/onboarding_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:material_ui/material_ui.dart';

const _memoryStore = bool.fromEnvironment('CC_IT_MEMORY_STORE');

final _passphrase = 'ui-${DateTime.now().microsecondsSinceEpoch}-Violet-Anchor-Muffin-Glacier';

/// Pumps (real time) until [finder] matches or [timeout] passes.
Future<void> pumpUntil(WidgetTester tester, Finder finder, {Duration timeout = const Duration(seconds: 30)}) async {
  final end = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(end)) {
    await tester.pump(const Duration(milliseconds: 100));
    if (finder.evaluate().isNotEmpty) return;
  }
  throw TestFailure('timed out waiting for $finder');
}

Future<void> tapKey(WidgetTester tester, String key) async {
  final f = find.byKey(ValueKey(key));
  await pumpUntil(tester, f);
  await tester.ensureVisible(f);
  await tester.pump();
  await tester.tap(f);
  await tester.pump(const Duration(milliseconds: 200));
}

Future<void> enterKey(WidgetTester tester, String key, String text) async {
  final f = find.byKey(ValueKey(key));
  await pumpUntil(tester, f);
  await tester.enterText(f, text);
  await tester.pump();
}

late Directory _root;
late RustCoreOptions _options;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() {
    _root = Directory.systemTemp.createTempSync('cc-bridge-ui-it-');
    _options = RustCoreOptions(
      dataDir: '${_root.path}/data',
      keychainService: 'io.consolecrypt.ConsoleCrypt.ui-it-${DateTime.now().millisecondsSinceEpoch}',
      inMemorySecureStore: _memoryStore,
      fastKdfForTests: true,
      backgroundSync: false,
      autoStartTunnels: false,
      hookAppExit: false,
    );
  });

  tearDownAll(() async {
    final rust = RustAppServices.current ?? await RustAppServices.open(_options);
    for (final p in rust.services.profiles.currentProfiles.profiles) {
      await rust.services.profiles.delete(p.id);
    }
    await rust.close();
    _root.deleteSync(recursive: true);
  });

  testWidgets('create a local vault in the app, restart → Vault Unlock, data intact', (tester) async {
    // Layout overflows of the shell chrome while the real window animates are
    // UI-layer issues, not what this test checks: record, don't fail.
    final overflows = <String>[];
    final onError = FlutterError.onError;
    FlutterError.onError = (details) {
      if (details.exceptionAsString().contains('overflowed')) {
        overflows.add(details.exceptionAsString());
        return;
      }
      onError?.call(details);
    };
    try {
      await _scenario(tester);
    } finally {
      FlutterError.onError = onError;
    }
    if (overflows.isNotEmpty) debugPrint('layout overflows during the run: ${overflows.length}');
  });
}

Future<void> _scenario(WidgetTester tester) async {
  await tester.binding.setSurfaceSize(const Size(1400, 900));
  var rust = await RustAppServices.open(_options);
  await tester.pumpWidget(const ProviderScope(child: ConsoleCryptApp()));

  // Welcome → "Use locally" → passphrase → Recovery Kit → 3 words → notice.
  final local = find.descendant(of: find.byKey(const ValueKey('option-local')), matching: find.byType(FilledButton));
  await pumpUntil(tester, local);
  await tester.tap(local);
  await enterKey(tester, 'new-passphrase', _passphrase);
  await enterKey(tester, 'confirm-passphrase', _passphrase);
  await tapKey(tester, 'create-vault');
  await tapKey(tester, 'reveal-kit');
  await tapKey(tester, 'kit-saved');
  await tapKey(tester, 'kit-continue');
  await pumpUntil(tester, find.byKey(const ValueKey('verify-submit')));
  final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
  final words = container.read(onboardingControllerProvider).kit!.exposeWords();
  for (var i = 0; i < RecoveryKit.wordCount; i++) {
    final field = find.byKey(ValueKey('verify-word-$i'));
    if (field.evaluate().isNotEmpty) await tester.enterText(field, words[i]);
  }
  await tester.pump();
  await tapKey(tester, 'verify-submit');
  await tapKey(tester, 'local-notice-continue');
  await pumpUntil(tester, find.byKey(const ValueKey('nav-hosts')));
  expect(rust.services.vault.currentStatus.phase, VaultPhase.unlocked);
  expect(rust.services.vault.currentStatus.recoveryKitConfirmed, isTrue);

  await rust.services.inventory.saveHostWithAuth(
    Host.create(name: 'persisted-host', address: '192.0.2.10'),
    HostAuthInlinePassword(password: SecretText('pw-${DateTime.now().microsecondsSinceEpoch}')),
  );
  await pumpUntil(tester, find.text('persisted-host'));

  if (_memoryStore) return; // keys do not survive a restart then

  // Restart: UI gone, core shut down (SQLCipher closed), then both again.
  await tester.pumpWidget(const SizedBox.shrink());
  await rust.close();
  rust = await RustAppServices.open(_options);
  await tester.pumpWidget(const ProviderScope(child: ConsoleCryptApp()));

  await pumpUntil(tester, find.byKey(const ValueKey('unlock-passphrase')));
  expect(find.byKey(const ValueKey('new-passphrase')), findsNothing, reason: 'not onboarding');
  expect(find.byKey(const ValueKey('option-local')), findsNothing, reason: 'not the welcome screen');
  await enterKey(tester, 'unlock-passphrase', _passphrase);
  await tapKey(tester, 'unlock-submit');
  await pumpUntil(tester, find.text('persisted-host'));
  expect(rust.services.vault.currentStatus.phase, VaultPhase.unlocked);
  await tester.pumpWidget(const SizedBox.shrink());
}
