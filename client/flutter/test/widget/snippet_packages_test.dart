import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/snippets/starter_catalog.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

ProviderContainer _container(WidgetTester tester) =>
    ProviderScope.containerOf(tester.element(find.byType(Navigator).first));

void main() {
  testWidgets('import, edit, filter, move and delete a starter package without recreating it', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-snippets');
    await tapKey(tester, 'snippet-starters');
    await tapKey(tester, 'snippet-catalog-add-linux');
    expect(backend.snippets.currentSnippets.where((s) => s.packageName == 'Linux diagnostics'), hasLength(5));
    expect(isEnabled(tester, 'snippet-catalog-add-linux'), isFalse);
    await tester.tap(find.text('Close').last);
    await settle(tester);
    await tapKey(tester, 'snippet-package-filter');
    await tester.tap(find.text('Linux diagnostics (5)').last);
    await settle(tester);
    await tapKey(tester, 'snippet-Disk space');
    await enterKey(tester, 'snippet-name', 'My disk check');
    await enterKey(tester, 'snippet-template', 'df -h /');
    await tapKey(tester, 'save-snippet');
    expect(
      backend.snippets.currentSnippets.firstWhere((s) => s.name == 'My disk check').catalogId,
      'consolecrypt.linux.disk.v1',
    );
    await tapKey(tester, 'snippet-starters');
    expect(isEnabled(tester, 'snippet-catalog-add-linux'), isFalse);
    await tester.tap(find.text('Close').last);
    await settle(tester);
    await tapKey(tester, 'snippet-package-menu');
    await tester.tap(find.text('Rename package').last);
    await settle(tester);
    await enterKey(tester, 'text-input-dialog-field', 'My diagnostics');
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(backend.snippets.currentSnippets.where((s) => s.packageName == 'My diagnostics'), hasLength(5));
    await tapKey(tester, 'snippet-menu-My disk check');
    await tester.tap(find.text('Move to package…').last);
    await settle(tester);
    await enterKey(tester, 'text-input-dialog-field', '');
    await tester.tap(find.text('Save').last);
    await settle(tester);
    expect(backend.snippets.currentSnippets.firstWhere((s) => s.name == 'My disk check').packageName, isNull);
    await tapKey(tester, 'snippet-package-menu');
    await tester.tap(find.text('Delete package').last);
    await settle(tester);
    await tapKey(tester, 'confirm-cancel');
    expect(backend.snippets.currentSnippets.where((s) => s.packageName == 'My diagnostics'), hasLength(4));
    await tapKey(tester, 'snippet-package-menu');
    await tester.tap(find.text('Delete package').last);
    await settle(tester);
    await tapKey(tester, 'confirm-ok');
    expect(backend.snippets.currentSnippets.where((s) => s.packageName == 'My diagnostics'), isEmpty);
    await tapKey(tester, 'nav-snippets');
    await tapKey(tester, 'nav-snippets');
    expect(backend.snippets.currentSnippets.where((s) => s.catalogId != null).single.name, 'My disk check');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('multi-run reviews selected connected terminals, cancels cleanly and runs each once', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final c = _container(tester);
    final controller = c.read(terminalTabsProvider.notifier);
    final initialUsage = backend.snippets.currentSnippets.firstWhere((s) => s.name == 'Disk usage').usageCount;
    final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi');
    final a = await controller.open(host);
    final b = await controller.open(host);
    await settle(tester);
    expect(a.isConnected && b.isConnected, isTrue);
    await tapKey(tester, 'nav-snippets');
    await enterKey(tester, 'snippet-search', 'Disk usage');
    await settle(tester);
    await tapKey(tester, 'snippet-menu-Disk usage');
    await tester.tap(find.text('Run in multiple terminals…').last);
    await settle(tester);
    expect(find.byKey(const ValueKey('snippet-targets')), findsOneWidget);
    await tapKey(tester, 'snippet-targets-continue');
    expect(find.byKey(const ValueKey('run-confirmation')), findsOneWidget);
    expect(c.read(snippetsProvider).value!.firstWhere((s) => s.name == 'Disk usage').usageCount, initialUsage);
    await tapKey(tester, 'run-confirm');
    await settle(tester);
    for (final tab in [a, b]) {
      expect(tab.terminal.buffer.lines.toList().map((line) => line.toString()).join('\n'), contains('Filesystem'));
    }
    expect(c.read(snippetsProvider).value!.firstWhere((s) => s.name == 'Disk usage').usageCount, initialUsage + 1);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('cancel and a closed target prevent multi-run from sending commands', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final c = _container(tester);
    final controller = c.read(terminalTabsProvider.notifier);
    final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi');
    final a = await controller.open(host);
    final b = await controller.open(host);
    await settle(tester);
    final initialUsage = backend.snippets.currentSnippets.firstWhere((s) => s.name == 'Disk usage').usageCount;
    await tapKey(tester, 'nav-snippets');
    await enterKey(tester, 'snippet-search', 'Disk usage');
    await settle(tester);
    Future<void> review() async {
      await tapKey(tester, 'snippet-menu-Disk usage');
      await tester.tap(find.text('Run in multiple terminals…').last);
      await settle(tester);
      await tapKey(tester, 'snippet-targets-continue');
    }

    await review();
    await tester.tap(
      find.descendant(of: find.byKey(const ValueKey('run-confirmation')), matching: find.text('Cancel')),
    );
    await settle(tester);
    expect(backend.snippets.currentSnippets.firstWhere((s) => s.name == 'Disk usage').usageCount, initialUsage);
    await review();
    await tester.runAsync(() => controller.close(b));
    await settle(tester);
    expect(c.read(terminalTabsProvider).tabs, isNot(contains(b)));
    await tapKey(tester, 'run-confirm');
    expect(find.text('The profile or selected terminals changed. Choose your targets again.'), findsOneWidget);
    expect(a.terminal.buffer.lines.toList().map((line) => line.toString()).join('\n'), isNot(contains('Filesystem')));
    expect(backend.snippets.currentSnippets.firstWhere((s) => s.name == 'Disk usage').usageCount, initialUsage);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('catalog and populated panel fit Russian 140% and shortcut toggles panel', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await backend.settings.updateLocal(backend.settings.currentLocal.copyWith(uiFontScale: 1.4));
    for (final s in starterSnippetPackages(lookupAppLocalizations(const Locale('ru'))).first.snippets) {
      await backend.snippets.saveSnippet(s);
    }
    await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.period);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await settle(tester);
    expect(_container(tester).read(workspaceToolsProvider).selected, WorkspaceTool.snippets);
    expect(tester.takeException(), isNull);
    await tapKey(tester, 'snippet-starters');
    expect(find.byKey(const ValueKey('snippet-catalog')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.macOS}));
}
