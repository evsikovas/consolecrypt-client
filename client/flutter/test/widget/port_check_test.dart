import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/port_check/port_check_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  test('parses inclusive intervals, pasted dashes and boundary values', () {
    expect(parsePortInterval(' 22 - 10000 '), (low: 22, high: 10000));
    expect(parsePortInterval('1–65535'), (low: 1, high: 65535));
    expect(parsePortInterval('443'), (low: 443, high: 443));
    for (final value in ['0', '1-65536', '90-80', '22-', '-22', '22 25', '1-2-3']) {
      expect(parsePortInterval(value), isNull, reason: value);
    }
  });

  testWidgets('checks one endpoint from this device or the selected SSH server', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final calls = <(String?, String, int, bool)>[];
    final inventories = <(String?, int, int, bool)>[];
    await pumpApp(
      tester,
      backend,
      size: const Size(1200, 900),
      overrides: [
        listeningPortsProvider.overrideWithValue(({hostId, required low, required high, required udp}) async {
          inventories.add((hostId, low, high, udp));
          return [53, 5353];
        }),
        checkPortProvider.overrideWithValue(({sourceId, required host, required port, required udp}) async {
          calls.add((sourceId, host, port, udp));
          return udp ? 'no_reply' : 'connected';
        }),
      ],
    );
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    container.read(routerProvider).go(AppRoutes.portCheck);
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('port-check-host')), '127.0.0.1');
    await tapKey(tester, 'port-check-submit');
    expect(calls.single, (null, '127.0.0.1', 443, false));
    expect(find.text('TCP connection established'), findsOneWidget);
    await tester.tap(find.text('SSH server'));
    await settle(tester);
    await tester.tap(find.text('Choose server'));
    await settle(tester);
    await tester.tap(find.descendant(of: find.byType(HostPickerDialog), matching: find.byType(ListTile)).first);
    await settle(tester);
    await tester.tap(find.text('UDP'));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('port-check-number')), '65536');
    await tapKey(tester, 'port-check-submit');
    expect(calls, hasLength(1));
    await tester.enterText(find.byKey(const ValueKey('port-check-number')), '53');
    await tapKey(tester, 'port-check-submit');
    expect(calls.last.$1, isNotNull);
    expect(calls.last, (calls.last.$1, '127.0.0.1', 53, true));
    expect(find.text('No UDP reply. An open port can also stay silent.'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('port-check-number')), '22-10000');
    await tapKey(tester, 'port-check-submit');
    expect(inventories.single, (calls.last.$1, 22, 10000, true));
    expect(find.text('5353/UDP'), findsOneWidget);
    final row = find.widgetWithText(ListTile, '5353/UDP');
    await tester.ensureVisible(row);
    await tester.tap(find.descendant(of: row, matching: find.byType(IconButton)));
    await settle(tester);
    expect(calls.last, (inventories.single.$1, '127.0.0.1', 5353, true));
    expect(find.text('53/UDP'), findsOneWidget);
  });
}
