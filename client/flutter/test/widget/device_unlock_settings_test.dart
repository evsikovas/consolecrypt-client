import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
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
    expect(find.textContaining('Отпечаток не настроен'), findsOneWidget);
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
}
