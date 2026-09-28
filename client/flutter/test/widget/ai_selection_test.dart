import 'package:consolecrypt/ai/ai_chat_controller.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

ProviderContainer _container(WidgetTester tester) =>
    ProviderScope.containerOf(tester.element(find.byType(Navigator).first));

CheckboxListTile _attachment(WidgetTester tester) => tester.widget(find.byKey(const ValueKey('ai-attach-selection')));

void _select(TerminalTab tab, String text) {
  tab.terminal.write('\x1b[2J\x1b[H$text');
  final buffer = tab.terminal.buffer;
  tab.controller.setSelection(buffer.createAnchor(0, 0), buffer.createAnchor(text.length, 0));
}

void main() {
  testWidgets('idle terminal selection enables the whole attachment row and reaches chat only when checked', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final c = _container(tester);
    final controller = c.read(terminalTabsProvider.notifier);
    final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi');
    final tab = await controller.open(host);
    c.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    await tapKey(tester, 'nav-ai');
    expect(_attachment(tester).onChanged, isNull);
    final revision = c.read(terminalTabsProvider).revision;
    _select(tab, 'sample output');
    await settle(tester);
    expect(c.read(terminalTabsProvider).revision, revision, reason: 'no unrelated terminal event may be needed');
    expect(_attachment(tester).onChanged, isNotNull);
    expect(_attachment(tester).value, isFalse);
    await tester.tap(find.text('Include selected terminal text'));
    await settle(tester);
    expect(_attachment(tester).value, isTrue);
    final selected = tab.selectedText!;
    await enterKey(tester, 'ai-input', 'Explain this output');
    await tapKey(tester, 'ai-send');
    expect(
      c.read(aiChatControllerProvider).messages.last.content,
      contains('the ${selected.length} characters of terminal output you selected'),
    );
    await tapKey(tester, 'ai-new-conversation');
    await tester.tap(find.text('Include selected terminal text'));
    await settle(tester);
    expect(_attachment(tester).value, isFalse);
    await enterKey(tester, 'ai-input', 'Answer without terminal context');
    await tapKey(tester, 'ai-send');
    expect(c.read(aiChatControllerProvider).messages.last.content, isNot(contains('characters of terminal output')));
    tab.controller.clearSelection();
    await settle(tester);
    expect(_attachment(tester).onChanged, isNull);
    expect(_attachment(tester).value, isFalse);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('attachment opt-in is cleared when switching or closing the active terminal', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final c = _container(tester);
    final controller = c.read(terminalTabsProvider.notifier);
    final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi');
    final a = await controller.open(host);
    final b = await controller.open(host);
    c.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    await tapKey(tester, 'nav-ai');
    _select(a, 'first tab');
    _select(b, 'second tab');
    await settle(tester);
    await tapKey(tester, 'ai-attach-selection');
    expect(_attachment(tester).value, isTrue);
    controller.activate(0);
    await settle(tester);
    expect(_attachment(tester).onChanged, isNotNull);
    expect(_attachment(tester).value, isFalse);
    await tapKey(tester, 'ai-attach-selection');
    await tester.runAsync(() => controller.close(a));
    await settle(tester);
    expect(_attachment(tester).value, isFalse);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
