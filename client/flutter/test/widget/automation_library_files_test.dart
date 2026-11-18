import 'package:consolecrypt/automation/automation_library_files.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const entries = [
  AutomationFileEntry(path: 'deploy.yml', directory: false),
  AutomationFileEntry(path: 'playbooks/verify.yaml', directory: false),
  AutomationFileEntry(path: 'roles/web/tasks/main.yml', directory: false),
  AutomationFileEntry(path: 'roles/web/vars/main.yml', directory: false),
  AutomationFileEntry(path: 'host_vars/demo.yml', directory: false),
  AutomationFileEntry(path: 'requirements.yml', directory: false),
  AutomationFileEntry(path: 'inventory.yaml', directory: false),
  AutomationFileEntry(path: 'collections/site.yml', directory: false),
];

Future<void> open(
  WidgetTester tester,
  Object scope, {
  List<AutomationFileEntry> files = entries,
  ValueChanged<String>? read,
  bool enabled = true,
  String id = 'project',
  double scale = 1,
  VoidCallback? createRole,
  VoidCallback? addDependency,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      home: MediaQuery(
        data: MediaQueryData(textScaler: TextScaler.linear(scale)),
        child: Scaffold(
          body: SizedBox(
            width: 260,
            child: SingleChildScrollView(
              child: AutomationLibraryFiles(
                scope: scope,
                projectId: id,
                digest: 'snapshot',
                entries: files,
                enabled: enabled,
                onRead: read ?? (_) {},
                onCreateRole: createRole,
                onAddDependency: addDependency,
              ),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> choose(WidgetTester tester, AutomationLibraryFilesView value) async {
  tester
      .widget<GlassSelect<AutomationLibraryFilesView>>(find.byKey(const ValueKey('automation-library-file-kind')))
      .onChanged!(value);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('playbooks excludes role configuration and known inventory/dependency YAML', (tester) async {
    String? read;
    await open(tester, Object(), read: (path) => read = path);
    await choose(tester, AutomationLibraryFilesView.playbooks);
    expect(find.byKey(const ValueKey('automation-library-item-deploy.yml')), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-library-item-playbooks/verify.yaml')), findsOneWidget);
    for (final path in ['requirements.yml', 'inventory.yaml', 'roles/web/tasks/main.yml', 'collections/site.yml']) {
      expect(find.byKey(ValueKey('automation-library-item-$path')), findsNothing);
    }
    expect(read, isNull);
    await tester.tap(find.byKey(const ValueKey('automation-library-item-deploy.yml')));
    expect(read, 'deploy.yml');
  });
  testWidgets('roles opens exact actual tasks YAML only on explicit click', (tester) async {
    final reads = <String>[];
    await open(tester, Object(), read: reads.add);
    await choose(tester, AutomationLibraryFilesView.roles);
    expect(reads, isEmpty);
    expect(find.text('web'), findsOneWidget);
    expect(find.text('Opens the role tasks for editing. Run roles through a playbook.'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('automation-library-item-roles/web/tasks/main.yml')));
    expect(reads, ['roles/web/tasks/main.yml']);
  });
  testWidgets('busy categories cannot read and project replacement resets to full files', (tester) async {
    final scope = Object(), reads = <String>[];
    await open(tester, scope, read: reads.add, enabled: false);
    await choose(tester, AutomationLibraryFilesView.roles);
    await tester.tap(find.byKey(const ValueKey('automation-library-item-roles/web/tasks/main.yml')));
    expect(reads, isEmpty);
    await open(tester, scope, id: 'replacement');
    expect(find.byKey(const ValueKey('automation-file-root')), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-library-item-roles/web/tasks/main.yml')), findsNothing);
  });
  testWidgets('role creation and verified dependency import need their own explicit action', (tester) async {
    var creates = 0, imports = 0;
    await open(tester, Object(), createRole: () => creates++, addDependency: () => imports++);
    await choose(tester, AutomationLibraryFilesView.roles);
    expect(creates, 0);
    expect(imports, 0);
    await tester.tap(find.byKey(const ValueKey('automation-library-new-role')));
    expect(creates, 1);
    expect(imports, 0);
    await tester.tap(find.byKey(const ValueKey('automation-library-add-dependency')));
    expect(imports, 1);
  });
  testWidgets('empty categories fit narrow scaled view with no reads', (tester) async {
    await open(tester, Object(), files: const [], scale: 1.4);
    await choose(tester, AutomationLibraryFilesView.roles);
    expect(find.text('No roles in this project. Create a role or add a verified dependency.'), findsOneWidget);
    await choose(tester, AutomationLibraryFilesView.playbooks);
    expect(find.text('No playbooks in this project. Create a playbook or import a project.'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
