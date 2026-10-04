import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import '../rdp/fake_rdp_service.dart';

Host _host(String name, HostProtocol protocol) {
  final now = DateTime.now().toUtc();
  return Host(
    id: ObjectId.generate(),
    name: name,
    address: '${name.toLowerCase()}.example.test',
    protocol: protocol,
    username: 'demo',
    port: protocol == HostProtocol.rdp ? 3389 : 22,
    createdAt: now,
    updatedAt: now,
  );
}

Future<({MockBackend backend, FakeRdpService rdp, ProviderContainer container})> _pump(
  WidgetTester tester, {
  bool empty = false,
  AppLocale locale = AppLocale.en,
  Size size = const Size(1600, 1000),
}) async {
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile();
  if (!empty) {
    await backend.inventory.saveHost(_host('Linux', HostProtocol.ssh));
    final windows = _host('Windows', HostProtocol.rdp);
    await backend.inventory.saveHost(windows);
  }
  setTestLocale(backend, locale);
  final rdp = FakeRdpService();
  if (!empty) {
    final host = backend.inventory.currentHosts.firstWhere((h) => h.isRdp);
    rdp.savedTicket = RdpSavedHostTicket(
      hostId: host.id.value,
      name: host.name,
      options: RdpConnectionOptions(address: host.address, username: host.username!),
      certificate: RdpCertificate(
        address: host.address,
        port: 3389,
        sha256: rdp.fingerprint,
        snapshotStamp: 'synthetic-snapshot',
      ),
      hasSavedPassword: true,
    );
  }
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      retry: (_, _) => null,
      overrides: [appServicesProvider.overrideWithValue(backend.services), rdpServiceProvider.overrideWithValue(rdp)],
      child: const ConsoleCryptApp(),
    ),
  );
  await settle(tester);
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  return (backend: backend, rdp: rdp, container: container);
}

