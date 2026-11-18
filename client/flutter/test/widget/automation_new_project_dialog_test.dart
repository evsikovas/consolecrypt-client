import 'package:consolecrypt/automation/automation_new_project_dialog.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  testWidgets('a role project returns the selected folder and role name', (tester) async {
    NewAutomationProject? result;
    await tester.pumpWidget(
      MaterialApp(
        locale: const Locale('en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () async => result = await showDialog<NewAutomationProject>(
                context: context,
                builder: (_) => AutomationNewProjectDialog(chooseDirectory: () async => 'D:/work/ansible'),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('automation-new-project-name')), 'App setup');
    await tester.tap(find.byKey(const ValueKey('automation-new-project-template')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ansible role and playbook').last);
    await tester.pumpAndSettle();
    expect(tester.widget<FilledButton>(find.widgetWithText(FilledButton, 'Create')).onPressed, isNull);
    await tester.enterText(find.byKey(const ValueKey('automation-new-project-role')), 'app_setup');
    await tester.tap(find.text('Choose folder…'));
    await tester.pumpAndSettle();
    expect(find.text('D:/work/ansible'), findsOneWidget);
    await tester.tap(find.text('Create'));
    await tester.pumpAndSettle();
    expect(result, (name: 'App setup', directory: 'D:/work/ansible', role: 'app_setup'));
  });
}
