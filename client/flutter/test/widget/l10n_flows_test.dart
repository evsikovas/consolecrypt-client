import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('Settings → Language switches the visible texts immediately and persists the choice', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await tapKey(tester, 'nav-settings');
    expect(find.text('Hosts'), findsOneWidget);
    expect(find.text('Language'), findsOneWidget);

    await tapKey(tester, 'app-locale');
    await tester.tap(find.byKey(const ValueKey('app-locale-ru')).last);
    await settle(tester);

    expect(backend.services.settings.currentLocal.appLocale, AppLocale.ru);
    // Sidebar, page title and the selector itself are Russian now (no restart).
    expect(find.text('Хосты'), findsOneWidget);
    expect(find.text('Hosts'), findsNothing);
    expect(find.text('Язык'), findsOneWidget);
    expect(find.text('Русский'), findsWidgets);
    expect(tester.takeException(), isNull);

    // …and back to English.
    await tapKey(tester, 'app-locale');
    await tester.tap(find.byKey(const ValueKey('app-locale-en')).last);
    await settle(tester);
    expect(backend.services.settings.currentLocal.appLocale, AppLocale.en);
    expect(find.text('Hosts'), findsOneWidget);
    expect(find.text('Хосты'), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Welcome screen language switcher works before any profile exists', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend, locale: AppLocale.system);

    // The test platform reports en_US → System resolves to English.
    expect(find.text('Welcome to ConsoleCrypt'), findsOneWidget);
    expect(find.text('Personal'), findsOneWidget, reason: 'default profile name');

    await tapKey(tester, 'language-menu');
    await tester.tap(find.byKey(const ValueKey('language-ru')).last);
    await settle(tester);

    expect(backend.services.settings.currentLocal.appLocale, AppLocale.ru);
    expect(find.text('Добро пожаловать в ConsoleCrypt'), findsOneWidget);
    expect(find.text('Работать локально (без аккаунта)'), findsOneWidget);
    // The untouched default profile name follows the language.
    expect(find.text('Личный'), findsOneWidget);
    expect(tester.takeException(), isNull);

    // A name typed by the user is kept when switching back.
    await tester.enterText(find.byType(TextField).first, 'Дом');
    await tapKey(tester, 'language-menu');
    await tester.tap(find.byKey(const ValueKey('language-en')).last);
    await settle(tester);
    expect(find.text('Welcome to ConsoleCrypt'), findsOneWidget);
    expect(find.text('Дом'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('System language follows the OS locale and falls back to English', (tester) async {
    tester.platformDispatcher.localesTestValue = const [Locale('ru', 'RU')];
    addTearDown(tester.platformDispatcher.clearLocalesTestValue);
    final backend = testBackend();
    addTearDown(backend.dispose);
    await pumpApp(tester, backend, locale: AppLocale.system);
    expect(find.text('Добро пожаловать в ConsoleCrypt'), findsOneWidget);

    tester.platformDispatcher.localesTestValue = const [Locale('de', 'DE')];
    await settle(tester);
    expect(find.text('Welcome to ConsoleCrypt'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
