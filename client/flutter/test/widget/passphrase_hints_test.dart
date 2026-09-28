import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/keyboard_layout.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Guards against "wrong passphrase" caused by Caps Lock or a different
/// keyboard layout (e.g. Russian instead of English).
void main() {
  final en = lookupAppLocalizations(const Locale('en'));
  final ru = lookupAppLocalizations(const Locale('ru'));

  test('non-A–Z letters are detected from the typed characters', () {
    expect(nonLatinScript('correct horse battery 42!'), isNull);
    expect(nonLatinScript(r'p@ss→word — “quoted” 😀 ~`$'), isNull, reason: 'punctuation, symbols and emoji');
    expect(nonLatinScript('пароль'), TypedScript.cyrillic);
    expect(nonLatinScript('ghbdtn мир'), TypedScript.cyrillic);
    expect(nonLatinScript('pässwörd'), TypedScript.otherNonLatin);
    expect(nonLatinScript('ü then я'), TypedScript.cyrillic, reason: 'Cyrillic wins');
    expect(nonLatinScript('密码'), TypedScript.otherNonLatin);
  });

  Future<void> pumpLocked(WidgetTester tester, {AppLocale locale = AppLocale.en}) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await backend.vault.lock();
    await pumpApp(tester, backend, locale: locale);
    expect(find.byKey(const ValueKey('unlock-passphrase')), findsOneWidget);
  }

  Future<void> toggleCapsLock(WidgetTester tester) async {
    await tester.sendKeyDownEvent(LogicalKeyboardKey.capsLock);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.capsLock);
    await tester.pump();
  }

  testWidgets('unlock: Caps Lock note while the passphrase field is focused', (tester) async {
    await pumpLocked(tester);
    await tester.tap(find.byKey(const ValueKey('unlock-passphrase')));
    await tester.pump();
    expect(find.byKey(const ValueKey('caps-lock-note')), findsNothing);

    await toggleCapsLock(tester);
    expect(find.byKey(const ValueKey('caps-lock-note')), findsOneWidget);
    expect(find.text(en.glassCapsLockOn), findsOneWidget);

    await toggleCapsLock(tester);
    expect(find.byKey(const ValueKey('caps-lock-note')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final (locale, l10n) in [(AppLocale.en, en), (AppLocale.ru, ru)]) {
    testWidgets('unlock: Cyrillic input shows a keyboard-layout hint (${locale.wireName})', (tester) async {
      await pumpLocked(tester, locale: locale);
      await enterKey(tester, 'unlock-passphrase', 'ыукеуч');
      expect(find.byKey(const ValueKey('keyboard-layout-note')), findsOneWidget);
      expect(find.text(l10n.secretFieldCyrillicHint), findsOneWidget);
      expect(
        find.byWidgetPredicate((w) => w is Text && (w.data?.contains('ыукеуч') ?? false)),
        findsNothing,
        reason: 'the hint never repeats the secret',
      );

      await enterKey(tester, 'unlock-passphrase', 'secret');
      expect(find.byKey(const ValueKey('keyboard-layout-note')), findsNothing);

      await enterKey(tester, 'unlock-passphrase', 'sëcret');
      expect(find.text(l10n.secretFieldNonLatinHint), findsOneWidget);
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  testWidgets('create vault: a non-A–Z passphrase warns about the layout but can still be used', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend);
    await tester.tap(find.text(en.welcomeLocalAction));
    await settle(tester);

    const passphrase = 'фиолетовый-якорь-маффин-42';
    await enterKey(tester, 'new-passphrase', 'violet-anchor-muffin-glacier-42');
    expect(find.byKey(const ValueKey('passphrase-layout-notice')), findsNothing);
    await enterKey(tester, 'new-passphrase', passphrase);
    await enterKey(tester, 'confirm-passphrase', passphrase);
    await settle(tester);
    expect(find.byKey(const ValueKey('passphrase-layout-notice')), findsOneWidget);
    expect(find.text(en.passphraseLayoutNotice), findsOneWidget);
    expect(isEnabled(tester, 'create-vault'), isTrue, reason: 'a notice, not a blocker');

    await tapKey(tester, 'create-vault');
    expect(find.text(en.recoveryKitTitle), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('unlock: "Delete this profile…" needs an explicit acknowledgement for a local vault', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    await backend.vault.lock();
    await pumpApp(tester, backend);

    await tapKey(tester, 'unlock-delete-profile');
    expect(find.byKey(const ValueKey('delete-profile-dialog')), findsOneWidget);
    expect(find.text(en.deleteProfileLocalWarning), findsOneWidget);
    expect(isEnabled(tester, 'delete-profile-confirm'), isFalse);

    await tapKey(tester, 'delete-profile-ack');
    expect(isEnabled(tester, 'delete-profile-confirm'), isTrue);
    await tapKey(tester, 'delete-profile-confirm');

    expect(backend.profiles.currentProfiles.profiles, isEmpty);
    expect(find.text(en.welcomeTitle), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
