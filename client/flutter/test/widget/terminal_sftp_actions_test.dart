import 'dart:ui' as ui;

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'terminal_minimal_mode_test.dart' show pumpMinimalTerminal;

void main() {
  testWidgets('local terminal hides SFTP instead of using a previous SSH host', (tester) async {
    final (backend, container) = await pumpMinimalTerminal(tester);
    backend.terminal.localShellProfiles = const LocalShellProfiles(
      profiles: [LocalShellProfile(id: 'windows:cmd', kind: 'cmd', label: 'Command Prompt', shell: 'cmd')],
    );
    final local = await container
        .read(terminalTabsProvider.notifier)
        .openTarget(
          const LocalTerminalTarget(
            LocalShellProfile(id: 'windows:cmd', kind: 'cmd', label: 'Command Prompt', shell: 'cmd'),
          ),
        );
    container.read(terminalMinimalModeProvider.notifier).toggle();
    await settle(tester);
    expect(find.byKey(const ValueKey('terminal-sftp')), findsNothing);
    await tester.tap(
      find.byKey(ValueKey('tab-${local.target.label}-2')),
      buttons: kSecondaryMouseButton,
      kind: ui.PointerDeviceKind.mouse,
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('terminal-tab-sftp')), findsNothing);
    expect(find.byKey(const ValueKey('terminal-tab-rename')), findsOneWidget);
    expect(container.read(sftpControllerProvider).host, isNull);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final placement in TerminalTabPlacement.values) {
    for (final minimal in [false, true]) {
      testWidgets('SFTP button and tab context target the correct SSH host ($placement, minimal=$minimal)', (
        tester,
      ) async {
        final (_, container) = await pumpMinimalTerminal(tester, placement: placement);
        if (minimal) container.read(terminalMinimalModeProvider.notifier).toggle();
        await settle(tester);
        final tabs = container.read(terminalTabsProvider).tabs.toList();
        final active = container.read(terminalTabsProvider).active!;
        final button = find.byKey(const ValueKey('terminal-sftp'));
        expect(button, findsOneWidget);
        expect(
          tester.getRect(button).left,
          lessThan(tester.getRect(find.byKey(const ValueKey('terminal-minimal-toggle'))).left),
        );
        await tester.tap(button);
        await settle(tester);
        expect(container.read(sftpControllerProvider).host?.id, active.host!.id);
        expect(container.read(sftpControllerProvider).isConnected, isTrue);
        expect(container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.sftp);
        expect(container.read(terminalTabsProvider).tabs, orderedEquals(tabs));

        container.read(routerProvider).go(AppRoutes.terminal);
        await settle(tester);
        if (minimal) container.read(terminalMinimalModeProvider.notifier).toggle();
        await settle(tester);
        final inactive = tabs.first;
        await tester.tap(
          find.byKey(ValueKey('tab-${inactive.target.label}-0')),
          buttons: kSecondaryMouseButton,
          kind: ui.PointerDeviceKind.mouse,
        );
        await settle(tester);
        expect(container.read(terminalTabsProvider).active, same(active));
        await tapKey(tester, 'terminal-tab-sftp');
        expect(container.read(sftpControllerProvider).host?.id, inactive.host!.id);
        expect(container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.sftp);
        expect(container.read(terminalTabsProvider).tabs, orderedEquals(tabs));
        expect(tester.takeException(), isNull);
      }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.macOS, TargetPlatform.linux}));
    }
  }
}
