import 'dart:async';

import 'package:consolecrypt/ai/ai_chat_controller.dart';
import 'package:consolecrypt/ai/terminal_attachment.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

Future<(MockBackend, ProviderContainer, TerminalTab)> _open(WidgetTester tester, {bool compact = false}) async {
  final b = testBackend();
  addTearDown(b.dispose);
  await b.debugSignInDemoAndUnlock();
  if (compact) await b.services.settings.updateLocal(b.services.settings.currentLocal.copyWith(uiFontScale: 1.4));
  await pumpApp(
    tester,
    b,
    locale: compact ? AppLocale.ru : AppLocale.en,
    size: compact ? const Size(1024, 720) : const Size(1600, 1000),
  );
  final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  final tab = await c
      .read(terminalTabsProvider.notifier)
      .open(b.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi'));
  c.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  return (b, c, tab);
}

void _select(TerminalTab tab, String text) {
  tab.terminal.write('\x1b[2J\x1b[H$text');
  tab.controller.setSelection(tab.terminal.buffer.createAnchor(0, 0), tab.terminal.buffer.createAnchor(text.length, 0));
}

Future<void> _menu(WidgetTester tester, {bool corner = false}) async {
  final terminal = find.byType(TerminalView).first;
  final point = corner
      ? tester.getBottomRight(terminal) - const Offset(10, 10)
      : tester.getTopLeft(terminal) + const Offset(100, 60);
  await tester.tapAt(point, buttons: kSecondaryMouseButton, kind: PointerDeviceKind.mouse);
  await settle(tester);
}

void _clipboard(WidgetTester tester, {Future<Object?> Function(MethodCall)? handler}) {
  tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, handler);
  addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
}

