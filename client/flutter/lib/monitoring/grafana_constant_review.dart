import 'dart:convert';

import 'package:consolecrypt/monitoring/grafana_import.dart';

/// Explicit, manual review only. No saved/current Grafana variable value is
/// read, no variable query is run, and datasource identifiers are never expanded.
enum GrafanaConstantProblem {
  unsupportedPanel,
  unknownPanel,
  invalidBindings,
  missingValue,
  unsupportedVariable,
  unsupportedContext,
  budget,
}

final class GrafanaConstantException implements Exception {
  const GrafanaConstantException(this.problem);
  final GrafanaConstantProblem problem;
}

final class GrafanaConstantReview {
  GrafanaConstantReview._({
    required this.original,
    required this.panelPath,
    required this.draft,
    required Iterable<String> names,
  }) : names = List.unmodifiable(names);

  /// Exact original dashboard, including every unsupported panel/variable.
  final GrafanaImportReport original;
  final String panelPath;
  final GrafanaPanelDraft draft;

  /// Resolved names only; values appear solely in the reviewed literal query.
  final List<String> names;
}

final _name = RegExp(r'^[A-Za-z_][A-Za-z0-9_]{0,127}$');
final _variable = RegExp(r'\$(?:[A-Za-z_]|\{)|\[\[[^\]\r\n]+\]\]');
final _wholeVariable = RegExp(
  r'^(?:\$([A-Za-z_][A-Za-z0-9_]*)|\$\{([A-Za-z_][A-Za-z0-9_]*)(?::(regex))?\}|\[\[([A-Za-z_][A-Za-z0-9_]*)\]\])$',
);
Never _deny(GrafanaConstantProblem p) => throw GrafanaConstantException(p);

Map<String, dynamic> _dashboard(Object? value) {
  var data = value as Map<String, dynamic>;
  if (data['kind'] == 'Dashboard') {
    data = data['spec'] as Map<String, dynamic>;
  } else if (data.containsKey('dashboard')) {
    data = data['dashboard'] as Map<String, dynamic>;
  }
  return data;
}

/// Returns one supported panel draft only after exact offline source inspection
/// and explicit values. The caller still chooses a Native source/revision and
/// saves manually through the existing admission/CAS path.
GrafanaConstantReview reviewGrafanaConstants(String input, String panelPath, Map<String, String> values) {
  final original = inspectGrafanaDashboard(input);
  final selected = original.panels.where((p) => p.path == panelPath).firstOrNull;
  if (selected == null) _deny(GrafanaConstantProblem.unknownPanel);
  if (values.length > 64) _deny(GrafanaConstantProblem.budget);
  for (final entry in values.entries) {
    if (!_name.hasMatch(entry.key) ||
        entry.key.startsWith('__') ||
        entry.value.length > 512 ||
        utf8.encode(entry.value).length > 512 ||
        entry.value.runes.any((r) => r < 32 || r >= 127 && r <= 159) ||
        _variable.hasMatch(entry.value)) {
      _deny(GrafanaConstantProblem.invalidBindings);
    }
  }
  final dashboard = _dashboard(jsonDecode(input));
  final declarations = <String, String>{};
  final templating = dashboard['templating'];
  if (templating is Map && templating['list'] is List) {
    for (final entry in templating['list'] as List) {
      if (entry is! Map || entry['name'] is! String || entry['type'] is! String) continue;
      final name = entry['name'] as String;
      if (declarations.containsKey(name)) _deny(GrafanaConstantProblem.invalidBindings);
      declarations[name] = entry['type'] as String;
    }
  }
  for (final name in values.keys) {
    if (declarations[name] != 'constant') _deny(GrafanaConstantProblem.unsupportedVariable);
  }
  if (selected.problems.any((p) => p != GrafanaImportProblem.variables && p != GrafanaImportProblem.unitHintOnly)) {
    _deny(GrafanaConstantProblem.unsupportedPanel);
  }
  // The path originates only from the bounded parser's own row traversal.
  Object? node = dashboard;
  final segments = panelPath.split('/');
  for (var i = 0; i < segments.length; i += 2) {
    if (node is! Map || segments[i] != 'panels' || node['panels'] is! List) {
      _deny(GrafanaConstantProblem.unknownPanel);
    }
    node = (node['panels'] as List)[int.parse(segments[i + 1])];
  }
  final panel = node as Map<String, dynamic>;
  final target = (panel['targets'] as List).single as Map<String, dynamic>;
  final expression = target['expr'] as String;
  final used = <String>{};
  final expanded = _expand(expression, values, declarations, used);
  // Reuse the original parser for every panel/type/datasource/query budget
  // rule after substitution. No other field is changed or silently inferred.
  final copied = jsonDecode(jsonEncode(panel)) as Map<String, dynamic>;
  final copiedTarget = (copied['targets'] as List).single as Map<String, dynamic>;
  copiedTarget['expr'] = expanded;
  final checked = inspectGrafanaDashboard(
    jsonEncode({
      'schemaVersion': original.schemaVersion,
      'title': original.title,
      'panels': [copied],
    }),
  ).panels.single.draft;
  if (checked == null) _deny(GrafanaConstantProblem.unsupportedPanel);
  return GrafanaConstantReview._(original: original, panelPath: panelPath, draft: checked, names: used);
}

