import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

void main() {
  for (final style in WorkspacePanelStyle.values) {
    testWidgets(
      '${style.name}: right tools preserve the terminal, chat draft and snippet search across toggles and resizing',
      (tester) async {
        final backend = testBackend();
        addTearDown(backend.dispose);
        await backend.debugSignInDemoAndUnlock();
        await backend.services.settings.updateLocal(
          backend.services.settings.currentLocal.copyWith(workspacePanelStyle: style),
        );
        await pumpApp(tester, backend);
        await enterKey(tester, 'hosts-search', 'staging-web');
        await tapKey(tester, 'connect-staging-web');
        await tapKey(tester, 'accept-host-key');
        final terminal = tester.widget<TerminalView>(find.byType(TerminalView).first).terminal;
        final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
        await tapKey(tester, 'nav-ai');
        expect(container.read(routerProvider).state.uri.path, AppRoutes.terminal);
        expect(tester.widget<TerminalView>(find.byType(TerminalView).first).terminal, same(terminal));
        if (style == WorkspacePanelStyle.floating) {
          expect(tester.getTopLeft(find.byKey(const ValueKey('nav-ai'))).dx, greaterThan(1500));
        } else {
          expect(find.byKey(const ValueKey('workspace-tools-rail')), findsNothing);
          expect(find.byKey(const ValueKey('workspace-tools-tabs')), findsOneWidget);
          expect(tester.getTopRight(find.byKey(const ValueKey('workspace-tools-panel'))).dx, 1600);
        }
        await enterKey(tester, 'ai-input', 'draft that must stay');
        await tapKey(tester, 'nav-snippets');
        await enterKey(tester, 'snippet-search', 'Disk');
        await settle(tester);
        await tapKey(tester, 'nav-ai');
        expect(
          tester.widget<TextField>(find.byKey(const ValueKey('ai-input'))).controller!.text,
          'draft that must stay',
        );
        final pane = find.byKey(const ValueKey('workspace-tools-panel'));
        final before = tester.getSize(pane).width;
        await tester.drag(find.byKey(const ValueKey('workspace-tools-resize')), const Offset(-70, 0));
        await settle(tester);
        expect(tester.getSize(pane).width, greaterThan(before));
        await tapKey(tester, 'workspace-tools-close');
        expect(pane, findsNothing);
        await tapKey(tester, 'nav-snippets');
        expect(find.byKey(const ValueKey('run-Disk usage')), findsOneWidget);
        expect(find.byKey(const ValueKey('run-Clean old compressed logs')), findsNothing);
        await tester.sendKeyEvent(LogicalKeyboardKey.escape);
        await settle(tester);
        expect(pane, findsNothing);
        expect(container.read(terminalTabsProvider).tabs, hasLength(1));
        expect(tester.widget<TerminalView>(find.byType(TerminalView).first).terminal, same(terminal));
        expect(tester.takeException(), isNull);
      },
      variant: TargetPlatformVariant.only(TargetPlatform.windows),
    );

    testWidgets('${style.name}: compact panel overlays the workspace and both tools fit Russian at 140%', (
      tester,
    ) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      await backend.services.settings.updateLocal(
        backend.services.settings.currentLocal.copyWith(workspacePanelStyle: style),
      );
      final settings = backend.services.settings;
      await settings.updateLocal(settings.currentLocal.copyWith(uiFontScale: 1.4));
      await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
      for (final tool in ['snippets', 'ai']) {
        await tapKey(tester, 'nav-$tool');
        expect(find.byKey(const ValueKey('workspace-tools-barrier')), findsOneWidget);
        expect(find.byKey(const ValueKey('workspace-tools-resize')), findsNothing);
        expect(tester.takeException(), isNull, reason: tool);
      }
      await tapKey(tester, 'workspace-tools-close');
      expect(find.byKey(const ValueKey('workspace-tools-barrier')), findsNothing);
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('${style.name}: legacy tool links open the right panel and profile changes clear its draft', (
      tester,
    ) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      await backend.services.settings.updateLocal(
        backend.services.settings.currentLocal.copyWith(workspacePanelStyle: style),
      );
      await pumpApp(tester, backend);
      final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
      container.read(routerProvider).go(AppRoutes.ai);
      await settle(tester);
      expect(container.read(workspaceToolsProvider).selected, WorkspaceTool.ai);
      await enterKey(tester, 'ai-input', 'private draft');
      await backend.debugCreateUnlockedLocalProfile(name: 'Other');
      await settle(tester);
      expect(container.read(workspaceToolsProvider).selected, isNull);
      await tapKey(tester, 'nav-ai');
      expect(tester.widget<TextField>(find.byKey(const ValueKey('ai-input'))).controller!.text, isEmpty);
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
