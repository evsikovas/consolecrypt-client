import 'dart:convert';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/theme/terminal_palettes.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/terminal_colors.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:consolecrypt/settings/terminal_theme_io.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

String itermPreset({String red = '0.5'}) {
  String color(String key) =>
      '<key>$key</key><dict><key>Red Component</key><real>$red</real>'
      '<key>Green Component</key><real>0.25</real><key>Blue Component</key><integer>1</integer>'
      '<key>Color Space</key><string>sRGB</string></dict>';
  return '<?xml version="1.0"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
      '"http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict>'
      '${color('Background Color')}${color('Foreground Color')}'
      '${List.generate(16, (i) => color('Ansi $i Color')).join()}</dict></plist>';
}

void main() {
  final colors = colorsFromTerminalTheme(AppTheme.terminalTheme(Brightness.dark)).withColor('background', 0x123456);
  test('custom colors export, import and local store round trip independently', () {
    final imported = parseTerminalTheme(encodeTerminalTheme(colors));
    expect(imported.colors, colors.colors);
    final settings = LocalSettings(
      terminalColorScheme: TerminalColorScheme.custom,
      customTerminalColors: imported,
      uiAccentColor: 0xFF0000,
      reopenLastProfile: true,
    );
    final restored = localSettingsFromJson(decodeObject(encodeJson(localSettingsToJson(settings))));
    expect(restored.customTerminalColors!.colors, colors.colors);
    expect(restored.reopenLastProfile, isTrue);
    final dark = AppTheme.terminalTheme(
      Brightness.dark,
      scheme: restored.terminalColorScheme,
      custom: restored.customTerminalColors,
    );
    final light = AppTheme.terminalTheme(
      Brightness.light,
      scheme: restored.terminalColorScheme,
      custom: restored.customTerminalColors,
    );
    expect(dark.background, const Color(0xFF123456));
    expect(light.background, dark.background);
    expect(() => colors.colors['background'] = 0, throwsUnsupportedError);
  });
  test('iTerm XML imports all ANSI colors, rounds components and defaults optional fields', () {
    final imported = parseTerminalTheme(itermPreset());
    expect(imported['background'], 0x8040FF);
    expect(imported['cursor'], imported['foreground']);
    expect(imported['selection'], imported['ansi4']);
    expect(imported.colors.length, 20);
  });
  test('invalid JSON and XML never produce a partial palette', () {
    for (final source in [
      '{}',
      '[]',
      '{"version":2,"colors":{}}',
      '<plist>',
      itermPreset(red: 'NaN'),
      itermPreset(red: '-0.1'),
      itermPreset(red: '1.1'),
      itermPreset().replaceFirst('Ansi 15 Color', 'Ansi 14 Color'),
      itermPreset().replaceFirst('<plist', '<!ENTITY x SYSTEM "file:///tmp/secret"><plist'),
      ' ' * (maxTerminalThemeBytes + 1),
      jsonEncode({
        ...colors.toJson(),
        'colors': {'background': '#NOPE00'},
      }),
    ]) {
      expect(() => parseTerminalTheme(source), throwsFormatException);
    }
  });
  test('old or broken preferences never auto-open a keychain and retain usable theme', () {
    final old = localSettingsFromJson({});
    expect(old.reopenLastProfile, isFalse);
    final broken = localSettingsFromJson({'custom_terminal_colors': 4, 'reopen_last_profile': 'true'});
    expect(broken.reopenLastProfile, isFalse);
    expect(broken.customTerminalColors, isNull);
    expect(AppTheme.terminalTheme(Brightness.dark, scheme: TerminalColorScheme.custom).background.a, 1);
    expect(TerminalColors.tryFromJson({'version': 1}), isNull);
  });
  test('keychain error explains OS password and does not call it a wrong vault password', () {
    final message = errorMessage(AppLocalizationsRu(), const AppException(AppErrorCode.secureStore, 'diagnostic'));
    expect(message, contains('пароль входа в Mac'));
    expect(message, contains('Не удаляйте'));
    expect(message, isNot(contains('diagnostic')));
  });
}
