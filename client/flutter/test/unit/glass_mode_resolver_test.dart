import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';

/// Effective glass = Glass setting × OS accessibility × platform × frame
/// guard (LIQUID_GLASS_SPEC §2.10).
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('resolveEffectiveGlass', () {
    test('defaults: frosted, refractive only on macOS with shader filters', () {
      expect(resolveEffectiveGlass(mode: GlassMode.standard).appearance.tier, GlassTier.frosted);
      expect(
        resolveEffectiveGlass(mode: GlassMode.standard, shaderFilterSupported: true).appearance.tier,
        GlassTier.refractive,
      );
      expect(
        resolveEffectiveGlass(
          mode: GlassMode.standard,
          platform: TargetPlatform.windows,
          shaderFilterSupported: true,
        ).appearance.tier,
        GlassTier.frosted,
      );
    });

    test('Glass = Solid → solid tier (reason: setting)', () {
      final e = resolveEffectiveGlass(mode: GlassMode.solid, shaderFilterSupported: true);
      expect(e.appearance.tier, GlassTier.solid);
      expect(e.solidReason, GlassSolidReason.setting);
      expect(e.appearance.mode, GlassMode.solid);
    });

    test('Reduce Transparency wins over every setting', () {
      for (final mode in GlassMode.values) {
        final e = resolveEffectiveGlass(
          mode: mode,
          os: const OsAccessibilitySignals(reduceTransparency: true),
          shaderFilterSupported: true,
        );
        expect(e.appearance.tier, GlassTier.solid, reason: mode.name);
        expect(e.solidReason, GlassSolidReason.reduceTransparency);
        expect(e.appearance.reduceTransparency, isTrue);
      }
    });

    test('Remote Desktop and battery saver → solid', () {
      expect(
        resolveEffectiveGlass(
          mode: GlassMode.standard,
          os: const OsAccessibilitySignals(remoteSession: true),
        ).solidReason,
        GlassSolidReason.remoteSession,
      );
      expect(
        resolveEffectiveGlass(mode: GlassMode.tinted, os: const OsAccessibilitySignals(batterySaver: true)).solidReason,
        GlassSolidReason.batterySaver,
      );
    });

    test('Increase Contrast from the OS bridge or MediaQuery; tier unchanged', () {
      final os = resolveEffectiveGlass(mode: GlassMode.clear, os: const OsAccessibilitySignals(increaseContrast: true));
      expect(os.appearance.increaseContrast, isTrue);
      expect(os.appearance.tier, GlassTier.frosted);
      expect(resolveEffectiveGlass(mode: GlassMode.clear, mediaHighContrast: true).appearance.increaseContrast, isTrue);
    });

    test('Reduce Motion disables refraction but keeps glass', () {
      final e = resolveEffectiveGlass(
        mode: GlassMode.standard,
        os: const OsAccessibilitySignals(reduceMotion: true),
        shaderFilterSupported: true,
      );
      expect(e.appearance.reduceMotion, isTrue);
      expect(e.appearance.tier, GlassTier.frosted);
      expect(
        resolveEffectiveGlass(mode: GlassMode.standard, mediaDisableAnimations: true).appearance.reduceMotion,
        isTrue,
      );
    });

    test('frame guard caps the tier', () {
      expect(
        resolveEffectiveGlass(
          mode: GlassMode.standard,
          shaderFilterSupported: true,
          performanceCap: GlassTier.frosted,
        ).appearance.tier,
        GlassTier.frosted,
      );
      final solid = resolveEffectiveGlass(mode: GlassMode.standard, performanceCap: GlassTier.solid);
      expect(solid.appearance.tier, GlassTier.solid);
      expect(solid.solidReason, GlassSolidReason.performance);
    });

    test('full matrix: settings × OS signals keep the invariants', () {
      const flags = [false, true];
      for (final mode in GlassMode.values) {
        for (final rt in flags) {
          for (final ic in flags) {
            for (final rm in flags) {
              for (final remote in flags) {
                for (final platform in [TargetPlatform.macOS, TargetPlatform.windows]) {
                  final e = resolveEffectiveGlass(
                    mode: mode,
                    os: OsAccessibilitySignals(
                      reduceTransparency: rt,
                      increaseContrast: ic,
                      reduceMotion: rm,
                      remoteSession: remote,
                    ),
                    platform: platform,
                    shaderFilterSupported: true,
                  );
                  final a = e.appearance;
                  final solid = rt || mode == GlassMode.solid || remote;
                  expect(a.tier == GlassTier.solid, solid, reason: '$mode rt=$rt remote=$remote');
                  expect(e.solidReason != null, solid);
                  expect(a.increaseContrast, ic);
                  expect(a.reduceMotion, rm);
                  expect(a.mode, mode);
                  final refractive = !solid && !rm && platform == TargetPlatform.macOS;
                  expect(a.tier == GlassTier.refractive, refractive);
                }
              }
            }
          }
        }
      }
    });
  });

  group('OsAccessibilitySignals', () {
    test('map round trip and tolerance', () {
      const s = OsAccessibilitySignals(reduceTransparency: true, batterySaver: true);
      expect(OsAccessibilitySignals.fromMap(s.toMap()), s);
      expect(OsAccessibilitySignals.fromMap(null), OsAccessibilitySignals.none);
      expect(OsAccessibilitySignals.fromMap({'reduceMotion': 'yes', 7: true}), OsAccessibilitySignals.none);
    });
  });

  group('GlassMode setting', () {
    test('defaults to Default and round-trips its wire name', () {
      expect(const LocalSettings().glassMode, GlassMode.standard);
      expect(const LocalSettings().copyWith(glassMode: GlassMode.tinted).glassMode, GlassMode.tinted);
      expect(
        const LocalSettings(glassMode: GlassMode.clear).copyWith(themeMode: AppThemeMode.dark).glassMode,
        GlassMode.clear,
      );
      for (final m in GlassMode.values) {
        expect(GlassMode.fromWire(m.wireName), m);
      }
      expect(GlassMode.standard.wireName, 'default');
      expect(GlassMode.fromWire('bogus'), GlassMode.standard);
    });
  });

  group('GlassBackdropBudget (§6.6)', () {
    test('one chrome and one overlay slot; refused requests are tracked', () {
      final budget = GlassBackdropBudget();
      final a = Object();
      final b = Object();
      final menu = Object();
      final submenu = Object();
      expect(budget.acquire(a, GlassBackdropKind.chrome), isTrue);
      expect(budget.acquire(a, GlassBackdropKind.chrome), isTrue, reason: 'idempotent');
      expect(budget.acquire(b, GlassBackdropKind.chrome), isFalse);
      expect(budget.acquire(menu, GlassBackdropKind.overlay), isTrue);
      expect(budget.acquire(submenu, GlassBackdropKind.overlay), isFalse);
      expect(budget.current.live, 2);
      expect(budget.current.denied, 2);
      budget.release(a);
      expect(budget.acquire(b, GlassBackdropKind.chrome), isTrue);
      expect(budget.current.chromeLive, 1);
      expect(budget.current.denied, 1);
    });

    test('suppression: chrome-only vs everything', () {
      final budget = GlassBackdropBudget();
      final terminal = Object();
      final approval = Object();
      budget.suppress(terminal, includeOverlays: false);
      expect(budget.current.suppression, GlassSuppression.chrome);
      budget.suppress(approval, includeOverlays: true);
      expect(budget.current.suppression, GlassSuppression.all);
      budget.unsuppress(approval);
      expect(budget.current.suppression, GlassSuppression.chrome);
      budget.unsuppress(terminal);
      expect(budget.current.suppression, GlassSuppression.none);
    });
  });

  group('nested budget', () {
    testWidgets('inherits the parent suppression', (tester) async {
      final app = GlassBackdropBudget();
      final demo = GlassBackdropBudget(maxChrome: 8, parent: app);
      addTearDown(demo.dispose);
      final approval = Object();
      app.suppress(approval, includeOverlays: true);
      await tester.pump();
      expect(demo.suppression.value, GlassSuppression.all);
      app.unsuppress(approval);
      await tester.pump();
      expect(demo.suppression.value, GlassSuppression.none);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  });

  group('GlassFrameGuard (§6.6)', () {
    test('p95 nearest rank', () {
      final samples = [for (var i = 1; i <= 100; i++) Duration(milliseconds: i)];
      expect(GlassFrameGuard.percentile95(samples), const Duration(milliseconds: 95));
      expect(GlassFrameGuard.percentile95(const []), Duration.zero);
    });

    test('warm-up and one slow window do not disable glass; sustained slow blur does', () {
      var steps = 0;
      final guard = GlassFrameGuard(onStepDown: () => steps++);
      guard.addSamples(List.filled(30, const Duration(milliseconds: 100)));
      guard.addSamples(List.filled(120, const Duration(milliseconds: 20)));
      expect(steps, 0);
      guard.addSamples(List.filled(119, const Duration(milliseconds: 20)));
      expect(steps, 0);
      guard.addSamples([const Duration(milliseconds: 20)]);
      expect(steps, 1);
    });

    test('healthy windows break the slow streak and reset clears old samples', () {
      var steps = 0;
      final guard = GlassFrameGuard(onStepDown: () => steps++, warmupFrames: 0);
      guard.addSamples(List.filled(120, const Duration(milliseconds: 20)));
      guard.addSamples([
        ...List.filled(114, const Duration(milliseconds: 6)),
        ...List.filled(6, const Duration(milliseconds: 30)),
      ]);
      guard.addSamples(List.filled(120, const Duration(milliseconds: 20)));
      expect(steps, 0, reason: 'p95 = 6 ms in the middle window resets the streak');
      guard.reset();
      guard.addSamples(List.filled(120, const Duration(milliseconds: 20)));
      expect(steps, 0, reason: 'samples before reset do not count');
      guard.addSamples(List.filled(120, const Duration(milliseconds: 20)));
      expect(steps, 1);
    });

    test('disabled measurement discards unrelated frames and partial windows', () {
      var steps = 0;
      final guard = GlassFrameGuard(onStepDown: () => steps++, warmupFrames: 0);
      guard.addSamples(List.filled(239, const Duration(milliseconds: 20)));
      guard.setEnabled(false);
      guard.addSamples(List.filled(1000, const Duration(milliseconds: 100)));
      guard.setEnabled(true);
      guard.addSamples([const Duration(milliseconds: 20)]);
      expect(steps, 0);
    });

    test('one timing batch can only cause one downgrade', () {
      var steps = 0;
      final guard = GlassFrameGuard(onStepDown: () => steps++);
      guard.addSamples(List.filled(2000, const Duration(milliseconds: 20)));
      expect(steps, 1);
    });
  });
}
