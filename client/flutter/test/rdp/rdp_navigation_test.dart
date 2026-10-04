import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_screen.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'fake_rdp_service.dart';

void main() {
  testWidgets('real shell keeps Ctrl shortcuts in RDP, releases offstage input and refocuses in one click', (
    tester,
  ) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    setTestLocale(backend, AppLocale.en);
    await backend.debugSignInDemoAndUnlock();
    final service = FakeRdpService();
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ProviderScope(
        retry: (_, _) => null,
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          rdpServiceProvider.overrideWithValue(service),
        ],
        child: const ConsoleCryptApp(),
      ),
    );
    await settle(tester);
    expect(tester.takeException(), isNull);
    await tapKey(tester, 'nav-rdp');
    final container = ProviderScope.containerOf(tester.element(find.byType(RdpScreen)));
    await container
        .read(rdpWorkspaceProvider)
        .connect(
          RdpConnectionOptions(address: 'example.test', username: 'demo'),
          Uint8List(0),
          service.fingerprint,
          isCurrent: () => true,
        );
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
    await tester.pump();
    final modifier = AppPlatform.usesMeta ? LogicalKeyboardKey.metaLeft : LogicalKeyboardKey.controlLeft;
    final physicalModifier = AppPlatform.usesMeta ? PhysicalKeyboardKey.metaLeft : PhysicalKeyboardKey.controlLeft;
    final modifierCode = AppPlatform.usesMeta ? 0x5b : 0x1d;
    for (final key in [LogicalKeyboardKey.keyK, LogicalKeyboardKey.keyC]) {
      await tester.sendKeyDownEvent(modifier, physicalKey: physicalModifier);
      expect(await tester.sendKeyEvent(key), isTrue);
      await tester.sendKeyUpEvent(modifier, physicalKey: physicalModifier);
    }
    await tester.pump();
    expect(
      service.inputs
          .expand((batch) => batch)
          .whereType<RdpScancodeInput>()
          .where((input) => input.down)
          .map((input) => input.code)
          .toList(),
      [modifierCode, 0x25, modifierCode, 0x2e],
    );
    tester.testTextInput.enterText('привет');
    await tester.pump();
    await tapKey(tester, 'nav-hosts');
    expect(FocusManager.instance.primaryFocus?.debugLabel == 'rdp-input', isFalse);
    expect(service.inputs.expand((batch) => batch).whereType<RdpReleaseAllInput>(), isNotEmpty);
    final pollCount = service.pollCount;
    await tester.pump(const Duration(seconds: 1));
    expect(service.pollCount, pollCount);
    await tapKey(tester, 'nav-rdp');
    await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
    await tester.pump();
    expect(tester.testTextInput.hasAnyClients, isTrue);
    tester.testTextInput.enterText('hello');
    await tester.pump();
    expect(service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>().map((input) => input.text).toList(), [
      'привет',
      'hello',
    ]);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
    expect(service.disconnected, ['rdp-1']);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.macOS}));
}
