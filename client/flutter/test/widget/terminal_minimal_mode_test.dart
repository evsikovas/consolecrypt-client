import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

Future<(MockBackend, ProviderContainer)> pumpMinimalTerminal(
  WidgetTester tester, {
  TerminalTabPlacement placement = TerminalTabPlacement.top,
  bool collapsed = false,
  Size size = const Size(1440, 900),
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  final settings = backend.services.settings;
  await settings.updateLocal(
    settings.currentLocal.copyWith(
      terminalTabPlacement: placement,
      terminalTabColumnCollapsed: collapsed,
      themeMode: AppThemeMode.dark,
    ),
  );
  await pumpApp(tester, backend, size: size, locale: AppLocale.ru);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  final tabs = container.read(terminalTabsProvider.notifier);
  for (final name in ['raspberry-pi', 'prod-web-1']) {
    await tabs.open(backend.inventory.currentHosts.firstWhere((host) => host.name == name));
  }
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  return (backend, container);
}

Future<void> minimalKeys(WidgetTester tester, LogicalKeyboardKey key, {bool shift = false}) async {
  // Real command dispatch de-duplicates native menu + keyboard events for
  // 250ms. Pumped fake time alone doesn't advance DateTime.now().
  if (key == LogicalKeyboardKey.keyM) {
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 260)));
  }
  final modifier = AppPlatform.usesMeta ? LogicalKeyboardKey.metaLeft : LogicalKeyboardKey.controlLeft;
  await tester.sendKeyDownEvent(modifier);
  if (shift) await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyEvent(key);
  if (shift) await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
  await tester.sendKeyUpEvent(modifier);
  await settle(tester);
}

