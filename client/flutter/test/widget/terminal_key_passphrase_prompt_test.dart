import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_screen.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

Future<void> _pump(WidgetTester tester, TerminalPasswordPrompt prompt, String locale) async {
  final budget = GlassBackdropBudget();
  addTearDown(budget.dispose);
  final tab = TerminalTab(
    sessionId: const TerminalSessionId('s1'),
    target: SshTerminalTarget(Host.create(name: 'web', address: '192.0.2.10')),
    terminal: Terminal(),
    controller: TerminalController(),
  );
  await tester.pumpWidget(
    ProviderScope(
      retry: (_, _) => null,
      child: MaterialApp(
        locale: Locale(locale),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        theme: AppTheme.build(Brightness.dark, platform: TargetPlatform.macOS),
        themeAnimationDuration: Duration.zero,
        builder: (_, child) => GlassScope(
          data: GlassScopeData(appearance: GlassAppearance.fallback, budget: budget),
          child: child!,
        ),
        home: Scaffold(
          body: SingleChildScrollView(
            child: TerminalPasswordPromptCard(tab: tab, prompt: prompt),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

void main() {
  testWidgets('TODO-05: an encrypted key asks for its passphrase, named after the key', (tester) async {
    await _pump(
      tester,
      const TerminalPasswordPrompt(username: '', hostLabel: 'web', keyName: 'deploy key', retry: true),
      'en',
    );
    expect(find.text('Passphrase for key “deploy key” on web'), findsOneWidget);
    expect(find.text('Key passphrase'), findsOneWidget);
    expect(find.text('Wrong key passphrase, please try again.'), findsOneWidget);
    expect(find.byIcon(Icons.key_rounded), findsOneWidget);
    expect(find.textContaining('Password for'), findsNothing);
  });

  testWidgets('TODO-05: the account password prompt is unchanged (ru)', (tester) async {
    await _pump(tester, const TerminalPasswordPrompt(username: 'deploy', hostLabel: 'web'), 'ru');
    expect(find.text('Пароль для deploy@web'), findsOneWidget);
    expect(find.byIcon(Icons.password_rounded), findsOneWidget);
    expect(find.textContaining('к ключу'), findsNothing);
  });

  testWidgets('TODO-05: the key passphrase prompt in Russian', (tester) async {
    await _pump(tester, const TerminalPasswordPrompt(username: '', hostLabel: 'web', keyName: 'deploy key'), 'ru');
    expect(find.text('Пароль к ключу «deploy key» для web'), findsOneWidget);
    expect(find.text('Пароль к ключу'), findsOneWidget);
  });
}
