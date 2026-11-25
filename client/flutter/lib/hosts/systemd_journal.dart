import 'dart:convert';

/// One journalctl JSON record. Preserve MESSAGE verbatim for the expanded view.
class ServiceLogEntry {
  const ServiceLogEntry({
    required this.raw,
    required this.summary,
    required this.fields,
    this.level,
    this.process = '',
    this.time,
  });
  final String raw, summary, process;
  final Map<String, String> fields;
  final int? level;
  final DateTime? time;

  static final _pairs = RegExp(r'(?:^|\s)([\w.-]+)=("(?:\\.|[^"\\])*"|[^\s"]+)');
  static const _levels = {
    'emerg': 0,
    'emergency': 0,
    'alert': 1,
    'critical': 2,
    'crit': 2,
    'fatal': 2,
    'panic': 2,
    'error': 3,
    'err': 3,
    'warn': 4,
    'warning': 4,
    'notice': 5,
    'info': 6,
    'information': 6,
    'debug': 7,
    'trace': 7,
  };

  static String _text(Object? value) {
    if (value is List && value.every((n) => n is int && n >= 0 && n <= 255)) {
      return utf8.decode(value.cast<int>(), allowMalformed: true);
    }
    return value?.toString() ?? '';
  }

  static Map<String, Object?>? _object(String source) {
    if (!source.trimLeft().startsWith('{')) return null;
    try {
      final decoded = jsonDecode(source);
      return decoded is Map<String, Object?> ? decoded : null;
    } on FormatException {
      return null;
    }
  }

  static ServiceLogEntry parse(String line) {
    final record = _object(line);
    final raw = record != null && record['MESSAGE'] != null ? _text(record['MESSAGE']) : line;
    final fields = _object(raw) ?? <String, Object?>{};
    if (fields.isEmpty && raw.contains('=')) {
      for (final match in _pairs.allMatches(raw)) {
        final key = match[1]!;
        var value = match[2]!;
        if (value.startsWith('"')) {
          try {
            value = jsonDecode(value) as String;
          } on FormatException {
            value = value.substring(1, value.length - 1);
          }
        }
        fields[key] = value;
      }
    }
    final applicationLevel = _text(fields['level'] ?? fields['severity']).toLowerCase();
    final priority = int.tryParse(_text(record?['PRIORITY']));
    final level = _levels[applicationLevel] ?? (priority != null && priority >= 0 && priority <= 7 ? priority : null);
    final message = _text(fields['msg'] ?? fields['message']);
    final error = _text(fields['error']);
    final identifier = _text(record?['SYSLOG_IDENTIFIER'] ?? record?['_COMM']);
    final pid = _text(record?['_PID']);
    final micros = int.tryParse(_text(record?['__REALTIME_TIMESTAMP']));
    return ServiceLogEntry(
      raw: raw,
      summary: message.isNotEmpty
          ? message
          : error.isNotEmpty
          ? error
          : raw,
      level: level,
      fields: {
        for (final field in fields.entries)
          if (!const {'msg', 'message', 'severity', 'level'}.contains(field.key))
            field.key: field.value is String ? field.value as String : jsonEncode(field.value),
      },
      process: pid.isEmpty ? identifier : '$identifier[$pid]',
      time: micros != null && micros >= 0 && micros <= 8640000000000000000
          ? DateTime.fromMicrosecondsSinceEpoch(micros, isUtc: true)
          : null,
    );
  }

  static List<ServiceLogEntry> parseOutput(String output) => [
    for (final line in const LineSplitter().convert(output))
      if (line.isNotEmpty && line != '-- No entries --') parse(line),
  ];
}
