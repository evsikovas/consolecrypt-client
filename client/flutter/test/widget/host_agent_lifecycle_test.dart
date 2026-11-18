import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/monitoring/monitoring_host_focus.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_screen.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';

class _NoProjects extends Fake implements AutomationService {
  final calls = <Symbol>[];
  @override
  dynamic noSuchMethod(Invocation invocation) {
    calls.add(invocation.memberName);
    throw StateError('Opening saved host metrics must not touch Automation');
  }
}

Future<(MockBackend, ProviderContainer, Host, _NoProjects)> _open(
  WidgetTester tester, {
  bool monitoring = true,
  bool rdp = false,
}) async {
  final backend = testBackend(seed: false), automation = _NoProjects();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Host metrics');
  final original = Host.create(name: 'Target host', address: '192.0.2.8');
  final host = await backend.inventory.saveHost(
    rdp
        ? Host(
            id: original.id,
            name: original.name,
            address: original.address,
            createdAt: original.createdAt,
            updatedAt: original.updatedAt,
            protocol: HostProtocol.rdp,
          )
        : original,
  );
  await pumpApp(
    tester,
    backend,
    size: const Size(1500, 1000),
    overrides: [
      automationServiceProvider.overrideWithValue(automation),
      // Deliberately has no AgentSetupCapability. Graph navigation must not
      // depend on installing or connecting an Agent.
      monitoringBackendProvider.overrideWithValue(monitoring ? MonitoringTestBackend() : null),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  await enterKey(tester, 'hosts-search', host.name);
  return (backend, container, host, automation);
}

Future<MonitoringHostFocus> _openHostMetrics(WidgetTester tester, ProviderContainer container, Host host) async {
  await tapKey(tester, 'host-menu-${host.name}');
  expect(find.byKey(ValueKey('host-agent-menu-${host.id.value}')), findsNothing);
  await tapKey(tester, 'host-monitoring-menu-${host.id.value}');
  expect(container.read(routerProvider).state.uri.path, AppRoutes.monitoring);
  return container.read(routerProvider).state.extra! as MonitoringHostFocus;
}

WidgetRef _monitoringRef(WidgetTester tester) => tester.element(find.byType(MonitoringScreen)) as WidgetRef;

void main() {
  testWidgets('saved SSH host opens its metrics without an Agent or Automation project', (tester) async {
    final (_, container, host, automation) = await _open(tester);
    final focus = await _openHostMetrics(tester, container, host);
    expect(focus.host.id, host.id);
    expect(focus.host.updatedAt, host.updatedAt);
    expect(focus.current(_monitoringRef(tester)), isTrue);
    expect(find.byKey(const ValueKey('monitoring-host-live')), findsOneWidget);
    expect(find.byKey(const ValueKey('monitoring-host-link-open')), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-dashboard-source')), findsNothing);
    expect(find.byKey(const ValueKey('agent-setup-wizard')), findsNothing);
    expect(container.read(terminalTabsProvider).tabs, isEmpty);
    expect(container.read(sftpControllerProvider).host, isNull);
    expect(automation.calls, isEmpty);
    expect(tester.takeException(), isNull);
  });

  for (final rdp in [false, true]) {
    testWidgets('host metrics hides for ${rdp ? 'RDP targets' : 'an unavailable monitoring session'}', (tester) async {
      final (_, _, host, automation) = await _open(tester, monitoring: rdp, rdp: rdp);
      await tapKey(tester, 'host-menu-${host.name}');
      expect(find.byKey(ValueKey('host-monitoring-menu-${host.id.value}')), findsNothing);
      expect(automation.calls, isEmpty);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('saved host edits retire original metric navigation instead of retargeting it', (tester) async {
    final (backend, container, host, automation) = await _open(tester);
    final focus = await _openHostMetrics(tester, container, host);
    final ref = _monitoringRef(tester);
    expect(focus.current(ref), isTrue);
    await backend.inventory.saveHost(host.withNotes('Changed saved target'));
    await settle(tester);
    expect(focus.current(ref), isFalse);
    expect(focus.host.updatedAt, host.updatedAt);
    expect(automation.calls, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('removed host and replaced monitoring session cannot reuse original focus', (tester) async {
    final (backend, container, host, automation) = await _open(tester);
    final focus = await _openHostMetrics(tester, container, host);
    final ref = _monitoringRef(tester);
    container.invalidate(monitoringSessionProvider);
    await settle(tester);
    expect(focus.current(ref), isFalse);
    await backend.inventory.deleteHost(host.id);
    await settle(tester);
    expect(focus.current(ref), isFalse);
    expect(automation.calls, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('locking retires host metric focus and never opens Agent setup', (tester) async {
    final (backend, container, host, automation) = await _open(tester);
    final focus = await _openHostMetrics(tester, container, host);
    final originalScope = container.read(monitoringScopeProvider);
    await backend.vault.lock();
    await settle(tester);
    expect(originalScope.current && container.read(monitoringScopeProvider).current, isFalse);
    expect(focus.host.id, host.id);
    expect(find.byKey(const ValueKey('agent-setup-wizard')), findsNothing);
    expect(automation.calls, isEmpty);
    expect(tester.takeException(), isNull);
  });

  for (final minimal in [false, true]) {
    testWidgets(
      'terminal ${minimal ? 'minimal menu' : 'toolbar'} opens active host metrics without reconnect or input',
      (tester) async {
        final (_, container, host, automation) = await _open(tester);
        final tabs = container.read(terminalTabsProvider.notifier);
        await tabs.open(host);
        final router = container.read(routerProvider);
        router.go(AppRoutes.terminal);
        if (minimal) container.read(terminalMinimalModeProvider.notifier).toggle();
        await settle(tester);
        final before = container.read(terminalTabsProvider).tabs.toList();
        final active = container.read(terminalTabsProvider).active!;
        // Leaving minimal chrome can resize empty terminal rows. Preserve the
        // actual terminal text rather than treating trailing rows as input.
        final buffer = active.terminal.buffer.getText().trimRight();
        if (minimal) {
          await tapKey(tester, 'terminal-minimal-menu');
          await tapKey(tester, 'terminal-minimal-host-metrics');
        } else {
          await tapKey(tester, 'terminal-host-metrics');
        }
        expect(router.state.uri.path, AppRoutes.monitoring);
        final focus = router.state.extra! as MonitoringHostFocus;
        expect(focus.host.id, host.id);
        expect(container.read(terminalTabsProvider).tabs, orderedEquals(before));
        expect(container.read(terminalTabsProvider).active, same(active));
        expect(active.terminal.buffer.getText().trimRight(), buffer);
        expect(container.read(sftpControllerProvider).host, isNull);
        expect(find.byKey(const ValueKey('agent-setup-wizard')), findsNothing);
        expect(automation.calls, isEmpty);
        expect(tester.takeException(), isNull);
      },
      variant: TargetPlatformVariant.only(TargetPlatform.windows),
    );
  }

  testWidgets('local terminal never inherits metric navigation from a previous SSH target', (tester) async {
    final (backend, container, host, automation) = await _open(tester);
    final tabs = container.read(terminalTabsProvider.notifier);
    await tabs.open(host);
    const localProfile = LocalShellProfile(id: 'windows:cmd', kind: 'cmd', label: 'Command Prompt', shell: 'cmd');
    backend.terminal.localShellProfiles = const LocalShellProfiles(profiles: [localProfile]);
    await tabs.openTarget(const LocalTerminalTarget(localProfile));
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(find.byKey(const ValueKey('terminal-host-metrics')), findsNothing);
    container.read(terminalMinimalModeProvider.notifier).toggle();
    await settle(tester);
    await tapKey(tester, 'terminal-minimal-menu');
    expect(find.byKey(const ValueKey('terminal-minimal-host-metrics')), findsNothing);
    expect(automation.calls, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
