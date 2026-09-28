import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

bool enabled(WidgetTester tester, String key) => isEnabled(tester, key);

void main() {
  testWidgets('add host: form, inheritance panel, saved host appears in the list', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await tapKey(tester, 'add-host');
    expect(find.text('New host'), findsWidgets);
    expect(find.byKey(const ValueKey('effective-panel')), findsOneWidget);

    // Validation: address is required.
    await enterKey(tester, 'host-name', 'grafana');
    await tapKey(tester, 'save-host');
    expect(find.text('Enter an address'), findsOneWidget);

    await enterKey(tester, 'host-address', 'grafana.example.net');
    await enterKey(tester, 'host-port', '2200');
    await enterKey(tester, 'host-username', 'ops');
    await settle(tester);
    expect(find.text('set on this host'), findsWidgets);
    await tapKey(tester, 'save-host');

    expect(find.byKey(const ValueKey('host-grafana')), findsOneWidget);
    final hosts = backend.inventory.currentHosts;
    final saved = hosts.firstWhere((h) => h.name == 'grafana');
    expect(saved.port, 2200);
    expect(saved.username, 'ops');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('run snippet: variables → confirmation (risk, host, command) → requires acknowledgment', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await tapKey(tester, 'nav-snippets');
    await tapKey(tester, 'run-Clean old compressed logs');

    // Variables form with defaults.
    expect(find.byKey(const ValueKey('var-dir')), findsOneWidget);
    await tapKey(tester, 'variables-continue');

    // No terminal open → choose a host.
    await pickHost(tester, 'raspberry-pi');

    expect(find.byKey(const ValueKey('run-confirmation')), findsOneWidget);
    expect(find.text("find /var/log -name '*.gz' -mtime +30 -delete"), findsOneWidget);
    expect(find.textContaining('raspberry-pi'), findsWidgets);
    expect(
      find.descendant(of: find.byKey(const ValueKey('run-confirmation')), matching: find.text('Destructive')),
      findsOneWidget,
    );
    expect(enabled(tester, 'run-confirm'), isFalse, reason: 'destructive needs explicit acknowledgment');

    await tapKey(tester, 'run-acknowledge');
    expect(enabled(tester, 'run-confirm'), isTrue);
    await tapKey(tester, 'run-confirm');

    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tabs = container.read(terminalTabsProvider).tabs;
    expect(tabs, hasLength(1));
    expect(tabs.single.host.name, 'raspberry-pi');
    final usage = backend.snippets.currentSnippets.firstWhere((s) => s.name == 'Clean old compressed logs');
    expect(usage.usageCount, 1);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('read-only snippet can run right after review', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await tapKey(tester, 'nav-snippets');
    await enterKey(tester, 'snippet-search', 'Disk usage');
    await settle(tester);
    await tapKey(tester, 'run-Disk usage');
    await pickHost(tester, 'raspberry-pi');
    expect(find.byKey(const ValueKey('run-acknowledge')), findsNothing);
    expect(enabled(tester, 'run-confirm'), isTrue);
    await tapKey(tester, 'run-confirm');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('device approval requires confirming that the verification codes match', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    // Pending request banner in the shell.
    expect(find.byKey(const ValueKey('pending-approval-banner')), findsOneWidget);
    await tapKey(tester, 'nav-devices');
    await tapKey(tester, 'review-request');

    expect(find.byKey(const ValueKey('approve-device-dialog')), findsOneWidget);
    for (var i = 0; i < 6; i++) {
      expect(find.byKey(ValueKey('code-group-$i')), findsOneWidget);
    }
    expect(enabled(tester, 'approve-device'), isFalse, reason: 'cannot approve before comparing codes');

    await tapKey(tester, 'codes-match');
    expect(enabled(tester, 'approve-device'), isTrue);
    await tapKey(tester, 'approve-device');

    expect(find.byKey(const ValueKey('approve-device-dialog')), findsNothing);
    expect(find.byKey(const ValueKey('pending-approval-banner')), findsNothing);
    final snapshot = backend.devices.currentSnapshot;
    expect(snapshot.pendingRequests, isEmpty);
    expect(snapshot.devices.firstWhere((d) => d.name == 'MacBook Air').trustedVaults, isNotEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('"codes don\'t match" rejects the request and warns', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await tapKey(tester, 'nav-devices');
    await tapKey(tester, 'review-request');
    await tapKey(tester, 'codes-mismatch');
    expect(find.text('Request rejected'), findsOneWidget);
    final snapshot = backend.devices.currentSnapshot;
    expect(snapshot.pendingRequests, isEmpty);
    expect(snapshot.devices.firstWhere((d) => d.name == 'MacBook Air').trustedVaults, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Ctrl+K opens the command palette on Windows; AI generates but never runs', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    expect(find.byKey(const ValueKey('command-palette')), findsNothing);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);

    await enterKey(tester, 'palette-input', 'disk space');
    await settle(tester);
    expect(find.byKey(const ValueKey('palette-snippet-Disk usage')), findsOneWidget);
    await tapKey(tester, 'palette-generate');
    await settle(tester);
    expect(find.byKey(const ValueKey('palette-ai-panel')), findsOneWidget);
    expect(find.text('df -h'), findsOneWidget);
    expect(find.byKey(const ValueKey('run-confirmation')), findsNothing, reason: 'never auto-runs');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('Cmd+K opens the command palette on macOS', (tester) async {
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.menu, (_) async => null);
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.menu, null));
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await settle(tester);
    expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
    expect(find.text('⌘K'), findsWidgets, reason: 'macOS shows Cmd shortcut labels');
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('terminal: unknown host key prompt, accept, then disconnect banner and reconnect', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);

    await enterKey(tester, 'hosts-search', 'staging-web');
    await tapKey(tester, 'connect-staging-web');
    expect(find.byKey(const ValueKey('host-key-prompt')), findsOneWidget);
    await tapKey(tester, 'accept-host-key');
    expect(find.byKey(const ValueKey('host-key-prompt')), findsNothing);

    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tab = container.read(terminalTabsProvider).active!;
    container.read(terminalTabsProvider.notifier).runInTab(tab, 'simulate-drop');
    await settle(tester);
    expect(find.byKey(const ValueKey('reconnect-banner')), findsOneWidget);
    await tapKey(tester, 'reconnect');
    expect(find.byKey(const ValueKey('reconnect-banner')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
