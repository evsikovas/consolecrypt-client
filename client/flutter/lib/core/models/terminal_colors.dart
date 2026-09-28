/// Device-local, opaque RGB terminal colours. No executable theme content.
final class TerminalColors {
  TerminalColors(Map<String, int> colors) : colors = Map.unmodifiable(colors) {
    if (keys.any((key) => !colors.containsKey(key) || colors[key]! < 0 || colors[key]! > 0xFFFFFF)) {
      throw const FormatException('Incomplete terminal palette');
    }
  }

  static const keys = [
    'background',
    'foreground',
    'cursor',
    'selection',
    'ansi0',
    'ansi1',
    'ansi2',
    'ansi3',
    'ansi4',
    'ansi5',
    'ansi6',
    'ansi7',
    'ansi8',
    'ansi9',
    'ansi10',
    'ansi11',
    'ansi12',
    'ansi13',
    'ansi14',
    'ansi15',
  ];
  final Map<String, int> colors;
  int operator [](String key) => colors[key]!;
  TerminalColors withColor(String key, int value) => TerminalColors({...colors, key: value});
  Map<String, Object> toJson() => {
    'version': 1,
    'colors': {for (final key in keys) key: hex(colors[key]!)},
  };
  static String hex(int rgb) => '#${rgb.toRadixString(16).padLeft(6, '0').toUpperCase()}';
  factory TerminalColors.fromJson(Object? value) {
    if (value is! Map || value['version'] != 1 || value['colors'] is! Map) {
      throw const FormatException('Unsupported terminal palette');
    }
    final values = value['colors'] as Map;
    final colors = <String, int>{};
    for (final key in keys) {
      final hex = values[key];
      if (hex is! String || !RegExp(r'^#[0-9a-fA-F]{6}$').hasMatch(hex)) {
        throw const FormatException('Invalid terminal colour');
      }
      colors[key] = int.parse(hex.substring(1), radix: 16);
    }
    return TerminalColors(colors);
  }

  static TerminalColors? tryFromJson(Object? value) {
    try {
      return TerminalColors.fromJson(value);
    } on FormatException {
      return null;
    }
  }
}