void main() {
  testWidgets('macOS minimal: actions clear native buttons with folded, resized and top tabs', (tester) async {
    final messenger = tester.binding.defaultBinaryMessenger;
    const native = MethodChannel('consolecrypt/window');
    const plugin = MethodChannel('macos_window_utils/window_manipulator');
    var leading = 118.0;
    messenger.setMockMethodCallHandler(plugin, (_) async => null);
    messenger.setMockMethodCallHandler(
      native,
      (call) async => call.method == 'getTrafficLightsLeading' ? leading : null,
    );
    addTearDown(() {
      messenger.setMockMethodCallHandler(plugin, null);
      messenger.setMockMethodCallHandler(native, null);
    });
    final (backend, container) = await pumpMinimalTerminal(
      tester,
      placement: TerminalTabPlacement.left,
      collapsed: true,
    );
    final pane = tester.state(find.byType(TerminalPane).first);
    await minimalKeys(tester, LogicalKeyboardKey.keyM, shift: true);
    void expectClear() {
      final action = tester.getRect(find.byKey(const ValueKey('terminal-sftp')));
      expect(action.left, greaterThanOrEqualTo(leading));
      expect(action.top, lessThan(52), reason: 'SFTP remains in the unified toolbar');
      expect(tester.state(find.byType(TerminalPane).first), same(pane));
      expect(tester.takeException(), isNull);
    }

    expectClear();
    leading = 132;
    tester.view.physicalSize = const Size(1000, 800);
    await settle(tester);
    expectClear();
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(terminalTabColumnCollapsed: false));
    await settle(tester);
    expectClear();
    final handle = find.byKey(const ValueKey('glass-tab-column-resize'));
    await tester.drag(handle, const Offset(100, 0));
    await settle(tester);
    expectClear();
    await settings.updateLocal(settings.currentLocal.copyWith(terminalTabColumnCollapsed: true));
    await settle(tester);
    expectClear();
    await settings.updateLocal(settings.currentLocal.copyWith(terminalTabPlacement: TerminalTabPlacement.top));
    await settle(tester);
    expectClear();
    await minimalKeys(tester, LogicalKeyboardKey.keyK);
    expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    expectClear();
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  for (final placement in TerminalTabPlacement.values) {
    testWidgets('minimal ${placement.name}: live panes survive, palette works, chrome returns', (tester) async {
      final (_, container) = await pumpMinimalTerminal(tester, placement: placement);
      final tabs = container.read(terminalTabsProvider).tabs.toList();
      final pane = tester.state(find.byType(TerminalPane).first);
      final before = tester.getRect(find.byKey(const ValueKey('terminal-card')));
      tabs.last.terminal.write('MINIMALISM_BUFFER_MARKER\r\n');
      await tester.tap(find.byType(TerminalPane).first);
      await minimalKeys(tester, LogicalKeyboardKey.keyM, shift: true);
      expect(container.read(terminalMinimalModeProvider), isTrue);
      expect(find.byType(GlassSidebar), findsNothing);
      expect(find.byKey(const ValueKey('terminal-minimal-workspace')), findsOneWidget);
      expect(tester.state(find.byType(TerminalPane).first), same(pane));
      expect(container.read(terminalTabsProvider).tabs, orderedEquals(tabs));
      expect(tabs.last.terminal.buffer.getText(), contains('MINIMALISM_BUFFER_MARKER'));
      expect(tester.getRect(find.byKey(const ValueKey('terminal-card'))).width, greaterThan(before.width));

      await minimalKeys(tester, LogicalKeyboardKey.keyK);
      expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
      await tester.enterText(find.byKey(const ValueKey('palette-input')), 'raspberry');
      await settle(tester);
      expect(find.byKey(ValueKey('palette-host-${tabs.first.host!.id.value}')), findsOneWidget);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await settle(tester);
      expect(container.read(terminalMinimalModeProvider), isTrue);
      await tester.tap(find.byKey(const ValueKey('terminal-minimal-toggle')));
      await settle(tester);
      expect(container.read(terminalMinimalModeProvider), isFalse);
      expect(find.byType(GlassSidebar), findsOneWidget);
      expect(tester.state(find.byType(TerminalPane).first), same(pane));
      expect(container.read(terminalTabsProvider).tabs, orderedEquals(tabs));
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.macOS, TargetPlatform.linux}));
  }

  testWidgets('minimal layout can move tabs, split panes and reopen tools without leaving the mode', (tester) async {
    final (backend, container) = await pumpMinimalTerminal(tester);
    await tester.tap(find.byKey(const ValueKey('terminal-minimal-toggle')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('terminal-minimal-menu')));
    await settle(tester);
    await tester.tap(find.text('Вкладки слева'));
    await settle(tester);
    expect(find.byType(GlassTabColumn), findsOneWidget);
    expect(backend.services.settings.currentLocal.terminalTabPlacement, TerminalTabPlacement.left);
    await minimalKeys(tester, LogicalKeyboardKey.backslash, shift: true);
    expect(container.read(terminalTabsProvider).activeTab!.panes, hasLength(1));
    await minimalKeys(tester, LogicalKeyboardKey.period);
    expect(container.read(workspaceToolsProvider).selected, WorkspaceTool.snippets);
    expect(container.read(terminalMinimalModeProvider), isTrue);
    container.read(workspaceToolsProvider.notifier).close();
    container.read(routerProvider).go(AppRoutes.settings);
    await settle(tester);
    expect(container.read(terminalMinimalModeProvider), isFalse);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(find.byType(GlassSidebar), findsOneWidget);
    expect(container.read(terminalTabsProvider).activeTab!.panes, hasLength(1));
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('closing the last tab keeps a visible exit and locking clears the mode', (tester) async {
    final (backend, container) = await pumpMinimalTerminal(tester);
    await minimalKeys(tester, LogicalKeyboardKey.keyM, shift: true);
    for (final tab in container.read(terminalTabsProvider).tabs.toList()) {
      await tester.runAsync(() => container.read(terminalTabsProvider.notifier).closeTab(tab));
    }
    await settle(tester);
    expect(find.byKey(const ValueKey('terminal-minimal-toggle')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('terminal-minimal-toggle')));
    await settle(tester);
    expect(container.read(terminalMinimalModeProvider), isFalse);
    await minimalKeys(tester, LogicalKeyboardKey.keyM, shift: true);
    await backend.services.vault.lock();
    await settle(tester);
    expect(container.read(terminalMinimalModeProvider), isFalse);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('mobile has no minimalism button or shortcut', (tester) async {
    final (_, container) = await pumpMinimalTerminal(tester, size: const Size(430, 900));
    expect(find.byKey(const ValueKey('terminal-minimal-toggle')), findsNothing);
    expect(container.read(appCommandsProvider).where((c) => c.id == AppCommandId.toggleTerminalMinimalMode), isEmpty);
    container.read(terminalMinimalModeProvider.notifier).toggle();
    expect(container.read(terminalMinimalModeProvider), isFalse);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.iOS}));
}
