import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:intl/date_symbol_data_local.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('a trusted device is told about a new device; "It\'s me" dismisses, "Revoke" revokes', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    expect(find.byKey(const ValueKey('new-device-banner')), findsNothing);

    backend.devices.debugAnnounceNewDevice('Stranger laptop');
    await settle(tester);
    expect(find.byKey(const ValueKey('new-device-banner')), findsOneWidget);
    expect(find.textContaining('Stranger laptop'), findsWidgets);
    await tapKey(tester, 'new-device-its-me');
    expect(find.byKey(const ValueKey('new-device-banner')), findsNothing);
    expect(
      backend.devices.currentSnapshot.devices.firstWhere((d) => d.name == 'Stranger laptop').isRevoked,
      isFalse,
      reason: '"It\'s me" only dismisses',
    );

    backend.devices.debugAnnounceNewDevice('Intruder PC', platform: DevicePlatform.windows);
    await settle(tester);
    await tapKey(tester, 'new-device-revoke');
    await tapKey(tester, 'confirm-ok');
    expect(find.byKey(const ValueKey('new-device-banner')), findsNothing);
    expect(backend.devices.currentSnapshot.devices.firstWhere((d) => d.name == 'Intruder PC').isRevoked, isTrue);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('device list marks unconfirmed devices and the quarantine only with the capability', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-devices');
    expect(find.text('Not confirmed'), findsNothing, reason: 'older servers: old labels');
    expect(find.textContaining('restricted until'), findsNothing);

    backend.devices.debugAnnounceNewDevice('Stranger laptop');
    await settle(tester);
    expect(find.byKey(const ValueKey('quarantine-Stranger laptop')), findsOneWidget);
    expect(find.text('Not confirmed'), findsWidgets);
    // The demo's trusted devices carry no quarantine badge.
    expect(find.byKey(const ValueKey('quarantine-Office PC')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  test('quarantine and envelope-limit errors name when the action is possible again', () async {
    await initializeDateFormatting('en');
    await initializeDateFormatting('ru');
    for (final locale in const [Locale('en'), Locale('ru')]) {
      final l = lookupAppLocalizations(locale);
      final until = DateTime.utc(2026, 11, 4, 10);
      final quarantine = errorMessage(
        l,
        AppException(
          AppErrorCode.deviceInQuarantine,
          'new',
          args: {'until_ms': until.millisecondsSinceEpoch.toString()},
        ),
      );
      expect(quarantine, contains('2026'), reason: locale.languageCode);
      expect(errorMessage(l, const AppException(AppErrorCode.deviceInQuarantine, 'new')), l.errorDeviceInQuarantine);
      final limited = errorMessage(
        l,
        const AppException(AppErrorCode.envelopeRateLimited, 'slow', args: {'retry_after_seconds': '600'}),
      );
      expect(limited, isNot(l.errorEnvelopeRateLimited));
      expect(errorMessage(l, const AppException(AppErrorCode.envelopeRateLimited, 'slow')), l.errorEnvelopeRateLimited);
    }
  });
}
