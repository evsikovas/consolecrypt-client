import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('right-panel preference changes through Settings and keeps live tool drafts', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-ai');
    await enterKey(tester, 'ai-input', 'keep this draft');
    await tapKey(tester, 'nav-snippets');
    await enterKey(tester, 'snippet-search', 'Disk');
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'workspace-panel-expanded');
    expect(backend.services.settings.currentLocal.workspacePanelStyle, WorkspacePanelStyle.expanded);
    expect(find.byKey(const ValueKey('workspace-tools-tabs')), findsOneWidget);
    expect(find.byKey(const ValueKey('workspace-tools-rail')), findsNothing);
    expect(find.byKey(const ValueKey('run-Disk usage')), findsOneWidget);
    expect(find.byKey(const ValueKey('run-Clean old compressed logs')), findsNothing);
    await tapKey(tester, 'nav-ai');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('ai-input'))).controller!.text, 'keep this draft');
    // Choosing the active tab does not collapse the unified panel.
    await tapKey(tester, 'nav-ai');
    final c = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    expect(c.read(workspaceToolsProvider).selected, WorkspaceTool.ai);
    await tapKey(tester, 'workspace-panel-floating');
    expect(backend.services.settings.currentLocal.workspacePanelStyle, WorkspacePanelStyle.floating);
    expect(find.byKey(const ValueKey('workspace-tools-tabs')), findsNothing);
    expect(find.byKey(const ValueKey('workspace-tools-rail')), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('ai-input'))).controller!.text, 'keep this draft');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('custom HEX, font scale and reset apply without changing terminal preferences', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final settings = backend.services.settings;
    await settings.updateLocal(
      settings.currentLocal.copyWith(terminalColorScheme: TerminalColorScheme.ocean, terminalFontSize: 17),
    );
    await pumpApp(tester, backend, size: const Size(1024, 720), locale: AppLocale.ru);
    await tapKey(tester, 'nav-settings');
    final slider = find.byKey(const ValueKey('ui-font-scale'));
    await tester.ensureVisible(slider);
    await tester.tapAt(tester.getTopLeft(slider) + Offset(24, tester.getSize(slider).height / 2));
    await settle(tester);
    expect(settings.currentLocal.uiFontScale, lessThan(1));

    await enterKey(tester, 'ui-accent-hex', '#ZZZZZZ');
    await tapKey(tester, 'ui-accent-apply');
    expect(settings.currentLocal.uiAccentColor, isNull);
    expect(find.textContaining('Введите шесть'), findsOneWidget);
    await enterKey(tester, 'ui-accent-hex', '#7C5CFC');
    await tapKey(tester, 'ui-accent-apply');
    expect(settings.currentLocal.uiAccentColor, 0x7C5CFC);
    expect(
      GlassTokens.of(tester.element(find.byKey(const ValueKey('ui-accent-hex')))).palette.accentFill,
      const Color(0xFF7C5CFC),
    );
    await enterKey(tester, 'ui-background-hex', '16324F');
    await tapKey(tester, 'ui-background-apply');
    expect(settings.currentLocal.uiBackgroundColor, 0x16324F);
    await tapKey(tester, 'reset-ui-appearance');
    expect(settings.currentLocal.uiFontScale, 1);
    expect(settings.currentLocal.uiAccentColor, isNull);
    expect(settings.currentLocal.uiBackgroundColor, isNull);
    expect(
      GlassTokens.of(tester.element(find.byKey(const ValueKey('ui-accent-hex')))).palette.accentFill,
      GlassPalette.brandBlue,
    );
    expect(settings.currentLocal.terminalColorScheme, TerminalColorScheme.ocean);
    expect(settings.currentLocal.terminalFontSize, 17);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('terminal scheme applies to an existing session independently of interface brightness', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await enterKey(tester, 'hosts-search', 'staging-web');
    await tapKey(tester, 'connect-staging-web');
    await tapKey(tester, 'accept-host-key');
    final terminal = tester.widget<TerminalView>(find.byType(TerminalView).first).terminal;
    await tapKey(tester, 'nav-settings');
    await tapKey(tester, 'terminal-color-scheme');
    await tapKey(tester, 'terminal-scheme-ocean');
    expect(backend.services.settings.currentLocal.terminalColorScheme, TerminalColorScheme.ocean);
    final settings = backend.services.settings;
    await settings.updateLocal(
      settings.currentLocal.copyWith(themeMode: AppThemeMode.light, uiFontScale: .8, uiAccentColor: 0xFF00FF),
    );
    await settle(tester);
    await tapKey(tester, 'nav-terminal');
    final view = tester.widget<TerminalView>(find.byType(TerminalView).first);
    expect(view.terminal, same(terminal), reason: 'appearance does not reconnect or discard the terminal buffer');
    expect(
      view.theme.background,
      AppTheme.terminalTheme(Brightness.dark, scheme: TerminalColorScheme.ocean).background,
    );
    expect(view.textStyle.fontSize, 13);
    final card = tester.widget<DecoratedBox>(find.byKey(const ValueKey('terminal-card')));
    expect((card.decoration as ShapeDecoration).color, view.theme.background);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final scale in [.8, 1.2]) {
    testWidgets('settings render in Russian at 1024px with interface scale $scale', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      final settings = backend.services.settings;
      await settings.updateLocal(settings.currentLocal.copyWith(uiFontScale: scale));
      await pumpApp(tester, backend, locale: AppLocale.ru, size: const Size(1024, 720));
      await tapKey(tester, 'nav-settings');
      for (final key in [
        'ui-font-scale',
        'ui-accent-hex',
        'ui-background-hex',
        'terminal-color-scheme',
        'terminal-theme-preview',
      ]) {
        await tester.ensureVisible(find.byKey(ValueKey(key)));
        await settle(tester);
        expect(tester.takeException(), isNull, reason: key);
      }
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
