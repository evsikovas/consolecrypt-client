import 'package:consolecrypt/app/commands.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  test('bindings round-trip and invalid overrides fall back to defaults', () {
    const binding = ShortcutBinding(LogicalKeyboardKey.keyJ, shift: true);
    expect(ShortcutBinding.decode(binding.encode()), binding);
    expect(ShortcutBinding.decode('garbage'), isNull);
    final commands = resolveAppCommands({
      AppCommandId.lockVault.name: binding.encode(),
      AppCommandId.snippets.name: '',
      AppCommandId.newHost.name: 'x:1',
    });
    expect(commandFor(AppCommandId.lockVault, commands).binding, binding);
    expect(commandFor(AppCommandId.snippets, commands).binding, isNull);
    expect(commandFor(AppCommandId.newHost, commands).binding, commandFor(AppCommandId.newHost).binding);
  });

  testWidgets('a shortcut taken from another command is freed, and defaults come back', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'shortcut-change-lockVault');
    expect(find.byKey(const ValueKey('shortcut-capture')), findsOneWidget);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(find.byKey(const ValueKey('shortcut-capture-conflict')), findsOneWidget);
    await tapKey(tester, 'shortcut-capture-assign');
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final commands = container.read(appCommandsProvider);
    expect(commandFor(AppCommandId.lockVault, commands).binding, const ShortcutBinding(LogicalKeyboardKey.keyK));
    expect(commandFor(AppCommandId.commandPalette, commands).binding, isNull);
    expect(backend.services.settings.currentLocal.shortcutOverrides, hasLength(2));
    await tapKey(tester, 'shortcuts-reset-all');
    expect(backend.services.settings.currentLocal.shortcutOverrides, isEmpty);
    expect(container.read(appCommandsProvider).map((c) => c.binding), appCommands.map((c) => c.binding));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
