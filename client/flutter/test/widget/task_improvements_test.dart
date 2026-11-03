import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/mock/mock_team_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/hosts/host_auth_section.dart';
import 'package:consolecrypt/teams/team_models.dart';
import 'package:consolecrypt/teams/team_providers.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('host address can be selected with the mouse without opening a connection', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await enterKey(tester, 'hosts-search', 'raspberry-pi');
    final endpoint = find.descendant(
      of: find.byKey(const ValueKey('host-raspberry-pi')),
      matching: find.byType(SelectableText),
    );
    await tester.dragFrom(
      tester.getTopLeft(endpoint) + const Offset(4, 8),
      const Offset(90, 0),
      kind: PointerDeviceKind.mouse,
    );
    await settle(tester);
    final editable = find.descendant(of: endpoint, matching: find.byType(EditableText));
    expect(tester.widget<EditableText>(editable).controller.selection.isCollapsed, isFalse);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    expect(container.read(terminalTabsProvider).tabs, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  test('local shell titles shorten executable paths, preserve dynamic titles and user names', () {
    final tab = TerminalTab(
      sessionId: TerminalSessionId.generate(),
      target: const LocalTerminalTarget(
        LocalShellProfile(id: 'pwsh', kind: 'powershell', label: 'PowerShell', shell: 'pwsh.exe'),
      ),
      terminal: Terminal(),
      controller: TerminalController(),
    );
    for (final title in [r'C:\Program\ Files\PowerShell/7/pwsh.exe', r'C:\Program Files\PowerShell\7\pwsh.exe']) {
      tab.title = title;
      expect(tab.displayTitle, 'pwsh.exe');
    }
    tab.title = 'PowerShell — project';
    expect(tab.displayTitle, 'PowerShell — project');
    tab.customTitle = 'My terminal';
    expect(tab.displayTitle, 'My terminal');
  });

  testWidgets('advanced connection sections start collapsed and toggle by their heading', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'add-host');
    expect(find.byKey(const ValueKey('jump-mode')), findsNothing);
    expect(find.byKey(const ValueKey('host-key-policy')), findsNothing);
    final section = find.byWidgetPredicate((w) => w is SectionCard && w.title == 'Jump hosts');
    final heading = find.descendant(of: section, matching: find.byType(InkWell)).first;
    await tester.ensureVisible(heading);
    await tester.tap(heading);
    await settle(tester);
    expect(find.byKey(const ValueKey('jump-mode')), findsOneWidget);
    await tester.tap(heading);
    await settle(tester);
    expect(find.byKey(const ValueKey('jump-mode')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('RDP identity fields live in Authentication and the full set survives save', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'add-host');
    await tester.tap(find.text('RDP'));
    await settle(tester);
    final auth = find.byType(HostAuthSection);
    expect(find.descendant(of: auth, matching: find.byKey(const ValueKey('host-username'))), findsOneWidget);
    expect(find.descendant(of: auth, matching: find.byKey(const ValueKey('host-rdp-domain'))), findsOneWidget);
    await enterKey(tester, 'host-name', 'RDP credentials');
    await enterKey(tester, 'host-address', 'rdp.example.test');
    await enterKey(tester, 'host-username', 'alice');
    await enterKey(tester, 'host-rdp-domain', 'LAB');
    await enterKey(tester, 'host-password', 'rdp-test-password');
    await tapKey(tester, 'save-host');
    final saved = backend.inventory.currentHosts.firstWhere((h) => h.name == 'RDP credentials');
    expect(saved.username, 'alice');
    expect(saved.rdpDomain, 'LAB');
    expect(backend.cloud.activeData!.credentials.firstWhere((c) => c.id == saved.credentialId).username, r'LAB\alice');
    expect(backend.cloud.activeData!.secrets[saved.credentialId]?.expose(), 'rdp-test-password');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('shared access lists team members and only resources belonging to that team', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final teams = MockTeamService(selfEmail: 'self@example.test');
    teams.seedTeam(
      vaultId: 'ops',
      name: 'Operators',
      role: TeamRole.owner,
      members: const [TeamMember(userId: 'alice', email: 'alice@example.test', role: TeamRole.editor)],
    );
    final host = backend.inventory.currentHosts.first;
    teams.objects[host.id.value] = 'ops';
    await pumpApp(tester, backend, overrides: [teamServiceProvider.overrideWithValue(teams)]);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.sharing);
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('sharing-team-ops')));
    await settle(tester);
    expect(find.text('alice@example.test'), findsOneWidget);
    expect(find.text('Owner · 1 resource'), findsOneWidget);
    expect(find.text(host.name), findsOneWidget);
    expect(find.text(backend.inventory.currentHosts.last.name), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
