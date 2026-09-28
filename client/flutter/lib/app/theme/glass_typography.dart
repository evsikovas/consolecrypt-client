import 'package:consolecrypt/app/platform.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart';

/// Type scale of LIQUID_GLASS_SPEC §2.8. System fonts only, nothing is
/// bundled: macOS explicitly resolves the OS font to SF Pro; Windows uses
/// Segoe UI Variable (Text ≤ 17, Display ≥ 20) with a Segoe UI fallback.
/// Colours are left unset; widgets apply palette colours.
@immutable
final class GlassTypography {
  const GlassTypography({
    required this.largeTitle,
    required this.title1,
    required this.title2,
    required this.title3,
    required this.body,
    required this.bodyEmph,
    required this.callout,
    required this.caption,
    required this.button,
    required this.mono,
    required this.code,
  });

  /// Onboarding, empty states.
  final TextStyle largeTitle;

  /// Page titles (content).
  final TextStyle title1;

  /// Dialog titles, section headers.
  final TextStyle title2;

  /// Toolbar page title, card titles.
  final TextStyle title3;
  final TextStyle body;

  /// Hostnames, key facts.
  final TextStyle bodyEmph;

  /// Secondary rows.
  final TextStyle callout;

  /// Badges, metadata.
  final TextStyle caption;
  final TextStyle button;

  /// Commands, paths, fingerprints.
  final TextStyle mono;

  /// Verification-code digits: mono, tabular figures, letter-spacing 1.5.
  final TextStyle code;

  static const windowsTextFamily = 'Segoe UI Variable Text';
  static const windowsDisplayFamily = 'Segoe UI Variable Display';
  static const windowsFallback = ['Segoe UI'];

  static GlassTypography forPlatform(TargetPlatform platform) {
    final mac = platform == TargetPlatform.macOS || platform == TargetPlatform.iOS;
    final android = platform == TargetPlatform.android;
    TextStyle s(double size, double line, FontWeight weight, {double? winSize, double? winLine}) {
      final fontSize = android
          ? (size < 18 ? size + 2 : size)
          : mac
          ? size
          : (winSize ?? size);
      final lineHeight = android
          ? line + 2
          : mac
          ? line
          : (winLine ?? line);
      return TextStyle(
        fontFamily: android
            ? 'Roboto'
            : mac
            ? '.AppleSystemUIFont'
            : (fontSize >= 20 ? windowsDisplayFamily : windowsTextFamily),
        fontFamilyFallback: mac || android ? null : windowsFallback,
        fontSize: fontSize,
        height: lineHeight / fontSize,
        fontWeight: weight,
        leadingDistribution: TextLeadingDistribution.even,
      );
    }

    final monoFamily = android
        ? 'monospace'
        : mac
        ? 'Menlo'
        : 'Consolas';
    TextStyle monoStyle(double size, double line, FontWeight weight) => TextStyle(
      fontFamily: monoFamily,
      fontFamilyFallback: AppPlatform.monospaceFallback,
      fontSize: size,
      height: line / size,
      fontWeight: weight,
      leadingDistribution: TextLeadingDistribution.even,
    );

    return GlassTypography(
      largeTitle: s(32, 39, FontWeight.w600).copyWith(letterSpacing: -1.0),
      title1: s(26, 33, FontWeight.w600).copyWith(letterSpacing: -0.7),
      title2: s(18, 24, FontWeight.w600).copyWith(letterSpacing: -0.3),
      title3: s(15, 20, FontWeight.w600),
      body: s(14, 20, FontWeight.w400),
      bodyEmph: s(14, 20, FontWeight.w600),
      callout: s(12, 18, FontWeight.w400, winSize: 13),
      caption: s(11, 16, FontWeight.w500, winSize: 12),
      button: s(13, 16, FontWeight.w600, winSize: 14, winLine: 18),
      mono: monoStyle(13, 18, FontWeight.w400),
      code: monoStyle(
        28,
        34,
        FontWeight.w600,
      ).copyWith(letterSpacing: 1.5, fontFeatures: const [FontFeature.tabularFigures()]),
    );
  }

  static GlassTypography lerp(GlassTypography a, GlassTypography b, double t) {
    TextStyle l(TextStyle x, TextStyle y) => TextStyle.lerp(x, y, t)!;
    return GlassTypography(
      largeTitle: l(a.largeTitle, b.largeTitle),
      title1: l(a.title1, b.title1),
      title2: l(a.title2, b.title2),
      title3: l(a.title3, b.title3),
      body: l(a.body, b.body),
      bodyEmph: l(a.bodyEmph, b.bodyEmph),
      callout: l(a.callout, b.callout),
      caption: l(a.caption, b.caption),
      button: l(a.button, b.button),
      mono: l(a.mono, b.mono),
      code: l(a.code, b.code),
    );
  }

  GlassTypography scaled(double factor) {
    final scale = factor.isFinite && factor > 0 ? factor : 1.0;
    TextStyle s(TextStyle style) => style.copyWith(fontSize: style.fontSize! * scale);
    return GlassTypography(
      largeTitle: s(largeTitle),
      title1: s(title1),
      title2: s(title2),
      title3: s(title3),
      body: s(body),
      bodyEmph: s(bodyEmph),
      callout: s(callout),
      caption: s(caption),
      button: s(button),
      mono: s(mono),
      code: s(code),
    );
  }
}
