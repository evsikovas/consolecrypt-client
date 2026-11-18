import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_launch_passwords.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart' show settle, tapKey;

Future<void> mount(
  WidgetTester tester, {
  required bool Function() isCurrent,
  required ValueChanged<AutomationLaunchPasswords?> result,
  bool allowVault = true,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      locale: const Locale('en'),
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Builder(
        builder: (context) => Scaffold(
          body: TextButton(
            key: const ValueKey('open'),
            onPressed: () async =>
                result(await showAutomationLaunchPasswords(context, isCurrent: isCurrent, allowVault: allowVault)),
            child: const Text('Open'),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
  await tapKey(tester, 'open');
}

Future<void> enter(WidgetTester tester, String key, String value) async {
  final parent = find.byKey(ValueKey(key));
  expect(
    tester.widget<EditableText>(find.descendant(of: parent, matching: find.byType(EditableText))).obscureText,
    isTrue,
  );
  await tester.enterText(find.descendant(of: parent, matching: find.byType(TextFormField)), value);
  await settle(tester);
}

void main() {
  testWidgets('one-launch Vault and become values stay masked and return only a secret owner', (tester) async {
    AutomationLaunchPasswords? value;
    await mount(tester, isCurrent: () => true, result: (v) => value = v);
    await tapKey(tester, 'automation-launch-vault-enable');
    await enter(tester, 'automation-launch-vault-password', 'synthetic-vault-password');
    await tapKey(tester, 'automation-launch-become-enable');
    await enter(tester, 'automation-launch-become-password', 'synthetic-sudo-password');
    await tapKey(tester, 'automation-launch-passwords-apply');
    expect(value!.vaultPassword!.expose(), 'synthetic-vault-password');
    expect(value!.becomePassword!.expose(), 'synthetic-sudo-password');
    expect(value!.toString(), isNot(contains('synthetic')));
    value!.wipe();
    expect(value!.vaultPassword!.isWiped, isTrue);
    expect(value!.becomePassword!.isWiped, isTrue);
    expect(value!.hasAny, isFalse);
    expect(tester.takeException(), isNull);
  });

  testWidgets('project password mode returns only fixed-source metadata without reading password', (tester) async {
    AutomationLaunchPasswords? value;
    await mount(tester, isCurrent: () => true, result: (v) => value = v);
    await tapKey(tester, 'automation-launch-vault-enable');
    await tapKey(tester, 'automation-launch-vault-file');
    expect(find.byKey(const ValueKey('automation-launch-vault-password')), findsNothing);
    await tapKey(tester, 'automation-launch-passwords-apply');
    expect(value!.useProjectVaultPassword, isTrue);
    expect(value!.vaultPassword, isNull);
    expect(value!.becomePassword, isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('saved Vault selection permits become input without a conflicting one-shot Vault source', (tester) async {
    AutomationLaunchPasswords? value;
    await mount(tester, isCurrent: () => true, result: (v) => value = v, allowVault: false);
    expect(find.byKey(const ValueKey('automation-launch-vault-enable')), findsNothing);
    await tapKey(tester, 'automation-launch-become-enable');
    await enter(tester, 'automation-launch-become-password', 'synthetic-sudo-password');
    await tapKey(tester, 'automation-launch-passwords-apply');
    expect(value!.hasVault, isFalse);
    expect(value!.becomePassword, isNotNull);
    value!.wipe();
    expect(tester.takeException(), isNull);
  });

  testWidgets('a retired original launch never accepts dialog secrets', (tester) async {
    AutomationLaunchPasswords? value;
    var current = true;
    await mount(tester, isCurrent: () => current, result: (v) => value = v);
    await tapKey(tester, 'automation-launch-vault-enable');
    await enter(tester, 'automation-launch-vault-password', 'synthetic-vault-password');
    current = false;
    await tapKey(tester, 'automation-launch-passwords-apply');
    expect(value, isNull);
    expect(tester.takeException(), isNull);
  });
}
