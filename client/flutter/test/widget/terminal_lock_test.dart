import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('an idle lock keeps open terminal tabs and their sessions (testing report 29.10)', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tabs = container.read(terminalTabsProvider.notifier);
    for (final name in ['raspberry-pi', 'prod-web-1']) {
      await tabs.open(backend.inventory.currentHosts.firstWhere((h) => h.name == name));
    }
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    final before = [for (final tab in container.read(terminalTabsProvider).tabs) tab.sessionId];
    expect(before, hasLength(2));

    await backend.vault.lock();
    await settle(tester);
    expect([for (final tab in container.read(terminalTabsProvider).tabs) tab.sessionId], before);

    await backend.vault.unlockWithPassphrase(SecretText(MockCloud.demoPassphrase));
    await settle(tester);
    final after = container.read(terminalTabsProvider);
    expect([for (final tab in after.tabs) tab.sessionId], before);
    expect(after.tabs.every((tab) => tab.isConnected), isTrue);
    expect(tester.takeException(), isNull);
  });
}
