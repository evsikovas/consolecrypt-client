import 'dart:async';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('actual settings recover from performance fallback and paint four distinct modes', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    setTestLocale(backend, AppLocale.ru);
    tester.view.physicalSize = const Size(1440, 960);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    late VoidCallback slowGlass;
    late GlassFrameGuard guard;
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          glassFrameGuardFactoryProvider.overrideWithValue((onStepDown) {
            slowGlass = onStepDown;
            return guard = GlassFrameGuard(onStepDown: onStepDown);
          }),
        ],
        child: const ConsoleCryptApp(),
      ),
    );
    await settle(tester);
    await tapKey(tester, 'nav-settings');
    GlassScopeData scope() => GlassScope.of(tester.element(find.byType(AppCommandsScope)));
    double sidebarAlpha() => tester
        .widgetList<CustomPaint>(
          find.descendant(of: find.byKey(const ValueKey('glass-sidebar')), matching: find.byType(CustomPaint)),
        )
        .map((paint) => paint.painter)
        .whereType<GlassFillPainter>()
        .first
        .resolved
        .tint
        .a;
    Future<void> degrade() async {
      for (var i = 0; i < 2 && !scope().appearance.isSolid; i++) {
        slowGlass();
        await settle(tester);
      }
      expect(scope().solidReason, GlassSolidReason.performance);
      expect(sidebarAlpha(), 1);
    }

    // Streaming terminal frames without live blur cannot trip the guard.
    await tapKey(tester, 'nav-terminal');
    guard.addSamples(List.filled(1000, const Duration(milliseconds: 100)));
    await settle(tester);
    expect(scope().solidReason, isNull);
    await tapKey(tester, 'nav-settings');
    await degrade();
    await tapKey(tester, 'glass-mode-clear');
    expect(scope().solidReason, isNull);
    expect(find.byKey(const ValueKey('glass-retry-effects')), findsNothing);
    final clear = sidebarAlpha();
    await tapKey(tester, 'glass-mode-default');
    final standard = sidebarAlpha();
    await tapKey(tester, 'glass-mode-tinted');
    final tinted = sidebarAlpha();
    await tapKey(tester, 'glass-mode-solid');
    expect(clear, lessThan(standard));
    expect(standard, lessThan(tinted));
    expect(tinted, lessThan(sidebarAlpha()));
    expect(sidebarAlpha(), 1);

    await tapKey(tester, 'glass-mode-tinted');
    await degrade();
    await tapKey(tester, 'glass-mode-tinted');
    expect(scope().solidReason, isNull, reason: 're-selecting the same mode retries glass');
    await degrade();
    await tapKey(tester, 'glass-retry-effects');
    expect(scope().solidReason, isNull);
    expect(backend.settings.currentLocal.glassMode, GlassMode.tinted);
    expect(sidebarAlpha(), tinted);

    // With live glass mounted, sustained slow raster work is still guarded.
    final context = tester.element(find.byKey(const ValueKey('glass-mode')));
    unawaited(
      showAppDialog<void>(
        context,
        builder: (_) => const GlassDialog(title: 'Preview', content: Text('Preview')),
      ),
    );
    await settle(tester);
    guard.addSamples(List.filled(270, const Duration(milliseconds: 20)));
    await settle(tester);
    if (!scope().appearance.isSolid) {
      guard.addSamples(List.filled(270, const Duration(milliseconds: 20)));
      await settle(tester);
    }
    expect(scope().solidReason, GlassSolidReason.performance);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    await tapKey(tester, 'glass-retry-effects');
    expect(scope().solidReason, isNull);

    // Explicit recovery never overrides the OS's Reduce Transparency.
    await degrade();
    final retry = scope().retryEffects!;
    await tester.binding.defaultBinaryMessenger.handlePlatformMessage(
      AccessibilityBridge.channelName,
      const StandardMethodCodec().encodeMethodCall(const MethodCall('signalsChanged', {'reduceTransparency': true})),
      (_) {},
    );
    await settle(tester);
    retry();
    await settle(tester);
    expect(scope().solidReason, GlassSolidReason.reduceTransparency);
    expect(sidebarAlpha(), 1);
    expect(find.byKey(const ValueKey('glass-retry-effects')), findsNothing);
    await tapKey(tester, 'glass-mode-clear');
    expect(scope().solidReason, GlassSolidReason.reduceTransparency);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
