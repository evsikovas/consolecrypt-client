import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/sftp_browser_service.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const _home = '/home/deploy';

/// Signed-in demo backend with the SFTP browser connected to prod-web-1
/// (user `deploy`, home /home/deploy).
Future<MockBackend> _openBrowser(
  WidgetTester tester, {
  AppLocale locale = AppLocale.en,
  Size size = const Size(1600, 1000),
  bool editHintSeen = true,
  MockBackend? backend,
}) async {
  final b = backend ?? testBackend();
  addTearDown(b.dispose);
  await b.debugSignInDemoAndUnlock();
  if (editHintSeen) {
    b.sftpBrowser.savePreferences(const SftpBrowserPreferences(editHintAcknowledged: true)).ignore();
  }
  await pumpApp(tester, b, locale: locale, size: size);
  await tapKey(tester, 'nav-sftp');
  await tapKey(tester, 'sftp-connect');
  await pickHost(tester, 'prod-web-1');
  await settle(tester);
  return b;
}

Finder _row(String path) => find.byKey(ValueKey('sftp-row-$path'));

String _summary(WidgetTester tester) => tester.widget<Text>(find.byKey(const ValueKey('sftp-status-summary'))).data!;

double _y(WidgetTester tester, String path) => tester.getTopLeft(_row(path)).dy;

Future<void> _click(WidgetTester tester, String path, {LogicalKeyboardKey? holding}) async {
  await tester.ensureVisible(_row(path));
  await tester.pump();
  if (holding != null) await tester.sendKeyDownEvent(holding);
  await tester.tap(_row(path));
  if (holding != null) await tester.sendKeyUpEvent(holding);
  await tester.pump(const Duration(milliseconds: 400)); // beyond the double-tap window
  await settle(tester, steps: 2);
}

Future<void> _doubleClick(WidgetTester tester, String path) async {
  await tester.ensureVisible(_row(path));
  await tester.pump();
  await tester.tap(_row(path));
  await tester.pump(const Duration(milliseconds: 60));
  await tester.tap(_row(path));
  await settle(tester);
}

LogicalKeyboardKey get _primaryKey =>
    AppPlatform.usesMeta ? LogicalKeyboardKey.metaLeft : LogicalKeyboardKey.controlLeft;

Future<void> _key(WidgetTester tester, LogicalKeyboardKey key, {bool primary = false, bool shift = false}) async {
  if (primary) await tester.sendKeyDownEvent(_primaryKey);
  if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyEvent(key);
  if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  if (primary) await tester.sendKeyUpEvent(_primaryKey);
  await settle(tester);
}

Future<void> _debugEdit(WidgetTester tester, String fileName, String action) async {
  await tapKey(tester, 'sftp-edit-debug-$fileName');
  await tester.tap(find.byKey(ValueKey(action)).last);
  await settle(tester);
}

String _editStatus(WidgetTester tester, String fileName) {
  final chip = find.byKey(ValueKey('sftp-edit-status-$fileName'));
  return tester.widgetList<Text>(find.descendant(of: chip, matching: find.byType(Text))).single.data!;
}

