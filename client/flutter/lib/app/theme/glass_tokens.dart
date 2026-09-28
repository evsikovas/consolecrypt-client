import 'dart:math' as math;

import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_motion.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:consolecrypt/app/theme/glass_typography.dart';
import 'package:consolecrypt/core/models/glass_settings.dart';
import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';

/// Effective, app-wide glass appearance: the in-app [GlassMode] merged with
/// OS accessibility signals and performance guards. Produced by
/// `resolveEffectiveGlass` and published by `GlassScope`.
@immutable
final class GlassAppearance {
  const GlassAppearance({
    this.mode = GlassMode.standard,
    this.tier = GlassTier.frosted,
    this.increaseContrast = false,
    this.reduceMotion = false,
    this.reduceTransparency = false,
  });

  /// The user's setting (Solid is also reflected in [tier]).
  final GlassMode mode;
  final GlassTier tier;
  final bool increaseContrast;
  final bool reduceMotion;
  final bool reduceTransparency;

  static const fallback = GlassAppearance();

  bool get isSolid => tier == GlassTier.solid;

  GlassAppearance copyWith({
    GlassMode? mode,
    GlassTier? tier,
    bool? increaseContrast,
    bool? reduceMotion,
    bool? reduceTransparency,
  }) => GlassAppearance(
    mode: mode ?? this.mode,
    tier: tier ?? this.tier,
    increaseContrast: increaseContrast ?? this.increaseContrast,
    reduceMotion: reduceMotion ?? this.reduceMotion,
    reduceTransparency: reduceTransparency ?? this.reduceTransparency,
  );

  @override
  bool operator ==(Object other) =>
      other is GlassAppearance &&
      other.mode == mode &&
      other.tier == tier &&
      other.increaseContrast == increaseContrast &&
      other.reduceMotion == reduceMotion &&
      other.reduceTransparency == reduceTransparency;

  @override
  int get hashCode => Object.hash(mode, tier, increaseContrast, reduceMotion, reduceTransparency);

  @override
  String toString() =>
      'GlassAppearance(${mode.name}, ${tier.name}${increaseContrast ? ', IC' : ''}'
      '${reduceMotion ? ', RM' : ''}${reduceTransparency ? ', RT' : ''})';
}

/// Everything a glass painter needs for one surface, after applying the
/// mode deltas, accessibility overrides and the tier (§2.2, §2.10).
@immutable
final class ResolvedGlass {
  const ResolvedGlass({
    required this.variant,
    required this.tint,
    required this.blurSigma,
    required this.saturation,
    required this.shadows,
    required this.rimColors,
    required this.rimAngleDegrees,
    required this.darkEdge,
    required this.highlightTop,
    required this.highlightHeight,
    required this.hotspot,
    required this.hotspotColor,
    required this.hotspotPressedColor,
    required this.hotspotRadius,
    required this.refraction,
  });

  final GlassVariant variant;

  /// Tint fill (colour + effective alpha).
  final Color tint;

  /// > 0 only for live, non-solid, non-secure surfaces.
  final double blurSigma;
  final double saturation;
  final List<BoxShadow> shadows;

  /// Rim colours at [GlassOptics.rimStopPositions]; all three equal for a
  /// flat rim (secure / Increase Contrast).
  final List<Color> rimColors;
  final double rimAngleDegrees;
  final Color darkEdge;

  /// `null` = no inner highlight (secure, Increase Contrast).
  final Color? highlightTop;
  final double highlightHeight;

  /// Whether the pointer hotspot may be painted.
  final bool hotspot;
  final Color hotspotColor;
  final Color hotspotPressedColor;
  final double hotspotRadius;

  /// [GlassRefraction.none] unless the refractive tier applies.
  final GlassRefraction refraction;

  bool get isLive => blurSigma > 0;

  bool get flatRim => rimColors.every((c) => c == rimColors.first);
}

/// Design tokens of the Liquid Glass kit (LIQUID_GLASS_SPEC §2), exposed as
/// a [ThemeExtension]: `GlassTokens.of(context)`.
///
/// Four instances per platform: light, dark, light-IC and dark-IC.
@immutable
class GlassTokens extends ThemeExtension<GlassTokens> {
  const GlassTokens({
    required this.brightness,
    required this.highContrast,
    required this.platform,
    required this.materials,
    required this.optics,
    required this.palette,
    required this.surfaces,
    required this.ambient,
    required this.radii,
    required this.typography,
  });

  factory GlassTokens.resolve({required Brightness brightness, bool highContrast = false, TargetPlatform? platform}) {
    final p = platform ?? defaultTargetPlatform;
    final dark = brightness == Brightness.dark;
    return GlassTokens(
      brightness: brightness,
      highContrast: highContrast,
      platform: p,
      materials: GlassMaterialSpec.resolve(brightness),
      optics: GlassOptics.resolve(brightness),
      palette: switch ((dark, highContrast)) {
        (false, false) => GlassPalette.light,
        (true, false) => GlassPalette.dark,
        (false, true) => GlassPalette.lightHighContrast,
        (true, true) => GlassPalette.darkHighContrast,
      },
      surfaces: GlassSurfaces.resolve(brightness, highContrast: highContrast),
      ambient: AmbientSpec.resolve(brightness),
      radii: GlassRadii.forPlatform(p),
      typography: GlassTypography.forPlatform(p),
    );
  }

