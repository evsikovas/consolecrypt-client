import 'dart:math' as math;
import 'dart:ui';

/// WCAG 2.x colour math used by the token tests and by the glass resolver
/// (LIQUID_GLASS_SPEC §2.1, §6.9).
abstract final class Wcag {
  /// Minimum contrast for body text (WCAG 1.4.3 AA).
  static const double bodyText = 4.5;

  /// Minimum contrast the spec requires for verification-code digits.
  static const double verificationDigits = 7.0;

  static double _linear(double channel) =>
      channel <= 0.04045 ? channel / 12.92 : math.pow((channel + 0.055) / 1.055, 2.4).toDouble();

  /// Relative luminance of an opaque colour (alpha is ignored).
  static double luminance(Color color) =>
      0.2126 * _linear(color.r) + 0.7152 * _linear(color.g) + 0.0722 * _linear(color.b);

  /// Contrast ratio (1…21) between two colours. Translucent colours must be
  /// composited first (see [composite]).
  static double contrast(Color a, Color b) {
    final la = luminance(a);
    final lb = luminance(b);
    final hi = math.max(la, lb);
    final lo = math.min(la, lb);
    return (hi + 0.05) / (lo + 0.05);
  }

  /// Source-over composite of [layers] (bottom first) onto an opaque [base].
  static Color composite(Color base, List<Color> layers) {
    var result = base.withValues(alpha: 1);
    for (final layer in layers) {
      result = Color.alphaBlend(layer, result);
    }
    return result;
  }
}
