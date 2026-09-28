import 'package:consolecrypt/core/models/settings.dart';
import 'package:consolecrypt/core/models/terminal_colors.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

/// Original terminal palettes. Fixed schemes never depend on UI brightness,
/// accent, background tint or glass intensity.
TerminalTheme terminalPalette(TerminalColorScheme scheme) {
  final (background, foreground, accent) = switch (scheme) {
    TerminalColorScheme.midnight => (0x101422, 0xDCE4FF, 0xADAEFF),
    TerminalColorScheme.ocean => (0x09212C, 0xD6EDF3, 0x72D8E8),
    TerminalColorScheme.forest => (0x12211C, 0xE0EEE5, 0x9BD7AD),
    TerminalColorScheme.amber => (0x1B1710, 0xFFD58A, 0xFFB454),
    _ => throw ArgumentError.value(scheme, 'scheme'),
  };
  Color rgb(int value) => Color(0xFF000000 | value);
  return TerminalTheme(
    background: rgb(background),
    foreground: rgb(foreground),
    cursor: rgb(accent),
    selection: rgb(accent).withValues(alpha: .25),
    black: const Color(0xFF26313C),
    red: const Color(0xFFF08080),
    green: const Color(0xFF9ED99E),
    yellow: const Color(0xFFF0D48A),
    blue: const Color(0xFF82B9F0),
    magenta: const Color(0xFFD0A0E8),
    cyan: const Color(0xFF80D6D0),
    white: rgb(foreground),
    brightBlack: const Color(0xFF8393A5),
    brightRed: const Color(0xFFFFA0A0),
    brightGreen: const Color(0xFFB8EDB0),
    brightYellow: const Color(0xFFFFE5A0),
    brightBlue: const Color(0xFFA6CEFF),
    brightMagenta: const Color(0xFFE8B8FF),
    brightCyan: const Color(0xFFA0EEE6),
    brightWhite: const Color(0xFFFFFFFF),
    searchHitBackground: rgb(accent),
    searchHitBackgroundCurrent: const Color(0xFFFFD166),
    searchHitForeground: rgb(background),
  );
}

TerminalColors colorsFromTerminalTheme(TerminalTheme theme) {
  final ansi = [
    theme.black,
    theme.red,
    theme.green,
    theme.yellow,
    theme.blue,
    theme.magenta,
    theme.cyan,
    theme.white,
    theme.brightBlack,
    theme.brightRed,
    theme.brightGreen,
    theme.brightYellow,
    theme.brightBlue,
    theme.brightMagenta,
    theme.brightCyan,
    theme.brightWhite,
  ];
  return TerminalColors({
    'background': theme.background.toARGB32() & 0xFFFFFF,
    'foreground': theme.foreground.toARGB32() & 0xFFFFFF,
    'cursor': theme.cursor.toARGB32() & 0xFFFFFF,
    'selection': theme.selection.toARGB32() & 0xFFFFFF,
    for (var i = 0; i < ansi.length; i++) 'ansi$i': ansi[i].toARGB32() & 0xFFFFFF,
  });
}

TerminalTheme customTerminalPalette(TerminalColors palette) {
  Color c(String key) => Color(0xFF000000 | palette[key]);
  return TerminalTheme(
    background: c('background'),
    foreground: c('foreground'),
    cursor: c('cursor'),
    selection: c('selection').withValues(alpha: .35),
    black: c('ansi0'),
    red: c('ansi1'),
    green: c('ansi2'),
    yellow: c('ansi3'),
    blue: c('ansi4'),
    magenta: c('ansi5'),
    cyan: c('ansi6'),
    white: c('ansi7'),
    brightBlack: c('ansi8'),
    brightRed: c('ansi9'),
    brightGreen: c('ansi10'),
    brightYellow: c('ansi11'),
    brightBlue: c('ansi12'),
    brightMagenta: c('ansi13'),
    brightCyan: c('ansi14'),
    brightWhite: c('ansi15'),
    searchHitBackground: c('cursor'),
    searchHitBackgroundCurrent: c('ansi3'),
    searchHitForeground: c('background'),
  );
}
