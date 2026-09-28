import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Gate screens (welcome → login / onboarding / unlock / recovery / device
/// approval / restore) in Russian at 1024 px: any overflow fails the test.
void main() {
  const size = Size(1024, 720);
  final ru = lookupAppLocalizations(const Locale('ru'));

  void noLayoutErrors(WidgetTester tester, String screen) => expect(tester.takeException(), isNull, reason: screen);

  testWidgets('welcome and restore render in Russian', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend, locale: AppLocale.ru, size: size);
    expect(find.text(ru.welcomeTitle), findsOneWidget);
    noLayoutErrors(tester, 'welcome');

    await tester.ensureVisible(find.text(ru.welcomeRestoreFromBackup));
    await tester.tap(find.text(ru.welcomeRestoreFromBackup));
    await settle(tester);
    noLayoutErrors(tester, 'restore');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('login and device approval wait render in Russian', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend, locale: AppLocale.ru, size: size);
    await tester.tap(find.text(ru.welcomeServerAction));
    await settle(tester);
    noLayoutErrors(tester, 'login');

    await enterKey(tester, 'server-url', 'https://sync.example.org');
    await enterKey(tester, 'email', MockCloud.newDeviceEmail);
    await enterKey(tester, 'password', MockCloud.demoPassword);
    // The Russian form is taller than the window: let the caret scroll finish.
    await settle(tester);
    await tapKey(tester, 'login-submit');
    noLayoutErrors(tester, 'device not trusted');

    await tapKey(tester, 'request-approval');
    for (var i = 0; i < 6; i++) {
      expect(find.byKey(ValueKey('code-group-$i')), findsOneWidget);
    }
    noLayoutErrors(tester, 'approval wait');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('local onboarding (passphrase, Recovery Kit, verification, no cloud copy) renders in Russian', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend, locale: AppLocale.ru, size: size);

    await tester.tap(find.text(ru.welcomeLocalAction));
    await settle(tester);
    noLayoutErrors(tester, 'create vault');
    await enterKey(tester, 'new-passphrase', 'violet-anchor-muffin-glacier-42');
    await enterKey(tester, 'confirm-passphrase', 'violet-anchor-muffin-glacier-42');
    await settle(tester);
    noLayoutErrors(tester, 'strength meter');
    await tapKey(tester, 'create-vault');

    noLayoutErrors(tester, 'recovery kit (hidden)');
    await tapKey(tester, 'reveal-kit');
    noLayoutErrors(tester, 'recovery kit (revealed)');
    await tapKey(tester, 'kit-saved');
    await tapKey(tester, 'kit-continue');
    noLayoutErrors(tester, 'verify kit');

    final words = backend.debugActiveRecoveryWords!;
    for (var i = 0; i < words.length; i++) {
      final field = find.byKey(ValueKey('verify-word-$i'));
      if (field.evaluate().isNotEmpty) await tester.enterText(field, words[i]);
    }
    await settle(tester);
    await tapKey(tester, 'verify-submit');
    noLayoutErrors(tester, 'local notice');
    await tapKey(tester, 'local-notice-continue');
    expect(backend.debugVaultStatus.recoveryKitConfirmed, isTrue);
    expect(find.text(ru.navHosts), findsWidgets);
    noLayoutErrors(tester, 'hosts');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('unlock and recovery render in Russian; errors are localized', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await backend.vault.lock();
    await pumpApp(tester, backend, locale: AppLocale.ru, size: size);
    noLayoutErrors(tester, 'unlock');

    await enterKey(tester, 'unlock-passphrase', 'not the passphrase');
    await tapKey(tester, 'unlock-submit');
    expect(find.byKey(const ValueKey('unlock-error')), findsOneWidget);
    expect(find.text(ru.errorWrongPassphrase), findsOneWidget, reason: 'localized, not the raw service message');
    noLayoutErrors(tester, 'unlock error');

    await tapKey(tester, 'forgot-passphrase');
    noLayoutErrors(tester, 'recovery');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
