import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

Future<(ProviderContainer, TerminalTab)> openTerminal(WidgetTester tester) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  await pumpApp(tester, backend);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  final tab = await container.read(terminalTabsProvider.notifier).open(backend.inventory.currentHosts.first);
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  return (container, tab);
}

void main() {
  testWidgets('Windows hardware letters use layout Unicode and consume the OS key exactly once', (tester) async {
    final (_, tab) = await openTerminal(tester);
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    expect(await tester.sendKeyDownEvent(LogicalKeyboardKey.keyA, character: 'ф'), isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyA);
    expect(output.join(), 'ф');
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.keyZ, character: 'Я'), isTrue);
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.backquote, character: 'ё'), isTrue);
    expect(await tester.sendKeyEvent(LogicalKeyboardKey.space, character: ' '), isTrue);
    expect(await tester.sendKeyDownEvent(LogicalKeyboardKey.keyL, character: 'l'), isTrue);
    expect(await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyL, character: 'l'), isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyL);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    expect(output.join(), 'фЯё ll\r');
    expect(tester.testTextInput.isRegistered, isTrue);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('one click after losing focus and selecting text reopens native Unicode input', (tester) async {
    final (_, tab) = await openTerminal(tester);
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    tab.controller.setSelection(tab.terminal.buffer.createAnchor(0, 0), tab.terminal.buffer.createAnchor(3, 0));
    FocusManager.instance.primaryFocus?.unfocus();
    await tester.pump();
    await tester.tap(find.byType(TerminalView), kind: PointerDeviceKind.mouse);
    await tester.pump();
    expect(tester.testTextInput.isRegistered, isTrue);
    tester.testTextInput.enterText('Привет, ёж!');
    await tester.pump();
    expect(output.join(), 'Привет, ёж!');
    expect(tab.controller.selection, isNull);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    expect(output.join(), 'Привет, ёж!\r');
    await settle(tester);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('switching tabs sends Cyrillic and IME composition only to the visible terminal', (tester) async {
    final (container, first) = await openTerminal(tester);
    final firstOutput = <String>[];
    first.terminal.onOutput = firstOutput.add;
    final second = await container.read(terminalTabsProvider.notifier).open(first.host);
    final secondOutput = <String>[];
    second.terminal.onOutput = secondOutput.add;
    await settle(tester);
    tester.testTextInput.enterText('вторая');
    await tester.pump();
    expect(firstOutput, isEmpty);
    expect(secondOutput.join(), 'вторая');
    container.read(terminalTabsProvider.notifier).activate(0);
    await settle(tester);
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(
        text: 'первая',
        selection: TextSelection.collapsed(offset: 6),
        composing: TextRange(start: 0, end: 6),
      ),
    );
    await tester.pump();
    expect(firstOutput, isEmpty);
    tester.testTextInput.updateEditingValue(
      const TextEditingValue(text: 'первая', selection: TextSelection.collapsed(offset: 6)),
    );
    await tester.pump();
    expect(firstOutput.join(), 'первая');
    expect(secondOutput.join(), 'вторая');
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyC);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(firstOutput.join(), 'первая\x03');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
