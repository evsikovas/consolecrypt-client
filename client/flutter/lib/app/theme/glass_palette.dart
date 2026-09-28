import 'dart:ui';

import 'package:flutter/foundation.dart';

/// Semantic tone of a status element (badge, toast, banner, dot).
enum GlassTone { neutral, accent, info, warning, danger, unknown, success }

/// Colour roles of LIQUID_GLASS_SPEC §2.7 (light, dark, light-IC, dark-IC).
///
/// Every text role is verified ≥ 4.5:1 on regular glass and on cards (see
/// `test/unit/glass_tokens_test.dart`). Risk never relies on colour alone:
/// always pair a role colour with an icon and a label.
@immutable
final class GlassPalette {
  const GlassPalette({
    required this.label,
    required this.secondary,
    required this.tertiary,
    required this.accent,
    required this.accentFill,
    required this.onAccent,
    required this.info,
    required this.warning,
    required this.warningFill,
    required this.danger,
    required this.dangerFill,
    required this.onDangerFill,
    required this.unknown,
    required this.success,
  });

  final Color label;
  final Color secondary;

  /// Content surfaces only; never essential text on glass.
  final Color tertiary;

  /// Brand accent for text and icons (links, plain buttons, the selected
  /// sidebar icon, focus ring): the brand blue where it reaches 4.5:1
  /// (dark themes), a deeper blue on light surfaces. Never used for
  /// warnings.
  final Color accent;

  /// Brand blue `#4B89FF` fill: prominent buttons, tinted glass, menu
  /// highlight, selection tints.
  final Color accentFill;

  /// Label on [accentFill]: black (white on this blue fails AA).
  final Color onAccent;

  /// Info / `read_only` risk.
  final Color info;

  /// Warning / `modifying` risk (text & icons): amber-yellow, kept clearly
  /// apart from the brand blue.
  final Color warning;

  /// Background base of warning badges (α .16) and warning fills (yellow).
  final Color warningFill;

  /// Danger / `destructive` risk (text & icons).
  final Color danger;

  /// Destructive button fill (white label ≥ 5.5:1).
  final Color dangerFill;
  final Color onDangerFill;

  /// Unverified / unknown risk (treated as caution).
  final Color unknown;
  final Color success;

  /// Default client brand colours: blue shield/wordmark and dark icon background.
  static const brandBlue = Color(0xFF4B89FF);
  static const brandBlack = Color(0xFF09090A);

  static const light = GlassPalette(
    label: Color(0xFF1D1D1F),
    secondary: Color(0xFF4A4A50),
    tertiary: Color(0xFF6E6E73),
    accent: Color(0xFF215BB4),
    accentFill: brandBlue,
    onAccent: Color(0xFF000000),
    info: Color(0xFF0A58B5),
    warning: Color(0xFF7A6100),
    warningFill: Color(0xFFFFCC00),
    danger: Color(0xFFB42318),
    dangerFill: Color(0xFFB42318),
    onDangerFill: Color(0xFFFFFFFF),
    unknown: Color(0xFF6A3FC8),
    success: Color(0xFF15732F),
  );

  static const dark = GlassPalette(
    label: Color(0xFFF5F5F7),
    secondary: Color(0xFFC7C7CC),
    tertiary: Color(0xFF98989F),
    accent: brandBlue,
    accentFill: brandBlue,
    onAccent: Color(0xFF000000),
    info: Color(0xFF6CB4FF),
    warning: Color(0xFFFFD60A),
    warningFill: Color(0xFFFFD60A),
    danger: Color(0xFFFF7A70),
    dangerFill: Color(0xFFC4302A),
    onDangerFill: Color(0xFFFFFFFF),
    unknown: Color(0xFFB99CFF),
    success: Color(0xFF4ADE80),
  );

  static const lightHighContrast = GlassPalette(
    label: Color(0xFF000000),
    secondary: Color(0xFF1D1D1F),
    tertiary: Color(0xFF3A3A3E),
    accent: Color(0xFF154486),
    accentFill: brandBlue,
    onAccent: Color(0xFF000000),
    info: Color(0xFF003F8A),
    warning: Color(0xFF5A4700),
    warningFill: Color(0xFFFFCC00),
    danger: Color(0xFF8A1009),
    dangerFill: Color(0xFF8A1009),
    onDangerFill: Color(0xFFFFFFFF),
    unknown: Color(0xFF4B2396),
    success: Color(0xFF0F5A24),
  );

  static const darkHighContrast = GlassPalette(
    label: Color(0xFFFFFFFF),
    secondary: Color(0xFFF5F5F7),
    tertiary: Color(0xFFD1D1D6),
    accent: Color(0xFF9DC1FF),
    accentFill: brandBlue,
    onAccent: Color(0xFF000000),
    info: Color(0xFFA6D2FF),
    warning: Color(0xFFFFE566),
    warningFill: Color(0xFFFFE566),
    danger: Color(0xFFFFA8A1),
    dangerFill: Color(0xFFC4302A),
    onDangerFill: Color(0xFFFFFFFF),
    unknown: Color(0xFFD6C6FF),
    success: Color(0xFF8CF0B0),
  );

  /// Foreground colour of a [tone] (text and icons).
  Color tone(GlassTone tone) => switch (tone) {
    GlassTone.neutral => secondary,
    GlassTone.accent => accent,
    GlassTone.info => info,
    GlassTone.warning => warning,
    GlassTone.danger => danger,
    GlassTone.unknown => unknown,
    GlassTone.success => success,
  };

  /// Base colour of the α .16 badge background of a [tone]. Warnings use the
  /// yellow fill and the accent its brand fill; everything else its own
  /// role colour.
  Color toneFill(GlassTone tone) => switch (tone) {
    GlassTone.warning => warningFill,
    GlassTone.accent => accentFill,
    _ => this.tone(tone),
  };

