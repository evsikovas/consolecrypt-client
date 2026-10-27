import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('streaming output in a background tab pulses it and counts in the sidebar', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    final grace = TerminalTabsController.activityGrace;
    TerminalTabsController.activityGrace = Duration.zero;
    addTearDown(() => TerminalTabsController.activityGrace = grace);
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, locale: AppLocale.ru);
    await tapKey(tester, 'nav-terminal');
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    final controller = container.read(terminalTabsProvider.notifier);
    final background = (await tester.runAsync(() async {
      final pi = (await backend.inventory.watchHosts().first).firstWhere((h) => h.name == 'raspberry-pi');
      final tab = await controller.open(pi);
      await controller.open(pi);
      return tab;
    }))!;
    await settle(tester);
    // Mock output and the attention timer run on the real clock; a loaded
    // CI host can need more than the usual two seconds.
    await tester.runAsync(() async {
      controller.runInTab(background, 'whoami');
      for (var i = 0; i < 100 && !background.attention; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
    });
    await tester.pump(const Duration(milliseconds: 300));
    expect(background.attention, isTrue);
    final nav = find.byKey(const ValueKey('nav-terminal'));
    final badges = tester.widgetList<GlassBadge>(find.descendant(of: nav, matching: find.byType(GlassBadge)));
    expect(badges.map((b) => (b.label, b.tone)), contains(('1', GlassTone.warning)));
    await tester.pumpWidget(const SizedBox.shrink());
    debugDefaultTargetPlatformOverride = null;
  });
}
