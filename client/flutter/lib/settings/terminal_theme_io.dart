import 'dart:convert';

import 'package:consolecrypt/core/models/terminal_colors.dart';
import 'package:xml/xml.dart';

const maxTerminalThemeBytes = 256 * 1024;

/// Accepts ConsoleCrypt JSON or iTerm XML colour presets only. No paths,
/// commands, fonts or profile settings are loaded from a theme.
TerminalColors parseTerminalTheme(String source) {
  if (utf8.encode(source).length > maxTerminalThemeBytes) {
    throw const FormatException('Theme too large');
  }
  if (source.trimLeft().startsWith('{')) {
    return TerminalColors.fromJson(jsonDecode(source));
  }
  // XML presets may have Apple's public plist DOCTYPE, but never entities
  // or an internal subset. The parser does not fetch external resources.
  if (source.contains('<!ENTITY') || RegExp(r'<!DOCTYPE[^>]*\[').hasMatch(source)) {
    throw const FormatException('Unsupported XML declaration');
  }
  try {
    final doc = XmlDocument.parse(source);
    if (doc.rootElement.name.local != 'plist') throw const FormatException('Not a plist');
    final dict = doc.rootElement.getElement('dict');
    if (dict == null) throw const FormatException('Missing palette');
    Map<String, XmlElement> entries(XmlElement element) {
      final nodes = element.childElements.toList();
      if (nodes.length.isOdd) throw const FormatException('Invalid dictionary');
      final result = <String, XmlElement>{};
      for (var i = 0; i < nodes.length; i += 2) {
        final key = nodes[i];
        if (key.name.local != 'key' || result.containsKey(key.innerText)) {
          throw const FormatException('Invalid dictionary key');
        }
        result[key.innerText] = nodes[i + 1];
      }
      return result;
    }

    final root = entries(dict);
    int color(String name) {
      final node = root[name];
      if (node == null || node.name.local != 'dict') throw const FormatException('Missing colour');
      final values = entries(node);
      int channel(String key) {
        final value = values[key];
        if (value == null || !{'real', 'integer'}.contains(value.name.local)) {
          throw const FormatException('Missing RGB component');
        }
        final number = double.tryParse(value.innerText);
        if (number == null || !number.isFinite || number < 0 || number > 1) {
          throw const FormatException('Invalid RGB component');
        }
        return (number * 255).round();
      }

      return channel('Red Component') << 16 | // l10n-ignore: iTerm plist format key.
          channel('Green Component') << 8 | // l10n-ignore: iTerm plist format key.
          channel('Blue Component'); // l10n-ignore: iTerm plist format keys.
    }

    final foreground = color('Foreground Color'); // l10n-ignore: iTerm plist format keys.
    return TerminalColors({
      'background': color('Background Color'), // l10n-ignore: iTerm plist format keys.
      'foreground': foreground,
      'cursor': root.containsKey('Cursor Color') ? color('Cursor Color') : foreground,
      'selection': root.containsKey('Selection Color') ? color('Selection Color') : color('Ansi 4 Color'),
      for (var i = 0; i < 16; i++) 'ansi$i': color('Ansi $i Color'), // l10n-ignore: iTerm plist format keys.
    });
  } on XmlException {
    throw const FormatException('Invalid iTerm colour preset');
  }
}

String encodeTerminalTheme(TerminalColors colors) => const JsonEncoder.withIndent('  ').convert(colors.toJson());
