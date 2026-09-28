import 'dart:ui';

import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart';

/// Glass thickness (LIQUID_GLASS_SPEC §2.1).
enum GlassVariant {
  /// Welcome hero only, over ambient art.
  clear,

  /// Toolbar capsules, status pill, tab-strip and segmented tracks.
  thin,

  /// Sidebar, menus, popovers, toasts, tooltips.
  regular,

  /// Dialogs, sheets, command palette.
  thick,

  /// Device approval, secret reveal, risky-command confirmation, Recovery
  /// Kit, password prompts. Effectively opaque, never blurred or refracted.
  secure,
}

/// Rendering tier, resolved once per app by `GlassScope` (§2.10).
enum GlassTier {
  /// macOS + Impeller: live surfaces add the refraction shader.
  refractive,

  /// Default: live surfaces blur the backdrop; chrome is static glass.
  frosted,

  /// Reduce Transparency / "Solid" / Remote Desktop / battery saver: opaque
  /// tint, no `BackdropFilter` anywhere.
  solid,
}

/// Whether a surface samples its backdrop (§3 rule 1).
enum BackdropMode {
  /// Tint + rim + shadow over the painted ambient backdrop; no
  /// `BackdropFilter`. Default for all persistent chrome.
  static,

  /// Live `BackdropFilter` (budgeted, see `GlassBackdropBudget`).
  live,
}

/// One shadow of a glass material, painted **outside** the shape only.
@immutable
final class GlassShadow {
  const GlassShadow({required this.dy, required this.blur, required this.alpha});

  final double dy;
  final double blur;

  /// Black at this opacity.
  final double alpha;

  BoxShadow toBoxShadow({double opacity = 1}) => BoxShadow(
    color: Color.fromRGBO(0, 0, 0, (alpha * opacity).clamp(0.0, 1.0)),
    offset: Offset(0, dy),
    blurRadius: blur,
  );
}

/// Refraction parameters (refractive tier only, §2.1).
@immutable
final class GlassRefraction {
  const GlassRefraction({required this.bezel, required this.maxDisplacement, required this.chroma});

  final double bezel;
  final double maxDisplacement;

  /// Chromatic aberration in logical px (0 = off).
  final double chroma;

  static const none = GlassRefraction(bezel: 0, maxDisplacement: 0, chroma: 0);

  bool get isNone => bezel <= 0 || maxDisplacement <= 0;
}

/// Token values of one [GlassVariant] for one brightness (§2.1 table).
@immutable
final class GlassMaterialSpec {
  const GlassMaterialSpec({
    required this.blurSigma,
    required this.tintAlpha,
    required this.saturation,
    required this.shadows,
    required this.refraction,
  });

  /// Backdrop blur σ for live surfaces (0 = never blurs).
  final double blurSigma;

  /// Opacity of the neutral tint over the backdrop.
  final double tintAlpha;

  /// Rec.709 saturation applied to the blurred backdrop.
  final double saturation;
  final List<GlassShadow> shadows;
  final GlassRefraction refraction;

  static Map<GlassVariant, GlassMaterialSpec> resolve(Brightness brightness) {
    final dark = brightness == Brightness.dark;
    double a(double light, double darkValue) => dark ? darkValue : light;
    return {
      GlassVariant.clear: GlassMaterialSpec(
        blurSigma: 6,
        tintAlpha: a(0.24, 0.28),
        saturation: 1.8,
        shadows: [GlassShadow(dy: 4, blur: 12, alpha: a(0.08, 0.30))],
        refraction: const GlassRefraction(bezel: 10, maxDisplacement: 5, chroma: 0),
      ),
      GlassVariant.thin: GlassMaterialSpec(
        blurSigma: 12,
        tintAlpha: 0.52,
        saturation: 1.6,
        shadows: [GlassShadow(dy: 2, blur: 8, alpha: a(0.10, 0.35))],
        refraction: const GlassRefraction(bezel: 10, maxDisplacement: 5, chroma: 0.4),
      ),
      GlassVariant.regular: GlassMaterialSpec(
        blurSigma: 20,
        tintAlpha: 0.66,
        saturation: 1.5,
        shadows: [
          GlassShadow(dy: 8, blur: 24, alpha: a(0.12, 0.45)),
          GlassShadow(dy: 1, blur: 2, alpha: a(0.06, 0.30)),
        ],
        refraction: const GlassRefraction(bezel: 14, maxDisplacement: 7, chroma: 0.4),
      ),
      GlassVariant.thick: GlassMaterialSpec(
        blurSigma: 28,
        tintAlpha: 0.80,
        saturation: 1.35,
        shadows: [
          GlassShadow(dy: 18, blur: 48, alpha: a(0.18, 0.55)),
          GlassShadow(dy: 2, blur: 6, alpha: a(0.08, 0.30)),
        ],
        refraction: const GlassRefraction(bezel: 18, maxDisplacement: 9, chroma: 0.4),
      ),
      GlassVariant.secure: GlassMaterialSpec(
        blurSigma: 0,
        tintAlpha: 0.96,
        saturation: 1.0,
        shadows: [GlassShadow(dy: 18, blur: 48, alpha: a(0.22, 0.60))],
        refraction: GlassRefraction.none,
      ),
    };
  }
}

