import 'dart:io';

import 'package:consolecrypt/automation/ansible_code_editor.dart';
import 'package:consolecrypt/core/glass/glass_tab_strip.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/native_drag_drop.dart';
import 'package:consolecrypt/editor/editor_language.dart';
import 'package:consolecrypt/editor/editor_screen.dart';
import 'package:consolecrypt/editor/editor_toolbar_button.dart';
import 'package:consolecrypt/editor/markdown_document.dart';
import 'package:consolecrypt/editor/project_move_dialog.dart';
import 'package:consolecrypt/editor/project_tree.dart';
import 'package:consolecrypt/editor/ssh_folder_picker.dart';
import 'package:consolecrypt/editor/workspace_code_editor.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path/path.dart' as p;

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';

void main() {
  testWidgets('unlock restores both SSH projects, the selected draft and caret only for the original profile', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-editor');
    final editor = find.byType(EditorScreen);
    Finder inside(Finder finder) => find.descendant(of: editor, matching: finder);
    final sessions = <SftpSessionId>[];
    for (final name in ['prod-web-1', 'prod-db-1']) {
      final open = sessions.isEmpty ? find.text('Open SSH folder') : inside(find.byTooltip('Open SSH folder'));
      await tester.tap(open);
      await settle(tester);
      await pickHost(tester, name);
      sessions.add(tester.widget<SshFolderPicker>(find.byType(SshFolderPicker)).session);
      await tester.tap(find.widgetWithText(FilledButton, 'Open'));
      await settle(tester);
      final notes = inside(find.text('notes.txt')).first;
      await tester.ensureVisible(notes);
      await tester.tap(notes);
      await settle(tester);
    }
    final field = inside(find.byType(WorkspaceCodeEditor));
    final before = tester.widget<WorkspaceCodeEditor>(field).controller;
    const expected = TextSelection(baseOffset: 2, extentOffset: 6);
    await tester.enterText(inside(find.byType(TextField)).hitTestable().last, 'unsaved editor draft');
    before.selection = expected;
    await settle(tester);
    for (var i = 0; i < 2; i++) {
      await tapKey(tester, 'lock-vault');
      expect(editor, findsNothing);
      for (final id in sessions) {
        expect(backend.sftp.isConnected(id), isFalse);
      }
      await enterKey(tester, 'unlock-passphrase', MockCloud.demoPassphrase);
      await tapKey(tester, 'unlock-submit');
      final tabs = tester.widget<GlassTabStrip>(inside(find.byType(GlassTabStrip)));
      expect(tabs.tabs.map((tab) => tab.subtitle), ['prod-web-1', 'prod-db-1']);
      expect(tabs.activeIndex, 1);
      final restored = tester.widget<WorkspaceCodeEditor>(field).controller;
      expect(restored.text, 'unsaved editor draft');
      expect(restored.selection, expected);
      expect(inside(find.text('• notes.txt')), findsOneWidget);
    }
    await tapKey(tester, 'lock-vault');
    await backend.debugCreateUnlockedLocalProfile(name: 'Other profile');
    await settle(tester);
    expect(editor, findsNothing);
    await tapKey(tester, 'nav-editor');
    expect(inside(find.text('Open SSH folder')), findsOneWidget);
    expect(field, findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('close tab shortcut cancels or saves dirty files and closes a clean Markdown preview immediately', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-editor');
    await tester.tap(find.text('Open SSH folder'));
    await settle(tester);
    await pickHost(tester, 'prod-web-1');
    final picker = tester.widget<SshFolderPicker>(find.byType(SshFolderPicker));
    await tester.tap(find.widgetWithText(FilledButton, 'Open'));
    await settle(tester);

    final tree = find.byType(ProjectTree);
    final divider = find.byKey(const ValueKey('editor-tree-divider'));
    final initialWidth = tester.getSize(tree).width;
    await tester.drag(divider, const Offset(80, 0));
    await settle(tester);
    expect(tester.getSize(tree).width, greaterThan(initialWidth));
    await tester.drag(divider, const Offset(-120, 0));
    await settle(tester);
    expect(tester.getSize(tree).width, lessThan(initialWidth));
    for (final name in ['README.md', 'notes.txt']) {
      final row = find.descendant(of: tree, matching: find.text(name));
      await tester.ensureVisible(row);
      await tester.tap(row);
      await settle(tester);
    }
    final path = '${picker.home}/notes.txt';
    final field = find.byKey(ValueKey(path));
    final original = await backend.sftpBrowser.readPreview(picker.session, path);
    await tester.enterText(field, 'save this SSH draft');
    final modifier = Theme.of(tester.element(tree)).platform == TargetPlatform.macOS
        ? LogicalKeyboardKey.metaLeft
        : LogicalKeyboardKey.controlLeft;
    Future<void> closeShortcut() async {
      await tester.sendKeyDownEvent(modifier);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyW);
      await tester.sendKeyUpEvent(modifier);
      await settle(tester);
    }

    await closeShortcut();
    expect(find.text('Save changes to this file?'), findsOneWidget);
    expect(find.descendant(of: findDialog(), matching: find.text(path)), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(tester.widget<TextField>(field).controller!.text, 'save this SSH draft');
    expect((await backend.sftpBrowser.readPreview(picker.session, path)).bytes, original.bytes);

    // Allow the shared native-menu / keyboard duplicate-event interval to elapse.
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 260)));
    await closeShortcut();
    await tapKey(tester, 'confirm-ok');
    expect(field, findsNothing);
    expect(
      String.fromCharCodes((await backend.sftpBrowser.readPreview(picker.session, path)).bytes),
      'save this SSH draft',
    );
    expect(find.byType(Markdown), findsOneWidget);

    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 260)));
    await closeShortcut();
    expect(findDialog(), findsNothing);
    expect(find.byType(WorkspaceCodeEditor), findsNothing);
    expect(tree, findsOneWidget);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.macOS}));

  testWidgets('opens a saved SSH host, saves a file and starts a terminal for its directory', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1');
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-editor');
    await tester.tap(find.text('Open SSH folder'));
    await settle(tester);
    expect(find.byKey(const ValueKey('host-picker')), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('host-picker-search')), host.name);
    await settle(tester);
    await tapKey(tester, 'pick-prod-web-1');
    final picker = tester.widget<SshFolderPicker>(find.byType(SshFolderPicker));
    await tester.tap(find.widgetWithText(FilledButton, 'Open'));
    await settle(tester);
    await tester.ensureVisible(find.text('notes.txt'));
    await tester.tap(find.text('notes.txt'));
    await settle(tester);
    final field = find.byKey(ValueKey('${picker.home}/notes.txt'));
    await tester.enterText(field, 'Changed on the server.');
    await settle(tester);
    final scope = find.byType(EditorScreen);
    await tester.tap(find.descendant(of: scope, matching: find.byTooltip('Save')));
    await settle(tester);
    final saved = await backend.sftpBrowser.readPreview(picker.session, '${picker.home}/notes.txt');
    expect(String.fromCharCodes(saved.bytes), 'Changed on the server.');
    await tester.tap(find.descendant(of: scope, matching: find.byTooltip('Show project terminal')));
    await settle(tester);
    final trust = find.byKey(const ValueKey('accept-host-key'));
    if (trust.evaluate().isNotEmpty) {
      await tester.tap(trust);
      await settle(tester);
    }
    final pane = tester.widget<TerminalPane>(find.descendant(of: scope, matching: find.byType(TerminalPane)));
    expect((pane.tab.target as SshTerminalTarget).host.id, host.id);
    expect(pane.tab.isConnected, isTrue);
    expect(pane.tab.terminal.buffer.getText(), contains("cd '${picker.home}'"));
    await tester.tap(find.descendant(of: scope, matching: find.byTooltip('Open SSH folder')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('host-picker-search')), host.name);
    await settle(tester);
    await tapKey(tester, 'pick-prod-web-1');
    final duplicate = tester.widget<SshFolderPicker>(find.byType(SshFolderPicker));
    await tester.tap(find.widgetWithText(FilledButton, 'Open'));
    await settle(tester);
    final strip = find.descendant(of: scope, matching: find.byType(GlassTabStrip));
    expect(tester.widget<GlassTabStrip>(strip).tabs, hasLength(1));
    expect(backend.sftp.isConnected(picker.session), isTrue);
    expect(backend.sftp.isConnected(duplicate.session), isFalse);
    await tester.tap(find.descendant(of: strip, matching: find.byTooltip(tester.element(strip).l10n.glassCloseTab)));
    await settle(tester);
    expect(backend.sftp.isConnected(picker.session), isFalse);
    expect(find.descendant(of: scope, matching: find.byType(TerminalPane)), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  testWidgets('tree context actions keep dirty buffers during rename and move, confirm before deleting', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-editor');
    await tester.tap(find.text('Open SSH folder'));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('host-picker-search')), 'prod-web-1');
    await settle(tester);
    await tapKey(tester, 'pick-prod-web-1');
    final picker = tester.widget<SshFolderPicker>(find.byType(SshFolderPicker));
    await tester.tap(find.widgetWithText(FilledButton, 'Open'));
    await settle(tester);
    final tree = find.byType(ProjectTree);
    Future<void> menu(String name, String action) async {
      final row = find.descendant(of: tree, matching: find.text(name));
      await tester.ensureVisible(row);
      if (Theme.of(tester.element(row)).platform == TargetPlatform.macOS) {
        await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
        await tester.tap(row, kind: PointerDeviceKind.mouse);
        await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      } else {
        await tester.tap(row, kind: PointerDeviceKind.mouse, buttons: kSecondaryMouseButton);
      }
      await settle(tester);
      await tester.tap(find.text(action).last);
      await settle(tester);
    }

    Future<void> nameDialog(String value, String submit) async {
      await tester.enterText(find.byKey(const ValueKey('text-input-dialog-field')), value);
      await tester.tap(find.text(submit).last);
      await settle(tester);
    }

    final rootName = p.posix.basename(picker.home);
    await menu(rootName, 'New folder');
    await nameDialog('drafts', 'Create');
    await menu('drafts', 'New file');
    await nameDialog('before.md.txt', 'Create');
    await tester.enterText(find.byKey(ValueKey('${picker.home}/drafts/before.md.txt')), 'unsaved buffer');
    await settle(tester);
    await menu('drafts', 'Rename');
    await nameDialog('renamed', 'Rename');
    final path = '${picker.home}/renamed/before.md.txt';
    expect(tester.widget<TextField>(find.byKey(ValueKey(path))).controller!.text, 'unsaved buffer');
    await tester.tap(find.descendant(of: tree, matching: find.text('renamed')));
    await settle(tester);
    await menu('before.md.txt', 'Move to folder…');
    final dialog = find.byType(ProjectMoveDialog);
    await tester.tap(find.descendant(of: dialog, matching: find.byIcon(Icons.arrow_upward)));
    await settle(tester);
    await tester.tap(find.widgetWithText(FilledButton, 'Move here'));
    await settle(tester);
    final target = '${picker.home}/before.md.txt';
    expect(tester.widget<TextField>(find.byKey(ValueKey(target))).controller!.text, 'unsaved buffer');
    await menu('before.md.txt', 'Delete');
    expect(find.textContaining('Unsaved files'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(find.byKey(ValueKey(target)), findsOneWidget);
    await menu('before.md.txt', 'Delete');
    await tapKey(tester, 'confirm-ok');
    expect(find.byKey(ValueKey(target)), findsNothing);
    expect(
      (await backend.sftpBrowser.listDirectory(picker.session, picker.home)).map((e) => e.name),
      isNot(contains('before.md.txt')),
    );
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.macOS}));

  testWidgets('Редактор opens a project, edits a tab and launches the selected shell below the file', (tester) async {
    final scratch = Directory.systemTemp.createTempSync('cc-editor-');
    final root = Directory(p.join(scratch.path, 'service-api'))..createSync();
    final docs = Directory(p.join(scratch.path, 'docs'))..createSync();
    final nested = Directory(p.join(root.path, 'nested'))..createSync();
    final external = File(p.join(scratch.path, 'extra.txt'))..writeAsStringSync('outside');
    final linked = File(p.join(docs.path, 'setup guide.txt'))..writeAsStringSync('linked content');
    var folderToOpen = root;
    final launched = <Uri>[];
    final readme = File(p.join(docs.path, 'README.MD'))
      ..writeAsStringSync(
        '# Manual\n\nFormat: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [SemVer](https://semver.org/).\n\n| Name | Value | Description |\n| --- | --- | --- |\n'
        '| Bind | 8080 | ${'very long description ' * 10} |\n\n[Setup](setup%20guide.txt#part)\n\n```sh\necho ok\n```\n\n'
        '`inline`\n\n~~~text\n  first & <one>\n\n    second\n~~~',
      );
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
      overrides: [
        editorFolderPickerProvider.overrideWithValue(() async => folderToOpen.path),
        markdownBrowserProvider.overrideWithValue((uri) async {
          launched.add(uri);
          return true;
        }),
      ],
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

    Future<void> saveActive() async {
      final button = find.ancestor(
        of: inside(find.byIcon(Icons.save_outlined)),
        matching: find.byType(EditorToolbarButton),
      );
      await waitFor(
        () => button.evaluate().length == 1 && tester.widget<EditorToolbarButton>(button).onPressed != null,
      );
      await tester.tap(button);
    }

    expect(inside(find.text('Редактор')), findsOneWidget);
    await tester.tap(inside(find.text('Открыть папку')));
    await waitFor(() => inside(find.text('server.py')).evaluate().isNotEmpty);
    await tester.tap(inside(find.text('server.py')));
    await waitFor(() => inside(find.byType(WorkspaceCodeEditor)).hitTestable().evaluate().isNotEmpty);
    WorkspaceCodeEditor code() =>
        tester.widget<WorkspaceCodeEditor>(inside(find.byType(WorkspaceCodeEditor)).hitTestable());
    expect(editorLanguage(code().path!), 'python');
    final surface = find
        .descendant(of: inside(find.byType(AnsibleCodeEditor)), matching: find.byType(TextField))
        .hitTestable();
    await tester.tap(surface);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.equal);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(settings.currentLocal.editorFontSize, 15);
    expect(tester.widget<TextField>(surface).style!.fontSize, 15);
    await tester.enterText(surface, 'def run(port):\n    return port + 1\n');
    await settle(tester);
    expect(inside(find.text('• server.py')), findsOneWidget);
    await saveActive();
    await waitFor(() => inside(find.text('• server.py')).evaluate().isEmpty);
    expect(file.readAsStringSync(), contains('port + 1'));
    await tester.tap(inside(find.text('config.yaml')));
    await waitFor(() => inside(find.text('config.yaml')).evaluate().length >= 2);
    expect(editorLanguage(code().path!), 'yaml');
    await tester.tap(inside(find.text('server.py')).last);
    await settle(tester);
    expect(code().controller.text, contains('port + 1'));
    final treeDrop = inside(find.byKey(const ValueKey('editor-tree-drop')));
    final nestedRow = inside(find.text('nested'));
    final dropPosition = tester.getCenter(nestedRow);
    tester.widget<OsFileDropTarget>(treeDrop).onDrop([external.path], dropPosition - tester.getTopLeft(treeDrop));
    final imported = File(p.join(nested.path, 'extra.txt'));
    await waitFor(() => imported.existsSync() && imported.readAsStringSync() == 'outside');
    await waitFor(() => tester.widget<OsFileDropTarget>(treeDrop).enabled);
    final openDrop = inside(find.byKey(const ValueKey('editor-open-drop')));
    tester.widget<OsFileDropTarget>(openDrop).onDrop([external.path], Offset.zero);
    await waitFor(
      () => inside(find.byKey(ValueKey(external.resolveSymbolicLinksSync()))).hitTestable().evaluate().isNotEmpty,
    );
    await tester.enterText(inside(find.byKey(ValueKey(external.resolveSymbolicLinksSync()))), 'changed outside');
    await settle(tester);
    await saveActive();
    await waitFor(() => inside(find.text('• extra.txt')).evaluate().isEmpty);
    expect(external.readAsStringSync(), 'changed outside');
    expect(imported.readAsStringSync(), 'outside');
    await waitFor(() => tester.widget<OsFileDropTarget>(treeDrop).enabled);
    await tester.tap(inside(find.text('server.py')).last);
    await settle(tester);
    await tester.enterText(surface, 'dirty text while moving');
    await settle(tester);
    await waitFor(() => tester.widget<OsFileDropTarget>(treeDrop).enabled);
    final start = tester.getCenter(find.descendant(of: treeDrop, matching: find.text('server.py')));
    final target = tester.getCenter(nestedRow);
    final drag = await tester.startGesture(start, kind: PointerDeviceKind.mouse);
    for (var step = 1; step <= 8; step++) {
      await drag.moveTo(Offset.lerp(start, target, step / 8)!);
      await tester.pump();
    }
    await drag.up();
    final moved = File(p.join(nested.path, 'server.py'));
    await waitFor(() => moved.existsSync() && !file.existsSync());
    await waitFor(() => tester.widget<OsFileDropTarget>(treeDrop).enabled);
    expect(code().controller.text, 'dirty text while moving');
    await saveActive();
    await waitFor(() => inside(find.text('• server.py')).evaluate().isEmpty);
    expect(moved.readAsStringSync(), 'dirty text while moving');
    await tester.tap(inside(find.byKey(const ValueKey('editor-terminal-toggle'))));
    await settle(tester);
    await tapKey(tester, 'pick-local-windows:cmd');
    await waitFor(() => inside(find.byType(TerminalPane)).evaluate().isNotEmpty);
    final pane = tester.widget<TerminalPane>(inside(find.byType(TerminalPane)));
    expect((pane.tab.target as LocalTerminalTarget).directory, root.resolveSymbolicLinksSync());
    expect(parent.read(terminalTabsProvider).tabs, isEmpty);
    expect(
      tester.getTopLeft(inside(find.byType(TerminalPane))).dy,
      greaterThanOrEqualTo(tester.getBottomLeft(inside(find.byType(WorkspaceCodeEditor)).hitTestable()).dy),
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
    await tester.enterText(surface, 'unsaved edit');
    await settle(tester);
    final tab = find.ancestor(of: inside(find.text('• server.py')), matching: find.byType(Row)).first;
    await tester.tap(find.descendant(of: tab, matching: find.byType(IconButton)).first);
    await settle(tester);
    expect(find.text('Сохранить изменения в файле?'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(code().controller.text, 'unsaved edit');

    folderToOpen = docs;
    await tester.tap(inside(find.byTooltip('Открыть папку')));
    await waitFor(() => inside(find.text('README.MD')).evaluate().isNotEmpty);
    final strip = inside(find.byType(GlassTabStrip));
    GlassTabStrip tabs() => tester.widget<GlassTabStrip>(strip);
    expect(tabs().tabs.map((tab) => tab.title), ['service-api', 'docs']);
    expect(inside(find.byType(TerminalPane)).hitTestable(), findsNothing);

    await tester.tap(inside(find.text('README.MD')));
    await waitFor(() => inside(find.byType(Markdown)).evaluate().isNotEmpty);
    final preview = inside(find.byType(Markdown));
    await tester.tapOnText(find.textRange.ofSubstring('Keep a Changelog'));
    await tester.tapOnText(find.textRange.ofSubstring('SemVer'));
    expect(launched, [Uri.parse('https://keepachangelog.com/en/1.1.0/'), Uri.parse('https://semver.org/')]);
    expect(preview, findsOneWidget);
    expect(find.descendant(of: preview, matching: find.byType(Table)), findsOneWidget);
    expect(find.descendant(of: preview, matching: find.text('echo ok', findRichText: true)), findsWidgets);
    String? copied;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') copied = (call.arguments as Map)['text'] as String;
      return null;
    });
    final copies = find.descendant(of: preview, matching: find.byTooltip('Копировать'));
    expect(copies, findsNWidgets(2));
    await tester.tap(copies.first);
    await settle(tester);
    expect(copied, 'echo ok');
    await tester.ensureVisible(copies.last);
    await tester.tap(copies.last);
    await settle(tester);
    expect(copied, '  first & <one>\n\n    second');
    await tester.ensureVisible(find.descendant(of: preview, matching: find.text('Setup', findRichText: true)));
    await tester.tap(find.descendant(of: preview, matching: find.text('Setup', findRichText: true)));
    await waitFor(
      () => inside(find.byKey(ValueKey(linked.resolveSymbolicLinksSync()))).hitTestable().evaluate().isNotEmpty,
    );
    expect(code().controller.text, 'linked content');
    await tester.tap(inside(find.text('README.MD')).last);
    await settle(tester);
    final desktop = tester.view.physicalSize;
    tester.view.physicalSize = const Size(410, 760);
    await settle(tester);
    final table = find.descendant(of: preview, matching: find.byType(Table));
    final scroll = find.ancestor(of: table, matching: find.byType(Scrollable)).first;
    expect(tester.state<ScrollableState>(scroll).position.maxScrollExtent, greaterThan(0));
    expect(tester.takeException(), isNull);
    tester.view.physicalSize = desktop;
    await settle(tester);
    await tester.tap(inside(find.text(tester.element(strip).l10n.notesEdit)));
    await settle(tester);
    final readmeField = inside(find.byKey(ValueKey(readme.resolveSymbolicLinksSync())));
    await tester.enterText(readmeField, '# Новый заголовок');
    await tester.tap(inside(find.text('Просмотр')));
    await settle(tester);
    expect(find.descendant(of: preview, matching: find.text('Новый заголовок', findRichText: true)), findsWidgets);
    expect(readme.readAsStringSync(), startsWith('# Manual'));

    Future<void> selectFolder(Directory directory) async {
      final tab = find.descendant(of: strip, matching: find.byTooltip(directory.resolveSymbolicLinksSync()));
      await tester.tap(tab);
      await settle(tester);
    }

    await selectFolder(root);
    expect(code().controller.text, 'unsaved edit');
    expect(tester.widget<TerminalPane>(inside(find.byType(TerminalPane))).tab, same(pane.tab));
    await tester.tap(inside(find.byTooltip('Открыть папку')));
    await waitFor(() => tabs().activeIndex == 1);
    expect(tabs().tabs.length, 2);
    final close = find.descendant(of: strip, matching: find.byTooltip(tester.element(strip).l10n.glassCloseTab));
    await tester.tap(close.last);
    await settle(tester);
    expect(find.text('Несохранённые файлы'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(tabs().tabs.length, 2);
    await tester.tap(close.last);
    await settle(tester);
    await tapKey(tester, 'confirm-ok');
    expect(tabs().tabs.length, 1);
    expect(code().controller.text, 'unsaved edit');
    expect(tester.widget<TerminalPane>(inside(find.byType(TerminalPane))).tab, same(pane.tab));
    await tester.tap(inside(find.byKey(const ValueKey('editor-terminal-move'))));
    await settle(tester);
    expect(parent.read(terminalTabsProvider).active, same(pane.tab));
    expect(find.byType(TerminalPane).hitTestable(), findsOneWidget);
    await tapKey(tester, 'nav-editor');
    expect(inside(find.byType(TerminalPane)).hitTestable(), findsNothing);
    await tapKey(tester, 'nav-hosts');
    await tapKey(tester, 'lock-vault');
    moved.writeAsStringSync('changed externally while locked');
    await enterKey(tester, 'unlock-passphrase', MockCloud.demoPassphrase);
    await tapKey(tester, 'unlock-submit');
    expect(editor, findsNothing);
    await tapKey(tester, 'nav-editor');
    await waitFor(() => inside(find.byType(WorkspaceCodeEditor)).hitTestable().evaluate().isNotEmpty);
    expect(tabs().tabs.map((tab) => tab.title), ['service-api']);
    expect(code().controller.text, 'unsaved edit');
    expect(inside(find.byType(TerminalPane)), findsNothing);
    await tester.tap(surface);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyW);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    await tapKey(tester, 'confirm-ok');
    await waitFor(() => find.text(tester.element(editor).l10n.editorChanged).evaluate().isNotEmpty);
    expect(code().controller.text, 'unsaved edit');
    expect(moved.readAsStringSync(), 'changed externally while locked');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
