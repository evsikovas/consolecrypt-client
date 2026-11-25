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
  testWidgets('checks one endpoint from this device or the selected SSH server', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final calls = <(String?, String, int, bool)>[];
    await pumpApp(
      tester,
      backend,
      size: const Size(1200, 900),
      overrides: [
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
  });
}
