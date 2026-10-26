import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/teams/team_providers.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final _desktop = TargetPlatformVariant.only(TargetPlatform.windows);

/// Unlocked demo vault with two SSH tabs, Terminal on screen.
Future<(MockBackend, ProviderContainer)> _terminal(WidgetTester tester) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  await pumpApp(tester, backend);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  final tabs = container.read(terminalTabsProvider.notifier);
  for (final name in ['raspberry-pi', 'prod-web-1']) {
    await tabs.open(backend.inventory.currentHosts.firstWhere((h) => h.name == name));
  }
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  return (backend, container);
}

Future<void> _keys(WidgetTester tester, List<LogicalKeyboardKey> modifiers, LogicalKeyboardKey key) async {
  for (final modifier in modifiers) {
    await tester.sendKeyDownEvent(modifier);
  }
  await tester.sendKeyEvent(key);
  for (final modifier in modifiers.reversed) {
    await tester.sendKeyUpEvent(modifier);
  }
  await settle(tester);
}

/// Closing a session waits for its output stream to stop, in real time.
Future<void> _closed(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
    await tester.pump();
  }
  await settle(tester);
}

void main() {
  testWidgets('split right and down: panes share the tab, the focused one is the active session', (tester) async {
    final (_, container) = await _terminal(tester);
    final controller = container.read(terminalTabsProvider.notifier);
    TerminalTabsState state() => container.read(terminalTabsProvider);
    final host = state().activeTab!;
    expect(find.byType(TerminalPane, skipOffstage: false), findsNWidgets(2), reason: 'one per tab');

    await tester.tap(find.byKey(const ValueKey('terminal-split')));
    await settle(tester);
    await tester.tap(find.text('Split right').last);
    await settle(tester);
    expect(host.panes, hasLength(1));
    expect(host.splitAxis, Axis.horizontal);
    expect(state().tabs, hasLength(2), reason: 'a pane is not a tab of the strip');
    expect(identical(state().active, host.panes.first), isTrue, reason: 'the new pane takes the keyboard');
    expect(
      identical(host.panes.first.target, host.target) || host.panes.first.target.label == host.target.label,
      isTrue,
    );
    expect(find.byKey(const ValueKey('terminal-pane-0')), findsOneWidget);
    expect(find.byKey(const ValueKey('terminal-pane-1')), findsOneWidget);
    final left = tester.getRect(find.byKey(const ValueKey('terminal-pane-0')));
    final right = tester.getRect(find.byKey(const ValueKey('terminal-pane-1')));
    expect(right.left, greaterThan(left.right - 1));

    // Ctrl+Shift+- joins the same axis (one axis per tab); Ctrl+] moves on.
    await _keys(tester, [LogicalKeyboardKey.control, LogicalKeyboardKey.shift], LogicalKeyboardKey.minus);
    expect(host.panes, hasLength(2));
    expect(host.focusedPane, 2);
    await _keys(tester, [LogicalKeyboardKey.control], LogicalKeyboardKey.bracketRight);
    expect(host.focusedPane, 0);
    expect(identical(state().active, host), isTrue);

    // A click focuses a pane; dragging a divider moves the space.
    await tester.tap(find.byKey(const ValueKey('terminal-pane-1')));
    await settle(tester);
    expect(host.focusedPane, 1);
    final before = host.paneShares.first;
    await tester.drag(find.byKey(const ValueKey('terminal-pane-divider-1')), const Offset(80, 0));
    await settle(tester);
    expect(host.paneShares.first, greaterThan(before));
    expect(host.paneShares.fold<double>(0, (a, b) => a + b), closeTo(1, 1e-9));

    // Closing a pane keeps the tab; closing the tab's own session hands the
    // tab to its first pane; closing the tab closes all of them.
    expect(find.byKey(const ValueKey('terminal-pane-close-2')).hitTestable(), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('terminal-pane-close-2')));
    await _closed(tester);
    expect(host.panes, hasLength(1));
    final heir = host.panes.first;
    final index = state().tabs.indexOf(host);
    unawaited(controller.close(host));
    await _closed(tester);
    expect(identical(state().tabs[index], heir), isTrue);
    expect(heir.panes, isEmpty);
    expect(heir.owner, isNull);
    expect(find.byKey(const ValueKey('terminal-pane-0')), findsNothing, reason: 'one pane again');

    await controller.split(heir, Axis.vertical);
    await settle(tester);
    expect(heir.splitAxis, Axis.vertical);
    unawaited(controller.closeTab(heir));
    await _closed(tester);
    expect(state().tabs, hasLength(1));
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  testWidgets('a tab holds at most four panes', (tester) async {
    final (_, container) = await _terminal(tester);
    final controller = container.read(terminalTabsProvider.notifier);
    final host = container.read(terminalTabsProvider).activeTab!;
    for (var i = 0; i < 3; i++) {
      expect(await controller.split(host, Axis.horizontal), isNotNull);
    }
    expect(await controller.split(host, Axis.horizontal), isNull);
    expect(host.allPanes, hasLength(TerminalTabsController.maxPanes));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('terminal-split')));
    await settle(tester);
    expect(find.text('Next pane'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  testWidgets('a host moved into a team meanwhile is followed when splitting (testing report 26.10)', (tester) async {
    final (backend, container) = await _terminal(tester);
    final controller = container.read(terminalTabsProvider.notifier);
    final tab = container.read(terminalTabsProvider).activeTab!;
    final old = tab.host!;
    // A move copies the host under a new id and deletes the original.
    Host copy(ObjectId id) => Host(
      id: id,
      name: old.name,
      address: old.address,
      createdAt: old.createdAt,
      updatedAt: old.updatedAt,
      port: old.port,
      username: old.username,
      credentialId: old.credentialId,
    );
    final moved = copy(ObjectId.generate());
    await backend.inventory.saveHost(moved);
    await backend.inventory.deleteHost(old.id);
    await settle(tester);
    final pane = await tester.runAsync(() => controller.split(tab, Axis.horizontal));
    expect(pane, isNotNull);
    expect(pane!.host!.id, moved.id);
    await _closed(tester);

    // Two equal hosts (an earlier copy and the move): this device's move decides.
    final other = copy(ObjectId.generate());
    await backend.inventory.saveHost(other);
    container.read(movedObjectsProvider.notifier).record([old.id.value], [other.id.value]);
    controller.focusPane(tab); // Split from the pane that still holds the old host.
    await settle(tester);
    final second = await tester.runAsync(() => controller.split(tab, Axis.horizontal));
    expect(second!.host!.id, other.id);
    expect(tester.takeException(), isNull);
  }, variant: _desktop);
}
