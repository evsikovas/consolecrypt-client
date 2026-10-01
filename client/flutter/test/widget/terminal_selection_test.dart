import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

Future<TerminalTab> _open(
  WidgetTester tester, {
  Size size = const Size(1600, 1000),
  WorkspacePanelStyle style = WorkspacePanelStyle.floating,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  await backend.services.settings.updateLocal(
    backend.services.settings.currentLocal.copyWith(workspacePanelStyle: style),
  );
  await pumpApp(tester, backend, size: size);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  final tab = await container.read(terminalTabsProvider.notifier).open(backend.inventory.currentHosts.first);
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  tab.terminal.write('\x1b[2J\x1b[H${List.generate(240, (i) => 'line ${i.toString().padLeft(3, '0')}').join('\r\n')}');
  await tester.pump();
  return tab;
}

ScrollController _scroll(WidgetTester tester) =>
    tester.widget<TerminalView>(find.byType(TerminalView)).scrollController!;

void main() {
  testWidgets('wheel and trackpad scroll selected scrollback without changing or sending the selection', (
    tester,
  ) async {
    final tab = await _open(tester);
    tab.controller.setSelection(tab.terminal.buffer.createAnchor(0, 190), tab.terminal.buffer.createAnchor(8, 192));
    final selected = tab.rawSelectedText;
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    final scroll = _scroll(tester);
    final start = scroll.offset;
    final point = tester.getCenter(find.byType(TerminalView));
    await tester.sendEventToBinding(PointerScrollEvent(position: point, scrollDelta: const Offset(0, -180)));
    await tester.pump();
    expect(scroll.offset, lessThan(start));
    expect(tab.rawSelectedText, selected);
    final afterWheel = scroll.offset;
    final trackpad = await tester.createGesture(kind: PointerDeviceKind.trackpad);
    await trackpad.panZoomStart(point);
    await trackpad.panZoomUpdate(point, pan: const Offset(0, 90));
    await tester.pump();
    await trackpad.panZoomUpdate(point, pan: const Offset(0, 180));
    await trackpad.panZoomEnd();
    await tester.pump();
    expect(scroll.offset, lessThan(afterWheel));
    expect(tab.rawSelectedText, selected);
    expect(output, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final direction in [-1, 1]) {
    testWidgets('drag selection continues scrolling ${direction < 0 ? 'above' : 'below'} the viewport while held', (
      tester,
    ) async {
      final tab = await _open(tester);
      final scroll = _scroll(tester);
      scroll.jumpTo(scroll.position.maxScrollExtent / 2);
      await tester.pump();
      final view = tester.state<TerminalViewState>(find.byType(TerminalView));
      final rect = tester.getRect(find.byType(TerminalView));
      final startPoint = rect.center;
      final anchor = view.renderTerminal.getCellOffset(view.renderTerminal.globalToLocal(startPoint));
      final drag = await tester.startGesture(startPoint, kind: PointerDeviceKind.mouse);
      await drag.moveBy(const Offset(30, 0));
      await tester.pump();
      final outside = Offset(startPoint.dx + 80, direction < 0 ? rect.top - 40 : rect.bottom + 40);
      await drag.moveTo(outside);
      await tester.pump();
      final beforeHold = scroll.offset;
      for (var i = 0; i < 8; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }
      expect(scroll.offset * direction, greaterThan(beforeHold * direction));
      final selection = tab.controller.selection!;
      expect(selection.begin, anchor, reason: 'the selection origin stays attached to the original buffer cell');
      expect((selection.end.y - anchor.y) * direction, greaterThan(0));
      await drag.up();
      await tester.pump();
      final stopped = scroll.offset;
      await tester.pump(const Duration(milliseconds: 300));
      expect(scroll.offset, stopped, reason: 'releasing the pointer stops auto-scroll');
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  for (final style in WorkspacePanelStyle.values) {
    for (final tool in ['ai', 'snippets']) {
      testWidgets('${style.name}: one click dismisses compact $tool and restores native terminal input', (
        tester,
      ) async {
        final tab = await _open(tester, size: const Size(1024, 720), style: style);
        final output = <String>[];
        tab.terminal.onOutput = output.add;
        await tapKey(tester, 'nav-$tool');
        await enterKey(tester, tool == 'ai' ? 'ai-input' : 'snippet-search', 'demo draft');
        expect(find.byKey(const ValueKey('workspace-tools-barrier')), findsOneWidget);
        await tester.tapAt(
          tester.getTopLeft(find.byType(TerminalView)) + const Offset(40, 40),
          kind: PointerDeviceKind.mouse,
        );
        await tester.pump();
        await tester.pump();
        expect(find.byKey(const ValueKey('workspace-tools-panel')), findsNothing);
        expect(tester.state<TerminalViewState>(find.byType(TerminalView)).hasInputConnection, isTrue);
        tester.testTextInput.enterText('Привет, ёж!');
        await tester.pump();
        expect(output.join(), 'Привет, ёж!');
        await tester.sendKeyEvent(LogicalKeyboardKey.enter);
        expect(output.join(), 'Привет, ёж!\r');
        expect(tester.takeException(), isNull);
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    }
  }

  testWidgets('docked AI input returns to the selected terminal in one click without stealing focus later', (
    tester,
  ) async {
    final tab = await _open(tester);
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    await tapKey(tester, 'nav-ai');
    await enterKey(tester, 'ai-input', 'kept draft');
    expect(find.byKey(const ValueKey('workspace-tools-barrier')), findsNothing);
    tab.controller.setSelection(tab.terminal.buffer.createAnchor(0, 190), tab.terminal.buffer.createAnchor(8, 192));
    await tester.pump();
    await tester.tap(find.byType(TerminalView), kind: PointerDeviceKind.mouse);
    await tester.pump();
    await tester.pump();
    expect(tester.state<TerminalViewState>(find.byType(TerminalView)).hasInputConnection, isTrue);
    tester.testTextInput.enterText('ёж');
    await tester.pump();
    expect(output.join(), 'ёж');
    expect(tab.controller.selection, isNull);
    await enterKey(tester, 'ai-input', 'still editing');
    await tester.pump();
    tester.testTextInput.enterText('chat only');
    await tester.pump();
    expect(output.join(), 'ёж');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('ai-input'))).controller!.text, 'chat only');
    await settle(tester);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('closing tools after navigating away never activates an offstage terminal', (tester) async {
    final tab = await _open(tester, size: const Size(1024, 720));
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    await tapKey(tester, 'nav-ai');
    await enterKey(tester, 'ai-input', 'demo');
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.hosts);
    container.read(workspaceToolsProvider.notifier).close();
    await settle(tester);
    await enterKey(tester, 'hosts-search', 'web');
    await tester.pump();
    tester.testTextInput.enterText('host search only');
    await tester.pump();
    final search = find.descendant(of: find.byKey(const ValueKey('hosts-search')), matching: find.byType(TextField));
    expect(tester.widget<TextField>(search).controller!.text, 'host search only');
    expect(output, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Shift selection auto-scroll is local in mouse-aware apps and cancellation restores mouse reporting', (
    tester,
  ) async {
    final tab = await _open(tester);
    tab.terminal.write('\x1b[?1000h\x1b[?1006h');
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    final scroll = _scroll(tester);
    scroll.jumpTo(scroll.position.maxScrollExtent / 2);
    await tester.pump();
    final rect = tester.getRect(find.byType(TerminalView));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    final drag = await tester.startGesture(rect.center, kind: PointerDeviceKind.mouse);
    await drag.moveBy(const Offset(30, 0));
    await tester.pump();
    await drag.moveTo(Offset(rect.center.dx + 50, rect.top - 50));
    await tester.pump();
    final before = scroll.offset;
    await tester.pump(const Duration(milliseconds: 100));
    expect(scroll.offset, lessThan(before));
    expect(output, isEmpty);
    expect(tab.controller.suspendedPointerInputs, isTrue);
    await drag.cancel();
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.pump();
    expect(tab.controller.suspendedPointerInputs, isFalse);
    final after = scroll.offset;
    await tester.pump(const Duration(milliseconds: 250));
    expect(scroll.offset, after);
    tab.controller.clearSelection();
    await tester.tapAt(rect.center + const Offset(100, 0), kind: PointerDeviceKind.mouse);
    await tester.pump();
    expect(output.join(), contains('\x1b[<0;'));
    await settle(tester);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('alternate-screen wheel retains remote TUI reporting when no text is selected', (tester) async {
    final tab = await _open(tester);
    tab.terminal.write('\x1b[?1049h\x1b[?1000h\x1b[?1006h');
    await tester.pump();
    final output = <String>[];
    tab.terminal.onOutput = output.add;
    await tester.sendEventToBinding(
      PointerScrollEvent(position: tester.getCenter(find.byType(TerminalView)), scrollDelta: const Offset(0, -120)),
    );
    await tester.pump();
    expect(output.join(), contains('\x1b[<${TerminalMouseButton.wheelUp.id};'));
    expect(tab.controller.selection, isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('wheel during a held selection extends it from the original buffer cell', (tester) async {
    final tab = await _open(tester);
    final scroll = _scroll(tester);
    scroll.jumpTo(scroll.position.maxScrollExtent / 2);
    await tester.pump();
    final view = tester.state<TerminalViewState>(find.byType(TerminalView));
    final point = tester.getCenter(find.byType(TerminalView));
    final anchor = view.renderTerminal.getCellOffset(view.renderTerminal.globalToLocal(point));
    final drag = await tester.startGesture(point, kind: PointerDeviceKind.mouse);
    await drag.moveBy(const Offset(40, 30));
    await tester.pump();
    final before = scroll.offset;
    await tester.sendEventToBinding(PointerScrollEvent(position: point, scrollDelta: const Offset(0, -200)));
    await tester.pump();
    expect(scroll.offset, lessThan(before));
    expect(tab.controller.selection!.begin, anchor);
    expect(tab.controller.selection!.end.y, lessThan(anchor.y));
    await drag.moveBy(const Offset(20, 10));
    await tester.pump();
    expect(tab.controller.selection!.begin, anchor, reason: 'xterm must not recalculate the origin after scrolling');
    await drag.up();
    await settle(tester);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('switching tabs stops a held selection without scrolling the inactive terminal', (tester) async {
    final first = await _open(tester);
    final scroll = _scroll(tester);
    scroll.jumpTo(scroll.position.maxScrollExtent / 2);
    await tester.pump();
    final rect = tester.getRect(find.byType(TerminalView));
    final drag = await tester.startGesture(rect.center, kind: PointerDeviceKind.mouse);
    await drag.moveBy(const Offset(40, 0));
    await tester.pump();
    await drag.moveTo(Offset(rect.center.dx + 60, rect.top - 40));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    await container.read(terminalTabsProvider.notifier).open(first.host);
    await tester.pump();
    final before = scroll.offset;
    await tester.pump(const Duration(milliseconds: 300));
    expect(scroll.offset, before);
    await drag.cancel();
    await settle(tester);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
