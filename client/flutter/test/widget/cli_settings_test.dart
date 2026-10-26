import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/settings/cli_installer.dart';
import 'package:consolecrypt/settings/cli_settings.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _Installer extends CliInstaller {
  _Installer({this.bundled = true});
  final bool bundled;
  bool installed = false;
  int installs = 0, removals = 0;

  @override
  Future<CliCommandState> state() async => CliCommandState(
    bundled: bundled ? '/Applications/ConsoleCrypt.app/Contents/Helpers/consolecrypt-cli' : null,
    installed: installed,
    location: '/usr/local/bin/consolecrypt-cli',
  );

  @override
  Future<void> install() async {
    installs++;
    installed = true;
  }

  @override
  Future<void> uninstall() async {
    removals++;
    installed = false;
  }
}

Future<void> _pump(WidgetTester tester, CliInstaller installer) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [cliInstallerProvider.overrideWithValue(installer)],
      child: const MaterialApp(
        locale: Locale('ru'),
        localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(body: SingleChildScrollView(child: CliSettingsSection())),
      ),
    ),
  );
  await settle(tester);
}

void main() {
  testWidgets('installs and removes the bundled command from Settings', (tester) async {
    final installer = _Installer();
    await _pump(tester, installer);
    expect(find.text('Командная строка'), findsOneWidget);
    expect(find.text('Команда consolecrypt-cli ещё не установлена в систему.'), findsOneWidget);
    await tapKey(tester, 'cli-install');
    expect(installer.installs, 1);
    expect(find.text('Установлено: /usr/local/bin/consolecrypt-cli'), findsOneWidget);
    await tapKey(tester, 'cli-uninstall');
    expect(installer.removals, 1);
    expect(find.byKey(const ValueKey('cli-install')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('a build without the command explains where to get it', (tester) async {
    await _pump(tester, _Installer(bundled: false));
    expect(find.byKey(const ValueKey('cli-install')), findsNothing);
    expect(find.textContaining('нет consolecrypt-cli'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.linux));

  testWidgets('phones have no command line section', (tester) async {
    await _pump(tester, _Installer());
    expect(find.byKey(const ValueKey('settings-cli')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.iOS));
}