String _expand(String expression, Map<String, String> values, Map<String, String> declarations, Set<String> used) {
  final out = StringBuffer(), prefix = StringBuffer();
  var at = 0, braces = 0;
  while (at < expression.length) {
    final quote = expression.codeUnitAt(at);
    if (quote == 35) {
      final end = expression.indexOf('\n', at);
      final stop = end < 0 ? expression.length : end;
      out.write(expression.substring(at, stop));
      prefix.write(' ');
      at = stop;
      continue;
    }
    if (quote != 34 && quote != 39 && quote != 96) {
      if (quote == 123) braces++;
      if (quote == 125) braces--;
      if (quote == 123 || quote == 125 || quote == 44) prefix.clear();
      prefix.writeCharCode(quote);
      out.writeCharCode(quote);
      at++;
      continue;
    }
    final start = at++;
    var closed = false;
    while (at < expression.length) {
      final ch = expression.codeUnitAt(at++);
      if (ch == 92 && quote != 96) {
        at++;
        continue;
      }
      if (ch == quote) {
        closed = true;
        break;
      }
    }
    if (!closed || at > expression.length) {
      _deny(GrafanaConstantProblem.unsupportedContext);
    }
    final contents = expression.substring(start + 1, at - 1), token = _wholeVariable.firstMatch(contents);
    if (token == null) {
      out.write(expression.substring(start, at));
      prefix.write(expression.substring(start, at));
      continue;
    }
    final matcher = _matcherOperator(prefix.toString(), prefix.length);
    if (quote != 34 || braces != 1 || matcher == null) _deny(GrafanaConstantProblem.unsupportedContext);
    final name = token.group(1) ?? token.group(2) ?? token.group(4)!;
    if (name.startsWith('__') || declarations[name] != 'constant') {
      _deny(GrafanaConstantProblem.unsupportedVariable);
    }
    final value = values[name];
    if (value == null) _deny(GrafanaConstantProblem.missingValue);
    final regex = matcher.endsWith('~');
    if (token.group(3) != null && !regex) _deny(GrafanaConstantProblem.unsupportedContext);
    out.write(jsonEncode(regex ? RegExp.escape(value) : value));
    prefix.write(expression.substring(start, at));
    used.add(name);
    if (out.length > 16384) _deny(GrafanaConstantProblem.budget);
  }
  final expanded = out.toString();
  if (braces != 0 || _variable.hasMatch(expanded)) _deny(GrafanaConstantProblem.unsupportedContext);
  if (utf8.encode(expanded).length > 16384) _deny(GrafanaConstantProblem.budget);
  return expanded;
}

/// Scan only the immediate matcher prefix backwards. Do not run a searching
/// regexp repeatedly over an untrusted 16KiB query prefix.
String? _matcherOperator(String expression, int beforeQuote) {
  var at = beforeQuote - 1;
  bool space(int c) => c == 32 || c == 9 || c == 10 || c == 13;
  bool alpha(int c) => c == 95 || c >= 65 && c <= 90 || c >= 97 && c <= 122;
  bool name(int c) => alpha(c) || c >= 48 && c <= 57;
  void skipSpace() {
    while (at >= 0 && space(expression.codeUnitAt(at))) {
      at--;
    }
  }

  skipSpace();
  if (at < 0) return null;
  String operator;
  if (expression.codeUnitAt(at) == 126) {
    if (at < 1 || !const [61, 33].contains(expression.codeUnitAt(at - 1))) return null;
    operator = expression.substring(at - 1, at + 1);
    at -= 2;
  } else if (expression.codeUnitAt(at) == 61) {
    operator = at > 0 && expression.codeUnitAt(at - 1) == 33 ? '!=' : '=';
    at -= operator.length;
  } else {
    return null;
  }
  skipSpace();
  final end = at;
  while (at >= 0 && name(expression.codeUnitAt(at))) {
    at--;
  }
  if (end == at || !alpha(expression.codeUnitAt(at + 1))) return null;
  skipSpace();
  return at >= 0 && const [123, 44].contains(expression.codeUnitAt(at)) ? operator : null;
}
