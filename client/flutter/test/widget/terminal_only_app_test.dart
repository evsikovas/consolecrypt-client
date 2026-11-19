import 'package:consolecrypt/core/models/terminal.dart';
import 'package:consolecrypt/terminal/terminal_only_app.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart' show tapKey, testBackend;

void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() {
    binding.platformDispatcher.localesTestValue = const [Locale('ru')];
    addTearDown(binding.platformDispatcher.clearLocalesTestValue);
  });

  testWidgets('terminal-only starts the default shell and exposes only local shells and new windows', (tester) async {
    const defaultShell = LocalShellProfile(id: 'macos:zsh', kind: 'zsh', label: 'zsh', shell: 'zsh');
    const bash = LocalShellProfile(id: 'macos:bash', kind: 'bash', label: 'bash', shell: 'bash');
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    final service = backend.terminal..localShellProfiles = const LocalShellProfiles(profiles: [defaultShell, bash]);
    var windows = 0;
    await tester.pumpWidget(
      TerminalOnlyApp(
        service: service,
        initialShell: defaultShell,
        openWindow: () async {
          windows++;
        },
      ),
    );
    await tester.pumpAndSettle();
    expect(tester.widget<TerminalPane>(find.byType(TerminalPane)).tab.target.label, 'zsh');

    for (final (key, physicalKey) in [
      (LogicalKeyboardKey.keyK, PhysicalKeyboardKey.keyK),
      (LogicalKeyboardKey.keyT, PhysicalKeyboardKey.keyT),
      (LogicalKeyboardKey.keyN, PhysicalKeyboardKey.keyN),
      (const LogicalKeyboardKey(0x043b), PhysicalKeyboardKey.keyK), // Russian keyboard layout.
    ]) {
      final terminal = tester.widget<TerminalPane>(find.byType(TerminalPane));
      await tester.tap(find.byWidget(terminal));
      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyEvent(key, physicalKey: physicalKey);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('terminal-only-search')), findsOneWidget);
      expect(find.text('Палитра команд'), findsOneWidget);
      expect(find.byKey(const ValueKey('terminal-only-new-window')), findsOneWidget);
      expect(find.text('zsh'), findsWidgets);
      expect(find.byKey(const ValueKey('terminal-only-shell-macos:bash')), findsOneWidget);
      expect(find.text('Hosts'), findsNothing);
      await tapKey(tester, 'terminal-only-shell-macos:bash');
    }
    expect(tester.widget<TerminalPane>(find.byType(TerminalPane)).tab.target.label, 'bash');
    await tapKey(tester, 'terminal-only-palette');
    await tapKey(tester, 'terminal-only-new-window');
    expect(windows, 1);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('Ctrl shortcuts select a shell and tabs can move without reconnecting', (tester) async {
    const powershell = LocalShellProfile(
      id: 'windows:pwsh',
      kind: 'powershell',
      label: 'PowerShell',
      shell: 'powershell',
    );
    const cmd = LocalShellProfile(id: 'windows:cmd', kind: 'cmd', label: 'Command Prompt', shell: 'cmd');
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    final service = backend.terminal..localShellProfiles = const LocalShellProfiles(profiles: [powershell, cmd]);
    await tester.pumpWidget(TerminalOnlyApp(service: service, initialShell: powershell));
    await tester.pumpAndSettle();
    TerminalTab activeTab() => tester.widget<TerminalPane>(find.byType(TerminalPane)).tab;
    final sessionIds = {activeTab().sessionId};
    for (final key in [LogicalKeyboardKey.keyK, LogicalKeyboardKey.keyN, LogicalKeyboardKey.keyT]) {
      await tester.tap(find.byType(TerminalPane));
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(key);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('terminal-only-search')), findsOneWidget);
      expect(find.text('Палитра команд'), findsOneWidget);
      await tapKey(tester, 'terminal-only-shell-windows:cmd');
      expect(activeTab().target.label, 'Command Prompt');
      expect(sessionIds.add(activeTab().sessionId), isTrue);
    }
    final tab = activeTab();
    final strip = find.byKey(const ValueKey('terminal-only-tabs'));
    expect(tester.widget<ListView>(strip).scrollDirection, Axis.horizontal);
    await tapKey(tester, 'terminal-only-tab-placement');
    await tapKey(tester, 'terminal-only-tabs-left');
    expect(tester.widget<ListView>(strip).scrollDirection, Axis.vertical);
    expect(activeTab(), same(tab));
    await tapKey(tester, 'terminal-only-tab-placement');
    await tapKey(tester, 'terminal-only-tabs-top');
    expect(tester.widget<ListView>(strip).scrollDirection, Axis.horizontal);
    expect(activeTab(), same(tab));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.linux}));
}
