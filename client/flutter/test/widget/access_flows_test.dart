import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Reads the "Word #N" labels on the verification screen and answers them
/// from the active kit.
Future<void> answerVerification(WidgetTester tester, List<String> words, {bool correct = true}) async {
  for (var i = 0; i < 24; i++) {
    final field = find.byKey(ValueKey('verify-word-$i'));
    if (field.evaluate().isNotEmpty) {
      await tester.enterText(field, correct ? words[i] : 'wrong');
    }
  }
  await tester.pump();
  await tapKey(tester, 'verify-submit');
}

void main() {
  testWidgets('login → unlock: wrong passphrase is rejected, correct one opens the hosts list', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend);

    // First launch: explicit choice between local and server (ADR-0106).
    expect(find.text('Use locally (no account)'), findsOneWidget);
    expect(find.text('Connect to a server'), findsOneWidget);
    await tester.tap(find.text('Sign in or create account'));
    await settle(tester);

    // No default server URL.
    expect(find.text('Connect to your server'), findsOneWidget);
    await enterKey(tester, 'server-url', 'https://sync.example.org');
    await enterKey(tester, 'email', MockCloud.demoEmail);
    await enterKey(tester, 'password', MockCloud.demoPassword);
    await tapKey(tester, 'login-submit');

    expect(find.byKey(const ValueKey('unlock-passphrase')), findsOneWidget);
    await enterKey(tester, 'unlock-passphrase', 'not the passphrase');
    await tapKey(tester, 'unlock-submit');
    expect(find.byKey(const ValueKey('unlock-error')), findsOneWidget);

    await enterKey(tester, 'unlock-passphrase', MockCloud.demoPassphrase);
    await tapKey(tester, 'unlock-submit');
    expect(find.text('Hosts'), findsWidgets);
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('register → onboarding: Recovery Kit must be revealed and 3 words verified', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend);

    await tester.tap(find.text('Sign in or create account'));
    await settle(tester);
    await tester.tap(find.text('Create account'));
    await settle(tester);
    await enterKey(tester, 'server-url', 'https://sync.example.org');
    await enterKey(tester, 'email', 'new-user@example.org');
    await enterKey(tester, 'password', 'account-password-123');
    await enterKey(tester, 'password-confirm', 'account-password-123');
    await tapKey(tester, 'login-submit');

    // Passphrase with strength meter.
    expect(find.text('Create your vault passphrase'), findsOneWidget);
    await enterKey(tester, 'new-passphrase', 'weak');
    await enterKey(tester, 'confirm-passphrase', 'weak');
    expect(isEnabled(tester, 'create-vault'), isFalse, reason: 'weak passphrase');
    await enterKey(tester, 'new-passphrase', 'violet-anchor-muffin-glacier-42');
    await enterKey(tester, 'confirm-passphrase', 'violet-anchor-muffin-glacier-42');
    await tapKey(tester, 'create-vault');

    // Recovery Kit: hidden until revealed; continue needs the checkbox.
    expect(find.text('Save your Recovery Kit'), findsOneWidget);
    expect(find.byKey(const ValueKey('kit-word-0')), findsNothing);
    await tapKey(tester, 'reveal-kit');
    expect(find.byKey(const ValueKey('kit-word-0')), findsOneWidget);
    expect(isEnabled(tester, 'kit-continue'), isFalse);
    await tapKey(tester, 'kit-saved');
    await tapKey(tester, 'kit-continue');

    // Verification: wrong words are rejected; the app stays gated.
    final words = backend.debugActiveRecoveryWords!;
    await answerVerification(tester, words, correct: false);
    expect(find.byKey(const ValueKey('verify-error')), findsOneWidget);
    expect(backend.debugVaultStatus.recoveryKitConfirmed, isFalse);

    await answerVerification(tester, words);
    expect(backend.debugVaultStatus.recoveryKitConfirmed, isTrue);
    expect(find.text('No hosts yet'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('local onboarding: no login, kit + verification, "no cloud copy" notice, Local only indicator', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend);

    await tester.tap(find.text('Use locally'));
    await settle(tester);
    expect(find.text('Create your vault passphrase'), findsOneWidget);
    expect(find.byKey(const ValueKey('server-url')), findsNothing, reason: 'no login in local mode');
    await enterKey(tester, 'new-passphrase', 'violet-anchor-muffin-glacier-42');
    await enterKey(tester, 'confirm-passphrase', 'violet-anchor-muffin-glacier-42');
    await tapKey(tester, 'create-vault');

    expect(find.text('Local profile — no server copy'), findsOneWidget);
    await tapKey(tester, 'reveal-kit');
    await tapKey(tester, 'kit-saved');
    await tapKey(tester, 'kit-continue');
    await answerVerification(tester, backend.debugActiveRecoveryWords!);

    expect(find.text('No cloud copy'), findsOneWidget);
    expect(backend.debugVaultStatus.recoveryKitConfirmed, isFalse, reason: 'completes after the notice');
    await tapKey(tester, 'local-notice-continue');

    expect(backend.debugVaultStatus.recoveryKitConfirmed, isTrue);
    expect(find.text('Local only'), findsWidgets);
    expect(find.byKey(const ValueKey('nav-devices')), findsNothing, reason: 'devices apply to synced profiles');
    expect(backend.profiles.currentProfiles.active!.kind, ProfileKind.local);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('enable sync wizard converts a local profile to synced', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    await pumpApp(tester, backend);

    await tapKey(tester, 'nav-sync');
    expect(find.text('Local only'), findsWidgets);
    await tapKey(tester, 'enable-sync');
    expect(find.byKey(const ValueKey('enable-sync-wizard')), findsOneWidget);

    await enterKey(tester, 'sync-server-url', 'https://sync.example.org');
    await tapKey(tester, 'sync-next');
    await enterKey(tester, 'sync-email', 'me@example.org');
    await enterKey(tester, 'sync-password', 'account-password-123');
    await tapKey(tester, 'sync-start');
    await settle(tester, steps: 20);

    expect(find.text('Sync enabled'), findsOneWidget);
    await tapKey(tester, 'sync-finish');
    final profile = backend.profiles.currentProfiles.active!;
    expect(profile.isSynced, isTrue);
    expect(profile.serverUrl, Uri.parse('https://sync.example.org'));
    expect(find.byKey(const ValueKey('nav-devices')), findsOneWidget, reason: 'devices appear once synced');
    expect(find.byKey(const ValueKey('disconnect-sync')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('forgot passphrase: Recovery Key restores access with a new passphrase', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await backend.vault.lock();
    await pumpApp(tester, backend);

    await tapKey(tester, 'forgot-passphrase');
    expect(find.text('Recover vault access'), findsOneWidget);
    await tester.tap(find.text('Recovery Key'));
    await settle(tester);
    await enterKey(tester, 'recovery-input', MockCloud.demoRecoveryWords.take(12).join(' '));
    await enterKey(tester, 'recovery-new-passphrase', 'seven brisk otters juggle neon');
    await enterKey(tester, 'recovery-confirm', 'seven brisk otters juggle neon');
    expect(isEnabled(tester, 'recovery-submit'), isFalse, reason: '12 of 24 words');

    await enterKey(tester, 'recovery-input', MockCloud.demoRecoveryWords.join(' '));
    await tapKey(tester, 'recovery-submit');
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsOneWidget);

    // The new passphrase now unlocks the vault.
    await tapKey(tester, 'lock-vault');
    await enterKey(tester, 'unlock-passphrase', 'seven brisk otters juggle neon');
    await tapKey(tester, 'unlock-submit');
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('new device: waits for approval showing its own verification code', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend);

    await tester.tap(find.text('Sign in or create account'));
    await settle(tester);
    await enterKey(tester, 'server-url', 'https://sync.example.org');
    await enterKey(tester, 'email', MockCloud.newDeviceEmail);
    await enterKey(tester, 'password', MockCloud.demoPassword);
    await tapKey(tester, 'login-submit');

    expect(find.text('This device is not trusted yet'), findsOneWidget);
    expect(isEnabled(tester, 'unlock-device'), isFalse);
    await tapKey(tester, 'request-approval');

    expect(find.text('Approve this device'), findsOneWidget);
    for (var i = 0; i < 6; i++) {
      expect(find.byKey(ValueKey('code-group-$i')), findsOneWidget);
    }
    await tapKey(tester, 'simulate-approval');
    expect(find.byKey(const ValueKey('host-prod-db-1')), findsOneWidget);
    expect(backend.debugVaultStatus.deviceTrusted, isTrue);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