void main() {
  testWidgets('global + offers SSH and RDP without connecting until a host is chosen', (tester) async {
    final f = await _pump(tester);
    await tapKey(tester, 'new-connection');
    expect(find.byKey(const ValueKey('pick-Linux')), findsOneWidget);
    expect(find.byKey(const ValueKey('pick-Windows')), findsOneWidget);
    expect(find.descendant(of: find.byKey(const ValueKey('host-picker')), matching: find.text('SSH')), findsOneWidget);
    expect(find.descendant(of: find.byKey(const ValueKey('host-picker')), matching: find.text('RDP')), findsOneWidget);
    expect(f.container.read(terminalTabsProvider).tabs, isEmpty);
    expect(f.rdp.options, isEmpty);
    await tester.tap(find.text('Cancel'));
    await settle(tester);
    expect(f.container.read(terminalTabsProvider).tabs, isEmpty);
    await tapKey(tester, 'new-connection');
    await pickHost(tester, 'Linux');
    expect(f.container.read(routerProvider).state.matchedLocation, AppRoutes.terminal);
    expect(f.container.read(terminalTabsProvider).tabs.single.host.name, 'Linux');
    expect(f.rdp.options, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('global + opens saved RDP trust flow and routes the accepted tab to remote desktop', (tester) async {
    final f = await _pump(tester);
    await tapKey(tester, 'new-connection');
    await pickHost(tester, 'Windows');
    expect(find.byKey(const ValueKey('rdp-connect-submit')), findsOneWidget);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('rdp-address'))).controller!.text,
      'windows.example.test',
    );
    expect(f.rdp.options, isEmpty);
    expect(f.container.read(terminalTabsProvider).tabs, isEmpty);
    await tapKey(tester, 'rdp-connect-submit');
    expect(f.rdp.options, isEmpty, reason: 'certificate approval is still required');
    await tapKey(tester, 'rdp-certificate-checked');
    await tapKey(tester, 'rdp-certificate-accept');
    expect(f.container.read(routerProvider).state.matchedLocation, AppRoutes.rdp);
    expect(f.rdp.usedTicket, same(f.rdp.savedTicket));
    expect(f.container.read(rdpWorkspaceProvider).tabs.single.title, 'Windows');
    expect(f.container.read(terminalTabsProvider).tabs, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('RDP central action and tab + choose only saved RDP hosts without duplicate header action', (
    tester,
  ) async {
    final f = await _pump(tester);
    await tapKey(tester, 'nav-rdp');
    expect(find.byKey(const ValueKey('rdp-new-connection')), findsNothing);
    expect(find.byKey(const ValueKey('rdp-empty-connect')), findsOneWidget);
    await tapKey(tester, 'rdp-empty-connect');
    expect(find.byKey(const ValueKey('pick-Windows')), findsOneWidget);
    expect(find.byKey(const ValueKey('pick-Linux')), findsNothing);
    await pickHost(tester, 'Windows');
    await tapKey(tester, 'rdp-connect-submit');
    await tapKey(tester, 'rdp-certificate-checked');
    await tapKey(tester, 'rdp-certificate-accept');
    expect(find.byKey(const ValueKey('rdp-empty-connect')), findsNothing);
    expect(find.byKey(const ValueKey('rdp-new-connection')), findsNothing);
    tester.widget<GlassTabStrip>(find.byType(GlassTabStrip)).onAdd!();
    await settle(tester);
    expect(find.byKey(const ValueKey('pick-Windows')), findsOneWidget);
    expect(find.byKey(const ValueKey('pick-Linux')), findsNothing);
    expect(f.container.read(rdpWorkspaceProvider).tabs, hasLength(1));
    await tester.tap(find.text('Cancel'));
    await settle(tester);
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final locale in [AppLocale.en, AppLocale.ru]) {
    testWidgets('empty RDP picker opens a new RDP host form at compact width in ${locale.name}', (tester) async {
      final f = await _pump(tester, empty: true, locale: locale, size: const Size(1000, 800));
      await tapKey(tester, 'nav-rdp');
      await tapKey(tester, 'rdp-empty-connect');
      expect(find.byKey(const ValueKey('host-picker-new')), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tapKey(tester, 'host-picker-new');
      expect(f.container.read(routerProvider).state.matchedLocation, AppRoutes.newHost);
      expect(
        tester.widget<GlassSegmented<HostProtocol>>(find.byKey(const ValueKey('host-protocol'))).selected,
        HostProtocol.rdp,
      );
      expect(tester.widget<TextFormField>(find.byKey(const ValueKey('host-port'))).controller!.text, '3389');
      expect(find.byKey(const ValueKey('host-rdp-domain')), findsOneWidget);
      expect(find.byKey(const ValueKey('jump-mode')), findsNothing);
      expect(f.rdp.options, isEmpty);
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  testWidgets('RDP new host replaces a retained SSH draft with the requested protocol', (tester) async {
    final f = await _pump(tester, empty: true);
    f.container.read(routerProvider).go(AppRoutes.newHost);
    await settle(tester);
    expect(
      tester.widget<GlassSegmented<HostProtocol>>(find.byKey(const ValueKey('host-protocol'))).selected,
      HostProtocol.ssh,
    );
    await enterKey(tester, 'host-name', 'Unfinished SSH draft');
    await tapKey(tester, 'nav-rdp');
    await tapKey(tester, 'rdp-empty-connect');
    await tapKey(tester, 'host-picker-new');
    expect(
      tester.widget<GlassSegmented<HostProtocol>>(find.byKey(const ValueKey('host-protocol'))).selected,
      HostProtocol.rdp,
    );
    expect(tester.widget<TextFormField>(find.byKey(const ValueKey('host-name'))).controller!.text, isEmpty);
    expect(find.byKey(const ValueKey('host-rdp-domain')), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('locking the workspace dismisses the connection chooser without connecting', (tester) async {
    final f = await _pump(tester);
    await tapKey(tester, 'new-connection');
    expect(find.byKey(const ValueKey('host-picker')), findsOneWidget);
    await f.backend.services.vault.lock();
    await settle(tester);
    expect(find.byKey(const ValueKey('host-picker')), findsNothing);
    expect(f.rdp.options, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('terminal command retains SSH-only host filtering', (tester) async {
    final f = await _pump(tester);
    f.container.read(appCommandDispatcherProvider).invoke(AppCommandId.newTerminalTab);
    await settle(tester);
    expect(find.byKey(const ValueKey('pick-Linux')), findsOneWidget);
    expect(find.byKey(const ValueKey('pick-Windows')), findsNothing);
    expect(find.byKey(const ValueKey('host-picker-new')), findsNothing);
    expect(f.rdp.options, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
