import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/host_metrics_panel.dart';
import 'package:consolecrypt/terminal/terminal_status_bar.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('Monitoring follows the focused SSH session and closes outside the terminal', (tester) async {
    final mobile = AppPlatform.isMobile;
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, size: mobile ? const Size(360, 800) : const Size(1600, 1000));
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tabs = container.read(terminalTabsProvider.notifier);
    final router = container.read(routerProvider);
    final entryKey = mobile ? 'terminal-server-info' : 'nav-server';
    final entry = find.byKey(ValueKey(entryKey));
    final panel = find.byType(HostMetricsPanel);
    expect(entry, findsNothing);
    final host = backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi');
    final first = await tabs.open(host);
    router.go(AppRoutes.terminal);
    await settle(tester);
    expect(first.isConnected, isTrue);
    final statusBar = find.byType(TerminalStatusBar);
    expect(tester.widget<TerminalStatusBar>(statusBar).history, same(first.hostMetrics));
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(terminalMetricsBar: false));
    await settle(tester);
    expect(statusBar, findsNothing);
    await settings.updateLocal(settings.currentLocal.copyWith(terminalMetricsBar: true));
    await settle(tester);
    expect(entry.hitTestable(), findsOneWidget);
    if (mobile) {
      expect(tester.getRect(entry).left, greaterThan(tester.getRect(find.byKey(const ValueKey('glass-tab-add'))).left));
    }

    await tapKey(tester, entryKey);
    expect(tester.widget<HostMetricsPanel>(panel).history, same(first.hostMetrics));
    expect(container.read(workspaceToolsProvider).selected, WorkspaceTool.server);
    expect(router.state.uri.path, AppRoutes.terminal);
    await tapKey(tester, 'workspace-tools-close');
    expect(panel, findsNothing);

    if (!mobile) {
      final settings = backend.services.settings;
      await settings.updateLocal(settings.currentLocal.copyWith(workspacePanelStyle: WorkspacePanelStyle.expanded));
      final pane = await tabs.split(first, Axis.horizontal);
      await settle(tester);
      expect(pane!.isConnected, isTrue);
      expect(tester.widget<TerminalStatusBar>(statusBar).history, same(pane.hostMetrics));
      await tapKey(tester, entryKey);
      expect(tester.widget<HostMetricsPanel>(panel).history, same(pane.hostMetrics));
      tabs.focusPane(first);
      await settle(tester);
      expect(tester.widget<HostMetricsPanel>(panel).history, same(first.hostMetrics));
      await tapKey(tester, 'nav-server');
      expect(panel, findsNothing);
    }

    final second = await tabs.open(backend.inventory.currentHosts.firstWhere((h) => h.name == 'prod-web-1'));
    await settle(tester);
    expect(second.isConnected, isTrue);
    await tapKey(tester, entryKey);
    expect(tester.widget<HostMetricsPanel>(panel).history, same(second.hostMetrics));
    tabs.activate(0);
    await settle(tester);
    expect(tester.widget<HostMetricsPanel>(panel).history, same(first.hostMetrics));
    router.go(AppRoutes.hosts);
    await settle(tester);
    expect(container.read(workspaceToolsProvider).selected, isNull);
    expect(panel, findsNothing);
    expect(entry, findsNothing);
    if (!mobile) {
      router.go(AppRoutes.terminal);
      await settle(tester);
      await tapKey(tester, entryKey);
      const cmd = LocalShellProfile(id: 'windows:cmd', kind: 'cmd', label: 'Command Prompt', shell: 'cmd');
      backend.terminal.localShellProfiles = const LocalShellProfiles(profiles: [cmd]);
      await tabs.openTarget(const LocalTerminalTarget(cmd));
      await settle(tester);
      expect(container.read(workspaceToolsProvider).selected, isNull);
      expect(entry, findsNothing);
      expect(panel, findsNothing);
    }
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.android}));
}
