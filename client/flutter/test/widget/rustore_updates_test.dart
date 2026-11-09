import 'dart:io';

import 'package:consolecrypt/app/distribution.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/updates/update_controller.dart';
import 'package:consolecrypt/updates/update_service.dart';
import 'package:consolecrypt/updates/update_settings.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

// Run with --flavor rustore; Flutter sets FLUTTER_APP_FLAVOR for the test.
void main() {
  test('RuStore cannot fetch feeds, download APKs or start an installer', () async {
    var networkCalls = 0;
    final service = UpdateService(
      platform: 'android-arm64',
      clientFactory: () {
        networkCalls++;
        throw StateError('A store build must not contact the update feed');
      },
    );
    final release = UpdateRelease(
      version: '1.0.6',
      build: 67020,
      platform: 'android-arm64',
      url: Uri.https('example.test', '/app.apk'),
      bytes: 1,
      sha256: '0' * 64,
      notes: const {},
    );
    final blocked = throwsA(isA<UpdateException>().having((e) => e.code, 'code', 'store'));
    await expectLater(service.check(), blocked);
    await expectLater(service.download(release, (_) {}), blocked);
    await expectLater(service.install(File('must-not-be-opened.apk'), release), blocked);
    expect(networkCalls, 0);
  }, skip: !isRuStoreDistribution);

  for (final language in ['ru', 'en']) {
    testWidgets(
      'RuStore $language offers only store updates',
      (tester) async {
        final backend = testBackend();
        addTearDown(backend.dispose);
        Uri? opened;
        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              appServicesProvider.overrideWithValue(backend.services),
              updateOpenUrlProvider.overrideWithValue((uri) async {
                opened = uri;
                return true;
              }),
            ],
            child: MaterialApp(
              locale: Locale(language),
              localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
              supportedLocales: AppLocalizations.supportedLocales,
              home: const Scaffold(body: UpdateNoticeScope(child: UpdateSettingsSection())),
            ),
          ),
        );
        await settle(tester);
        expect(AppPlatform.supportsDirectUpdates, isFalse);
        expect(find.byKey(const ValueKey('updates-check')), findsNothing);
        expect(find.byKey(const ValueKey('updates-automatic')), findsNothing);
        expect(find.byKey(const ValueKey('updates-install')), findsNothing);
        expect(find.byKey(const ValueKey('update-notice')), findsNothing);
        await tapKey(tester, 'updates-rustore');
        expect(opened, ruStoreApplicationPage);
        expect(tester.takeException(), isNull);
      },
      skip: !isRuStoreDistribution,
      variant: TargetPlatformVariant.only(TargetPlatform.android),
    );
  }
}
