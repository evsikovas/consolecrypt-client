import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:material_ui/material_ui.dart';

const monoFont = 'Cascadia Mono';
const monoFallback = ['Consolas', 'Courier New'];

/// Settings accept opaque RGB only; malformed saved values fall back to defaults.
Color? parseHexColor(String? value) {
  if (value == null) return null;
  final hex = value.trim().replaceFirst(RegExp(r'^#'), '');
  if (!RegExp(r'^[0-9a-fA-F]{6}$').hasMatch(hex)) return null;
  return Color(0xff000000 | int.parse(hex, radix: 16));
}

String colorToHex(Color value) => '#${(value.toARGB32() & 0xffffff).toRadixString(16).padLeft(6, '0').toUpperCase()}';

/// Pick readable foregrounds even for white/black custom accents.
Color contrastingText(Color color) => color.computeLuminance() > .179 ? Colors.black : Colors.white;

/// Design tokens shared by every screen. Read with `DeskPalette.of(context)`.
@immutable
class DeskPalette extends ThemeExtension<DeskPalette> {
  const DeskPalette({
    required this.background,
    required this.sidebar,
    required this.surface,
    required this.raised,
    required this.border,
    required this.text,
    required this.muted,
    required this.faint,
    required this.accent,
    required this.accentEnd,
    required this.success,
    required this.warning,
    required this.danger,
    required this.terminal,
    required this.terminalText,
  });

  final Color background;
  final Color sidebar;
  final Color surface;
  final Color raised;
  final Color border;
  final Color text;
  final Color muted;
  final Color faint;
  final Color accent;
  final Color accentEnd;
  final Color success;
  final Color warning;
  final Color danger;
  final Color terminal;
  final Color terminalText;

  LinearGradient get gradient =>
      LinearGradient(begin: Alignment.topLeft, end: Alignment.bottomRight, colors: [accent, accentEnd]);

  static DeskPalette of(BuildContext context) => _consoleCryptPalette(context);

  static const dark = DeskPalette(
    background: Color(0xff0a0e15),
    sidebar: Color(0xff0d121b),
    surface: Color(0xff121924),
    raised: Color(0xff19212f),
    border: Color(0xff222c3c),
    text: Color(0xffe7ecf5),
    muted: Color(0xff8b97ac),
    faint: Color(0xff5c687c),
    accent: Color(0xff7c6cff),
    accentEnd: Color(0xff38bdf8),
    success: Color(0xff34d399),
    warning: Color(0xfffbbf24),
    danger: Color(0xfff87171),
    terminal: Color(0xff070a10),
    terminalText: Color(0xffcbd5e1),
  );

  static const light = DeskPalette(
    background: Color(0xfff4f6fa),
    sidebar: Color(0xffffffff),
    surface: Color(0xffffffff),
    raised: Color(0xfff0f2f8),
    border: Color(0xffe3e7ef),
    text: Color(0xff0f172a),
    muted: Color(0xff5b6476),
    faint: Color(0xff98a1b3),
    accent: Color(0xff5b4bff),
    accentEnd: Color(0xff0ea5e9),
    success: Color(0xff059669),
    warning: Color(0xffd97706),
    danger: Color(0xffdc2626),
    terminal: Color(0xff0b1020),
    terminalText: Color(0xffcbd5e1),
  );

  @override
  DeskPalette copyWith({Color? accent, Color? backgroundTint}) {
    Color tinted(Color base, double opacity) =>
        backgroundTint == null ? base : Color.alphaBlend(backgroundTint.withValues(alpha: opacity), base);
    return DeskPalette(
      background: tinted(background, .12),
      sidebar: tinted(sidebar, .08),
      surface: tinted(surface, .06),
      raised: tinted(raised, .10),
      border: tinted(border, .18),
      text: text,
      muted: muted,
      faint: faint,
      accent: accent ?? this.accent,
      accentEnd: accent ?? accentEnd,
      success: success,
      warning: warning,
      danger: danger,
      terminal: terminal,
      terminalText: terminalText,
    );
  }

  @override
  DeskPalette lerp(DeskPalette? other, double t) {
    if (other == null) return this;
    Color c(Color a, Color b) => Color.lerp(a, b, t)!;
    return DeskPalette(
      background: c(background, other.background),
      sidebar: c(sidebar, other.sidebar),
      surface: c(surface, other.surface),
      raised: c(raised, other.raised),
      border: c(border, other.border),
      text: c(text, other.text),
      muted: c(muted, other.muted),
      faint: c(faint, other.faint),
      accent: c(accent, other.accent),
      accentEnd: c(accentEnd, other.accentEnd),
      success: c(success, other.success),
      warning: c(warning, other.warning),
      danger: c(danger, other.danger),
      terminal: c(terminal, other.terminal),
      terminalText: c(terminalText, other.terminalText),
    );
  }
}

DeskPalette _consoleCryptPalette(BuildContext context) {
  final t = GlassTokens.of(context);
  final c = Theme.of(context).colorScheme;
  return DeskPalette(
    background: c.surface,
    sidebar: c.surface,
    surface: c.surface,
    raised: c.surfaceContainerHighest,
    border: c.outlineVariant,
    text: t.palette.label,
    muted: t.secondaryLabel,
    faint: c.outline,
    accent: t.palette.accent,
    accentEnd: t.palette.accent,
    success: t.palette.success,
    warning: const Color(0xffd79d38),
    danger: c.error,
    terminal: const Color(0xff0e1013),
    terminalText: t.palette.label,
  );
}
