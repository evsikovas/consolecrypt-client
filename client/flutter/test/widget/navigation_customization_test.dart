import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/navigation_customization.dart';
import 'package:consolecrypt/app/navigation_registry.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'terminal_minimal_mode_test.dart' show pumpMinimalTerminal;

void main() {
  testWidgets(
    'secondary click customizes the sidebar without closing terminal state; all sections survive empty pins',
    (tester) async {
      final (_, container) = await pumpMinimalTerminal(tester);
      final tabs = container.read(terminalTabsProvider).tabs.toList();
      final pane = tester.state(find.byType(TerminalPane).first);
      final hosts = find.byKey(const ValueKey('nav-hosts'));
      if (defaultTargetPlatform == TargetPlatform.macOS) {
        await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
        await tester.tap(hosts);
        await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      } else {
        await tester.tap(hosts, buttons: kSecondaryButton);
      }
      await settle(tester);
      expect(container.read(routerProvider).state.matchedLocation, '/terminal');
      await tester.tap(find.text('Настроить панель').last);
      await settle(tester);
      final terminal = find.byKey(const ValueKey('pin-nav-terminal'));
      await tester.ensureVisible(terminal);
      await tester.tap(terminal);
      await settle(tester);
      expect(find.byKey(const ValueKey('nav-terminal')), findsNothing);
      expect(container.read(terminalTabsProvider).tabs, orderedEquals(tabs));
      expect(tester.state(find.byType(TerminalPane).first), same(pane));
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await settle(tester);
      final profile = container.read(activeProfileProvider)!.id.value;
      for (final section in navigationSections) {
        await container.read(navigationPreferencesControllerProvider).setVisible(profile, section.id, false);
      }
      await settle(tester);
      expect(find.byKey(const ValueKey('nav-hosts')), findsNothing);
      await tapKey(tester, 'nav-all-sections');
      expect(find.byKey(const ValueKey('all-nav-terminal')), findsOneWidget);
      await tester.ensureVisible(find.byKey(const ValueKey('all-nav-terminal')));
      await tapKey(tester, 'all-nav-terminal');
      expect(container.read(terminalTabsProvider).tabs, orderedEquals(tabs));
      expect(tester.state(find.byType(TerminalPane).first), same(pane));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await settle(tester);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.macOS}),
  );

  testWidgets(
    'preferences are profile-local; stale dialog writes are rejected and defaults restore only current profile',
    (tester) async {
      final (backend, container) = await pumpMinimalTerminal(tester);
      final original = container.read(activeProfileProvider)!.id.value;
      await container.read(navigationPreferencesControllerProvider).setVisible(original, 'docker', false);
      await settle(tester);
      final saved = backend.services.settings.currentLocal.navigationByProfile[original];
      final context = tester.element(find.byKey(const ValueKey('nav-all-sections')));
      final dialog = showNavigationCustomization(context);
      await settle(tester);
      await backend.debugCreateUnlockedLocalProfile(name: 'Other profile');
      await settle(tester);
      await dialog;
      final other = container.read(activeProfileProvider)!.id.value;
      expect(other, isNot(original));
      expect(find.byKey(const ValueKey('navigation-customization')), findsNothing);
      await container.read(navigationPreferencesControllerProvider).setVisible(original, 'docker', true);
      expect(backend.services.settings.currentLocal.navigationByProfile[original], same(saved));
      expect(container.read(navigationPreferencesProvider), isNull);
      await container.read(navigationPreferencesControllerProvider).setVisible(other, 'hosts', false);
      await container.read(navigationPreferencesControllerProvider).restore(other);
      await settle(tester);
      expect(container.read(navigationPreferencesProvider)!.visible('hosts'), isTrue);
      expect(backend.services.settings.currentLocal.navigationByProfile[original]!.visible('docker'), isFalse);
      expect(tester.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets('dragging sections orders the sidebar and persists until reset', (tester) async {
    final (backend, container) = await pumpMinimalTerminal(tester);
    final profile = container.read(activeProfileProvider)!.id.value;
    final dialog = showNavigationCustomization(tester.element(find.byKey(const ValueKey('nav-all-sections'))));
    await settle(tester);
    final hosts = find.byKey(const ValueKey('order-nav-hosts'));
    final original = tester.getTopLeft(hosts);
    final credentials = find.byKey(const ValueKey('drag-nav-credentials'));
    final drag = await tester.startGesture(tester.getCenter(credentials));
    await drag.moveBy(const Offset(0, -18));
    await tester.pump();
    await drag.moveTo(original + const Offset(16, 26));
    await tester.pump(const Duration(milliseconds: 300));
    await drag.up();
    await settle(tester);
    final prefs = backend.services.settings.currentLocal.navigationByProfile[profile]!;
    expect(prefs.order.first, 'credentials');
    final sidebarCredentials = find.byKey(const ValueKey('nav-credentials'));
    final sidebarHosts = find.byKey(const ValueKey('nav-hosts'));
    expect(tester.getTopLeft(sidebarCredentials).dy, lessThan(tester.getTopLeft(sidebarHosts).dy));
    await tapKey(tester, 'navigation-restore-defaults');
    expect(backend.services.settings.currentLocal.navigationByProfile[profile]!.order, isEmpty);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    await dialog;
    expect(tester.getTopLeft(sidebarHosts).dy, lessThan(tester.getTopLeft(sidebarCredentials).dy));
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('narrow sidebar dialog remains scrollable and keyboard-accessible', (tester) async {
    final (_, container) = await pumpMinimalTerminal(tester, size: const Size(720, 640));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyB);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(container.read(appCommandsProvider).any((c) => c.id == AppCommandId.customizeSidebar), isTrue);
    expect(find.byKey(const ValueKey('navigation-customization')), findsOneWidget);
    final settings = find.byKey(const ValueKey('pin-nav-settings'));
    await tester.scrollUntilVisible(
      settings,
      160,
      scrollable: find
          .descendant(of: find.byKey(const ValueKey('navigation-customization')), matching: find.byType(Scrollable))
          .last,
    );
    expect(find.byKey(const ValueKey('navigation-restore-defaults')), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.linux));

  testWidgets('touch navigation remains reachable when every primary section is hidden', (tester) async {
    final (_, container) = await pumpMinimalTerminal(tester, size: const Size(430, 900));
    final profile = container.read(activeProfileProvider)!.id.value;
    final tabs = container.read(terminalTabsProvider).tabs.toList();
    for (final section in navigationSections) {
      await container.read(navigationPreferencesControllerProvider).setVisible(profile, section.id, false);
    }
    await settle(tester);
    await tapKey(tester, 'mobile-all-sections-only');
    await tapKey(tester, 'mobile-customize-navigation');
    expect(find.byKey(const ValueKey('navigation-customization')), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    await tapKey(tester, 'mobile-nav-hosts');
    expect(container.read(routerProvider).state.matchedLocation, '/hosts');
    expect(container.read(terminalTabsProvider).tabs, orderedEquals(tabs));
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.iOS}));
}
