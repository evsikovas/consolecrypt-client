import 'package:consolecrypt/automation/automation_file_tree.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const entries = [
  AutomationFileEntry(path: 'deploy.yml', directory: false),
  AutomationFileEntry(path: 'roles/web/tasks/main.yml', directory: false),
  AutomationFileEntry(path: 'empty', directory: true),
  AutomationFileEntry(path: automationGitSourceFile, directory: false),
];

Future<void> pumpTree(
  WidgetTester tester,
  Object scope, {
  List<AutomationFileEntry> files = entries,
  String? selected,
  void Function(AutomationFileDrag, String)? onMove,
  ValueChanged<String>? onRead,
}) => tester.pumpWidget(
  MaterialApp(
    home: Scaffold(
      body: SizedBox(
        width: 320,
        child: SingleChildScrollView(
          child: AutomationFileTree(
            scope: scope,
            projectId: 'project',
            digest: 'original',
            entries: files,
            selectedPath: selected,
            rootLabel: 'Project root',
            emptyLabel: 'Empty',
            onRead: onRead ?? (_) {},
            onMove: onMove,
          ),
        ),
      ),
    ),
  ),
);

void main() {
  test('hierarchy includes implicit and empty folders and hides internal Git metadata', () {
    final tree = automationFileHierarchy(entries);
    expect(tree.at('roles/web/tasks')!.directory, true);
    expect(tree.at('roles/web/tasks/main.yml')!.name, 'main.yml');
    expect(tree.at('empty')!.children, isEmpty);
    expect(tree.at(automationGitSourceFile), isNull);
    expect(tree.sortedChildren.map((node) => node.name), ['empty', 'roles', 'deploy.yml']);
  });

  testWidgets('folders expand compact rows and selected nested files reveal their parents', (tester) async {
    final scope = Object();
    String? read;
    await pumpTree(tester, scope, onRead: (path) => read = path);
    expect(find.byKey(const ValueKey('automation-file-roles/web')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('automation-file-roles')));
    await tester.pump();
    expect(find.byKey(const ValueKey('automation-file-roles/web')), findsOneWidget);
    expect(tester.getSize(find.byKey(const ValueKey('automation-file-roles'))).height, lessThan(40));
    await pumpTree(tester, scope, selected: 'roles/web/tasks/main.yml', onRead: (path) => read = path);
    await tester.pump();
    expect(find.text('main.yml'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('automation-file-roles/web/tasks/main.yml')));
    expect(read, 'roles/web/tasks/main.yml');
    expect(tester.takeException(), isNull);
  });

  testWidgets('actual drag moves a file to a folder using references without contents', (tester) async {
    final scope = Object();
    final moves = <(AutomationFileDrag, String)>[];
    await pumpTree(tester, scope, onMove: (file, directory) => moves.add((file, directory)));
    await tester.pumpAndSettle();
    final gesture = await tester.startGesture(
      tester.getCenter(find.byKey(const ValueKey('automation-file-deploy.yml'))),
    );
    await gesture.moveBy(const Offset(15, 0));
    await tester.pump();
    await gesture.moveTo(tester.getCenter(find.byKey(const ValueKey('automation-drop-roles'))));
    await tester.pump();
    await gesture.up();
    await tester.pumpAndSettle();
    expect(moves.length, 1);
    expect(moves.single.$1.path, 'deploy.yml');
    expect(moves.single.$1.digest, 'original');
    expect(identical(moves.single.$1.scope, scope), true);
    expect(moves.single.$2, 'roles');
  });

  testWidgets('a nested file can be dragged back to the project root', (tester) async {
    final moves = <String>[];
    await pumpTree(
      tester,
      Object(),
      selected: 'roles/web/tasks/main.yml',
      onMove: (_, directory) => moves.add(directory),
    );
    await tester.pumpAndSettle();
    final gesture = await tester.startGesture(
      tester.getCenter(find.byKey(const ValueKey('automation-file-roles/web/tasks/main.yml'))),
    );
    await gesture.moveBy(const Offset(15, 0));
    await tester.pump();
    await gesture.moveTo(tester.getCenter(find.byKey(const ValueKey('automation-drop-'))));
    await tester.pump();
    await gesture.up();
    await tester.pumpAndSettle();
    expect(moves, ['']);
  });

  testWidgets('drop rejects foreign scope, stale snapshot and existing destination files', (tester) async {
    final scope = Object();
    var moves = 0;
    await pumpTree(
      tester,
      scope,
      files: [
        ...entries,
        const AutomationFileEntry(path: 'roles/deploy.yml', directory: false),
      ],
      onMove: (_, _) => moves++,
    );
    final target = tester.widget<DragTarget<AutomationFileDrag>>(find.byKey(const ValueKey('automation-drop-roles')));
    for (final data in [
      const AutomationFileDrag(scope: Object(), projectId: 'project', digest: 'original', path: 'deploy.yml'),
      AutomationFileDrag(scope: scope, projectId: 'project', digest: 'stale', path: 'deploy.yml'),
      AutomationFileDrag(scope: scope, projectId: 'project', digest: 'original', path: 'deploy.yml'),
    ]) {
      expect(target.onWillAcceptWithDetails!(DragTargetDetails(data: data, offset: Offset.zero)), false);
    }
    expect(moves, 0);
  });

  testWidgets('services without move capability keep files readable and never draggable', (tester) async {
    await pumpTree(tester, Object());
    expect(find.byType(Draggable<AutomationFileDrag>), findsNothing);
    expect(find.byKey(const ValueKey('automation-file-deploy.yml')), findsOneWidget);
  });
}
