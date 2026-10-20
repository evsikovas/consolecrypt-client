import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('ssh_config import creates hosts with their jump host first', (tester) async {
    final backend = testBackend(seed: false);
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    setTestLocale(backend, AppLocale.en);
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [appServicesProvider.overrideWithValue(backend.services)],
        child: const ConsoleCryptApp(),
      ),
    );
    await settle(tester);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.newHost);
    await settle(tester);
    await tapKey(tester, 'host-editor-import-ssh-config');
    expect(find.byKey(const ValueKey('ssh-config-host-nethtrap')), findsOneWidget);
    expect(find.textContaining('root@198.51.100.7:2222'), findsOneWidget);
    // Only nethtrap is chosen; its jump host comes along.
    await tester.tap(find.byKey(const ValueKey('ssh-config-host-bastion')));
    await settle(tester);
    await tapKey(tester, 'ssh-config-import-save');
    final hosts = container.read(hostsProvider).requireValue;
    final bastion = hosts.singleWhere((h) => h.name == 'bastion');
    final nethtrap = hosts.singleWhere((h) => h.name == 'nethtrap');
    expect(nethtrap.address, '198.51.100.7');
    expect(nethtrap.port, 2222);
    expect(nethtrap.username, 'root');
    expect(nethtrap.jumpChain, [bastion.id]);
    expect(find.byKey(const ValueKey('ssh-config-import')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
