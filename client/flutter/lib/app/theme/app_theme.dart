import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme/glass_input_border.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/app/theme/personalization.dart';
import 'package:consolecrypt/app/theme/terminal_palettes.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/terminal_colors.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

/// Light/dark (and increased-contrast) themes built from the Liquid Glass
/// tokens (LIQUID_GLASS_SPEC §2, §6.8). `material_ui` still provides the
/// widgets (checkboxes, text selection, scrollbars); the kit in
/// `lib/core/glass/` reads [GlassTokens] from the theme.
abstract final class AppTheme {
  /// Brand accent (the logo blue). Kept for callers that need a seed colour.
  static const seed = GlassPalette.brandBlue;

  static ThemeData light({
    bool highContrast = false,
    TargetPlatform? platform,
    LocalSettings preferences = const LocalSettings(),
  }) => build(Brightness.light, highContrast: highContrast, platform: platform, preferences: preferences);

  static ThemeData dark({
    bool highContrast = false,
    TargetPlatform? platform,
    LocalSettings preferences = const LocalSettings(),
  }) => build(Brightness.dark, highContrast: highContrast, platform: platform, preferences: preferences);

  static ThemeData build(
    Brightness brightness, {
    bool highContrast = false,
    TargetPlatform? platform,
    LocalSettings preferences = const LocalSettings(),
  }) {
    final tokens = personalizeTokens(
      GlassTokens.resolve(brightness: brightness, highContrast: highContrast, platform: platform),
      preferences,
    );
    final p = tokens.palette;
    final s = tokens.surfaces;
    final r = tokens.radii;
    final dark = brightness == Brightness.dark;
    final canvas = s.contentSolid;
    Color over(Color color, double alpha) => Color.alphaBlend(color.withValues(alpha: alpha), canvas);

    // Roles the spec defines are set explicitly; container tones are derived
    // from them so existing material_ui widgets stay coherent.
    // `primary` is the text-safe accent (material_ui also draws text with it);
    // brand-blue fills come from `accentFill` explicitly (buttons below).
    final scheme = ColorScheme.fromSeed(seedColor: p.accentFill, brightness: brightness).copyWith(
      primary: p.accent,
      onPrimary: dark ? p.onAccent : const Color(0xFFFFFFFF),
      primaryContainer: over(p.accentFill, 0.16),
      onPrimaryContainer: p.label,
      secondary: p.secondary,
      onSecondary: canvas,
      secondaryContainer: over(p.accentFill, 0.16),
      onSecondaryContainer: p.label,
      tertiary: p.warning,
      onTertiary: dark ? const Color(0xFF000000) : const Color(0xFFFFFFFF),
      tertiaryContainer: over(p.warningFill, 0.16),
      onTertiaryContainer: p.warning,
      error: p.danger,
      onError: dark ? const Color(0xFF000000) : const Color(0xFFFFFFFF),
      errorContainer: over(p.danger, 0.16),
      onErrorContainer: p.danger,
      surface: canvas,
      onSurface: p.label,
      onSurfaceVariant: tokens.secondaryLabel,
      surfaceContainerLowest: canvas,
      surfaceContainerLow: over(p.label, 0.03),
      surfaceContainer: over(p.label, 0.05),
      surfaceContainerHigh: over(p.label, 0.07),
      surfaceContainerHighest: s.inset,
      outline: p.tertiary,
      outlineVariant: Color.alphaBlend(s.separator, canvas),
      shadow: const Color(0xFF000000),
      scrim: const Color(0xFF000000),
      inverseSurface: p.label,
      onInverseSurface: canvas,
      inversePrimary: p.accentFill,
    );

    final t = tokens.typography;
    final textTheme = TextTheme(
      displayLarge: t.largeTitle,
      displayMedium: t.largeTitle,
      displaySmall: t.largeTitle,
      headlineLarge: t.largeTitle,
      headlineMedium: t.title1,
      headlineSmall: t.title1,
      titleLarge: t.title2,
      titleMedium: t.title3,
      titleSmall: t.bodyEmph,
      bodyLarge: t.body,
      bodyMedium: t.body,
      bodySmall: t.callout,
      labelLarge: t.button,
      labelMedium: t.caption,
      labelSmall: t.caption,
    ).apply(bodyColor: p.label, displayColor: p.label);

    final control = RoundedSuperellipseBorder(borderRadius: BorderRadius.circular(r.md));
    final hair = BorderSide(color: s.hairlineCard);
    final menuShape = RoundedSuperellipseBorder(borderRadius: BorderRadius.circular(r.menu), side: hair);
    // Opaque glass tint for Material overlays that cannot blur (tooltips,
    // dropdown menus, fallback dialogs): the kit's solid tier (§2.2).
    final overlayFill = tokens.optics.tintBase.withValues(alpha: 1);
    WidgetStateProperty<Color?> overlay(Color base) => WidgetStateProperty.resolveWith((states) {
      if (states.contains(WidgetState.pressed)) return base.withValues(alpha: dark ? 0.12 : 0.09);
      if (states.contains(WidgetState.hovered) || states.contains(WidgetState.focused)) {
        return base.withValues(alpha: dark ? 0.07 : 0.05);
      }
      return null;
    });
    WidgetStateProperty<Color?> fg(Color color) => WidgetStateProperty.resolveWith(
      (states) => states.contains(WidgetState.disabled) ? color.withValues(alpha: 0.38) : color,
    );
    // Controls are md (28 high) under the compact density used app-wide (§2.6).
    final mobile = tokens.platform == TargetPlatform.android || tokens.platform == TargetPlatform.iOS;
    final buttonMinimum = WidgetStatePropertyAll(Size(0, mobile ? 48 : 36));
    const buttonPadding = WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 12));
    final buttonText = WidgetStatePropertyAll(t.button);
    final buttonShape = WidgetStatePropertyAll<OutlinedBorder>(control);

    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      brightness: brightness,
      // System fonts only (§2.8): the platform typography supplies SF Pro on
      // macOS; the tokens name Segoe UI Variable on Windows.
      typography: Typography.material2021(platform: tokens.platform),
      textTheme: textTheme,
      visualDensity: mobile ? VisualDensity.standard : VisualDensity.compact,
      // Glass lights up on hover/press; it does not ripple (§6.8).
      splashFactory: NoSplash.splashFactory,
      // Pages are transparent: the shell paints `AmbientBackdrop` under the
      // router (layer 1) and content sits on `ContentSurface` cards (§3, §6.8).
      scaffoldBackgroundColor: const Color(0x00000000),
      canvasColor: canvas,
      dividerTheme: DividerThemeData(color: s.separator, space: 1, thickness: 1),
      // Fields are fills, never glass (§4.4): r.md, no stroke at rest, 2 px
      // accent ring on focus, 1.5 px danger border on error.
      inputDecorationTheme: InputDecorationTheme(
        isDense: true,
        filled: true,
        fillColor: s.fillField,
        hoverColor: const Color(0x00000000),
        contentPadding: const EdgeInsets.symmetric(horizontal: 10, vertical: 10),
        border: GlassInputBorder(radius: r.md),
        enabledBorder: GlassInputBorder(radius: r.md),
        disabledBorder: GlassInputBorder(radius: r.md),
        focusedBorder: GlassInputBorder(
          radius: r.md,
          borderSide: BorderSide(color: tokens.focusRingColor, width: tokens.focusRingWidth),
        ),
        errorBorder: GlassInputBorder(
          radius: r.md,
          borderSide: BorderSide(color: p.danger, width: 1.5),
        ),
        focusedErrorBorder: GlassInputBorder(
          radius: r.md,
          borderSide: BorderSide(color: p.danger, width: 2),
        ),
        labelStyle: t.body.copyWith(color: tokens.secondaryLabel),
        floatingLabelStyle: t.body.copyWith(color: tokens.secondaryLabel),
        hintStyle: t.body.copyWith(color: tokens.secondaryLabel),
        helperStyle: t.callout.copyWith(color: tokens.secondaryLabel),
        helperMaxLines: 3,
        errorStyle: t.callout.copyWith(color: p.danger),
        errorMaxLines: 3,
        prefixIconColor: tokens.secondaryLabel,
        suffixIconColor: tokens.secondaryLabel,
      ),
      // Buttons mirror `GlassButton` for screens that still use material_ui
      // buttons: filled = prominent (accent), outlined = glass secondary (a
      // fill on content), text = plain (accent label).
      filledButtonTheme: FilledButtonThemeData(
        style: ButtonStyle(
          backgroundColor: WidgetStateProperty.resolveWith((states) {
            if (states.contains(WidgetState.disabled)) return p.accentFill;
            if (states.contains(WidgetState.pressed)) return tokens.accentFillPressed;
            if (states.contains(WidgetState.hovered)) return tokens.accentFillHover;
            return p.accentFill;
          }),
          foregroundColor: fg(p.onAccent),
          iconColor: fg(p.onAccent),
          overlayColor: const WidgetStatePropertyAll(Color(0x00000000)),
          elevation: const WidgetStatePropertyAll(0),
          textStyle: buttonText,
          shape: buttonShape,
          minimumSize: buttonMinimum,
          padding: buttonPadding,
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          splashFactory: NoSplash.splashFactory,
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: ButtonStyle(
          backgroundColor: WidgetStatePropertyAll(s.fillField),
          foregroundColor: fg(p.label),
          iconColor: fg(p.label),
          overlayColor: overlay(p.label),
          side: WidgetStatePropertyAll(hair),
          textStyle: buttonText,
          shape: buttonShape,
          minimumSize: buttonMinimum,
          padding: buttonPadding,
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          splashFactory: NoSplash.splashFactory,
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: ButtonStyle(
          foregroundColor: fg(p.accent),
          iconColor: fg(p.accent),
          overlayColor: overlay(p.label),
          textStyle: buttonText,
          shape: buttonShape,
          minimumSize: buttonMinimum,
          padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 10)),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          splashFactory: NoSplash.splashFactory,
        ),
      ),
      iconButtonTheme: IconButtonThemeData(
        style: ButtonStyle(
          foregroundColor: fg(p.label),
          overlayColor: overlay(p.label),
          splashFactory: NoSplash.splashFactory,
        ),
      ),
      chipTheme: ChipThemeData(
        backgroundColor: s.fillField,
        selectedColor: tokens.sidebarSelection,
        disabledColor: s.fillField,
        checkmarkColor: p.accent,
        side: BorderSide.none,
        shape: const StadiumBorder(),
        labelStyle: t.callout.copyWith(color: p.label),
        secondaryLabelStyle: t.callout.copyWith(color: p.label, fontWeight: FontWeight.w600),
        padding: const EdgeInsets.symmetric(horizontal: 4),
        iconTheme: IconThemeData(color: tokens.secondaryLabel, size: 16),
      ),
      checkboxTheme: CheckboxThemeData(
        shape: RoundedSuperellipseBorder(borderRadius: BorderRadius.circular(r.xs)),
        side: WidgetStateBorderSide.resolveWith(
          (states) => states.contains(WidgetState.selected)
              ? BorderSide.none
              : BorderSide(color: tokens.secondaryLabel.withValues(alpha: 0.7), width: 1.5),
        ),
        fillColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.selected)
              ? (states.contains(WidgetState.disabled) ? p.accentFill.withValues(alpha: 0.38) : p.accentFill)
              : const Color(0x00000000),
        ),
        checkColor: WidgetStatePropertyAll(p.onAccent),
        splashRadius: 0,
      ),
      switchTheme: SwitchThemeData(
        trackOutlineColor: const WidgetStatePropertyAll(Color(0x00000000)),
        trackColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.selected) ? p.accentFill : s.fillPressed,
        ),
        thumbColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.selected) ? p.onAccent : const Color(0xFFFFFFFF),
        ),
      ),
      cardTheme: CardThemeData(
        elevation: 0,
        margin: EdgeInsets.zero,
        color: s.content,
        surfaceTintColor: const Color(0x00000000),
        shape: RoundedSuperellipseBorder(borderRadius: BorderRadius.circular(r.card), side: hair),
      ),
      listTileTheme: ListTileThemeData(
        shape: RoundedSuperellipseBorder(borderRadius: BorderRadius.circular(r.row)),
        selectedColor: p.accent,
        selectedTileColor: tokens.rowSelection,
        iconColor: tokens.secondaryLabel,
        textColor: p.label,
        titleTextStyle: t.body.copyWith(color: p.label),
        subtitleTextStyle: t.callout.copyWith(color: tokens.secondaryLabel),
        horizontalTitleGap: 12,
        minVerticalPadding: 6,
      ),
      expansionTileTheme: ExpansionTileThemeData(
        shape: const Border(),
        collapsedShape: const Border(),
        iconColor: tokens.secondaryLabel,
        collapsedIconColor: tokens.secondaryLabel,
        textColor: p.label,
        collapsedTextColor: p.label,
      ),
      popupMenuTheme: PopupMenuThemeData(
        color: overlayFill,
        surfaceTintColor: const Color(0x00000000),
        shape: menuShape,
        elevation: 8,
        textStyle: t.body.copyWith(color: p.label),
        labelTextStyle: WidgetStatePropertyAll(t.body.copyWith(color: p.label)),
      ),
      menuTheme: MenuThemeData(
        style: MenuStyle(
          backgroundColor: WidgetStatePropertyAll(overlayFill),
          surfaceTintColor: const WidgetStatePropertyAll(Color(0x00000000)),
          shape: WidgetStatePropertyAll(menuShape),
          padding: const WidgetStatePropertyAll(EdgeInsets.all(6)),
        ),
      ),
      dropdownMenuTheme: DropdownMenuThemeData(
        menuStyle: MenuStyle(
          backgroundColor: WidgetStatePropertyAll(overlayFill),
          surfaceTintColor: const WidgetStatePropertyAll(Color(0x00000000)),
          shape: WidgetStatePropertyAll(menuShape),
        ),
      ),
      tooltipTheme: TooltipThemeData(
        waitDuration: const Duration(milliseconds: 500),
        decoration: ShapeDecoration(
          color: overlayFill.withValues(alpha: 0.97),
          shape: RoundedSuperellipseBorder(borderRadius: BorderRadius.circular(r.sm), side: hair),
          shadows: const [BoxShadow(color: Color(0x24000000), blurRadius: 8, offset: Offset(0, 2))],
        ),
        textStyle: t.callout.copyWith(color: p.label),
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      ),
      snackBarTheme: SnackBarThemeData(behavior: SnackBarBehavior.floating, width: mobile ? null : 480),
      dialogTheme: DialogThemeData(
        backgroundColor: overlayFill,
        surfaceTintColor: const Color(0x00000000),
        shape: RoundedSuperellipseBorder(borderRadius: BorderRadius.circular(r.dialog)),
        titleTextStyle: t.title2.copyWith(color: p.label),
        contentTextStyle: t.body.copyWith(color: p.label),
      ),
      scrollbarTheme: ScrollbarThemeData(
        thickness: const WidgetStatePropertyAll(6),
        radius: const Radius.circular(3),
        thumbColor: WidgetStatePropertyAll(p.label.withValues(alpha: 0.28)),
      ),
      badgeTheme: BadgeThemeData(backgroundColor: p.accentFill, textColor: p.onAccent),
      progressIndicatorTheme: ProgressIndicatorThemeData(color: p.accentFill, linearTrackColor: s.fillPressed),
      sliderTheme: SliderThemeData(
        activeTrackColor: p.accentFill,
        thumbColor: p.accentFill,
        inactiveTrackColor: s.fillPressed,
        valueIndicatorColor: p.accentFill,
        valueIndicatorTextStyle: t.caption.copyWith(color: p.onAccent),
      ),
      textSelectionTheme: TextSelectionThemeData(
        cursorColor: p.accent,
        selectionColor: tokens.textSelection,
        selectionHandleColor: p.accentFill,
      ),
      focusColor: tokens.selectionOnGlass,
      hoverColor: s.fillHover,
      highlightColor: s.fillPressed,
      extensions: [tokens],
    );
  }

  static ThemeMode mode(AppThemeMode mode) => switch (mode) {
    AppThemeMode.system => ThemeMode.system,
    AppThemeMode.light => ThemeMode.light,
    AppThemeMode.dark => ThemeMode.dark,
  };

  /// Legacy sidebar background (until the phase-2 `GlassSidebar`): the
  /// ambient base colour, which the floating glass sidebar will sit on.
  static Color sidebarColor(ColorScheme scheme) => GlassTokens.resolve(brightness: scheme.brightness).ambient.base;

  static TextStyle mono(BuildContext context, {double? size, Color? color, FontWeight? weight}) => TextStyle(
    fontFamily: AppPlatform.monospaceFamily,
    fontFamilyFallback: AppPlatform.monospaceFallback,
    fontSize: size ?? 13,
    color: color,
    fontWeight: weight,
  );

  /// Terminal colours: independent of the glass settings, always opaque.
  static TerminalTheme terminalTheme(
    Brightness brightness, {
    TerminalColorScheme scheme = TerminalColorScheme.system,
    TerminalColors? custom,
  }) => switch (scheme) {
    TerminalColorScheme.system => brightness == Brightness.dark ? _darkTerminal : _lightTerminal,
    TerminalColorScheme.dark => _darkTerminal,
    TerminalColorScheme.light => _lightTerminal,
    TerminalColorScheme.custom => custom == null ? _darkTerminal : customTerminalPalette(custom),
    _ => terminalPalette(scheme),
  };

  static final _darkSurfaces = GlassSurfaces.resolve(Brightness.dark, highContrast: false);

  static final _darkTerminal = TerminalTheme(
    cursor: const Color(0xFFE6E6E6),
    selection: GlassPalette.dark.accentFill.withValues(alpha: 0.35),
    foreground: _darkSurfaces.terminalForeground,
    background: _darkSurfaces.terminalBackground,
    black: TerminalThemes.defaultTheme.black,
    red: TerminalThemes.defaultTheme.red,
    green: TerminalThemes.defaultTheme.green,
    yellow: TerminalThemes.defaultTheme.yellow,
    blue: TerminalThemes.defaultTheme.blue,
    magenta: TerminalThemes.defaultTheme.magenta,
    cyan: TerminalThemes.defaultTheme.cyan,
    white: TerminalThemes.defaultTheme.white,
    brightBlack: TerminalThemes.defaultTheme.brightBlack,
    brightRed: TerminalThemes.defaultTheme.brightRed,
    brightGreen: TerminalThemes.defaultTheme.brightGreen,
    brightYellow: TerminalThemes.defaultTheme.brightYellow,
    brightBlue: TerminalThemes.defaultTheme.brightBlue,
    brightMagenta: TerminalThemes.defaultTheme.brightMagenta,
    brightCyan: TerminalThemes.defaultTheme.brightCyan,
    brightWhite: TerminalThemes.defaultTheme.brightWhite,
    searchHitBackground: TerminalThemes.defaultTheme.searchHitBackground,
    searchHitBackgroundCurrent: TerminalThemes.defaultTheme.searchHitBackgroundCurrent,
    searchHitForeground: TerminalThemes.defaultTheme.searchHitForeground,
  );

  static const _lightTerminal = TerminalTheme(
    cursor: Color(0xFF1F2328),
    selection: Color(0x594B89FF),
    foreground: Color(0xFF1F2328),
    background: Color(0xFFFBFBFA),
    black: Color(0xFF24292F),
    red: Color(0xFFCF222E),
    green: Color(0xFF116329),
    yellow: Color(0xFF7D4E00),
    blue: Color(0xFF0969DA),
    magenta: Color(0xFF8250DF),
    cyan: Color(0xFF1B7C83),
    white: Color(0xFF6E7781),
    brightBlack: Color(0xFF57606A),
    brightRed: Color(0xFFA40E26),
    brightGreen: Color(0xFF1A7F37),
    brightYellow: Color(0xFF633C01),
    brightBlue: Color(0xFF218BFF),
    brightMagenta: Color(0xFFA475F9),
    brightCyan: Color(0xFF3192AA),
    brightWhite: Color(0xFF8C959F),
    searchHitBackground: Color(0xFFFFDF5D),
    searchHitBackgroundCurrent: Color(0xFFFF9632),
    searchHitForeground: Color(0xFF1F2328),
  );
}
