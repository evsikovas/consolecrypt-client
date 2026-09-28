import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/app/theme/personalization.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  test('appearance survives the real local-settings JSON round trip', () {
    const source = LocalSettings(
      themeMode: AppThemeMode.light,
      uiFontScale: .85,
      uiAccentColor: 0x7C5CFC,
      uiBackgroundColor: 0x16324F,
      terminalColorScheme: TerminalColorScheme.ocean,
      terminalFontSize: 17,
      appLocale: AppLocale.ru,
      glassMode: GlassMode.solid,
      sidebarStyle: SidebarStyle.edgeToEdge,
      workspacePanelStyle: WorkspacePanelStyle.expanded,
    );
    final restored = localSettingsFromJson(decodeObject(encodeJson(localSettingsToJson(source))));
    expect(localSettingsToJson(restored), localSettingsToJson(source));
    final reset = restored.copyWith(uiFontScale: 1, resetUiColors: true);
    expect(reset.uiAccentColor, isNull);
    expect(reset.uiBackgroundColor, isNull);
    expect(reset.terminalColorScheme, source.terminalColorScheme);
    expect(reset.terminalFontSize, 17);
    expect(reset.sidebarStyle, source.sidebarStyle);
    expect(reset.workspacePanelStyle, WorkspacePanelStyle.expanded);
  });

  test('old settings migrate; malformed new fields fall back safely', () {
    final old = localSettingsFromJson({'theme_mode': 'dark', 'terminal_font_size': 16});
    expect(old.workspacePanelStyle, WorkspacePanelStyle.floating);
    expect(old.uiFontScale, 1);
    expect(old.uiAccentColor, isNull);
    expect(old.terminalColorScheme, TerminalColorScheme.system);
    final invalid = localSettingsFromJson({
      'ui_font_scale': 'huge',
      'ui_accent_color': -1,
      'ui_background_color': '#BAD',
      'terminal_color_scheme': 'unknown',
      'workspace_panel_style': 'unknown',
    });
    expect(invalid.workspacePanelStyle, WorkspacePanelStyle.floating);
    expect(invalid.uiFontScale, 1);
    expect(invalid.uiAccentColor, isNull);
    expect(invalid.uiBackgroundColor, isNull);
    expect(invalid.terminalColorScheme, TerminalColorScheme.system);
    expect(localSettingsFromJson({'ui_font_scale': 500}).uiFontScale, 1.4);
    expect(localSettingsFromJson({'ui_font_scale': double.nan}).uiFontScale, 1);
    expect(localSettingsFromJson({'ui_font_scale': .01}).uiFontScale, .8);
  });

  test('old 90% migrates to the new 100% once and retains visible size', () {
    final migrated = localSettingsFromJson({'ui_font_scale': .9});
    expect(migrated.uiFontScale, 1);
    expect(migrated.effectiveUiFontScale, .9);
    final restored = localSettingsFromJson(localSettingsToJson(migrated));
    expect(restored.uiFontScale, 1);
    expect(restored.effectiveUiFontScale, .9);
    expect(localSettingsFromJson({'ui_font_scale': 1.2}).effectiveUiFontScale, closeTo(1.2, .0001));
    expect(const LocalSettings().effectiveUiFontScale, .9);
    expect(const LocalSettings(uiFontScale: .8).effectiveUiFontScale, closeTo(.72, .0001));
    expect(const LocalSettings(uiFontScale: 1.4).effectiveUiFontScale, closeTo(1.26, .0001));
  });

  test('arbitrary interface colours retain text and button contrast in both themes', () {
    for (final brightness in Brightness.values) {
      for (final highContrast in [false, true]) {
        for (final color in [0x000000, 0xFFFFFF, 0x808080, 0xFFFF00, 0x00FF00, 0x0000FF, 0xFF00FF, 0x16324F]) {
          final tokens = AppTheme.build(
            brightness,
            highContrast: highContrast,
            preferences: LocalSettings(uiAccentColor: color, uiBackgroundColor: color),
          ).extension<GlassTokens>()!;
          expect(tokens.palette.accentFill, Color(0xFF000000 | color));
          expect(colorContrast(tokens.palette.onAccent, tokens.palette.accentFill), greaterThanOrEqualTo(4.5));
          for (final surface in [tokens.surfaces.contentSolid, tokens.surfaces.inset, tokens.surfaces.paper]) {
            expect(colorContrast(tokens.palette.label, surface), greaterThanOrEqualTo(4.5));
            expect(colorContrast(tokens.palette.accent, surface), greaterThanOrEqualTo(highContrast ? 7 : 4.5));
          }
        }
      }
    }
  });

  test('interface font scaling does not change terminal settings or opaque colours', () {
    for (final scale in [.8, 1.2]) {
      final tokens = AppTheme.light(
        platform: TargetPlatform.macOS,
        preferences: LocalSettings(uiFontScale: scale),
      ).extension<GlassTokens>()!;
      expect(tokens.typography.body.fontSize, closeTo(14 * .9 * scale, .001));
    }
    for (final scheme in TerminalColorScheme.values.where((s) => s != TerminalColorScheme.system)) {
      final light = AppTheme.terminalTheme(Brightness.light, scheme: scheme);
      final dark = AppTheme.terminalTheme(Brightness.dark, scheme: scheme);
      expect(light.background, dark.background);
      expect(light.foreground, dark.foreground);
      expect(light.selection, dark.selection);
      expect(light.background.a, 1);
      expect(light.foreground.a, 1);
      expect(colorContrast(light.foreground, light.background), greaterThanOrEqualTo(4.5));
    }
  });
}
