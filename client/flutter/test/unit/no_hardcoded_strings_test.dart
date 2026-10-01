import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// Guards against user-facing English creeping back into widgets: every
/// visible string must come from `lib/l10n/app_*.arb` (ADR-0101 "Localization").
///
/// Scans `lib/` (except generated l10n output, mocks and models) for string
/// literals that are
///  * in a UI position — `Text('…')`, `label: '…'`, `tooltip: '…'`,
///    `hintText: '…'`, snackbars, dialog titles, … — and contain a word, or
///  * sentence-like anywhere (two or more words), e.g. in a ternary.
/// Technical tokens that are never translated are allow-listed below; a line
/// can opt out with a trailing `// l10n-ignore: <reason>` comment.

/// Named arguments / constructors whose string value is shown to the user.
const _uiPositions = [
  r'\bText\(\s*',
  r'\bSelectableText\(\s*',
  r'\bTextSpan\(\s*text:\s*',
  r'\blabel:\s*',
  r'\blabelText:\s*',
  r'\bhintText:\s*',
  r'\bhelperText:\s*',
  r'\berrorText:\s*',
  r'\bprefixText:\s*',
  r'\bsuffixText:\s*',
  r'\btooltip:\s*',
  r'\bmessage:\s*',
  r'\btitle:\s*',
  r'\bsubtitle:\s*',
  r'\bconfirmLabel:\s*',
  r'\bsemanticLabel:\s*',
  r'\bsemanticsLabel:\s*',
  r'\bshowSnack\(\s*context,\s*',
  r'\bbody:\s*',
];

/// Literals that may appear verbatim: protocols, algorithms, products, the
/// brand, font families and example values for technical input.
const _allowed = {
  'ConsoleCrypt',
  'SSH',
  'SFTP',
  'SOCKS5',
  'Ed25519',
  'RSA',
  'OpenSSH',
  'Touch ID',
  'Windows Hello',
  'LM Studio',
  'Ollama',
  'DeepSeek',
  'English',
  'Русский',
  'known_hosts',
  'OK',
  'SF Mono',
  'Cascadia Mono',
  'DejaVu Sans Mono',
  'Segoe UI',
  'Segoe UI Variable Text',
  'Segoe UI Variable Display',
  // Product metadata (lib/app/app_info.dart): proper name and SPDX expression.
  'Alexander Evsikov',
  'AGPL-3.0-only',
};

/// Files owned by another stream, with the reason they are exempt.
const _handoff = {
  'lib/core/glass/glass_budget.dart': 'debug-only performance HUD',
  'lib/core/glass/glass_field.dart': 'assert message (developer-facing)',
  'lib/core/glass/glass_perf_overlay.dart': 'debug-only performance overlay',
  'lib/app/design_gallery_screen.dart': 'debug-only design gallery (kDebugMode, developer tool)',
};

/// Folders that are not UI (generated code, fake data, Rust mirrors).
const _skippedDirs = ['lib/l10n/', 'lib/core/mock/', 'lib/core/models/', 'lib/src/rust/'];

/// Lines that legitimately hold English literals (developer-facing).
final _exemptLine = RegExp(
  r'^\s*(import|export|part)\b|l10n-ignore|Key\(|Error\(|Exception\(|assert\(|debugPrint|RegExp\(|toString\(\)|@Deprecated',
);

final _literal = RegExp(
  r"(r?)'((?:[^'\\\n]|\\.)*)'"
  r'|(r?)"((?:[^"\\\n]|\\.)*)"',
);

/// Removes `${…}` / `$name` interpolations and escapes.
String _stripInterpolation(String s) {
  var out = s.replaceAll(RegExp(r'\$\{[^}]*\}'), ' ');
  out = out.replaceAll(RegExp(r'\$[A-Za-z_]\w*'), ' ');
  return out.replaceAll(RegExp(r'\\.'), ' ');
}

String _clean(String literal) => _stripInterpolation(literal).trim();

bool _hasWord(String text) {
  if (text.isEmpty || _allowed.contains(text)) return false;
  // Keys, routes, asset paths, identifiers: lowercase/snake/kebab, no spaces.
  if (RegExp(r'^[a-z0-9_.\-/:#?=&]+$').hasMatch(text)) return false;
  return RegExp(r'[A-Za-zА-Яа-яЁё]{2,}').hasMatch(text);
}

bool _isSentence(String text) {
  if (_allowed.contains(text)) return false;
  return RegExp(r'[A-Za-zА-Яа-яЁё]{2,}[ ,;:!?…]+[A-Za-zА-Яа-яЁё]{2,}').hasMatch(text) &&
      RegExp('[A-Za-zА-Яа-яЁё]{3,}').hasMatch(text);
}

List<File> _uiSources() =>
    Directory('lib')
        .listSync(recursive: true)
        .whereType<File>()
        .where((f) => f.path.endsWith('.dart'))
        .where((f) => !_skippedDirs.any((d) => f.path.replaceAll(r'\', '/').startsWith(d)))
        .where((f) => !_handoff.containsKey(f.path.replaceAll(r'\', '/')))
        .toList();

void main() {
  test('no hard-coded user-facing strings in lib/ widgets', () {
    final violations = <String>{};
    final position = RegExp('(?:${_uiPositions.join('|')})(?:const\\s+)?');
    for (final file in _uiSources()) {
      final lines = file.readAsLinesSync();
      // Blank out comments so doc examples are ignored (line numbers stay).
      final code = [for (final l in lines) l.trimLeft().startsWith('//') ? '' : l];
      final source = code.join('\n');
      int lineOf(int offset) => '\n'.allMatches(source.substring(0, offset)).length + 1;

      void report(int offset, String literal) {
        final line = lineOf(offset);
        if (_exemptLine.hasMatch(lines[line - 1])) return;
        violations.add('${file.path}:$line: $literal');
      }

      for (final match in position.allMatches(source)) {
        final literal = _literal.matchAsPrefix(source, match.end);
        if (literal == null) continue;
        if (_hasWord(_clean(literal.group(2) ?? literal.group(4) ?? ''))) report(match.start, literal.group(0)!);
      }
      for (final literal in _literal.allMatches(source)) {
        if (_isSentence(_clean(literal.group(2) ?? literal.group(4) ?? ''))) report(literal.start, literal.group(0)!);
      }
    }
    expect(
      violations,
      isEmpty,
      reason: 'Move these strings to lib/l10n/app_en.arb (+ app_ru.arb):\n${violations.join('\n')}',
    );
  });

  test('translatable model enums carry no English display labels', () {
    // Display names live in lib/core/l10n/labels.dart; models keep wire names
    // (technical enums — key algorithms, OS names, snippet languages — keep their token).
    const translatable = [
      'BackupFrequency',
      'PrivacyProfile',
      'CredentialKind',
      'HostKeyPolicy',
      'SshBackend',
      'KnownHostSource',
      'TerminalHistoryMode',
      'ProfileKind',
      'EnableSyncStep',
      'TunnelKind',
      'RiskLevel',
      'SnippetSource',
    ];
    final source = Directory('lib/core/models')
        .listSync()
        .whereType<File>()
        .map((f) => f.readAsStringSync())
        .join('\n');
    for (final name in translatable) {
      final start = source.indexOf('enum $name ');
      expect(start, isNot(-1), reason: name);
      final end = source.indexOf('\n}', start);
      final body = source.substring(start, end);
      expect(body, isNot(contains('final String label')), reason: name);
      expect(body, isNot(contains('final String description')), reason: name);
    }
    expect(source, isNot(contains('String get subtitle')), reason: 'Profile.subtitle → localizedSubtitle');
  });
}