  static GlassTokens light({TargetPlatform? platform}) =>
      GlassTokens.resolve(brightness: Brightness.light, platform: platform);

  static GlassTokens dark({TargetPlatform? platform}) =>
      GlassTokens.resolve(brightness: Brightness.dark, platform: platform);

  static GlassTokens lightHighContrast({TargetPlatform? platform}) =>
      GlassTokens.resolve(brightness: Brightness.light, highContrast: true, platform: platform);

  static GlassTokens darkHighContrast({TargetPlatform? platform}) =>
      GlassTokens.resolve(brightness: Brightness.dark, highContrast: true, platform: platform);

  /// Tokens of the ambient [Theme]; resolved on the fly if the theme has no
  /// extension (e.g. a bare `MaterialApp` in a test).
  static GlassTokens of(BuildContext context) {
    final theme = Theme.of(context);
    return theme.extension<GlassTokens>() ?? GlassTokens.resolve(brightness: theme.brightness);
  }

  final Brightness brightness;
  final bool highContrast;
  final TargetPlatform platform;
  final Map<GlassVariant, GlassMaterialSpec> materials;
  final GlassOptics optics;
  final GlassPalette palette;
  final GlassSurfaces surfaces;
  final AmbientSpec ambient;
  final GlassRadii radii;

  /// Type scale (not `type`: that is the ThemeExtension key).
  final GlassTypography typography;

  bool get isDark => brightness == Brightness.dark;

  GlassMaterialSpec material(GlassVariant variant) => materials[variant]!;

  /// Focus ring: 2 px accent, offset 2 (IC: 3 px in the label colour).
  double get focusRingWidth => highContrast ? 3 : 2;
  double get focusRingOffset => 2;
  Color get focusRingColor => highContrast ? palette.label : palette.accent;

  /// Selection on glass: brand fill α .18 light / .26 dark.
  Color get selectionOnGlass => palette.accentFill.withValues(alpha: isDark ? 0.26 : 0.18);

  /// Selected sidebar item fill: brand fill α .16.
  Color get sidebarSelection => palette.accentFill.withValues(alpha: 0.16);

  /// Selected list row: brand fill α .12.
  Color get rowSelection => palette.accentFill.withValues(alpha: 0.12);

  /// Prominent (tinted) glass: brand fill α .92 (label: `onAccent`).
  Color get prominentTint => palette.accentFill.withValues(alpha: 0.92);

  /// Brand fill while hovered / pressed: darkened by the `onAccent` ink, so
  /// the near-black label keeps its contrast.
  Color get accentFillHover => Color.alphaBlend(palette.onAccent.withValues(alpha: 0.08), palette.accentFill);
  Color get accentFillPressed => Color.alphaBlend(palette.onAccent.withValues(alpha: 0.16), palette.accentFill);

  /// Text selection highlight (and terminal selection, §2.7): brand fill α .30.
  Color get textSelection => palette.accentFill.withValues(alpha: 0.30);

  /// Secondary text; promoted to the label colour under Increase Contrast.
  Color get secondaryLabel => highContrast ? palette.label : palette.secondary;

  /// Solid-tier tint: tint base mixed 10 % with the ambient average.
  Color get solidTint => Color.lerp(optics.tintBase, ambient.average, 0.10)!.withValues(alpha: 1);

  /// Hairline width: 1 physical px, minimum 0.5 logical px.
  static double hairline(double devicePixelRatio) => math.max(1 / devicePixelRatio, 0.5);

  // --- §2.2 intensity table -------------------------------------------------

  static const double clearDelta = -0.12;
  static const double clearFloor = 0.40;
  static const double tintedDelta = 0.14;
  static const double tintedCeiling = 0.94;
  static const double contrastDelta = 0.20;

  /// Effective tint alpha of [variant] under [appearance].
  ///
  /// [live]: a live (blurred) regular/thin surface keeps the token tint in
  /// Clear mode so labels stay ≥ 4.5:1 over any backdrop (implementation note
  /// in the spec). [forceOpaque]: device approval — secure at α 1.0.
  double tintAlpha(GlassVariant variant, GlassAppearance appearance, {bool live = false, bool forceOpaque = false}) {
    final base = material(variant).tintAlpha;
    final ic = appearance.increaseContrast || highContrast;
    if (variant == GlassVariant.secure) {
      if (forceOpaque || appearance.isSolid) return 1.0;
      return ic ? math.min(base + contrastDelta, 1.0) : base;
    }
    if (appearance.isSolid) return 1.0;
    var alpha = base;
    final scalable = variant == GlassVariant.thin || variant == GlassVariant.regular || variant == GlassVariant.thick;
    if (scalable) {
      switch (appearance.mode) {
        case GlassMode.clear:
          if (!live || variant == GlassVariant.thick) alpha = math.max(alpha + clearDelta, clearFloor);
        case GlassMode.tinted:
          alpha = math.min(alpha + tintedDelta, tintedCeiling);
        case GlassMode.standard || GlassMode.solid:
          break;
      }
    }
    if (ic) alpha = math.min(alpha + contrastDelta, 1.0);
    return alpha;
  }

