import 'dart:math' as math;

import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/models/settings.dart';
import 'package:material_ui/material_ui.dart';

/// Keep user hues while preserving the light/dark surface hierarchy and the
/// meaning of warning/danger/success. Terminal colours are resolved separately.
GlassTokens personalizeTokens(GlassTokens tokens, LocalSettings settings) {
  final dark = tokens.isDark;
  final p = tokens.palette;
  final s = tokens.surfaces;
  final tint = settings.uiBackgroundColor == null ? null : Color(0xFF000000 | settings.uiBackgroundColor!);
  Color surface(Color base) =>
      tint == null ? base : Color.lerp(base.withValues(alpha: 1), tint, dark ? .08 : .06)!.withValues(alpha: base.a);
  final surfaces = GlassSurfaces(
    content: surface(s.content),
    contentSolid: surface(s.contentSolid),
    paper: surface(s.paper),
    inset: surface(s.inset),
    fillField: s.fillField,
    fillHover: s.fillHover,
    fillPressed: s.fillPressed,
    separator: s.separator,
    hairlineCard: s.hairlineCard,
    cardTopHighlight: s.cardTopHighlight,
    barrier: s.barrier,
    secureBarrier: s.secureBarrier,
    approvalBarrier: s.approvalBarrier,
    segmentThumb: s.segmentThumb,
    terminalBackground: s.terminalBackground,
    terminalForeground: s.terminalForeground,
  );
  final ambient = tint == null
      ? tokens.ambient
      : AmbientSpec(
          base: surface(tokens.ambient.base),
          blobs: [
            AmbientBlob(
              center: const Offset(.12, .08),
              radius: .65,
              color: tint.withValues(alpha: dark ? .16 : .12),
            ),
          ],
          localCueBlob: AmbientBlob(
            center: const Offset(.12, .08),
            radius: .65,
            color: tint.withValues(alpha: dark ? .16 : .12),
          ),
          grain: tokens.ambient.grain,
        );
  final fill = settings.uiAccentColor == null ? p.accentFill : Color(0xFF000000 | settings.uiAccentColor!);
  // Text colour must work on both cards and static glass. The chosen fill is
  // kept exact; only text/icons are lightened or darkened when necessary.
  final backgrounds = [
    surfaces.contentSolid,
    surfaces.inset,
    Color.alphaBlend(tokens.optics.tintBase.withValues(alpha: .66), ambient.base),
  ];
  var accent = settings.uiAccentColor == null ? p.accent : fill;
  final target = dark ? const Color(0xFFFFFFFF) : const Color(0xFF000000);
  final minimum = tokens.highContrast ? 7.0 : 4.5;
  for (var step = 0; step <= 100; step++) {
    if (backgrounds.every((b) => colorContrast(accent, b) >= minimum)) break;
    accent = Color.lerp(fill, target, step / 100)!;
  }
  final onAccent = colorContrast(fill, const Color(0xFFFFFFFF)) > colorContrast(fill, const Color(0xFF000000))
      ? const Color(0xFFFFFFFF)
      : const Color(0xFF000000);
  final palette = GlassPalette(
    label: p.label,
    secondary: p.secondary,
    tertiary: p.tertiary,
    accent: accent,
    accentFill: fill,
    onAccent: settings.uiAccentColor == null ? p.onAccent : onAccent,
    info: p.info,
    warning: p.warning,
    warningFill: p.warningFill,
    danger: p.danger,
    dangerFill: p.dangerFill,
    onDangerFill: p.onDangerFill,
    unknown: p.unknown,
    success: p.success,
  );
  return tokens.copyWith(
    palette: palette,
    surfaces: surfaces,
    ambient: ambient,
    typography: tokens.typography.scaled(settings.effectiveUiFontScale),
  );
}

double colorContrast(Color a, Color b) {
  final x = a.computeLuminance();
  final y = b.computeLuminance();
  return (math.max(x, y) + .05) / (math.min(x, y) + .05);
}
