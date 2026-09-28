import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

/// Token math and legibility guarantees of LIQUID_GLASS_SPEC §2 (the
/// contrast check of §6.9, ported from the spec's worst-case analysis).
void main() {
  const black = Color(0xFF000000);
  const white = Color(0xFFFFFFFF);

  final themes = {
    'light': GlassTokens.light(),
    'dark': GlassTokens.dark(),
    'light-IC': GlassTokens.lightHighContrast(),
    'dark-IC': GlassTokens.darkHighContrast(),
  };

  /// Effective colour of a glass surface over [backdrop].
  Color glassOver(GlassTokens t, GlassVariant v, Color backdrop, {GlassAppearance a = GlassAppearance.fallback}) {
    final alpha = t.tintAlpha(v, a);
    return Wcag.composite(backdrop, [t.optics.tintBase.withValues(alpha: alpha)]);
  }

  /// Worst case of §2.1: the blurred backdrop is pure black (light theme)
  /// or pure white (dark theme).
  Color worst(GlassTokens t) => t.isDark ? white : black;

  group('concentric radii (§1.5, §2.5)', () {
    test('inner = outer − inset', () {
      expect(GlassRadii.concentric(18, 8), 10); // sidebar row in the panel
      expect(GlassRadii.concentric(22, 8), 14); // palette rows
      expect(GlassRadii.concentric(12, 6, height: 26), 6); // menu items
    });

    test('falls back to rMin (6, or 4 below 20 px)', () {
      expect(GlassRadii.concentric(8, 6), 6);
      expect(GlassRadii.concentric(8, 6, height: 16), 4);
      expect(GlassRadii.concentric(4, 8, height: 30), 6);
    });

    test('capsule when height ≤ 2 × radius', () {
      expect(GlassRadii.isCapsule(36, 18), isTrue);
      expect(GlassRadii.isCapsule(28, 8), isFalse);
    });

    test('platform scales', () {
      expect(GlassRadii.macOS.panel, 18);
      expect(GlassRadii.windows.panel, 12);
      expect(GlassRadii.macOS.row, GlassRadii.concentric(GlassRadii.macOS.panel, 8));
      expect(GlassTokens.resolve(brightness: Brightness.light, platform: TargetPlatform.windows).radii.dialog, 20);
    });
  });

  group('WCAG formula', () {
    test('reference values', () {
      expect(Wcag.contrast(black, white), closeTo(21, 0.001));
      expect(Wcag.contrast(white, white), closeTo(1, 0.001));
      expect(Wcag.contrast(const Color(0xFF767676), white), closeTo(4.54, 0.01));
    });

    test('composite is source-over', () {
      expect(Wcag.composite(black, [white.withValues(alpha: 0.5)]).r, closeTo(0.5, 0.01));
    });
  });

  group('labels on glass, worst-case backdrop (§2.1)', () {
    for (final MapEntry(key: name, value: t) in themes.entries) {
      test('$name: label ≥ 4.5:1 on regular / thick / secure', () {
        for (final v in [GlassVariant.regular, GlassVariant.thick, GlassVariant.secure]) {
          final ratio = Wcag.contrast(t.palette.label, glassOver(t, v, worst(t)));
          expect(ratio, greaterThanOrEqualTo(Wcag.bodyText), reason: '$name ${v.name}: $ratio');
        }
      });
    }

    test('spec figures (non-IC)', () {
      final light = themes['light']!;
      final dark = themes['dark']!;
      double c(GlassTokens t, GlassVariant v) => Wcag.contrast(t.palette.label, glassOver(t, v, worst(t)));
      expect(c(light, GlassVariant.regular), greaterThanOrEqualTo(6.7));
      // The spec rounds 4.98 to "≥ 5.0"; still above AA.
      expect(c(dark, GlassVariant.regular), greaterThanOrEqualTo(4.95));
      expect(c(light, GlassVariant.thick), greaterThanOrEqualTo(9.85));
      expect(c(dark, GlassVariant.thick), greaterThanOrEqualTo(8.05));
      expect(c(light, GlassVariant.secure), greaterThanOrEqualTo(13));
      expect(c(dark, GlassVariant.secure), greaterThanOrEqualTo(13));
    });

    test('Tinted raises contrast; live regular keeps the token tint in Clear mode', () {
      for (final t in themes.values) {
        const tinted = GlassAppearance(mode: GlassMode.tinted);
        const clear = GlassAppearance(mode: GlassMode.clear);
        final base = Wcag.contrast(t.palette.label, glassOver(t, GlassVariant.regular, worst(t)));
        final more = Wcag.contrast(t.palette.label, glassOver(t, GlassVariant.regular, worst(t), a: tinted));
        expect(more, greaterThanOrEqualTo(base));
        final liveClear = Wcag.composite(worst(t), [
          t.optics.tintBase.withValues(alpha: t.tintAlpha(GlassVariant.regular, clear, live: true)),
        ]);
        expect(Wcag.contrast(t.palette.label, liveClear), greaterThanOrEqualTo(Wcag.bodyText));
        final thickClear = glassOver(t, GlassVariant.thick, worst(t), a: clear);
        expect(Wcag.contrast(t.palette.label, thickClear), greaterThanOrEqualTo(Wcag.bodyText));
      }
    });
  });

  group('over the ambient backdrop (§2.1, §2.4)', () {
    for (final MapEntry(key: name, value: t) in themes.entries) {
      test('$name: static chrome is highly legible, even in Clear mode', () {
        final backdrops = [
          t.ambient.base,
          for (final b in t.ambient.blobs) Wcag.composite(t.ambient.base, [b.color]),
        ];
        for (final bg in backdrops) {
          for (final mode in GlassMode.values.where((m) => m != GlassMode.solid)) {
            for (final v in [GlassVariant.thin, GlassVariant.regular]) {
              final glass = glassOver(t, v, bg, a: GlassAppearance(mode: mode));
              expect(Wcag.contrast(t.palette.label, glass), greaterThanOrEqualTo(12), reason: '$name $mode $v');
              expect(Wcag.contrast(t.secondaryLabel, glass), greaterThanOrEqualTo(Wcag.bodyText));
            }
          }
          final regular = glassOver(t, GlassVariant.regular, bg);
          expect(Wcag.contrast(t.palette.label, regular), greaterThanOrEqualTo(14));
          expect(Wcag.contrast(t.secondaryLabel, regular), greaterThanOrEqualTo(7));
        }
      });
    }
  });

  group('content, codes, accents, risk (§2.3, §2.7, §4.12, §4.13)', () {
    for (final MapEntry(key: name, value: t) in themes.entries) {
      test('$name: verification digits ≥ 7:1 on inset and paper', () {
        expect(Wcag.contrast(t.palette.label, t.surfaces.inset), greaterThanOrEqualTo(Wcag.verificationDigits));
        expect(Wcag.contrast(t.palette.label, t.surfaces.paper), greaterThanOrEqualTo(Wcag.verificationDigits));
      });

      test('$name: text roles on content surfaces', () {
        final card = Wcag.composite(t.ambient.base, [t.surfaces.content]);
        for (final surface in [card, t.surfaces.contentSolid]) {
          for (final c in [t.palette.label, t.palette.secondary, t.palette.tertiary, t.palette.accent]) {
            expect(Wcag.contrast(c, surface), greaterThanOrEqualTo(Wcag.bodyText), reason: '$name $c');
          }
        }
      });

      test('$name: fills and their labels', () {
        // Brand blue fills carry a near-black label (white on blue fails AA).
        expect(Wcag.contrast(t.palette.onAccent, t.palette.accentFill), greaterThanOrEqualTo(Wcag.bodyText));
        expect(Wcag.contrast(t.palette.onAccent, t.accentFillHover), greaterThanOrEqualTo(Wcag.bodyText));
        expect(Wcag.contrast(t.palette.onAccent, t.accentFillPressed), greaterThanOrEqualTo(Wcag.bodyText));
        expect(Wcag.contrast(t.palette.onDangerFill, t.palette.dangerFill), greaterThanOrEqualTo(Wcag.bodyText));
        final prominent = Wcag.composite(t.ambient.base, [t.prominentTint]);
        expect(Wcag.contrast(t.palette.onAccent, prominent), greaterThanOrEqualTo(Wcag.bodyText));
        final glass = glassOver(t, GlassVariant.regular, t.ambient.base);
        expect(Wcag.contrast(t.palette.accent, glass), greaterThanOrEqualTo(Wcag.bodyText));
      });

      test('$name: badge text on its α .16 background', () {
        final card = Wcag.composite(t.ambient.base, [t.surfaces.content]);
        for (final tone in [
          GlassTone.info,
          GlassTone.warning,
          GlassTone.danger,
          GlassTone.unknown,
          GlassTone.success,
        ]) {
          final bg = Wcag.composite(card, [t.palette.toneFill(tone).withValues(alpha: 0.16)]);
          final ratio = Wcag.contrast(t.palette.tone(tone), bg);
          expect(ratio, greaterThanOrEqualTo(Wcag.bodyText), reason: '$name ${tone.name}: $ratio');
        }
      });
    }

    test('brand: blue fill everywhere and cool dark ambience', () {
      for (final t in themes.values) {
        expect(t.palette.accentFill, GlassPalette.brandBlue);
      }
      expect(GlassTokens.dark().ambient.base.b, greaterThan(GlassTokens.dark().ambient.base.r));
      // Blue text only where it is legible; light mode uses a deeper blue.
      expect(GlassPalette.dark.accent, GlassPalette.brandBlue);
      expect(Wcag.contrast(GlassPalette.brandBlue, const Color(0xFFFFFFFF)), lessThan(Wcag.bodyText));
      expect(GlassPalette.light.accent, isNot(GlassPalette.brandBlue));
    });

    for (final MapEntry(key: name, value: t) in themes.entries) {
      test('$name: the brand accent cannot be mistaken for a risk colour', () {
        double hue(Color c) => HSVColor.fromColor(c).hue;
        double distance(Color a, Color b) {
          final d = (hue(a) - hue(b)).abs() % 360;
          return d > 180 ? 360 - d : d;
        }

        final p = t.palette;
        // `modifying` (warning) is amber-yellow, `destructive` red.
        expect(distance(p.accentFill, p.warningFill), greaterThanOrEqualTo(15), reason: '$name warning fill');
        expect(distance(p.accent, p.warning), greaterThanOrEqualTo(15), reason: '$name warning text');
        expect(distance(p.accentFill, p.dangerFill), greaterThanOrEqualTo(20), reason: '$name danger fill');
        expect(distance(p.accent, p.danger), greaterThanOrEqualTo(20), reason: '$name danger text');
      });
    }

    test('risk levels map to icon + colour role', () {
      expect(GlassRiskBadge.describe(RiskLevel.readOnly).$1, GlassTone.info);
      expect(GlassRiskBadge.describe(RiskLevel.modifying).$1, GlassTone.warning);
      expect(GlassRiskBadge.describe(RiskLevel.destructive).$1, GlassTone.danger);
      expect(GlassRiskBadge.describe(RiskLevel.unknown).$1, GlassTone.unknown);
    });
  });

  group('intensity table (§2.2) and IC override', () {
    final t = GlassTokens.light();

    test('Default / Clear / Tinted', () {
      const d = GlassAppearance.fallback;
      expect(t.tintAlpha(GlassVariant.regular, d), 0.66);
      expect(t.tintAlpha(GlassVariant.regular, const GlassAppearance(mode: GlassMode.clear)), closeTo(0.54, 1e-9));
      expect(t.tintAlpha(GlassVariant.thin, const GlassAppearance(mode: GlassMode.clear)), closeTo(0.40, 1e-9));
      expect(t.tintAlpha(GlassVariant.regular, const GlassAppearance(mode: GlassMode.tinted)), closeTo(0.80, 1e-9));
      expect(t.tintAlpha(GlassVariant.thick, const GlassAppearance(mode: GlassMode.tinted)), 0.94);
      expect(t.tintAlpha(GlassVariant.clear, const GlassAppearance(mode: GlassMode.tinted)), 0.24);
    });

    test('solid tier and Reduce Transparency are opaque', () {
      const solid = GlassAppearance(mode: GlassMode.solid, tier: GlassTier.solid);
      for (final v in GlassVariant.values) {
        expect(t.tintAlpha(v, solid), 1.0, reason: v.name);
      }
    });

    test('secure ignores Clear, is opaque when forced / solid / IC', () {
      expect(t.tintAlpha(GlassVariant.secure, const GlassAppearance(mode: GlassMode.clear)), 0.96);
      expect(t.tintAlpha(GlassVariant.secure, GlassAppearance.fallback, forceOpaque: true), 1.0);
      expect(t.tintAlpha(GlassVariant.secure, const GlassAppearance(increaseContrast: true)), 1.0);
    });

    test('Increase Contrast: +0.20, flat rim, no highlight or hotspot', () {
      const ic = GlassAppearance(increaseContrast: true);
      expect(t.tintAlpha(GlassVariant.regular, ic), closeTo(0.86, 1e-9));
      final r = t.resolveSurface(GlassVariant.regular, ic, interactive: true);
      expect(r.flatRim, isTrue);
      expect(r.rimColors.first, t.optics.highContrastRim);
      expect(r.highlightTop, isNull);
      expect(r.hotspot, isFalse);
      expect(GlassTokens.lightHighContrast().focusRingWidth, 3);
      expect(t.focusRingWidth, 2);
    });

    test('resolveSurface: static vs live vs solid vs refractive', () {
      const frosted = GlassAppearance.fallback;
      expect(t.resolveSurface(GlassVariant.regular, frosted).blurSigma, 0);
      expect(t.resolveSurface(GlassVariant.regular, frosted, live: true).blurSigma, 20);
      expect(t.resolveSurface(GlassVariant.thick, frosted, live: true, presence: 0.5).blurSigma, 14);
      final solid = t.resolveSurface(
        GlassVariant.regular,
        const GlassAppearance(tier: GlassTier.solid),
        live: true,
        interactive: true,
      );
      expect(solid.blurSigma, 0);
      expect(solid.tint, t.solidTint);
      expect(solid.hotspot, isFalse);
      const refractive = GlassAppearance(tier: GlassTier.refractive);
      expect(t.resolveSurface(GlassVariant.regular, refractive, live: true).refraction.isNone, isTrue);
      final lens = t.resolveSurface(GlassVariant.regular, refractive, live: true, refractionAvailable: true);
      expect(lens.refraction.bezel, 14);
      expect(lens.refraction.maxDisplacement, 7);
      expect(t.resolveSurface(GlassVariant.regular, refractive, refractionAvailable: true).refraction.isNone, isTrue);
      final secure = t.resolveSurface(GlassVariant.secure, refractive, live: true, refractionAvailable: true);
      expect(secure.blurSigma, 0);
      expect(secure.refraction.isNone, isTrue);
      expect(secure.highlightTop, isNull);
      expect(secure.flatRim, isTrue);
    });

    test('Rec.709 saturation matrix is the identity at 1', () {
      expect(glassSaturationMatrix(1), [1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0]);
      final m = glassSaturationMatrix(1.5);
      expect(m[0] + m[1] + m[2], closeTo(1, 1e-9)); // greys stay grey
    });
  });

  group('theme integration (§6.8)', () {
    test('GlassTokens is reachable as a ThemeExtension', () {
      for (final theme in [AppTheme.light(), AppTheme.dark(), AppTheme.light(highContrast: true)]) {
        expect(theme.extension<GlassTokens>(), isNotNull);
      }
      expect(AppTheme.light(highContrast: true).extension<GlassTokens>()!.highContrast, isTrue);
    });

    test('ColorScheme and typography come from the tokens', () {
      final light = AppTheme.build(Brightness.light, platform: TargetPlatform.macOS);
      expect(light.colorScheme.primary, GlassPalette.light.accent);
      expect(light.colorScheme.onSurface, GlassPalette.light.label);
      expect(light.textTheme.bodyMedium!.fontSize, 12.6);
      // SF Pro via the platform typography, nothing bundled.
      final macFamily = Typography.material2021(platform: TargetPlatform.macOS).black.bodyMedium!.fontFamily;
      expect(light.textTheme.bodyMedium!.fontFamily, macFamily);
      expect(macFamily, isNot('Roboto'));
      final win = AppTheme.build(Brightness.dark, platform: TargetPlatform.windows);
      expect(win.textTheme.bodyMedium!.fontSize, 12.6);
      expect(win.textTheme.bodyMedium!.fontFamily, GlassTypography.windowsTextFamily);
      expect(win.textTheme.headlineSmall!.fontFamily, GlassTypography.windowsDisplayFamily);
      expect(win.splashFactory, NoSplash.splashFactory);
    });

    test('terminal colours are opaque and independent of glass', () {
      expect(AppTheme.terminalTheme(Brightness.dark).background, const Color(0xFF0F1115));
      expect(AppTheme.terminalTheme(Brightness.light).background, const Color(0xFFFBFBFA));
      final dark = AppTheme.terminalTheme(Brightness.dark);
      expect(Wcag.contrast(dark.foreground, dark.background), greaterThan(14));
    });

    test('lerp between themes does not throw', () {
      final a = GlassTokens.light();
      final b = GlassTokens.darkHighContrast(platform: TargetPlatform.windows);
      expect(a.lerp(b, 0.3).palette.label, isNot(a.palette.label));
      expect(a.lerp(b, 0.7).highContrast, isTrue);
    });

    test('motion tokens', () {
      expect(GlassMotion.medium, const Duration(milliseconds: 240));
      expect(const GlassMotion(reduceMotion: true).resolve(GlassMotion.slow), GlassMotion.fast);
      expect(const GlassMotion(reduceMotion: true).allowsMovement, isFalse);
      final curve = GlassMotion.springCurve(GlassMotion.springSnappy, GlassMotion.medium);
      expect(curve.transform(1), 1);
      expect(curve.transform(0.5), greaterThan(0.5));
    });
  });
}