/// Shared optical layers: rim, dark edge, inner highlight, pointer hotspot.
@immutable
final class GlassOptics {
  const GlassOptics({
    required this.tintBase,
    required this.rimStops,
    required this.rimAngleDegrees,
    required this.secureRim,
    required this.highContrastRim,
    required this.darkEdge,
    required this.highlightTop,
    required this.highlightHeight,
    required this.hotspotRadius,
    required this.hotspotAlpha,
    required this.hotspotPressedAlpha,
    required this.clearDimming,
  });

  /// Neutral tint colour (light `#F7F8FA`, dark `#1C1D21`).
  final Color tintBase;

  /// Rim gradient colours at stops 0, 0.45, 1.
  final List<Color> rimStops;
  final double rimAngleDegrees;

  /// Flat rim of the secure material.
  final Color secureRim;

  /// Solid 1 px rim under Increase Contrast.
  final Color highContrastRim;

  /// 1 physical px stroke just outside the shape (2026 refinement).
  final Color darkEdge;

  /// Inner highlight: vertical gradient from this colour to transparent.
  final Color highlightTop;
  final double highlightHeight;
  final double hotspotRadius;
  final double hotspotAlpha;
  final double hotspotPressedAlpha;

  /// Dimming layer behind `glass.clear` over bright content (HIG, 35 %).
  final Color clearDimming;

  static const rimStopPositions = [0.0, 0.45, 1.0];

  static GlassOptics resolve(Brightness brightness) {
    final dark = brightness == Brightness.dark;
    const white = Color(0xFFFFFFFF);
    return GlassOptics(
      tintBase: dark ? const Color(0xFF1C1D21) : const Color(0xFFF7F8FA),
      rimStops: dark
          ? [white.withValues(alpha: 0.30), white.withValues(alpha: 0.06), white.withValues(alpha: 0.16)]
          : [white.withValues(alpha: 0.75), white.withValues(alpha: 0.12), white.withValues(alpha: 0.40)],
      rimAngleDegrees: 135,
      secureRim: white.withValues(alpha: dark ? 0.14 : 0.55),
      highContrastRim: dark ? white.withValues(alpha: 0.55) : const Color(0xFF000000).withValues(alpha: 0.45),
      darkEdge: const Color(0xFF000000).withValues(alpha: dark ? 0.50 : 0.10),
      highlightTop: white.withValues(alpha: dark ? 0.10 : 0.30),
      highlightHeight: 14,
      hotspotRadius: 120,
      hotspotAlpha: dark ? 0.06 : 0.10,
      hotspotPressedAlpha: 0.18,
      clearDimming: const Color(0x59000000),
    );
  }
}

/// One radial blob of the ambient backdrop (§2.4). Centre and radius are
/// fractions of the window (radius of `max(w, h)`).
@immutable
final class AmbientBlob {
  const AmbientBlob({required this.center, required this.radius, required this.color});

  final Offset center;
  final double radius;
  final Color color;
}

/// The static ambient backdrop the glass "refracts" (layer 1).
@immutable
final class AmbientSpec {
  const AmbientSpec({required this.base, required this.blobs, required this.localCueBlob, required this.grain});

  final Color base;
  final List<AmbientBlob> blobs;

  /// Replaces blob A for local-only profiles (ADR-0106).
  final AmbientBlob localCueBlob;

  /// Monochrome grain amplitude against banding (dark mode only).
  final double grain;

  static AmbientSpec resolve(Brightness brightness) {
    final dark = brightness == Brightness.dark;
    return AmbientSpec(
      // Cool blue ambience follows the default shield and interface accent.
      base: dark ? const Color(0xFF0D131E) : const Color(0xFFF0F1F4),
      blobs: [
        AmbientBlob(
          center: const Offset(0.12, 0.08),
          radius: 0.55,
          color: dark ? const Color(0x6B224B89) : const Color(0x4D8FB6FF),
        ),
        AmbientBlob(
          center: const Offset(0.92, 0.18),
          radius: 0.50,
          color: dark ? const Color(0x4224395B) : const Color(0x2EA8BCCC),
        ),
        AmbientBlob(
          center: const Offset(0.70, 0.95),
          radius: 0.60,
          color: dark ? const Color(0x33243754) : const Color(0x26BFCFE8),
        ),
      ],
      // Local-only profiles (ADR-0106) swap the blue glow for cool slate, so
      // the profile kind is recognisable at a glance.
      localCueBlob: AmbientBlob(
        center: const Offset(0.12, 0.08),
        radius: 0.55,
        color: dark ? const Color(0x662E3A48) : const Color(0x478FA3B8),
      ),
      grain: dark ? 0.015 : 0,
    );
  }

  /// Approximate average colour of the backdrop: each blob weighted by the
  /// share of the window it covers (used for the solid tier's tint mix).
  Color get average {
    var result = base;
    for (final blob in blobs) {
      // A radial gradient to transparent covers about a third of its disc on
      // average; clamp to the window.
      final coverage = (3.14159 * blob.radius * blob.radius / 3).clamp(0.0, 1.0);
      result = Color.alphaBlend(blob.color.withValues(alpha: blob.color.a * coverage), result);
    }
    return result;
  }
}