  static GlassPalette lerp(GlassPalette a, GlassPalette b, double t) {
    Color c(Color x, Color y) => Color.lerp(x, y, t)!;
    return GlassPalette(
      label: c(a.label, b.label),
      secondary: c(a.secondary, b.secondary),
      tertiary: c(a.tertiary, b.tertiary),
      accent: c(a.accent, b.accent),
      accentFill: c(a.accentFill, b.accentFill),
      onAccent: c(a.onAccent, b.onAccent),
      info: c(a.info, b.info),
      warning: c(a.warning, b.warning),
      warningFill: c(a.warningFill, b.warningFill),
      danger: c(a.danger, b.danger),
      dangerFill: c(a.dangerFill, b.dangerFill),
      onDangerFill: c(a.onDangerFill, b.onDangerFill),
      unknown: c(a.unknown, b.unknown),
      success: c(a.success, b.success),
    );
  }
}

/// Content surfaces and fills (§2.3). These are **not** glass: no blur, no
/// refraction. Terminal, SFTP lists, editors, Recovery Kit words and codes
/// live on the opaque ones.
@immutable
final class GlassSurfaces {
  const GlassSurfaces({
    required this.content,
    required this.contentSolid,
    required this.paper,
    required this.inset,
    required this.fillField,
    required this.fillHover,
    required this.fillPressed,
    required this.separator,
    required this.hairlineCard,
    required this.cardTopHighlight,
    required this.barrier,
    required this.secureBarrier,
    required this.approvalBarrier,
    required this.segmentThumb,
    required this.terminalBackground,
    required this.terminalForeground,
  });

  /// Cards, lists, forms: alpha only, lets the ambient hue through faintly.
  final Color content;

  /// SFTP panes, editors: opaque.
  final Color contentSolid;

  /// Recovery Kit words, verification codes: opaque, always.
  final Color paper;

  /// Code groups, inline code, key fingerprints: opaque.
  final Color inset;

  /// Text fields and search fields (also inside glass).
  final Color fillField;
  final Color fillHover;
  final Color fillPressed;

  /// 1 physical px separators (α .30 under Increase Contrast).
  final Color separator;
  final Color hairlineCard;

  /// Dark mode only: white α .04 top line on cards (transparent in light).
  final Color cardTopHighlight;

  /// Modal barriers: standard / secure dialogs / device approval.
  final Color barrier;
  final Color secureBarrier;
  final Color approvalBarrier;

  /// Segmented-control / active-tab thumb.
  final Color segmentThumb;

  /// Terminal colours are independent of the glass settings (§2.7).
  final Color terminalBackground;
  final Color terminalForeground;

  static GlassSurfaces resolve(Brightness brightness, {required bool highContrast}) {
    final dark = brightness == Brightness.dark;
    final ink = dark ? const Color(0xFFFFFFFF) : const Color(0xFF000000);
    return GlassSurfaces(
      content: dark ? const Color(0xE61D2431) : const Color(0xE0FFFFFF),
      contentSolid: dark ? const Color(0xFF171D29) : const Color(0xFFFFFFFF),
      paper: dark ? const Color(0xFF16171B) : const Color(0xFFFFFFFF),
      inset: dark ? const Color(0xFF232B3B) : const Color(0xFFF2F4F7),
      fillField: ink.withValues(alpha: dark ? 0.07 : 0.05),
      fillHover: ink.withValues(alpha: dark ? 0.07 : 0.05),
      fillPressed: ink.withValues(alpha: dark ? 0.12 : 0.09),
      separator: ink.withValues(alpha: highContrast ? 0.30 : (dark ? 0.12 : 0.10)),
      hairlineCard: ink.withValues(alpha: highContrast ? 0.30 : 0.08),
      cardTopHighlight: dark ? const Color(0x0AFFFFFF) : const Color(0x00FFFFFF),
      barrier: const Color(0xFF000000).withValues(alpha: dark ? 0.45 : 0.25),
      secureBarrier: const Color(0xFF000000).withValues(alpha: dark ? 0.60 : 0.45),
      approvalBarrier: const Color(0xFF000000).withValues(alpha: dark ? 0.65 : 0.50),
      segmentThumb: dark ? const Color(0x29FFFFFF) : const Color(0xF5FFFFFF),
      terminalBackground: dark ? const Color(0xFF0F1115) : const Color(0xFFFBFBFA),
      terminalForeground: dark ? const Color(0xFFE6E6E6) : const Color(0xFF1F2328),
    );
  }

  static GlassSurfaces lerp(GlassSurfaces a, GlassSurfaces b, double t) {
    Color c(Color x, Color y) => Color.lerp(x, y, t)!;
    return GlassSurfaces(
      content: c(a.content, b.content),
      contentSolid: c(a.contentSolid, b.contentSolid),
      paper: c(a.paper, b.paper),
      inset: c(a.inset, b.inset),
      fillField: c(a.fillField, b.fillField),
      fillHover: c(a.fillHover, b.fillHover),
      fillPressed: c(a.fillPressed, b.fillPressed),
      separator: c(a.separator, b.separator),
      hairlineCard: c(a.hairlineCard, b.hairlineCard),
      cardTopHighlight: c(a.cardTopHighlight, b.cardTopHighlight),
      barrier: c(a.barrier, b.barrier),
      secureBarrier: c(a.secureBarrier, b.secureBarrier),
      approvalBarrier: c(a.approvalBarrier, b.approvalBarrier),
      segmentThumb: c(a.segmentThumb, b.segmentThumb),
      terminalBackground: c(a.terminalBackground, b.terminalBackground),
      terminalForeground: c(a.terminalForeground, b.terminalForeground),
    );
  }
}
