import 'package:consolecrypt/automation/ansible_editor_folding.dart';
import 'package:material_ui/material_ui.dart';

enum AnsibleTokenKind {
  key,
  keyword,
  module,
  string,
  scalar,
  comment,
  anchor,
  jinjaDelimiter,
  jinjaExpression,
  jinjaFilter,
}

class AnsibleSyntaxToken {
  const AnsibleSyntaxToken(this.start, this.end, this.kind);
  final int start, end;
  final AnsibleTokenKind kind;
}

const _directives = {
  'name',
  'hosts',
  'tasks',
  'pre_tasks',
  'post_tasks',
  'handlers',
  'roles',
  'vars',
  'vars_files',
  'become',
  'become_user',
  'when',
  'changed_when',
  'failed_when',
  'loop',
  'with_items',
  'register',
  'notify',
  'tags',
  'block',
  'rescue',
  'always',
  'delegate_to',
  'run_once',
  'no_log',
  'check_mode',
  'environment',
  'collections',
  'import_tasks',
  'include_tasks',
  'include_role',
  'import_role',
};
final _yamlKey = RegExp(r'''^(\s*(?:-\s+)?)([A-Za-z_][\w.-]*)(\s*:)(?=\s|$)''');
final _scalar = RegExp(
  r'(?:true|false|yes|no|on|off|null|none|~|[-+]?(?:0x[0-9a-f]+|\d+(?:\.\d+)?(?:e[-+]?\d+)?))(?=$|[\s,\]}#])',
  caseSensitive: false,
);
final _anchor = RegExp(r'[&*!][A-Za-z_][\w.:-]*');
final _scalarBoundary = RegExp(r'[\s\[,{:]');

/// Bounded lexical coloring, not YAML/Jinja evaluation or validation.
List<AnsibleSyntaxToken> ansibleSyntaxTokens(String source) {
  if (source.length > 200000) return const [];
  final out = <AnsibleSyntaxToken>[];
  void token(int start, int end, AnsibleTokenKind kind) {
    if (end > start) out.add(AnsibleSyntaxToken(start, end, kind));
  }

  int jinja(String line, int i, int base) {
    final opener = line.substring(i, i + 2);
    final closer = opener == '{{'
        ? '}}'
        : opener == '{%'
        ? '%}'
        : '#}';
    final found = line.indexOf(closer, i + 2), end = found < 0 ? line.length : found;
    token(base + i, base + i + 2, AnsibleTokenKind.jinjaDelimiter);
    if (opener == '{#') {
      token(base + i + 2, base + end, AnsibleTokenKind.comment);
    } else {
      var cursor = i + 2;
      for (final filter in RegExp(r'\|\s*([A-Za-z_]\w*)').allMatches(line.substring(cursor, end))) {
        final filterStart = i + 2 + filter.start, filterEnd = i + 2 + filter.end;
        token(base + cursor, base + filterStart, AnsibleTokenKind.jinjaExpression);
        token(base + filterStart, base + filterEnd, AnsibleTokenKind.jinjaFilter);
        cursor = filterEnd;
      }
      token(base + cursor, base + end, AnsibleTokenKind.jinjaExpression);
    }
    if (found >= 0) token(base + end, base + end + 2, AnsibleTokenKind.jinjaDelimiter);
    return found < 0 ? line.length : found + 2;
  }

  bool opens(String line, int i) =>
      i + 1 < line.length && (line.startsWith('{{', i) || line.startsWith('{%', i) || line.startsWith('{#', i));
  int? literalIndent;
  for (final line in ansibleSourceLines(source)) {
    final text = line.text;
    final literal = literalIndent != null && (text.trim().isEmpty || line.indent > literalIndent);
    if (!literal) literalIndent = null;
    var i = 0;
    final key = literal ? null : _yamlKey.firstMatch(text);
    if (key != null) {
      final name = key.group(2)!;
      final start = key.group(1)!.length;
      token(
        line.start + start,
        line.start + start + name.length,
        _directives.contains(name)
            ? AnsibleTokenKind.keyword
            : name.split('.').length >= 3
            ? AnsibleTokenKind.module
            : AnsibleTokenKind.key,
      );
      i = key.end;
    }
    if (literal) {
      var from = 0;
      while (i < text.length) {
        if (!opens(text, i)) {
          i++;
          continue;
        }
        token(line.start + from, line.start + i, AnsibleTokenKind.string);
        i = jinja(text, i, line.start);
        from = i;
      }
      token(line.start + from, line.start + text.length, AnsibleTokenKind.string);
      continue;
    }
    while (i < text.length) {
      if (opens(text, i)) {
        i = jinja(text, i, line.start);
        continue;
      }
      final c = text[i];
      if (c == '#' && (i == 0 || text[i - 1].trim().isEmpty)) {
        token(line.start + i, line.start + text.length, AnsibleTokenKind.comment);
        break;
      }
      if (c == '"' || c == "'") {
        final quote = c;
        var from = i++;
        while (i < text.length) {
          if (opens(text, i)) {
            token(line.start + from, line.start + i, AnsibleTokenKind.string);
            i = jinja(text, i, line.start);
            from = i;
            continue;
          }
          if (quote == '"' && text[i] == r'\') {
            i = (i + 2).clamp(0, text.length);
            continue;
          }
          if (text[i] == quote) {
            if (quote == "'" && i + 1 < text.length && text[i + 1] == quote) {
              i += 2;
              continue;
            }
            i++;
            break;
          }
          i++;
        }
        token(line.start + from, line.start + i, AnsibleTokenKind.string);
        continue;
      }
      if ((c == '&' || c == '*' || c == '!') && (i == 0 || text[i - 1].trim().isEmpty)) {
        final match = _anchor.matchAsPrefix(text, i);
        if (match != null) {
          token(line.start + i, line.start + match.end, AnsibleTokenKind.anchor);
          i = match.end;
          continue;
        }
      }
      if (i == 0 || _scalarBoundary.hasMatch(text[i - 1])) {
        final match = _scalar.matchAsPrefix(text, i);
        if (match != null) {
          token(line.start + i, line.start + match.end, AnsibleTokenKind.scalar);
          i = match.end;
          continue;
        }
      }
      i++;
    }
    if (RegExp(r':\s*[|>][+\-0-9]*\s*(?:#.*)?$').hasMatch(text)) literalIndent = line.indent;
  }
  return out;
}

