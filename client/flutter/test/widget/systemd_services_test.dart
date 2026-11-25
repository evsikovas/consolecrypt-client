import 'package:consolecrypt/hosts/systemd_services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart' show Size;

import '../helpers/test_app.dart';

void main() {
  testWidgets('compact host card opens services and explains when systemd is unavailable', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(
      tester,
      backend,
      size: const Size(390, 800),
      overrides: [
        systemdRequestProvider.overrideWithValue((_, operation) async {
          expect(operation, {'action': 'list'});
          return {'status': 69, 'output': '', 'diagnostic': ''};
        }),
      ],
    );
    await enterKey(tester, 'hosts-search', 'prod-web');
    await tapKey(tester, 'host-menu-prod-web-1');
    await tester.tap(find.text('System services'));
    await settle(tester);
    expect(find.text('This host does not have a running systemd system manager.'), findsOneWidget);
    expect(find.text('Restart'), findsNothing);
  });

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
    await tapKey(tester, 'inspect-prod-web-1');
    await tapKey(tester, 'host-inspector-systemd');
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
  });
}
