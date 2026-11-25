import 'dart:io';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/editor/editor_screen.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path/path.dart' as p;
import 'package:re_editor/re_editor.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';

void main() {
  testWidgets('Редактор opens a project, edits a tab and launches the selected shell below the file', (tester) async {
    final scratch = Directory.systemTemp.createTempSync('cc-editor-');
    final root = Directory(p.join(scratch.path, 'service-api'))..createSync();
    addTearDown(() => scratch.deleteSync(recursive: true));
    expect(Process.runSync('git', ['-C', root.path, 'init', '-b', 'main']).exitCode, 0);
    final file = File(p.join(root.path, 'server.py'))..writeAsStringSync('def run(port):\n    return port\n');
    File(p.join(root.path, 'config.yaml')).writeAsStringSync('port: 8080\n');
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    backend.terminal.localShellProfiles = const LocalShellProfiles(
      profiles: [LocalShellProfile(id: 'windows:cmd', kind: 'cmd', label: 'Command Prompt', shell: 'cmd')],
    );
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
    await loadAutomationProofFonts(tester);
    await pumpApp(
      tester,
      backend,
      locale: AppLocale.ru,
      captureBoundary: true,
      overrides: [editorFolderPickerProvider.overrideWithValue(() async => root.path)],
    );
    final parent = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    await tapKey(tester, 'nav-editor');
    await settle(tester);
    final editor = find.byType(EditorScreen);
    Finder inside(Finder child) => find.descendant(of: editor, matching: child);
    Future<void> waitFor(bool Function() predicate) async {
      for (var n = 0; n < 120; n++) {
        await tester.pump(const Duration(milliseconds: 32));
        if (predicate()) return;
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      }
      fail('Editor did not reach the expected state');
    }

    expect(inside(find.text('Редактор')), findsOneWidget);
    await tester.tap(inside(find.text('Открыть папку')));
    await waitFor(() => inside(find.text('server.py')).evaluate().isNotEmpty);
    await tester.tap(inside(find.text('server.py')));
    await waitFor(() => inside(find.byType(CodeEditor)).evaluate().isNotEmpty);
    CodeEditor code() => tester.widget<CodeEditor>(inside(find.byType(CodeEditor)));
    expect(code().style!.codeTheme!.languages, contains('python'));
    code().controller!.text = 'def run(port):\n    return port + 1\n';
    await settle(tester);
    expect(inside(find.text('• server.py')), findsOneWidget);
    await tester.tap(inside(find.byTooltip('Сохранить')));
    await waitFor(() => inside(find.text('• server.py')).evaluate().isEmpty);
    expect(file.readAsStringSync(), contains('port + 1'));
    await tester.tap(inside(find.text('config.yaml')));
    await waitFor(() => inside(find.text('config.yaml')).evaluate().length >= 2);
    expect(code().style!.codeTheme!.languages, contains('yaml'));
    await tester.tap(inside(find.text('server.py')).last);
    await settle(tester);
    expect(code().controller!.text, contains('port + 1'));
    await tester.tap(inside(find.text('Терминал')).last);
    await settle(tester);
    await tapKey(tester, 'pick-local-windows:cmd');
    await waitFor(() => inside(find.byType(TerminalPane)).evaluate().isNotEmpty);
    final pane = tester.widget<TerminalPane>(inside(find.byType(TerminalPane)));
    expect((pane.tab.target as LocalTerminalTarget).directory, root.resolveSymbolicLinksSync());
    expect(parent.read(terminalTabsProvider).tabs, isEmpty);
    expect(
      tester.getTopLeft(inside(find.byType(TerminalPane))).dy,
      greaterThanOrEqualTo(tester.getBottomLeft(inside(find.byType(CodeEditor))).dy),
    );
    await waitFor(() => inside(find.text('main')).evaluate().isNotEmpty);
    for (var i = 0; i < 5; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 70)));
      await tester.pump(const Duration(milliseconds: 100));
    }
    await captureAutomationWorkspace(tester, 'editor-project-ru');
    await tester.tap(inside(find.text('main')));
    await waitFor(() => find.text('Локальные ветки').evaluate().isNotEmpty);
    expect(find.text('main'), findsNWidgets(2));
    await tester.tap(find.widgetWithText(TextButton, 'Закрыть'));
    await settle(tester);
    code().controller!.text = 'unsaved edit';
    await settle(tester);
    final tab = find.ancestor(of: inside(find.text('• server.py')), matching: find.byType(Row)).first;
    await tester.tap(find.descendant(of: tab, matching: find.byType(IconButton)).first);
    await settle(tester);
    expect(find.text('Несохранённые файлы'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(code().controller!.text, 'unsaved edit');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
