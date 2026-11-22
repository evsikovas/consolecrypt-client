import 'dart:convert';
import 'dart:typed_data';

import 'package:csv/csv.dart';
import 'package:csv/csv_settings_autodetection.dart';

enum KdbxExchangeFormat { csv, json }

const kdbxExchangeMaxBytes = 16 * 1024 * 1024;
const kdbxExchangeMaxEntries = 10000;
const kdbxStandardFields = ['Title', 'UserName', 'Password', 'URL', 'Notes'];

class ExchangeEntry {
  const ExchangeEntry(this.fields, {this.groups = const [], this.custom = const {}});

  final List<String> fields;
  final List<String> groups;
  final Map<String, String> custom;

  Map<String, Object> toJson() => {
    for (var i = 0; i < kdbxStandardFields.length; i++) kdbxStandardFields[i]: fields[i],
    'groups': groups,
    'custom': custom,
  };
}

const _format = 'consolecrypt-passwords';
const _groupHeader = 'consolecrypt group path'; // l10n-ignore: stable CSV column used for export and import
const _csvHeaders = ['title', 'username', 'password', 'url', 'notes', _groupHeader];
const _aliases = [
  ['title', 'name', 'account'],
  ['username', 'user name', 'login name', 'login_username', 'login', 'user'], // l10n-ignore: CSV import header aliases
  ['password', 'login_password'],
  ['url', 'website', 'web site', 'login_uri', 'origin'], // l10n-ignore: CSV import header aliases
  ['notes', 'note', 'comments', 'comment'],
  [_groupHeader, 'group', 'folder', 'category'],
];

final _spreadsheetFormula = RegExp(r'^[\s\x00-\x1f\ufeff]*[=+@\-]|^[\t\r\n]');
String _encodeCell(String value) => value.startsWith("'") || _spreadsheetFormula.hasMatch(value) ? "'$value" : value;
String _decodeCell(String value) => value.startsWith("'") ? value.substring(1) : value;

Uint8List encodeKdbxExchange(KdbxExchangeFormat format, List<ExchangeEntry> entries) {
  if (entries.length > kdbxExchangeMaxEntries) throw const FormatException();
  final text = switch (format) {
    KdbxExchangeFormat.csv => const ListToCsvConverter().convert([
      _csvHeaders,
      for (final entry in entries) [...entry.fields.map(_encodeCell), jsonEncode(entry.groups)],
    ]),
    KdbxExchangeFormat.json => const JsonEncoder.withIndent(
      '  ',
    ).convert({'format': _format, 'version': 1, 'entries': entries.map((entry) => entry.toJson()).toList()}),
  };
  final bytes = Uint8List.fromList(utf8.encode(text));
  if (bytes.length > kdbxExchangeMaxBytes) {
    bytes.fillRange(0, bytes.length, 0);
    throw const FormatException();
  }
  return bytes;
}

List<ExchangeEntry> decodeKdbxExchange(KdbxExchangeFormat format, Uint8List bytes) {
  if (bytes.length > kdbxExchangeMaxBytes) throw const FormatException();
  final decoded = utf8.decode(bytes);
  final text = decoded.startsWith('\ufeff') ? decoded.substring(1) : decoded;
  final entries = switch (format) {
    KdbxExchangeFormat.csv => _decodeCsv(text),
    KdbxExchangeFormat.json => _decodeJson(text),
  };
  if (entries.length > kdbxExchangeMaxEntries) throw const FormatException();
  for (final entry in entries) {
    if (entry.groups.length > 32 || entry.groups.any((name) => name.isEmpty || name.length > 512)) {
      throw const FormatException();
    }
    final names = kdbxStandardFields.map((field) => field.toLowerCase()).toSet();
    for (final name in entry.custom.keys) {
      if (name.trim().isEmpty || name.length > 256 || !names.add(name.trim().toLowerCase())) {
        throw const FormatException();
      }
    }
  }
  return entries;
}

List<ExchangeEntry> _decodeCsv(String text) {
  final firstLine = text.split(RegExp(r'\r\n|\n|\r')).first;
  const detector = FirstOccurrenceSettingsDetector(eols: ['\r\n', '\n', '\r'], fieldDelimiters: [',', ';', '\t']);
  final data = firstLine.toLowerCase() == 'sep=;' ? text.substring(firstLine.length).trimLeft() : text;
  final rows = const CsvToListConverter(
    shouldParseNumbers: false,
    allowInvalid: false,
    csvSettingsDetector: detector,
  ).convert(data);
  if (rows.isEmpty || rows.first.length > 128) throw const FormatException();
  final headers = rows.first.map((cell) => (cell as String).trim().toLowerCase()).toList();
  final columns = [for (final candidates in _aliases) headers.indexWhere(candidates.contains)];
  if (columns[0] < 0 || columns[2] < 0) throw const FormatException();
  final ownFormat =
      headers.length == _csvHeaders.length &&
      List.generate(headers.length, (i) => headers[i] == _csvHeaders[i]).every((match) => match);
  final entries = <ExchangeEntry>[];
  for (final row in rows.skip(1)) {
    if (row.every((cell) => cell == '')) continue;
    if (row.length != headers.length || entries.length == kdbxExchangeMaxEntries) throw const FormatException();
    String field(int column) => columns[column] < 0 ? '' : row[columns[column]] as String;
    final group = field(5);
    entries.add(
      ExchangeEntry([
        for (var i = 0; i < 5; i++) ownFormat ? _decodeCell(field(i)) : field(i),
      ], groups: ownFormat ? _strings(jsonDecode(group)) : (group.isEmpty ? const [] : [group])),
    );
  }
  return entries;
}

List<ExchangeEntry> _decodeJson(String text) {
  final payload = jsonDecode(text);
  if (payload is! Map || payload['format'] != _format || payload['version'] != 1 || payload['entries'] is! List) {
    throw const FormatException();
  }
  final rows = payload['entries'] as List;
  if (rows.length > kdbxExchangeMaxEntries) throw const FormatException();
  return [
    for (final row in rows)
      if (row is Map && row['custom'] is Map)
        ExchangeEntry(
          [for (final key in kdbxStandardFields) row[key] as String],
          groups: _strings(row['groups']),
          custom: Map<String, String>.from(row['custom'] as Map),
        )
      else
        throw const FormatException(),
  ];
}

List<String> _strings(Object? value) => value is List ? List<String>.from(value) : (throw const FormatException());