  /// Resolves everything a painter needs for one surface.
  ///
  /// * [live]: the surface was granted a live backdrop (budget permitting).
  /// * [interactive]: pointer hotspot allowed.
  /// * [tint]: prominent / destructive fill replacing the neutral tint.
  /// * [refractionAvailable]: the shader is loaded and supported.
  /// * [presence]: 0…1 materialize factor (σ, α and shadows scale with it).
  ResolvedGlass resolveSurface(
    GlassVariant variant,
    GlassAppearance appearance, {
    bool live = false,
    bool interactive = false,
    Color? tint,
    bool forceOpaque = false,
    bool refractionAvailable = false,
    double presence = 1,
  }) {
    final spec = material(variant);
    final secure = variant == GlassVariant.secure;
    final ic = appearance.increaseContrast || highContrast;
    final solid = appearance.isSolid;
    final isLive = live && !solid && !secure;
    final p = presence.clamp(0.0, 1.0);

    final alpha = tintAlpha(variant, appearance, live: isLive, forceOpaque: forceOpaque);
    Color fill;
    if (tint != null) {
      // Prominent / destructive: keep the caller's colour; opaque when solid or IC.
      fill = (solid || ic) ? tint.withValues(alpha: 1) : tint;
    } else if (solid && !secure) {
      fill = solidTint;
    } else {
      fill = optics.tintBase.withValues(alpha: alpha);
    }
    fill = fill.withValues(alpha: fill.a * p);

    final List<Color> rim;
    if (ic) {
      rim = List.filled(3, optics.highContrastRim.withValues(alpha: optics.highContrastRim.a * p));
    } else if (secure) {
      rim = List.filled(3, optics.secureRim.withValues(alpha: optics.secureRim.a * p));
    } else {
      rim = [for (final c in optics.rimStops) c.withValues(alpha: c.a * p)];
    }

    final refraction =
        isLive && appearance.tier == GlassTier.refractive && refractionAvailable && !appearance.reduceMotion
        ? GlassRefraction(
            bezel: spec.refraction.bezel,
            maxDisplacement: spec.refraction.maxDisplacement * p,
            chroma: spec.refraction.chroma,
          )
        : GlassRefraction.none;

    return ResolvedGlass(
      variant: variant,
      tint: fill,
      blurSigma: isLive ? spec.blurSigma * p : 0,
      saturation: spec.saturation,
      shadows: [for (final s in spec.shadows) s.toBoxShadow(opacity: p)],
      rimColors: rim,
      rimAngleDegrees: optics.rimAngleDegrees,
      darkEdge: optics.darkEdge.withValues(alpha: optics.darkEdge.a * p),
      highlightTop: (secure || ic) ? null : optics.highlightTop.withValues(alpha: optics.highlightTop.a * p),
      highlightHeight: optics.highlightHeight,
      hotspot: interactive && !secure && !ic && !solid && !appearance.reduceMotion,
      hotspotColor: const Color(0xFFFFFFFF).withValues(alpha: optics.hotspotAlpha),
      hotspotPressedColor: const Color(0xFFFFFFFF).withValues(alpha: optics.hotspotPressedAlpha),
      hotspotRadius: optics.hotspotRadius,
      refraction: refraction,
    );
  }

  /// Motion tokens for the given Reduce Motion state.
  GlassMotion motion(GlassAppearance appearance) => GlassMotion(reduceMotion: appearance.reduceMotion);

  @override
  GlassTokens copyWith({
    Brightness? brightness,
    bool? highContrast,
    TargetPlatform? platform,
    GlassPalette? palette,
    GlassSurfaces? surfaces,
    GlassRadii? radii,
    GlassTypography? typography,
    AmbientSpec? ambient,
    GlassOptics? optics,
  }) => GlassTokens(
    brightness: brightness ?? this.brightness,
    highContrast: highContrast ?? this.highContrast,
    platform: platform ?? this.platform,
    materials: materials,
    optics: optics ?? this.optics,
    palette: palette ?? this.palette,
    surfaces: surfaces ?? this.surfaces,
    ambient: ambient ?? this.ambient,
    radii: radii ?? this.radii,
    typography: typography ?? this.typography,
  );

  @override
  GlassTokens lerp(covariant GlassTokens? other, double t) {
    if (other == null) return this;
    final discrete = t < 0.5 ? this : other;
    return GlassTokens(
      brightness: discrete.brightness,
      highContrast: discrete.highContrast,
      platform: discrete.platform,
      materials: discrete.materials,
      optics: discrete.optics,
      palette: GlassPalette.lerp(palette, other.palette, t),
      surfaces: GlassSurfaces.lerp(surfaces, other.surfaces, t),
      ambient: discrete.ambient,
      radii: GlassRadii.lerp(radii, other.radii, t),
      typography: GlassTypography.lerp(typography, other.typography, t),
    );
  }
}
