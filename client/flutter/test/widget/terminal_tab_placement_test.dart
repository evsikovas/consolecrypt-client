import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/sidebar_state.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'glass_kit_test.dart' show pumpGlass;

final _desktop = TargetPlatformVariant.only(TargetPlatform.windows);

/// Unlocked demo vault with [placement], two SSH tabs, Terminal on screen.
Future<(MockBackend, ProviderContainer)> _terminal(
  WidgetTester tester, {
  TerminalTabPlacement placement = TerminalTabPlacement.top,
  AppLocale locale = AppLocale.en,
  Size size = const Size(1600, 1000),
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  final settings = backend.services.settings;
  await settings.updateLocal(
    settings.currentLocal.copyWith(terminalTabPlacement: placement, terminalTabSubtitle: TerminalTabSubtitle.address),
  );
  await pumpApp(tester, backend, locale: locale, size: size);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  final tabs = container.read(terminalTabsProvider.notifier);
  for (final name in ['raspberry-pi', 'prod-web-1']) {
    await tabs.open(backend.inventory.currentHosts.firstWhere((h) => h.name == name));
  }
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  return (backend, container);
}

LocalSettings _saved(MockBackend backend) => backend.services.settings.currentLocal;

bool _sidebarCompact(WidgetTester tester) => tester.widget<GlassSidebar>(find.byType(GlassSidebar)).compact;

Finder _row(int index, String host) => find.byKey(ValueKey('tab-$host-$index'));

Future<void> _drag(WidgetTester tester, double dx) async {
  await tester.drag(find.byKey(const ValueKey('glass-tab-column-resize')), Offset(dx, 0));
  await settle(tester);
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

void main() {
  testWidgets('top placement keeps the tab strip with the session tools in its row', (tester) async {
    await _terminal(tester);
    expect(find.byType(GlassTabStrip), findsOneWidget);
    expect(find.byType(GlassTabColumn), findsNothing);
    expect(find.byKey(const ValueKey('terminal-session-bar')), findsNothing);
    final strip = tester.getRect(find.byType(GlassTabStrip));
    final tools = tester.getRect(find.byKey(const ValueKey('terminal-snippets')));
    expect(tools.center.dy, closeTo(strip.center.dy, 1));
    expect(_sidebarCompact(tester), isFalse);
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  for (final locale in [AppLocale.en, AppLocale.ru]) {
    testWidgets(
      'left placement: tab column with subtitles, close, menu and "+", tools above the terminal (${locale.name})',
      (tester) async {
        final (backend, container) = await _terminal(tester, placement: TerminalTabPlacement.left, locale: locale);
        expect(find.byType(GlassTabStrip), findsNothing);
        final column = find.byKey(const ValueKey('terminal-tab-column'));
        expect(column, findsOneWidget);
        // Title and subtitle on separate lines inside the column.
        expect(find.descendant(of: column, matching: find.text('pi@raspberry-pi')), findsOneWidget);
        expect(find.descendant(of: column, matching: find.text('192.168.1.50')), findsOneWidget);
        expect(find.descendant(of: column, matching: find.text('10.10.20.11')), findsOneWidget);
        final first = tester.getRect(_row(0, 'raspberry-pi'));
        final second = tester.getRect(_row(1, 'prod-web-1'));
        expect(second.top, greaterThanOrEqualTo(first.bottom));
        expect(first.left, closeTo(second.left, .5));
        // "+" sits at the bottom of the column, below the tabs.
        final add = tester.getRect(find.descendant(of: column, matching: find.byKey(const ValueKey('glass-tab-add'))));
        expect(add.top, greaterThan(second.bottom));
        // Session tools stay available in the bar above the terminal card.
        final bar = tester.getRect(find.byKey(const ValueKey('terminal-session-bar')));
        final card = tester.getRect(find.byKey(const ValueKey('terminal-card')));
        expect(
          find.descendant(
            of: find.byKey(const ValueKey('terminal-session-bar')),
            matching: find.byKey(const ValueKey('terminal-snippets')),
          ),
          findsOneWidget,
        );
        expect(bar.bottom, lessThanOrEqualTo(card.top));
        expect(tester.getRect(column).right, lessThanOrEqualTo(card.left));
        expect(
          tester.getRect(column).width,
          closeTo(LocalSettings.terminalTabColumnDefault + GlassTabColumn.handleWidth, .5),
        );

        await tester.tap(_row(0, 'raspberry-pi'));
        await settle(tester);
        expect(container.read(terminalTabsProvider).activeIndex, 0);
        await tester.tap(_row(1, 'prod-web-1'), buttons: kSecondaryButton);
        await settle(tester);
        expect(find.byKey(const ValueKey('terminal-tab-rename')), findsOneWidget);
        await tester.sendKeyEvent(LogicalKeyboardKey.escape);
        await settle(tester);
        expect(find.byKey(const ValueKey('terminal-tab-rename')), findsNothing);
        final close = find.descendant(
          of: _row(1, 'prod-web-1'),
          matching: find.byTooltip(locale == AppLocale.ru ? 'Закрыть вкладку' : 'Close tab'),
        );
        await tester.tap(close);
        // Closing ends the mock session's streams on the real clock.
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 50)));
        await settle(tester);
        expect(container.read(terminalTabsProvider).tabs, hasLength(1));
        expect(_saved(backend).terminalTabPlacement, TerminalTabPlacement.left);
        expect(tester.takeException(), isNull);
      },
      variant: _desktop,
    );
  }

  testWidgets('the tab column resizes, folds to icons and remembers both after a restart', (tester) async {
    final (backend, _) = await _terminal(tester, placement: TerminalTabPlacement.left);
    final column = find.byKey(const ValueKey('terminal-tab-column'));
    await _drag(tester, 80);
    expect(_saved(backend).terminalTabColumnWidth, closeTo(LocalSettings.terminalTabColumnDefault + 80, 1));
    expect(
      tester.getRect(column).width,
      closeTo(LocalSettings.terminalTabColumnDefault + 80 + GlassTabColumn.handleWidth, 1),
    );
    await _drag(tester, 1000);
    expect(_saved(backend).terminalTabColumnWidth, LocalSettings.terminalTabColumnMax);

    await _drag(tester, -1000);
    expect(_saved(backend).terminalTabColumnCollapsed, isTrue);
    expect(
      _saved(backend).terminalTabColumnWidth,
      LocalSettings.terminalTabColumnMax,
      reason: 'width survives folding',
    );
    expect(tester.getRect(column).width, closeTo(GlassTabColumn.collapsedWidth + GlassTabColumn.handleWidth, .5));
    // Folded rows: tab numbers, full title in the tooltip.
    expect(find.descendant(of: column, matching: find.text('1')), findsOneWidget);
    expect(find.descendant(of: column, matching: find.text('2')), findsOneWidget);
    expect(find.descendant(of: column, matching: find.text('192.168.1.50')), findsNothing);
    final tooltip = tester.widget<Tooltip>(
      find
          .ancestor(
            of: find.descendant(of: _row(0, 'raspberry-pi'), matching: find.text('1')),
            matching: find.byType(Tooltip),
          )
          .first,
    );
    expect(tooltip.message, contains('raspberry-pi'));
    expect(tooltip.message, contains('192.168.1.50'));

    await tapKey(tester, 'glass-tab-column-toggle');
    expect(_saved(backend).terminalTabColumnCollapsed, isFalse);
    expect(tester.getRect(column).width, closeTo(LocalSettings.terminalTabColumnMax + GlassTabColumn.handleWidth, .5));
    await _drag(tester, -100);
    final width = _saved(backend).terminalTabColumnWidth;
    expect(width, closeTo(LocalSettings.terminalTabColumnMax - 100, 1));

    // A new start of the app shows the column at the saved width.
    await tester.pumpWidget(const SizedBox.shrink());
    await pumpApp(tester, backend);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    await container
        .read(terminalTabsProvider.notifier)
        .open(backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi'));
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(tester.getRect(column).width, closeTo(width + GlassTabColumn.handleWidth, 1));
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  testWidgets('a window without room for the column folds it and hides the expand toggle', (tester) async {
    await _terminal(tester, placement: TerminalTabPlacement.left, size: const Size(640, 700));
    final column = find.byKey(const ValueKey('terminal-tab-column'));
    expect(tester.getRect(column).width, closeTo(GlassTabColumn.collapsedWidth + GlassTabColumn.handleWidth, .5));
    expect(find.byKey(const ValueKey('glass-tab-column-toggle')), findsNothing);
    expect(find.byKey(const ValueKey('terminal-session-bar')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  testWidgets('the sidebar folds next to the tab column; the manual choice wins and is remembered', (tester) async {
    final (backend, container) = await _terminal(tester, placement: TerminalTabPlacement.left);
    expect(_sidebarCompact(tester), isTrue, reason: 'folds by default beside the tab column');
    expect(_saved(backend).sidebarCollapsed, isFalse);
    expect(_saved(backend).sidebarCollapsedBesideTabs, isNull);

    await tapKey(tester, 'toggle-sidebar');
    expect(_sidebarCompact(tester), isFalse);
    expect(_saved(backend).sidebarCollapsedBesideTabs, isFalse);

    container.read(routerProvider).go(AppRoutes.hosts);
    await settle(tester);
    expect(_sidebarCompact(tester), isFalse);
    await tapKey(tester, 'sidebar-footer-toggle');
    expect(_sidebarCompact(tester), isTrue);
    expect(_saved(backend).sidebarCollapsed, isTrue);

    // Back beside the tab column: the choice made there still applies.
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(_sidebarCompact(tester), isFalse);
    // Top placement: no tab column, the general choice applies again.
    await backend.services.settings.updateLocal(
      _saved(backend).copyWith(terminalTabPlacement: TerminalTabPlacement.top),
    );
    await settle(tester);
    expect(find.byType(GlassTabStrip), findsOneWidget);
    expect(_sidebarCompact(tester), isTrue);
    // Moving the tabs keeps the terminal panes (focus, selection, scroll).
    final pane = tester.state(find.byType(TerminalPane).first);
    await backend.services.settings.updateLocal(
      _saved(backend).copyWith(terminalTabPlacement: TerminalTabPlacement.left),
    );
    await settle(tester);
    expect(find.byType(GlassTabColumn), findsOneWidget);
    expect(tester.state(find.byType(TerminalPane).first), same(pane));
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  testWidgets('sidebar: clear toggle tooltips, Ctrl+Shift+S, state kept across restarts', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    expect(_sidebarCompact(tester), isFalse);
    String tooltip(String key) => tester.widget<GlassIconButton>(find.byKey(ValueKey(key))).tooltip;
    expect(tooltip('toggle-sidebar'), 'Collapse sidebar to icons (Ctrl+Shift+S)');
    final footer = tester.widget<GlassButton>(find.byKey(const ValueKey('sidebar-footer-toggle')));
    expect(footer.label, 'Collapse');
    expect(footer.tooltip, 'Collapse sidebar to icons (Ctrl+Shift+S)');

    await _keys(tester, [LogicalKeyboardKey.controlLeft, LogicalKeyboardKey.shiftLeft], LogicalKeyboardKey.keyS);
    expect(_sidebarCompact(tester), isTrue);
    expect(_saved(backend).sidebarCollapsed, isTrue);
    expect(tooltip('toggle-sidebar'), 'Expand sidebar (Ctrl+Shift+S)');
    expect(tooltip('sidebar-footer-toggle'), 'Expand sidebar (Ctrl+Shift+S)');

    await tester.pumpWidget(const SizedBox.shrink());
    await pumpApp(tester, backend, locale: AppLocale.ru);
    expect(_sidebarCompact(tester), isTrue, reason: 'remembered between launches');
    expect(tooltip('toggle-sidebar'), 'Развернуть боковую панель (Ctrl+Shift+S)');
    await tapKey(tester, 'sidebar-footer-toggle');
    expect(_sidebarCompact(tester), isFalse);
    expect(_saved(backend).sidebarCollapsed, isFalse);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('sidebar-footer-toggle'))).label, 'Свернуть');

    // A turned-off shortcut leaves the plain action in the tooltip.
    await backend.services.settings.updateLocal(
      _saved(backend).copyWith(shortcutOverrides: {AppCommandId.toggleSidebar.name: ''}),
    );
    await settle(tester);
    expect(tooltip('toggle-sidebar'), 'Свернуть боковую панель до значков');
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  testWidgets('placement setting appears under Terminal and Appearance and is saved', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru);
    await tapKey(tester, 'nav-settings');
    expect(find.byKey(const ValueKey('terminal-tab-placement')), findsOneWidget);
    expect(find.byKey(const ValueKey('appearance-tab-placement')), findsOneWidget);
    expect(find.text('Расположение вкладок'), findsNWidgets(2));
    await tapKey(tester, 'terminal-tab-placement-left');
    expect(_saved(backend).terminalTabPlacement, TerminalTabPlacement.left);
    expect(
      tester
          .widget<GlassSegmented<TerminalTabPlacement>>(find.byKey(const ValueKey('appearance-tab-placement')))
          .selected,
      TerminalTabPlacement.left,
    );
    await tapKey(tester, 'appearance-tab-placement-top');
    expect(_saved(backend).terminalTabPlacement, TerminalTabPlacement.top);
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  testWidgets('phones keep the tab strip and offer no placement setting', (tester) async {
    final (_, container) = await _terminal(tester, placement: TerminalTabPlacement.left, size: const Size(360, 800));
    expect(find.byType(GlassTabStrip), findsOneWidget);
    expect(find.byType(GlassTabColumn), findsNothing);
    expect(container.read(sidebarCollapsedProvider), isFalse);
    container.read(routerProvider).go(AppRoutes.settings);
    await settle(tester);
    expect(find.byKey(const ValueKey('terminal-tab-placement')), findsNothing);
    expect(find.byKey(const ValueKey('appearance-tab-placement')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  test('sidebar state: Kubernetes folding, the tab column default and manual toggles', () async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    final container = ProviderContainer(overrides: [appServicesProvider.overrideWithValue(backend.services)]);
    addTearDown(container.dispose);
    final sidebar = container.listen(sidebarCollapsedProvider, (_, _) {});
    addTearDown(sidebar.close);
    await container.read(localSettingsProvider.future);
    final notifier = container.read(sidebarCollapsedProvider.notifier);
    bool collapsed() => container.read(sidebarCollapsedProvider);
    Future<void> settled() => pumpEventQueue();

    expect(collapsed(), isFalse);
    notifier.autoCollapse();
    expect(collapsed(), isTrue);
    notifier.restore();
    expect(collapsed(), isFalse, reason: 'restores the remembered choice');

    notifier.autoCollapse();
    notifier.toggle();
    await settled();
    expect(collapsed(), isFalse, reason: 'a manual toggle wins over the automatic folding');
    expect(_saved(backend).sidebarCollapsed, isFalse);
    notifier.restore();
    expect(collapsed(), isFalse);

    notifier.setBesideTabs(true);
    expect(collapsed(), isTrue, reason: 'folds beside the tab column by default');
    notifier.toggle();
    await settled();
    expect(collapsed(), isFalse);
    expect(_saved(backend).sidebarCollapsedBesideTabs, isFalse);
    notifier.setBesideTabs(false);
    notifier.toggle();
    await settled();
    expect(collapsed(), isTrue);
    expect(_saved(backend).sidebarCollapsed, isTrue);
    notifier.setBesideTabs(true);
    expect(collapsed(), isFalse, reason: 'the choice made beside the tab column is kept for it');
  });

  group('tab column keyboard', () {
    Future<(List<int>, List<int>)> pumpColumn(
      WidgetTester tester, {
      int count = 4,
      Size size = const Size(600, 700),
    }) async {
      final selected = <int>[];
      final menus = <int>[];
      await pumpGlass(
        tester,
        Align(
          alignment: Alignment.topLeft,
          child: SizedBox(
            height: size.height - 40,
            child: GlassTabColumn(
              tabs: [
                for (var i = 0; i < count; i++)
                  GlassTab(
                    key: ValueKey('kb-$i'),
                    title: 'Server $i',
                    subtitle: '10.0.0.$i',
                    state: GlassTabState.connected,
                  ),
              ],
              activeIndex: 0,
              onSelect: selected.add,
              onClose: (_) {},
              onContextMenu: (i, _) => menus.add(i),
              onAdd: () {},
              onCollapsedChanged: (_) {},
            ),
          ),
        ),
        size: size,
      );
      await settle(tester);
      return (selected, menus);
    }

    FocusNode node(WidgetTester tester, int i) => tester
        .widget<FocusableActionDetector>(
          find.descendant(of: find.byKey(ValueKey('kb-$i')), matching: find.byType(FocusableActionDetector)).first,
        )
        .focusNode!;

    int focused(WidgetTester tester, int count) =>
        [for (var i = 0; i < count; i++) i].firstWhere((i) => node(tester, i).hasFocus, orElse: () => -1);

    testWidgets('arrows, Home/End move focus; Enter and Space switch; Shift+F10 opens the menu', (tester) async {
      final (selected, menus) = await pumpColumn(tester);
      node(tester, 0).requestFocus();
      await tester.pump();
      await _keys(tester, [], LogicalKeyboardKey.arrowDown);
      expect(focused(tester, 4), 1);
      await _keys(tester, [], LogicalKeyboardKey.arrowDown);
      await _keys(tester, [], LogicalKeyboardKey.arrowDown);
      await _keys(tester, [], LogicalKeyboardKey.arrowDown);
      expect(focused(tester, 4), 3, reason: 'stops at the last tab');
      await _keys(tester, [], LogicalKeyboardKey.home);
      expect(focused(tester, 4), 0);
      await _keys(tester, [], LogicalKeyboardKey.arrowUp);
      expect(focused(tester, 4), 0, reason: 'stops at the first tab');
      await _keys(tester, [], LogicalKeyboardKey.end);
      expect(focused(tester, 4), 3);
      await _keys(tester, [], LogicalKeyboardKey.arrowUp);
      await _keys(tester, [], LogicalKeyboardKey.enter);
      expect(selected, [2]);
      await _keys(tester, [], LogicalKeyboardKey.arrowUp);
      await _keys(tester, [], LogicalKeyboardKey.space);
      expect(selected, [2, 1]);
      expect(selected, isNot(contains(0)), reason: 'moving the focus alone never switches tabs');
      await _keys(tester, [LogicalKeyboardKey.shiftLeft], LogicalKeyboardKey.f10);
      expect(menus, [1]);
      await _keys(tester, [], LogicalKeyboardKey.contextMenu);
      expect(menus, [1, 1]);
      expect(tester.takeException(), isNull);
    }, variant: _desktop);

    testWidgets('keyboard focus scrolls a long column, also when folded', (tester) async {
      await pumpColumn(tester, count: 30, size: const Size(600, 400));
      final viewport = tester.getRect(find.byKey(const ValueKey('glass-tab-column-scroll')));
      node(tester, 0).requestFocus();
      await tester.pump();
      await _keys(tester, [], LogicalKeyboardKey.end);
      expect(focused(tester, 30), 29);
      final last = tester.getRect(find.byKey(const ValueKey('kb-29')));
      expect(last.bottom, lessThanOrEqualTo(viewport.bottom + 1));
      expect(last.top, greaterThanOrEqualTo(viewport.top - 1));
      await tapKey(tester, 'glass-tab-column-toggle');
      node(tester, 29).requestFocus();
      await tester.pump();
      await _keys(tester, [], LogicalKeyboardKey.home);
      expect(focused(tester, 30), 0);
      final first = tester.getRect(find.byKey(const ValueKey('kb-0')));
      expect(first.top, greaterThanOrEqualTo(viewport.top - 1));
      expect(find.text('30'), findsOneWidget);
      expect(tester.takeException(), isNull);
    }, variant: _desktop);
  });

  List<String> order(ProviderContainer c) => [for (final t in c.read(terminalTabsProvider).tabs) t.target.label];

  Future<void> dragOnto(WidgetTester tester, Finder from, Finder to) async {
    final gesture = await tester.startGesture(tester.getCenter(from));
    await tester.pump();
    await gesture.moveBy(const Offset(8, 8));
    await tester.pump();
    await gesture.moveTo(tester.getCenter(to));
    await tester.pump();
    await gesture.up();
    await settle(tester);
  }

  testWidgets('tabs are reordered by dragging in the strip; the active tab stays active', (tester) async {
    final (_, c) = await _terminal(tester);
    expect(order(c), ['raspberry-pi', 'prod-web-1']);
    final active = c.read(terminalTabsProvider).active;
    await dragOnto(tester, _row(1, 'prod-web-1'), _row(0, 'raspberry-pi'));
    expect(order(c), ['prod-web-1', 'raspberry-pi']);
    expect(identical(c.read(terminalTabsProvider).active, active), isTrue);
    expect(tester.takeException(), isNull);
  }, variant: _desktop);

  testWidgets('the tab column reorders by dragging and with Alt+arrows', (tester) async {
    final (_, c) = await _terminal(tester, placement: TerminalTabPlacement.left);
    await dragOnto(tester, _row(0, 'raspberry-pi'), _row(1, 'prod-web-1'));
    expect(order(c), ['prod-web-1', 'raspberry-pi']);
    // Keyboard: focus the row (as Tab / arrows would) and move it up.
    tester
        .widget<FocusableActionDetector>(
          find.descendant(of: _row(1, 'raspberry-pi'), matching: find.byType(FocusableActionDetector)).first,
        )
        .focusNode!
        .requestFocus();
    await settle(tester);
    await _keys(tester, [LogicalKeyboardKey.alt], LogicalKeyboardKey.arrowUp);
    expect(order(c), ['raspberry-pi', 'prod-web-1']);
    expect(tester.takeException(), isNull);
  }, variant: _desktop);
}
