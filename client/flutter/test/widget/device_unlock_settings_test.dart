import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('vault settings opt in, unlock, then disable device unlock', (tester) async {
    final b = testBackend();
    addTearDown(b.dispose);
    await b.debugSignInDemoAndUnlock();
    await pumpApp(tester, b, locale: AppLocale.ru, size: const Size(1024, 720));
    await tapKey(tester, 'nav-settings');
    final toggle = find.byKey(const ValueKey('device-unlock-switch'));
    expect(tester.widget<SwitchListTile>(toggle).value, isFalse);
    await tapKey(tester, 'device-unlock-switch');
    expect(b.vault.currentStatus.deviceUnlock.enabled, isTrue);
    await tapKey(tester, 'lock-vault');
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('unlock-device'))).onPressed, isNotNull);
    await tapKey(tester, 'unlock-device');
    expect(b.vault.currentStatus.isUnlocked, isTrue);
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'device-unlock-switch');
    expect(b.vault.currentStatus.deviceUnlock.enabled, isFalse);
    await tapKey(tester, 'lock-vault');
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('unlock-device'))).onPressed, isNull);
    await tapKey(tester, 'forgot-passphrase');
    await tester.tap(find.text('Это доверенное устройство'));
    await settle(tester);
    expect(find.textContaining('выключена или сейчас недоступна'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('confirming actions with biometrics is on with device unlock and can be turned off', (tester) async {
    final b = testBackend();
    addTearDown(b.dispose);
    await b.debugSignInDemoAndUnlock();
    await pumpApp(tester, b, locale: AppLocale.ru, size: const Size(1024, 720));
    await tapKey(tester, 'nav-settings');
    final confirm = find.byKey(const ValueKey('confirm-actions-switch'));
    expect(confirm, findsNothing, reason: 'only while device unlock is on');
    await tapKey(tester, 'device-unlock-switch');
    expect(tester.widget<SwitchListTile>(confirm).value, isTrue);
    expect(find.text('Подтверждать действия биометрией'), findsOneWidget);
    expect(find.textContaining('Смена пароля, новый комплект восстановления'), findsOneWidget);
    expect(b.vault.currentStatus.deviceUnlock.presenceAvailable, isTrue);

    // The mock core mints and redeems a single-use token.
    final token = await b.vault.verifyUserPresence(action: StepUpAction.confirm, object: 'k', reason: 'r');
    expect(await b.vault.confirmStepUp(PresenceProof(token), action: StepUpAction.confirm, object: 'k'), isTrue);
    expect(await b.vault.confirmStepUp(PresenceProof(token), action: StepUpAction.confirm, object: 'k'), isFalse);

    await tapKey(tester, 'confirm-actions-switch');
    expect(tester.widget<SwitchListTile>(confirm).value, isFalse);
    expect(b.vault.currentStatus.deviceUnlock.presenceAvailable, isFalse);
    await expectLater(
      b.vault.verifyUserPresence(action: StepUpAction.confirm, object: 'k', reason: 'r'),
      throwsA(isA<AppException>().having((e) => e.code, 'code', AppErrorCode.passphraseRequired)),
    );
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('cancelled authentication leaves the setting off and reports the failure', (tester) async {
    final b = testBackend();
    addTearDown(b.dispose);
    await b.debugSignInDemoAndUnlock();
    b.vault.deviceAuthSucceeds = false;
    await pumpApp(tester, b, locale: AppLocale.ru);
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'device-unlock-switch');
    expect(b.vault.currentStatus.deviceUnlock.enabled, isFalse);
    expect(find.byKey(const ValueKey('device-unlock-error')), findsOneWidget);
    expect(tester.widget<SwitchListTile>(find.byKey(const ValueKey('device-unlock-switch'))).onChanged, isNotNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('unavailable biometric switch explains enrollment and rechecks capability', (tester) async {
    final b = testBackend();
    addTearDown(b.dispose);
    await b.debugSignInDemoAndUnlock();
    b.vault.deviceAuthKind = null;
    b.vault.deviceAuthNotEnrolled = true;
    await pumpApp(tester, b, locale: AppLocale.ru);
    await tapKey(tester, 'nav-settings');
    final toggle = find.byKey(const ValueKey('device-unlock-switch'));
    expect(tester.widget<SwitchListTile>(toggle).onChanged, isNull);
    expect(find.textContaining('Биометрия не настроена'), findsOneWidget);
    b.vault.deviceAuthKind = DeviceAuthKind.touchId;
    b.vault.deviceAuthNotEnrolled = false;
    await tapKey(tester, 'device-unlock-refresh');
    expect(tester.widget<SwitchListTile>(toggle).onChanged, isNotNull);
    expect(tester.widget<SwitchListTile>(toggle).value, isFalse);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('enabled setting can be disabled even when hardware becomes unavailable', (tester) async {
    final b = testBackend();
    addTearDown(b.dispose);
    await b.debugSignInDemoAndUnlock();
    await b.vault.setDeviceUnlockEnabled(true, reason: 'test');
    b.vault.deviceAuthKind = null;
    await pumpApp(tester, b, locale: AppLocale.ru);
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'device-unlock-switch');
    expect(b.vault.currentStatus.deviceUnlock.enabled, isFalse);
    expect(b.vault.currentStatus.deviceTrusted, isTrue);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('system protection is stated; Linux warns and asks before enabling', (tester) async {
    final b = testBackend();
    addTearDown(b.dispose);
    await b.debugSignInDemoAndUnlock();
    await pumpApp(tester, b, locale: AppLocale.ru);
    await tapKey(tester, 'nav-settings');
    expect(find.byKey(const ValueKey('device-unlock-protected')), findsOneWidget);
    expect(find.textContaining('Защищено системой'), findsOneWidget);

    b.vault
      ..deviceAuthKind = DeviceAuthKind.keyring
      ..deviceProtection = DeviceUnlockProtection.convenience;
    await b.vault.refreshDeviceUnlockAvailability();
    await settle(tester);
    expect(find.byKey(const ValueKey('device-unlock-convenience')), findsOneWidget);
    expect(find.textContaining('доступна любой программе вашего пользователя'), findsWidgets);
    await tapKey(tester, 'device-unlock-switch');
    expect(find.byKey(const ValueKey('confirm-ok')), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(b.vault.currentStatus.deviceUnlock.enabled, isFalse, reason: 'declined the warning');
    await tapKey(tester, 'device-unlock-switch');
    await tapKey(tester, 'confirm-ok');
    expect(b.vault.currentStatus.deviceUnlock.enabled, isTrue);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.linux));

  testWidgets('after a passphrase unlock the app offers to turn device unlock on again', (tester) async {
    final b = testBackend();
    addTearDown(b.dispose);
    await b.debugSignInDemoAndUnlock();
    await b.vault.setDeviceUnlockEnabled(true, reason: 'test');
    // A new fingerprint invalidates the OS key.
    b.vault.simulateBiometricChange();
    await b.vault.lock();
    await pumpApp(tester, b, locale: AppLocale.ru);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('unlock-device'))).onPressed, isNull);
    await tester.enterText(find.byKey(const ValueKey('unlock-passphrase')), MockCloud.demoPassphrase);
    await tapKey(tester, 'unlock-submit');
    await settle(tester);
    expect(find.text('Отпечатки изменились'), findsOneWidget);
    await tapKey(tester, 'confirm-ok');
    expect(b.vault.currentStatus.deviceUnlock.enabled, isTrue);
    expect(b.vault.currentStatus.deviceUnlock.reoffer, isNull);

    // "Not now" clears the offer for good.
    b.vault.simulateBiometricChange();
    await b.vault.lock();
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('unlock-passphrase')), MockCloud.demoPassphrase);
    await tapKey(tester, 'unlock-submit');
    await settle(tester);
    await tapKey(tester, 'confirm-cancel');
    expect(b.vault.currentStatus.deviceUnlock.enabled, isFalse);
    expect(b.vault.currentStatus.deviceUnlock.reoffer, isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