void main() {
  testWidgets('Linux Open With selects an executable, reuses it and preserves the saved editor', (tester) async {
    final backend = await _openBrowser(tester);
    const savedEditor = AppRef(AppRefKind.path, '/usr/bin/code');
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(sftpDefaultEditor: savedEditor));
    await _doubleClick(tester, '$_home/README.md');
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    expect(container.read(editSessionsProvider).requireValue.single.app, savedEditor);
    expect(backend.files.applicationChoices, 0);

    backend.files.applicationPath = '/usr/bin/zed';
    await _click(tester, '$_home/notes.txt');
    await tapKey(tester, 'sftp-toolbar-actions');
    await tapKey(tester, 'sftp-action-openWith');
    expect(backend.files.applicationChoices, 1);
    final session = container.read(editSessionsProvider).requireValue.singleWhere((s) => s.fileName == 'notes.txt');
    expect(session.app, const AppRef(AppRefKind.path, '/usr/bin/zed'));
    expect(settings.currentLocal.sftpDefaultEditor, savedEditor);
    await tapKey(tester, 'sftp-edit-reopen-notes.txt');
    expect(backend.files.applicationChoices, 1, reason: 'reopening reuses the selected executable');
    expect(
      container.read(editSessionsProvider).requireValue.singleWhere((s) => s.fileName == 'notes.txt').app,
      session.app,
    );
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
    expect(container.read(editSessionsProvider).requireValue.singleWhere((s) => s.fileName == 'notes.txt').uploads, 1);
    expect(_editStatus(tester, 'notes.txt'), 'Synced');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.linux));

  testWidgets('normal Open uses the saved editor; Open With overrides one file without changing it', (tester) async {
    final b = await _openBrowser(tester);
    final settings = b.services.settings;
    const code = AppRef(AppRefKind.path, '/Applications/Visual Studio Code.app');
    await settings.updateLocal(settings.currentLocal.copyWith(sftpDefaultEditor: code));
    await _doubleClick(tester, '$_home/notes.txt');
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    expect(container.read(editSessionsProvider).requireValue.single.app, code);
    expect(b.files.applicationChoices, 0);
    b.files.applicationPath = '/Applications/Zed.app';
    await _click(tester, '$_home/README.md');
    await tapKey(tester, 'sftp-toolbar-actions');
    await tapKey(tester, 'sftp-action-openWith');
    expect(b.files.applicationChoices, 1);
    expect(
      container.read(editSessionsProvider).requireValue.map((s) => s.app),
      contains(const AppRef(AppRefKind.path, '/Applications/Zed.app')),
    );
    expect(settings.currentLocal.sftpDefaultEditor, code);
    await _doubleClick(tester, '$_home/deploy.sh');
    expect(container.read(editSessionsProvider).requireValue.where((s) => s.app == code).length, 2);
    expect(b.files.applicationChoices, 1);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  for (final name in ['Visual Studio Code', 'Zed']) {
    testWidgets('Open With selects $name by bundle path, reuses it and uploads saves', (tester) async {
      final backend = await _openBrowser(tester);
      backend.files.applicationPath = '/Applications/$name.app';
      await _click(tester, '$_home/notes.txt');
      await tapKey(tester, 'sftp-toolbar-actions');
      await tapKey(tester, 'sftp-action-openWith');
      expect(backend.files.applicationChoices, 1);
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      final session = container.read(editSessionsProvider).requireValue.single;
      expect(session.app, AppRef(AppRefKind.path, '/Applications/$name.app'));
      expect(_editStatus(tester, 'notes.txt'), 'Synced');
      await tapKey(tester, 'sftp-edit-reopen-notes.txt');
      expect(backend.files.applicationChoices, 1, reason: 'reopening reuses the chosen app');
      expect(container.read(editSessionsProvider).requireValue.single.app, session.app);
      await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
      expect(container.read(editSessionsProvider).requireValue.single.uploads, 1);
      expect(_editStatus(tester, 'notes.txt'), 'Synced');
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.macOS}));
  }

  testWidgets('cancelling the application picker creates no edit session or error', (tester) async {
    final backend = await _openBrowser(tester);
    backend.files.applicationPath = null;
    await _click(tester, '$_home/notes.txt');
    await tapKey(tester, 'sftp-toolbar-actions');
    await tapKey(tester, 'sftp-action-openWith');
    expect(backend.files.applicationChoices, 1);
    expect(find.byKey(const ValueKey('sftp-edit-session-notes.txt')), findsNothing);
    expect(find.text('Internal error. Please try again.'), findsNothing);
    expect(find.text('Cancelled.'), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.macOS, TargetPlatform.linux}));

  testWidgets('Windows Open With still uses the core system chooser', (tester) async {
    final backend = await _openBrowser(tester);
    await _click(tester, '$_home/notes.txt');
    await tapKey(tester, 'sftp-toolbar-actions');
    await tapKey(tester, 'sftp-action-openWith');
    expect(backend.files.applicationChoices, 0);
    expect(find.byKey(const ValueKey('sftp-edit-session-notes.txt')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('connects and shows toolbar, breadcrumbs, dense list with symlink badges and status bar', (tester) async {
    await _openBrowser(tester);
    expect(tester.takeException(), isNull);
    expect(find.byKey(const ValueKey('sftp-toolbar-title')), findsOneWidget);
    expect(find.byKey(const ValueKey('sftp-crumb-host')), findsOneWidget);
    expect(find.byKey(const ValueKey('sftp-crumb-/')), findsOneWidget);
    expect(find.byKey(const ValueKey('sftp-crumb-/home')), findsOneWidget);
    expect(find.byKey(const ValueKey('sftp-crumb-$_home')), findsOneWidget);
    expect(_row('$_home/app'), findsOneWidget);
    expect(_row('$_home/notes.txt'), findsOneWidget);
    expect(_row('$_home/.bashrc'), findsNothing, reason: 'dot-files hidden by default');
    expect(find.byKey(const ValueKey('sftp-symlink-current')), findsOneWidget);
    expect(find.byKey(const ValueKey('sftp-symlink-old-backup')), findsOneWidget);
    expect(find.text('Link → Folder'), findsOneWidget, reason: 'kind of a symlink to a folder');
    expect(find.text('drwxr-xr-x'), findsWidgets);
    expect(find.byKey(const ValueKey('sftp-status-protocol')), findsOneWidget);
    // Folders first, sorted by name.
    expect(_y(tester, '$_home/app'), lessThan(_y(tester, '$_home/logs')));
    expect(_y(tester, '$_home/logs'), lessThan(_y(tester, '$_home/deploy.sh')));
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('status bar counts the view and summarises the selection', (tester) async {
    await _openBrowser(tester);
    // app, backups, current→, logs | deploy.sh, latest.log→, notes.txt, old-backup (dangling), README.md
    expect(_summary(tester), startsWith('4 folders, 5 files, '));
    await _click(tester, '$_home/notes.txt');
    expect(_summary(tester), startsWith('1 of 9 selected, '));
    await _key(tester, LogicalKeyboardKey.escape);
    expect(_summary(tester), startsWith('4 folders'));
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('navigates by double-click, breadcrumbs, parent button and the host crumb', (tester) async {
    await _openBrowser(tester);
    await _doubleClick(tester, '$_home/app');
    expect(find.byKey(const ValueKey('sftp-crumb-$_home/app')), findsOneWidget);
    expect(_row('$_home/app/config.yml'), findsOneWidget);
    await tapKey(tester, 'sftp-crumb-/home');
    expect(_row(_home), findsOneWidget);
    await tapKey(tester, 'sftp-crumb-/');
    expect(_row('/var'), findsOneWidget);
    await tapKey(tester, 'sftp-crumb-host');
    expect(_row('$_home/app'), findsOneWidget);
    await tapKey(tester, 'sftp-up');
    expect(_row(_home), findsOneWidget);
    expect(_summary(tester), startsWith('1 of 1 selected'), reason: 'the folder we came from is selected');
    // Symlinked folder opens under its own path.
    await tapKey(tester, 'sftp-crumb-host');
    await _doubleClick(tester, '$_home/current');
    expect(_row('$_home/current/server.js'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('sorts by any column, toggles direction, keeps folders first', (tester) async {
    await _openBrowser(tester);
    await tapKey(tester, 'sftp-col-size');
    expect(find.byKey(const ValueKey('sftp-sort-asc')), findsOneWidget);
    expect(_y(tester, '$_home/notes.txt'), lessThan(_y(tester, '$_home/deploy.sh')));
    expect(_y(tester, '$_home/logs'), lessThan(_y(tester, '$_home/notes.txt')));
    await tapKey(tester, 'sftp-col-size');
    expect(find.byKey(const ValueKey('sftp-sort-desc')), findsOneWidget);
    expect(_y(tester, '$_home/deploy.sh'), lessThan(_y(tester, '$_home/notes.txt')));
    expect(_y(tester, '$_home/app'), lessThan(_y(tester, '$_home/deploy.sh')), reason: 'folders first');
    await tapKey(tester, 'sftp-col-name');
    await tapKey(tester, 'sftp-col-name');
    expect(_y(tester, '$_home/logs'), lessThan(_y(tester, '$_home/app')), reason: 'name descending');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('expands a folder inline (lazy) and collapses it', (tester) async {
    await _openBrowser(tester);
    expect(_row('$_home/app/config.yml'), findsNothing);
    await tapKey(tester, 'sftp-disclosure-$_home/app');
    expect(_row('$_home/app/config.yml'), findsOneWidget);
    expect(_row('$_home/app/releases'), findsOneWidget);
    await tapKey(tester, 'sftp-disclosure-$_home/app/releases');
    expect(_row('$_home/app/releases/v1.8.2.tar.gz'), findsOneWidget);
    expect(find.text('TAR.GZ archive'), findsOneWidget);
    expect(tester.getTopLeft(find.text('config.yml')).dx, greaterThan(tester.getTopLeft(find.text('app')).dx));
    await tapKey(tester, 'sftp-disclosure-$_home/app');
    expect(_row('$_home/app/config.yml'), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('search filters the current folder instantly', (tester) async {
    await _openBrowser(tester);
    await enterKey(tester, 'sftp-search', 'log');
    await settle(tester, steps: 2);
    expect(_row('$_home/logs'), findsOneWidget);
    expect(_row('$_home/latest.log'), findsOneWidget);
    expect(_row('$_home/notes.txt'), findsNothing);
    expect(_summary(tester), startsWith('1 folder, 1 file'));
    await enterKey(tester, 'sftp-search', 'zzz');
    await settle(tester, steps: 2);
    expect(find.byKey(const ValueKey('sftp-list-empty')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('multi-select with Cmd/Ctrl and Shift, delete asks for confirmation', (tester) async {
    await _openBrowser(tester);
    await _click(tester, '$_home/notes.txt');
    await _click(tester, '$_home/README.md', holding: _primaryKey);
    expect(_summary(tester), startsWith('2 of 9 selected'));
    await _click(tester, '$_home/deploy.sh', holding: LogicalKeyboardKey.shiftLeft);
    expect(_summary(tester), startsWith('5 of 9 selected'), reason: 'deploy.sh … README.md range');
    await _click(tester, '$_home/notes.txt');
    await _click(tester, '$_home/README.md', holding: _primaryKey);
    await _key(tester, LogicalKeyboardKey.delete);
    expect(find.text('Delete 2 items?'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('confirm-ok')));
    await settle(tester);
    expect(_row('$_home/notes.txt'), findsNothing);
    expect(_row('$_home/README.md'), findsNothing);
    expect(_row('$_home/deploy.sh'), findsOneWidget);
    // Cancel keeps the file.
    await _click(tester, '$_home/deploy.sh');
    await _key(tester, LogicalKeyboardKey.backspace);
    expect(find.text('Delete deploy.sh?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await settle(tester);
    expect(_row('$_home/deploy.sh'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('renames inline (F2 / Action menu), validates names', (tester) async {
    await _openBrowser(tester);
    await _click(tester, '$_home/notes.txt');
    await _key(tester, LogicalKeyboardKey.f2);
    final field = find.byKey(const ValueKey('sftp-rename-field'));
    expect(field, findsOneWidget);
    expect(tester.widget<TextField>(field).controller!.selection.textInside('notes.txt'), 'notes');
    await tester.enterText(field, 'todo.txt');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(_row('$_home/todo.txt'), findsOneWidget);
    expect(_row('$_home/notes.txt'), findsNothing);
    // Via the Action menu; a name with "/" is refused.
    await tapKey(tester, 'sftp-toolbar-actions');
    await tester.tap(find.byKey(const ValueKey('sftp-action-rename')));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('sftp-rename-field')), 'a/b');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(find.textContaining('can’t be empty or contain'), findsOneWidget);
    expect(_row('$_home/todo.txt'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('new folder is created and renamed inline; duplicate adds a copy', (tester) async {
    await _openBrowser(tester);
    await _click(tester, '$_home/notes.txt');
    await _key(tester, LogicalKeyboardKey.keyN, primary: true, shift: true);
    expect(_row('$_home/untitled folder'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('sftp-rename-field')), 'static');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(_row('$_home/static'), findsOneWidget);
    await _click(tester, '$_home/notes.txt');
    await _key(tester, LogicalKeyboardKey.keyD, primary: true);
    expect(_row('$_home/notes copy.txt'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Get Info edits permissions with checkboxes and octal (chmod)', (tester) async {
    await _openBrowser(tester);
    await _click(tester, '$_home/deploy.sh');
    await _key(tester, LogicalKeyboardKey.keyI, primary: true);
    expect(find.byKey(const ValueKey('sftp-info-dialog')), findsOneWidget);
    String octal() => tester.widget<TextField>(find.byKey(const ValueKey('sftp-info-octal'))).controller!.text;
    expect(octal(), '0755');
    expect(find.text('deploy (1000)'), findsNWidgets(2), reason: 'owner and group');
    await tester.tap(find.byKey(const ValueKey('sftp-info-perm-group-x')));
    await tester.tap(find.byKey(const ValueKey('sftp-info-perm-others-x')));
    await tester.pump();
    expect(octal(), '0744');
    expect(find.text('-rwxr--r--'), findsWidgets);
    await tester.enterText(find.byKey(const ValueKey('sftp-info-octal')), '750');
    await tester.pump();
    expect(tester.widget<Checkbox>(find.byKey(const ValueKey('sftp-info-perm-group-x'))).value, isTrue);
    expect(tester.widget<Checkbox>(find.byKey(const ValueKey('sftp-info-perm-others-r'))).value, isFalse);
    await tapKey(tester, 'sftp-info-apply');
    expect(find.byKey(const ValueKey('sftp-info-dialog')), findsNothing);
    expect(find.text('-rwxr-x---'), findsOneWidget, reason: 'list shows the new mode');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Quick Look previews text in memory (Space) and images; folders have no preview', (tester) async {
    await _openBrowser(tester);
    await _click(tester, '$_home/notes.txt');
    await _key(tester, LogicalKeyboardKey.space);
    expect(find.byKey(const ValueKey('sftp-quicklook')), findsOneWidget);
    final text = tester.widget<SelectableText>(find.byKey(const ValueKey('sftp-quicklook-text'))).data!;
    expect(text, contains('rotate nginx logs'));
    await _key(tester, LogicalKeyboardKey.space);
    expect(find.byKey(const ValueKey('sftp-quicklook')), findsNothing);
    // Folder.
    await _click(tester, '$_home/app');
    await tapKey(tester, 'sftp-toolbar-quicklook');
    expect(find.text('Folders have no preview.'), findsOneWidget);
    await _key(tester, LogicalKeyboardKey.escape);
    // Image.
    await _doubleClick(tester, '$_home/app'); // leave home…
    await tapKey(tester, 'sftp-crumb-/');
    await _doubleClick(tester, '/var');
    await _doubleClick(tester, '/var/www');
    await _doubleClick(tester, '/var/www/html');
    await _doubleClick(tester, '/var/www/html/assets');
    await _doubleClick(tester, '/var/www/html/assets/img');
    await _click(tester, '/var/www/html/assets/img/logo.png');
    await _key(tester, LogicalKeyboardKey.space);
    expect(find.byKey(const ValueKey('sftp-quicklook-image')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('keyboard: arrows, Shift, select all, type-to-select, Enter, Cmd/Ctrl+Up, Cmd/Ctrl+L', (tester) async {
    await _openBrowser(tester);
    await _key(tester, LogicalKeyboardKey.arrowDown);
    expect(_summary(tester), startsWith('1 of 9 selected'));
    await _key(tester, LogicalKeyboardKey.arrowDown, shift: true);
    await _key(tester, LogicalKeyboardKey.arrowDown, shift: true);
    expect(_summary(tester), startsWith('3 of 9 selected'));
    await _key(tester, LogicalKeyboardKey.keyA, primary: true);
    expect(_summary(tester), startsWith('9 of 9 selected'));
    await _key(tester, LogicalKeyboardKey.escape);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyN);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyO);
    await settle(tester, steps: 2);
    expect(_summary(tester), startsWith('1 of 9 selected'));
    await _key(tester, LogicalKeyboardKey.home);
    await _key(tester, LogicalKeyboardKey.arrowRight);
    expect(_row('$_home/app/config.yml'), findsOneWidget, reason: '→ expands');
    await _key(tester, LogicalKeyboardKey.arrowLeft);
    expect(_row('$_home/app/config.yml'), findsNothing, reason: '← collapses');
    await _key(tester, LogicalKeyboardKey.enter);
    expect(find.byKey(const ValueKey('sftp-crumb-$_home/app')), findsOneWidget, reason: 'Enter opens the folder');
    await _key(tester, LogicalKeyboardKey.arrowUp, primary: true);
    expect(_row('$_home/app'), findsOneWidget, reason: 'Cmd/Ctrl+↑ = parent');
    await _key(tester, LogicalKeyboardKey.keyL, primary: true);
    final field = find.byKey(const ValueKey('sftp-path-field'));
    expect(field, findsOneWidget);
    await tester.enterText(field, '/var/www');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(_row('/var/www/html'), findsOneWidget);
    // Unknown folders keep the field open with an error.
    await _key(tester, LogicalKeyboardKey.keyL, primary: true);
    await tester.enterText(find.byKey(const ValueKey('sftp-path-field')), '/nope');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    expect(find.textContaining('No such directory'), findsOneWidget);
    await _key(tester, LogicalKeyboardKey.escape);
    expect(find.byKey(const ValueKey('sftp-path-field')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('hidden files: header menu toggle shows dot-files', (tester) async {
    await _openBrowser(tester);
    await tester.tap(find.byKey(const ValueKey('sftp-col-kind')), buttons: kSecondaryButton);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('sftp-show-hidden')));
    await settle(tester);
    expect(_row('$_home/.bashrc'), findsOneWidget);
    expect(_row('$_home/.ssh'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('sftp-col-kind')), buttons: kSecondaryButton);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('sftp-column-toggle-owner')));
    await settle(tester);
    expect(find.byKey(const ValueKey('sftp-col-owner')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('open in editor → Editing panel → simulated save uploads → Synced; Stop editing', (tester) async {
    await _openBrowser(tester);
    await _doubleClick(tester, '$_home/notes.txt');
    expect(find.byKey(const ValueKey('sftp-edit-session-notes.txt')), findsOneWidget);
    expect(_editStatus(tester, 'notes.txt'), 'Synced');
    expect(find.textContaining('uploaded just now'), findsNothing);
    expect(find.byKey(const ValueKey('sftp-status-edits')), findsOneWidget);
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
    expect(_editStatus(tester, 'notes.txt'), 'Synced');
    expect(find.textContaining('uploaded just now'), findsOneWidget);
    // A failed upload offers Retry.
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-fail-upload');
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
    expect(_editStatus(tester, 'notes.txt'), 'Upload failed');
    await tapKey(tester, 'sftp-edit-retry-notes.txt');
    expect(_editStatus(tester, 'notes.txt'), 'Synced');
    await tapKey(tester, 'sftp-edit-stop-notes.txt');
    expect(find.byKey(const ValueKey('sftp-edit-session-notes.txt')), findsNothing);
    expect(find.text('Stopped editing “notes.txt”.'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('first use shows the external-editor hint once', (tester) async {
    final backend = await _openBrowser(tester, editHintSeen: false);
    await _doubleClick(tester, '$_home/notes.txt');
    expect(find.byKey(const ValueKey('sftp-edit-hint')), findsOneWidget);
    expect(find.textContaining('can’t control what that app does'), findsOneWidget);
    await tapKey(tester, 'sftp-edit-hint-open');
    expect(find.byKey(const ValueKey('sftp-edit-session-notes.txt')), findsOneWidget);
    expect(tester.takeException(), isNull);
    // Remembered.
    await _doubleClick(tester, '$_home/README.md');
    expect(find.byKey(const ValueKey('sftp-edit-hint')), findsNothing);
    expect(find.byKey(const ValueKey('sftp-edit-session-README.md')), findsOneWidget);
    backend.sftpBrowser.loadPreferences().then((p) => expect(p.editHintAcknowledged, isTrue)).ignore();
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('conflict: nothing is uploaded; resolutions keep both / overwrite', (tester) async {
    await _openBrowser(tester);
    await _doubleClick(tester, '$_home/notes.txt');
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-remote-change');
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
    expect(_editStatus(tester, 'notes.txt'), 'Conflict');
    await tapKey(tester, 'sftp-edit-resolve-notes.txt');
    expect(find.byKey(const ValueKey('sftp-conflict-dialog')), findsOneWidget);
    expect(find.text('“notes.txt” changed on the server'), findsOneWidget);
    expect(find.text('Overwrite the server file'), findsOneWidget);
    expect(find.text('Keep both'), findsOneWidget);
    expect(find.text('Discard my changes'), findsOneWidget);
    await tapKey(tester, 'sftp-conflict-keep_remote_copy_locally');
    expect(_editStatus(tester, 'notes.txt'), 'Modified');
    // Next save uploads normally.
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
    expect(_editStatus(tester, 'notes.txt'), 'Synced');
    // Overwrite.
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-remote-change');
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
    await tapKey(tester, 'sftp-edit-resolve-notes.txt');
    await tapKey(tester, 'sftp-conflict-overwrite_remote');
    expect(_editStatus(tester, 'notes.txt'), 'Synced');
    // Decide later keeps the conflict.
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-remote-change');
    await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
    await tapKey(tester, 'sftp-edit-resolve-notes.txt');
    await tapKey(tester, 'sftp-conflict-later');
    expect(_editStatus(tester, 'notes.txt'), 'Conflict');
    // Stop editing during a conflict asks again.
    await tapKey(tester, 'sftp-edit-stop-notes.txt');
    expect(find.byKey(const ValueKey('sftp-conflict-dialog')), findsOneWidget);
    await tapKey(tester, 'sftp-conflict-discard_local');
    expect(_editStatus(tester, 'notes.txt'), 'Synced');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('after unlock, "Recover unsaved edits?" resumes selected leftovers and deletes the rest', (tester) async {
    final backend = MockBackend(config: const MockConfig.test(seedEditLeftovers: true));
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await settle(tester);
    expect(find.byKey(const ValueKey('sftp-leftovers-dialog')), findsOneWidget);
    expect(find.text('Recover unsaved edits?'), findsOneWidget);
    final modified = tester.widget<CheckboxListTile>(find.byKey(const ValueKey('sftp-leftover-wp-config.php')));
    expect(modified.value, isTrue, reason: 'locally modified leftovers are preselected');
    final damaged = tester.widget<CheckboxListTile>(find.byKey(const ValueKey('sftp-leftover-settings.json')));
    expect(damaged.onChanged, isNull, reason: 'a damaged leftover can only be deleted');
    await tapKey(tester, 'sftp-leftovers-recover');
    expect(find.byKey(const ValueKey('sftp-leftovers-dialog')), findsNothing);
    expect(find.text('Recovered 1 file'), findsOneWidget);
    await tapKey(tester, 'nav-sftp');
    expect(find.byKey(const ValueKey('sftp-edit-session-wp-config.php')), findsOneWidget);
    expect(_editStatus(tester, 'wp-config.php'), 'Synced');
    backend.sftpBrowser.listEditLeftovers().then((l) => expect(l, isEmpty)).ignore();
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('local pane: upload by double-click and drag, transfers drawer with retry', (tester) async {
    final backend = await _openBrowser(tester);
    await tapKey(tester, 'sftp-toolbar-local-pane');
    expect(find.byKey(const ValueKey('sftp-local-pane')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('local-row-notes.txt')));
    await tester.pump(const Duration(milliseconds: 60));
    await tester.tap(find.byKey(const ValueKey('local-row-notes.txt')));
    await settle(tester);
    expect(find.byKey(const ValueKey('sftp-activity')), findsOneWidget);
    expect(find.byKey(const ValueKey('transfer-notes.txt')), findsOneWidget);
    expect(tester.widget<Text>(find.byKey(const ValueKey('transfer-status-notes.txt'))).data, startsWith('Done'));
    // Drag a local file onto a remote folder row = upload into it (demo failure → Retry).
    backend.sftpBrowser.debugFailNextTransfer();
    final to = tester.getCenter(_row('$_home/logs'));
    final gesture = await tester.startGesture(tester.getCenter(find.byKey(const ValueKey('local-row-notes.txt'))));
    await tester.pump();
    await gesture.moveBy(const Offset(40, 0));
    await tester.pump();
    await gesture.moveTo(to);
    await tester.pump();
    await gesture.up();
    await settle(tester);
    expect(find.byKey(const ValueKey('transfer-retry-notes.txt')), findsOneWidget, reason: 'demo failure');
    await tapKey(tester, 'transfer-retry-notes.txt');
    expect(find.byKey(const ValueKey('transfer-retry-notes.txt')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final locale in const [AppLocale.ru, AppLocale.en]) {
    testWidgets('renders every SFTP part at 1024 px in ${locale.wireName} without overflow', (tester) async {
      final l10n = lookupAppLocalizations(Locale(locale.wireName));
      await _openBrowser(tester, locale: locale, size: const Size(1024, 720));
      expect(tester.takeException(), isNull, reason: 'browser');
      await tapKey(tester, 'sftp-toolbar-local-pane');
      expect(tester.takeException(), isNull, reason: 'local pane');
      await tapKey(tester, 'sftp-toolbar-transfers');
      expect(tester.takeException(), isNull, reason: 'transfers drawer');
      await tapKey(tester, 'sftp-toolbar-actions');
      expect(find.byKey(const ValueKey('sftp-action-getInfo')), findsOneWidget);
      expect(tester.takeException(), isNull, reason: 'action menu');
      await _key(tester, LogicalKeyboardKey.escape);
      await tapKey(tester, 'sftp-toolbar-local-pane');
      await _click(tester, '$_home/deploy.sh');
      await _key(tester, LogicalKeyboardKey.keyI, primary: true);
      expect(tester.takeException(), isNull, reason: 'get info');
      await tester.tap(find.text(l10n.commonClose).last);
      await settle(tester);
      await _click(tester, '$_home/notes.txt');
      await _key(tester, LogicalKeyboardKey.space);
      expect(tester.takeException(), isNull, reason: 'quick look');
      await _key(tester, LogicalKeyboardKey.escape);
      await _doubleClick(tester, '$_home/notes.txt');
      expect(tester.takeException(), isNull, reason: 'editing panel');
      await _debugEdit(tester, 'notes.txt', 'sftp-debug-remote-change');
      await _debugEdit(tester, 'notes.txt', 'sftp-debug-save');
      await tapKey(tester, 'sftp-edit-resolve-notes.txt');
      expect(tester.takeException(), isNull, reason: 'conflict dialog');
      await tapKey(tester, 'sftp-conflict-later');
      await _key(tester, LogicalKeyboardKey.keyL, primary: true);
      expect(tester.takeException(), isNull, reason: 'path field');
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