void main() {
  testWidgets('right click preserves selection and copies exact indentation without reading the clipboard', (
    tester,
  ) async {
    final (_, _, tab) = await _open(tester);
    _select(tab, '  echo sample  ');
    final selected = tab.rawSelectedText;
    String? copied;
    var reads = 0;
    _clipboard(
      tester,
      handler: (call) async {
        if (call.method == 'Clipboard.setData') copied = (call.arguments as Map)['text'] as String;
        if (call.method == 'Clipboard.getData') reads++;
        return null;
      },
    );
    await _menu(tester);
    expect(tab.rawSelectedText, selected);
    expect(find.byKey(const ValueKey('terminal-menu-ask-ai')), findsOneWidget);
    await tapKey(tester, 'terminal-menu-copy');
    expect(copied, selected);
    expect(copied, startsWith('  '));
    expect(reads, 0);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('Ask AI attaches a snapshot, keeps the draft, sends only on Send and clears after use', (tester) async {
    final (_, c, tab) = await _open(tester);
    await tapKey(tester, 'nav-ai');
    await enterKey(tester, 'ai-input', 'Explain this');
    await tapKey(tester, 'workspace-tools-close');
    _select(tab, 'sample output');
    final selected = tab.rawSelectedText!;
    await _menu(tester);
    await tapKey(tester, 'terminal-menu-ask-ai');
    expect(c.read(aiChatControllerProvider).messages, isEmpty);
    expect(c.read(terminalAttachmentProvider)?.text, selected);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('ai-input'))).controller!.text, 'Explain this');
    _select(tab, 'different output');
    await settle(tester);
    expect(tester.widget<Text>(find.byKey(const ValueKey('ai-terminal-attachment-text'))).data, selected);
    await tapKey(tester, 'ai-send');
    expect(
      c.read(aiChatControllerProvider).messages.last.content,
      contains('the ${selected.length} characters of terminal output you selected'),
    );
    expect(c.read(terminalAttachmentProvider), isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('attachment removal, tab changes and vault lock clear the explicit selection', (tester) async {
    final (b, c, tab) = await _open(tester);
    _select(tab, 'sample output');
    await _menu(tester);
    await tapKey(tester, 'terminal-menu-ask-ai');
    await tapKey(tester, 'ai-terminal-attachment-remove');
    expect(c.read(terminalAttachmentProvider), isNull);
    c.read(terminalAttachmentProvider.notifier).attach(tab, 'again');
    await c.read(terminalTabsProvider.notifier).open(tab.host);
    await settle(tester);
    expect(c.read(terminalAttachmentProvider), isNull);
    final active = c.read(terminalTabsProvider).active!;
    c.read(terminalAttachmentProvider.notifier).attach(active, 'local only');
    await b.vault.lock();
    await settle(tester);
    expect(c.read(terminalAttachmentProvider), isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('selected text becomes an editable history snippet only after Save', (tester) async {
    final (b, _, tab) = await _open(tester);
    final before = b.snippets.currentSnippets.length;
    _select(tab, '  echo {{message}}');
    final selected = tab.rawSelectedText!;
    await _menu(tester);
    await tapKey(tester, 'terminal-menu-snippet');
    expect(b.snippets.currentSnippets.length, before);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('snippet-template'))).controller!.text, selected);
    expect(find.textContaining('Drafted by AI'), findsNothing);
    await enterKey(tester, 'snippet-name', 'From my terminal');
    await tapKey(tester, 'save-snippet');
    final saved = b.snippets.currentSnippets.singleWhere((s) => s.name == 'From my terminal');
    expect(saved.template, selected);
    expect(saved.source, SnippetSource.history);
    expect(saved.effectiveVariables.single.name, 'message');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('empty selection disables text actions and disconnected terminal cannot paste', (tester) async {
    final (_, _, tab) = await _open(tester);
    tab.state = SessionConnectionState.disconnected;
    await _menu(tester);
    for (final key in ['terminal-menu-copy', 'terminal-menu-ask-ai', 'terminal-menu-snippet', 'terminal-menu-paste']) {
      await tapKey(tester, key);
      expect(find.byKey(const ValueKey('terminal-menu-select-all')), findsOneWidget);
    }
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('keyboard paste follows the same control-character confirmation and cancel sends nothing', (
    tester,
  ) async {
    final (_, _, tab) = await _open(tester);
    final writes = <String>[];
    tab.terminal.onOutput = writes.add;
    _clipboard(
      tester,
      handler: (call) async => call.method == 'Clipboard.getData' ? {'text': 'echo test\x1b[201~\n'} : null,
    );
    await tester.tap(find.byType(TerminalView).first);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyV);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await settle(tester);
    expect(find.byKey(const ValueKey('confirm-cancel')), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(writes, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('menu action is discarded when the active tab changes while the menu is open', (tester) async {
    final (_, c, tab) = await _open(tester);
    _select(tab, 'old selection');
    await _menu(tester);
    await c.read(terminalTabsProvider.notifier).open(tab.host);
    await settle(tester);
    await tapKey(tester, 'terminal-menu-ask-ai');
    expect(c.read(terminalAttachmentProvider), isNull);
    expect(find.byKey(const ValueKey('ai-input')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('plain paste writes to the target terminal; multiline paste needs confirmation', (tester) async {
    final (_, _, tab) = await _open(tester);
    final writes = <String>[];
    tab.terminal.onOutput = writes.add;
    var clipboard = 'echo sample';
    _clipboard(tester, handler: (call) async => call.method == 'Clipboard.getData' ? {'text': clipboard} : null);
    await _menu(tester);
    await tapKey(tester, 'terminal-menu-paste');
    expect(writes, ['echo sample']);
    writes.clear();
    clipboard = 'echo first\necho second\n';
    await _menu(tester);
    await tapKey(tester, 'terminal-menu-paste');
    expect(writes, isEmpty);
    await tapKey(tester, 'confirm-cancel');
    expect(writes, isEmpty);
    await _menu(tester);
    await tapKey(tester, 'terminal-menu-paste');
    await tapKey(tester, 'confirm-ok');
    expect(writes.join(), contains('echo first'));
    expect(writes.join(), contains('echo second'));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('clipboard read finishing after a tab switch never writes to either session', (tester) async {
    final (_, c, tab) = await _open(tester);
    final read = Completer<Object?>();
    final writes = <String>[];
    tab.terminal.onOutput = writes.add;
    _clipboard(tester, handler: (call) => call.method == 'Clipboard.getData' ? read.future : Future.value());
    await _menu(tester);
    await tapKey(tester, 'terminal-menu-paste');
    final other = await c.read(terminalTabsProvider.notifier).open(tab.host);
    other.terminal.onOutput = writes.add;
    read.complete({'text': 'late clipboard'});
    await settle(tester);
    expect(writes, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('entire buffer includes scrollback; clear selection and keyboard menu work at the window edge', (
    tester,
  ) async {
    final (_, _, tab) = await _open(tester, compact: true);
    tab.terminal.write(List.generate(200, (i) => 'scrollback line $i\r\n').join());
    await _menu(tester, corner: true);
    await tapKey(tester, 'terminal-menu-select-all');
    expect(tab.rawSelectedText, contains('scrollback line 0'));
    expect(tab.rawSelectedText, contains('scrollback line 199'));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.f10);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await settle(tester);
    await tapKey(tester, 'terminal-menu-clear-selection');
    expect(tab.controller.selection, isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('mouse-aware apps keep right click; selected text and Shift right click open the local menu', (
    tester,
  ) async {
    final (_, _, tab) = await _open(tester);
    tab.terminal.write('\x1b[?1000h\x1b[?1006h');
    final writes = <String>[];
    tab.terminal.onOutput = writes.add;
    await _menu(tester);
    expect(find.byKey(const ValueKey('terminal-menu-copy')), findsNothing);
    expect(writes, isNotEmpty);
    writes.clear();
    _select(tab, 'sample');
    await _menu(tester);
    expect(find.byKey(const ValueKey('terminal-menu-copy')), findsOneWidget);
    expect(writes, isEmpty);
    expect(tab.controller.suspendedPointerInputs, isFalse);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    tab.controller.clearSelection();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await _menu(tester);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    expect(find.byKey(const ValueKey('terminal-menu-copy')), findsOneWidget);
    expect(writes, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
}
