import 'dart:convert';

enum LogLevel { error, warn, info, debug }

final _levelWord = RegExp(
  r'\b(FATAL|PANIC|CRIT(?:ICAL)?|EMERG|ALERT|ERROR|ERR|WARN(?:ING)?|INFO|NOTICE|DEBUG|TRACE)\b',
  caseSensitive: false,
);
final _jsonLevel = RegExp(r'"(?:level|severity|lvl)"\s*:\s*"([a-zA-Z]+)"', caseSensitive: false);

/// Level from a JSON `level` field or the first level word near the start of the line.
LogLevel? detectLevel(String text) {
  final head = text.length > 240 ? text.substring(0, 240) : text;
  final word = (_jsonLevel.firstMatch(head)?.group(1) ?? _levelWord.firstMatch(head)?.group(1))?.toUpperCase();
  if (word == null) return null;
  if (const ['FATAL', 'PANIC', 'CRIT', 'CRITICAL', 'EMERG', 'ALERT', 'ERROR', 'ERR'].contains(word)) {
    return LogLevel.error;
  }
  if (word.startsWith('WARN')) return LogLevel.warn;
  if (word == 'INFO' || word == 'NOTICE') return LogLevel.info;
  return LogLevel.debug;
}

/// Pretty-printed JSON for a line that is a JSON object, otherwise null.
String? prettyJson(String text) {
  final t = text.trimLeft();
  if (!t.startsWith('{')) return null;
  try {
    final value = jsonDecode(t);
    return value is Map ? const JsonEncoder.withIndent('  ').convert(value) : null;
  } on FormatException {
    return null;
  }
}

final _stamp = RegExp(r'^(\d{4}-\d\d-\d\dT[\d:.]+(?:Z|[+-]\d\d:?\d\d)) ');

/// Splits the `--timestamps` prefix: (local "HH:MM:SS.mmm", rest) or null.
(String, String)? splitTimestamp(String text) {
  final m = _stamp.firstMatch(text);
  if (m == null) return null;
  final t = DateTime.tryParse(m.group(1)!)?.toLocal();
  if (t == null) return null;
  String two(int v) => v.toString().padLeft(2, '0');
  final ms = t.millisecond.toString().padLeft(3, '0');
  return ('${two(t.hour)}:${two(t.minute)}:${two(t.second)}.$ms', text.substring(m.end));
}
