import 'package:consolecrypt/automation/automation_ansible_config.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  Future<void> open(
    WidgetTester tester, {
    String text = '',
    Locale locale = const Locale('en'),
    void Function(AutomationAnsibleConfigResult?)? result,
  }) async {
    await tester.pumpWidget(
      MaterialApp(
        locale: locale,
        supportedLocales: const [Locale('en'), Locale('ru')],
        localizationsDelegates: [...GlobalMaterialLocalizations.delegates, AppLocalizations.delegate],
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () async {
                final value = await showDialog<AutomationAnsibleConfigResult>(
                  context: context,
                  builder: (_) => AutomationAnsibleConfigDialog(text: text),
                );
                result?.call(value);
              },
              child: const Text('open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
  }

  testWidgets('opening makes no proposal; Cancel returns null', (tester) async {
    AutomationAnsibleConfigResult? result;
    await open(tester, text: '[defaults]\nhost_key_checking=False\n', result: (value) => result = value);
    expect(find.textContaining('does not disable Native host verification'), findsOneWidget);
    expect(tester.widget<FilledButton>(find.widgetWithText(FilledButton, 'Apply changes')).onPressed, isNull);
    expect(find.byType(Switch), findsNothing);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(result, isNull);
    expect(find.byType(AutomationAnsibleConfigDialog), findsNothing);
  });

  testWidgets('explicit timeout edit returns lossless proposal and does not save directly', (tester) async {
    AutomationAnsibleConfigResult? result;
    const original = '# keep\r\n[defaults]\r\nunknown=yes\r\n';
    await open(tester, text: original, result: (value) => result = value);
    await tester.enterText(find.byKey(const ValueKey('task_timeout')), '15');
    await tester.pumpAndSettle();
    await tester.tap(find.text('Apply changes'));
    await tester.pumpAndSettle();
    expect(result?.action, AutomationAnsibleConfigAction.apply);
    expect(result?.proposedText, '# keep\r\n[defaults]\r\ntask_timeout = 15\r\nunknown=yes\r\n');
  });

  testWidgets('invalid cleared timeout blocks apply; reset restores unchanged state', (tester) async {
    await open(tester, text: '[defaults]\ntask_timeout=15\n');
    await tester.enterText(find.byKey(const ValueKey('task_timeout')), '');
    await tester.pumpAndSettle();
    expect(find.text('Enter a whole number from 0.'), findsOneWidget);
    expect(tester.widget<FilledButton>(find.widgetWithText(FilledButton, 'Apply changes')).onPressed, isNull);
    await tester.ensureVisible(find.text('Reset form changes'));
    await tester.tap(find.text('Reset form changes'));
    await tester.pumpAndSettle();
    expect(tester.widget<TextField>(find.byKey(const ValueKey('task_timeout'))).controller?.text, '15');
    expect(find.text('Enter a whole number from 0.'), findsNothing);
  });

  testWidgets('advanced navigation returns no partially edited proposal', (tester) async {
    AutomationAnsibleConfigResult? result;
    await open(tester, result: (value) => result = value);
    await tester.enterText(find.byKey(const ValueKey('task_timeout')), '12');
    await tester.tap(find.text('Advanced editor'));
    await tester.pumpAndSettle();
    expect(result?.action, AutomationAnsibleConfigAction.advanced);
    expect(result?.proposedText, isNull);
  });

  testWidgets('ambiguous INI offers advanced with disabled form', (tester) async {
    await open(tester, text: '[defaults]\ngathering=implicit\ngathering=smart\n');
    expect(find.textContaining('Ambiguous or complex INI'), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('task_timeout'))).enabled, isFalse);
    expect(tester.widget<FilledButton>(find.widgetWithText(FilledButton, 'Apply changes')).onPressed, isNull);
    expect(find.text('Advanced editor'), findsOneWidget);
  });

  testWidgets('local Russian labels and narrow viewport remain usable', (tester) async {
    tester.view.physicalSize = const Size(480, 640);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await open(tester, locale: const Locale('ru'));
    expect(find.text('Настройки Ansible'), findsOneWidget);
    expect(find.text('Расширенный редактор'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