TextSpan ansibleSyntaxSpan(String text, TextStyle? style, ColorScheme colors, {bool enabled = true}) {
  if (!enabled) return TextSpan(text: text, style: style);
  final spans = <InlineSpan>[];
  var cursor = 0;
  for (final token in ansibleSyntaxTokens(text)) {
    if (token.start > cursor) spans.add(TextSpan(text: text.substring(cursor, token.start)));
    final color = switch (token.kind) {
      AnsibleTokenKind.comment => colors.onSurfaceVariant,
      AnsibleTokenKind.string => colors.tertiary,
      AnsibleTokenKind.scalar || AnsibleTokenKind.anchor => colors.secondary,
      AnsibleTokenKind.jinjaDelimiter || AnsibleTokenKind.jinjaFilter => colors.error,
      AnsibleTokenKind.jinjaExpression => colors.tertiary,
      _ => colors.primary,
    };
    spans.add(
      TextSpan(
        text: text.substring(token.start, token.end),
        style: TextStyle(
          color: color,
          fontWeight: token.kind == AnsibleTokenKind.keyword || token.kind == AnsibleTokenKind.module
              ? FontWeight.w600
              : null,
          fontStyle: token.kind == AnsibleTokenKind.comment ? FontStyle.italic : null,
        ),
      ),
    );
    cursor = token.end;
  }
  if (cursor < text.length) spans.add(TextSpan(text: text.substring(cursor)));
  return TextSpan(style: style, children: spans);
}

class AnsibleSyntaxController extends TextEditingController {
  AnsibleSyntaxController({super.text});
  bool highlight = true;
  @override
  TextSpan buildTextSpan({required BuildContext context, TextStyle? style, required bool withComposing}) {
    if (withComposing && value.isComposingRangeValid && !value.composing.isCollapsed) {
      return super.buildTextSpan(context: context, style: style, withComposing: withComposing);
    }
    return ansibleSyntaxSpan(text, style, Theme.of(context).colorScheme, enabled: highlight);
  }
}
