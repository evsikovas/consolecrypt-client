import 'package:consolecrypt/app/capture_protection.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _FakeAffinity extends WindowCaptureAffinity {
  final calls = <bool>[];

  @override
  Future<bool> setExcluded({required bool excluded}) async {
    calls.add(excluded);
    return true;
  }
}

void main() {
  testWidgets('revealing a passphrase hides the window from capture; Settings changes the policy', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final affinity = _FakeAffinity();
    await pumpApp(tester, backend, overrides: [windowCaptureAffinityProvider.overrideWithValue(affinity)]);
    expect(affinity.calls, [false]);

    await tapKey(tester, 'nav-settings');
    final change = find.text('Change passphrase…');
    await tester.scrollUntilVisible(change, 200, scrollable: find.byType(Scrollable).last);
    await tester.tap(change);
    await settle(tester);
    expect(find.byKey(const ValueKey('change-passphrase-dialog')), findsOneWidget);
    expect(affinity.calls, [false], reason: 'obscured fields are not revealed secrets');
    await tester.tap(find.byTooltip('Show').first);
    await settle(tester);
    expect(affinity.calls, [false, true]);
    await tester.tap(find.widgetWithText(GlassButton, 'Cancel'));
    await settle(tester);
    expect(affinity.calls, [false, true, false]);

    final select = find.byKey(const ValueKey('capture-protection'));
    await tester.scrollUntilVisible(select, 200, scrollable: find.byType(Scrollable).last);
    expect(find.text('While secrets are shown'), findsOneWidget);
    expect(find.textContaining('Zoom, Teams'), findsOneWidget);
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(screenCaptureProtection: ScreenCaptureProtection.always));
    await settle(tester);
    expect(affinity.calls.last, isTrue);
    expect(find.text('Always'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('the capture setting is hidden where it is not supported', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-settings');
    expect(find.byKey(const ValueKey('capture-protection-settings'), skipOffstage: false), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
}
