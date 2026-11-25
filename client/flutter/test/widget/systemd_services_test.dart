import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/hosts/systemd_services.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('SSH session tools follow the selected host and explain when systemd is unavailable', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(
      tester,
      backend,
      size: AppPlatform.isMobile ? const Size(390, 800) : const Size(1440, 900),
      overrides: [
        systemdRequestProvider.overrideWithValue((host, operation) async {
          expect(operation, {'action': 'list'});
          if (host.name == 'raspberry-pi') {
            return {'status': 0, 'output': 'pi.service loaded active running Pi', 'diagnostic': ''};
          }
          return {'status': 69, 'output': '', 'diagnostic': ''};
        }),
      ],
    );
    await enterKey(tester, 'hosts-search', 'prod-web');
    final entry = AppPlatform.isMobile ? 'terminal-tool-systemd' : 'nav-systemd';
    expect(find.byKey(ValueKey(entry)), findsNothing);
    await tapKey(tester, 'connect-prod-web-1');
    await tapKey(tester, entry);
    expect(find.text('This host does not have a running systemd system manager.'), findsOneWidget);
    expect(find.text('Restart'), findsNothing);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tabs = container.read(terminalTabsProvider.notifier);
    await tabs.open(backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi'));
    await settle(tester);
    expect(find.text('pi.service'), findsOneWidget);
    expect(find.text('This host does not have a running systemd system manager.'), findsNothing);
    container.read(routerProvider).go(AppRoutes.hosts);
    await settle(tester);
    expect(container.read(workspaceToolsProvider).selected, isNull);
    expect(find.byType(SystemServicesPanel), findsNothing);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.android, TargetPlatform.iOS}));

  testWidgets('host services open the selected journal and restart only after confirmation', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final operations = <Map<String, Object?>>[];
    var journalReads = 0;
    await pumpApp(
      tester,
      backend,
      overrides: [
        systemdRequestProvider.overrideWithValue((host, request) async {
          expect(host.name, 'prod-web-1');
          operations.add(request);
          final output = switch (request['action']) {
            'list' => 'demo.service loaded active running Example\nother.service loaded inactive dead Other unit\n',
            'journal' => 'log entry #${++journalReads}',
            'restart' => '',
            _ => throw StateError('Unexpected action'),
          };
          return {'status': 0, 'output': output, 'diagnostic': ''};
        }),
      ],
    );
    await enterKey(tester, 'hosts-search', 'prod-web');
    await tapKey(tester, 'connect-prod-web-1');
    await tapKey(tester, 'nav-systemd');
    await tester.tap(find.text('demo.service'));
    await settle(tester);
    expect(find.text('log entry #1'), findsOneWidget);
    expect(operations.last, {'action': 'journal', 'unit': 'demo.service'});

    await tester.tap(find.text('Restart'));
    await settle(tester);
    expect(find.textContaining('Restart demo.service on prod-web-1'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(operations.where((op) => op['action'] == 'restart'), isEmpty);

    await tester.tap(find.text('Restart'));
    await settle(tester);
    await tester.tap(find.text('Use sudo (passwordless)'));
    await tapKey(tester, 'confirm-ok');
    expect(operations.where((op) => op['action'] == 'restart').single, {
      'action': 'restart',
      'unit': 'demo.service',
      'sudo': true,
    });
    expect(find.text('Service restarted.'), findsOneWidget);
    expect(find.text('log entry #2'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
