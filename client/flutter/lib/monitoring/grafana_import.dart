import 'dart:convert';

part 'grafana_v2.dart';

/// Offline review only. This parser cannot read a file, contact Grafana, resolve
/// a datasource, execute a query or persist a ConsoleCrypt panel.
enum GrafanaImportProblem {
  invalidJson,
  inputTooLarge,
  tooDeep,
  duplicateKey,
  invalidDashboard,
  unsupportedSchema,
  invalidPanel,
  tooManyPanels,
  unsupportedPanel,
  unsupportedDatasource,
  missingExpression,
  multipleTargets,
  hiddenTarget,
  variables,
  transformations,
  repeat,
  timeOverride,
  legacyAlert,
  unitHintOnly,
  presentationNotImported,
}

final class GrafanaImportException implements Exception {
  const GrafanaImportException(this.problem);
  final GrafanaImportProblem problem;
}

final class GrafanaPanelDraft {
  const GrafanaPanelDraft({
    required this.title,
    required this.expression,
    required this.datasourceHint,
    required this.instant,
    required this.unitHint,
  });
  final String title, expression;

  /// Untrusted display hint. The user must select an existing Native source;
  /// matching a Grafana UID/name never establishes host/source authority.
  final String? datasourceHint;
  final bool instant;

  /// Display metadata only; no sample conversion or verified-unit claim.
  final String? unitHint;
}

final class GrafanaPanelReview {
  GrafanaPanelReview({
    required this.path,
    required this.title,
    required Iterable<GrafanaImportProblem> problems,
    this.draft,
  }) : problems = List.unmodifiable(problems);
  final String path, title;
  final List<GrafanaImportProblem> problems;
  final GrafanaPanelDraft? draft;
  bool get supported => draft != null;
}

final class GrafanaImportReport {
  GrafanaImportReport({
    required this.originalJson,
    required this.title,
    required this.schemaVersion,
    required Iterable<GrafanaPanelReview> panels,
    required Iterable<GrafanaImportProblem> problems,
    required this.variables,
  }) : panels = List.unmodifiable(panels),
       problems = List.unmodifiable(problems);

  /// Exact original input, including unsupported panels/variables. Memory only;
  /// callers must not log it or silently overwrite the original file.
  final String originalJson, title;
  final int? schemaVersion;
  final List<GrafanaPanelReview> panels;
  final List<GrafanaImportProblem> problems;

  /// Names only, never current/saved variable values or substitution commands.
  final List<String> variables;
  Iterable<GrafanaPanelReview> get supported =>
      panels.where((p) => p.supported);
  Iterable<GrafanaPanelReview> get unsupported =>
      panels.where((p) => !p.supported);
}

Never _fail(GrafanaImportProblem value) => throw GrafanaImportException(value);
bool _display(String value, int max) =>
    value.isNotEmpty &&
    value.trim() == value &&
    value.runes.length <= max &&
    !value.runes.any((r) => r < 32 || r >= 127 && r <= 159);
Map<String, dynamic>? _map(Object? value) =>
    value is Map<String, dynamic> ? value : null;

GrafanaImportReport inspectGrafanaDashboard(String input) {
  const maxBytes = 2 * 1024 * 1024;
  if (input.length > maxBytes || utf8.encode(input).length > maxBytes) {
    _fail(GrafanaImportProblem.inputTooLarge);
  }
  _JsonShape(input).check();
  Object? decoded;
  try {
    decoded = jsonDecode(input);
  } on FormatException {
    _fail(GrafanaImportProblem.invalidJson);
  }
  var dashboard = _map(decoded);
  if (dashboard == null) _fail(GrafanaImportProblem.invalidDashboard);
  if (dashboard.containsKey('apiVersion') || dashboard.containsKey('kind')) {
    if (dashboard['apiVersion'] == 'dashboard.grafana.app/v2' &&
        dashboard['kind'] == 'Dashboard') {
      return _inspectV2(input, dashboard);
    }
    if (dashboard['apiVersion'] != 'dashboard.grafana.app/v1' ||
        dashboard['kind'] != 'Dashboard') {
      return _unsupportedDashboard(input);
    }
    dashboard = _map(dashboard['spec']);
    if (dashboard == null) _fail(GrafanaImportProblem.invalidDashboard);
  } else if (dashboard.containsKey('dashboard')) {
    dashboard = _map(dashboard['dashboard']);
    if (dashboard == null) _fail(GrafanaImportProblem.invalidDashboard);
  }
  final schema = dashboard['schemaVersion'];
  if (schema is! int || schema < 1 || schema > 42) {
    return _unsupportedDashboard(input);
  }
  final title = dashboard['title'];
  final rawPanels = dashboard['panels'];
  if (title is! String || !_display(title, 240) || rawPanels is! List) {
    _fail(GrafanaImportProblem.invalidDashboard);
  }
  final variables = <String>[];
  final templating = _map(dashboard['templating']);
  if (templating != null && templating['list'] is List) {
    for (final entry in templating['list'] as List) {
      final name = _map(entry)?['name'];
      if (name is String && _display(name, 128)) variables.add(name);
    }
  }
  final panels = <GrafanaPanelReview>[];
  void visit(List<dynamic> entries, String parent, int depth) {
    if (depth > 4) _fail(GrafanaImportProblem.tooDeep);
    for (var i = 0; i < entries.length; i++) {
      if (panels.length >= 512) _fail(GrafanaImportProblem.tooManyPanels);
      final path = '$parent/$i', panel = _map(entries[i]);
      if (panel == null) _fail(GrafanaImportProblem.invalidPanel);
      panels.add(_panel(panel, path));
      final children = panel['panels'];
      if (children != null) {
        if (children is! List) _fail(GrafanaImportProblem.invalidPanel);
        visit(children, '$path/panels', depth + 1);
      }
    }
  }

  visit(rawPanels, 'panels', 0);
  return GrafanaImportReport(
    originalJson: input,
    title: title,
    schemaVersion: schema,
    panels: panels,
    variables: List.unmodifiable(variables),
    problems: [
      GrafanaImportProblem.presentationNotImported,
      if (variables.isNotEmpty) GrafanaImportProblem.variables,
    ],
  );
}

GrafanaImportReport _unsupportedDashboard(String original) =>
    GrafanaImportReport(
      originalJson: original,
      title: '',
      schemaVersion: null,
      panels: const [],
      variables: const [],
      problems: const [GrafanaImportProblem.unsupportedSchema],
    );

GrafanaPanelReview _panel(Map<String, dynamic> panel, String path) {
  final rawTitle = panel['title'];
  final title = rawTitle is String && _display(rawTitle, 120) ? rawTitle : '';
  final issues = <GrafanaImportProblem>[];
  if (title.isEmpty) issues.add(GrafanaImportProblem.invalidPanel);
  if (!const {'timeseries', 'graph', 'stat'}.contains(panel['type'])) {
    issues.add(GrafanaImportProblem.unsupportedPanel);
  }
  if (panel['repeat'] != null || panel['repeatDirection'] != null) {
    issues.add(GrafanaImportProblem.repeat);
  }
  if (panel['timeFrom'] != null || panel['timeShift'] != null) {
    issues.add(GrafanaImportProblem.timeOverride);
  }
  if (panel['alert'] != null) issues.add(GrafanaImportProblem.legacyAlert);
  final transforms = panel['transformations'];
  if (transforms != null && (transforms is! List || transforms.isNotEmpty)) {
    issues.add(GrafanaImportProblem.transformations);
  }
  final targets = panel['targets'];
  Map<String, dynamic>? target;
  if (targets is List && targets.length == 1) {
    target = _map(targets.single);
    if (target == null) issues.add(GrafanaImportProblem.invalidPanel);
  } else {
    issues.add(
      targets is List && targets.length > 1
          ? GrafanaImportProblem.multipleTargets
          : GrafanaImportProblem.missingExpression,
    );
  }
  if (target?['hide'] == true) issues.add(GrafanaImportProblem.hiddenTarget);
  final expression = target?['expr'];
  if (expression is! String ||
      expression.trim().isEmpty ||
      utf8.encode(expression).length > 16384 ||
      expression.runes.any(
        (r) => (r < 32 && r != 9 && r != 10 && r != 13) || r >= 127 && r <= 159,
      )) {
    issues.add(GrafanaImportProblem.missingExpression);
  } else if (RegExp(r'\$(?:[A-Za-z_]|\{)|\[\[[^\]\r\n]+\]\]')
      .hasMatch(expression)) {
    // No macro/default substitution, including $__rate_interval and $node.
    issues.add(GrafanaImportProblem.variables);
  }
  final rawSource = target?['datasource'] ?? panel['datasource'];
  final source = _map(rawSource);
  String? hint;
  if (source == null ||
      source['type'] != 'prometheus' ||
      source['uid'] is! String ||
      !_display(source['uid'] as String, 128) ||
      (source['uid'] as String).contains(r'$')) {
    issues.add(GrafanaImportProblem.unsupportedDatasource);
  } else {
    hint = source['uid'] as String;
  }
  final rawUnit = _map(_map(panel['fieldConfig'])?['defaults'])?['unit'];
  final unit = rawUnit is String && _display(rawUnit, 64) ? rawUnit : null;
  return GrafanaPanelReview(
    path: path,
    title: title,
    problems: [...issues, if (unit != null) GrafanaImportProblem.unitHintOnly],
    draft: issues.isEmpty
        ? GrafanaPanelDraft(
            title: title,
            expression: expression as String,
            datasourceHint: hint,
            instant: target?['instant'] == true,
            unitHint: unit,
          )
        : null,
  );
}

/// Bound nesting before jsonDecode and reject duplicate object keys before any
/// datasource/expression can be selected. JSON grammar remains jsonDecode's job.
final class _JsonShape {
  _JsonShape(this.text);
  final String text;
  int at = 0;
  void space() {
    while (at < text.length &&
        const [32, 9, 10, 13].contains(text.codeUnitAt(at))) {
      at++;
    }
  }

  void check() {
    value(0);
    space();
    if (at != text.length) _fail(GrafanaImportProblem.invalidJson);
  }

  String string() {
    final start = at++;
    while (at < text.length) {
      final ch = text.codeUnitAt(at++);
      if (ch == 92) {
        at++;
        continue;
      }
      if (ch == 34) {
        try {
          return jsonDecode(text.substring(start, at)) as String;
        } on FormatException {
          _fail(GrafanaImportProblem.invalidJson);
        }
      }
    }
    _fail(GrafanaImportProblem.invalidJson);
  }

  void value(int depth) {
    if (depth > 32) _fail(GrafanaImportProblem.tooDeep);
    space();
    if (at >= text.length) _fail(GrafanaImportProblem.invalidJson);
    final ch = text.codeUnitAt(at);
    if (ch == 34) {
      string();
      return;
    }
    if (ch == 123 || ch == 91) {
      final object = ch == 123, end = object ? 125 : 93, keys = <String>{};
      at++;
      space();
      if (at < text.length && text.codeUnitAt(at) == end) {
        at++;
        return;
      }
      while (true) {
        if (object) {
          space();
          if (at >= text.length || text.codeUnitAt(at) != 34) {
            _fail(GrafanaImportProblem.invalidJson);
          }
          if (!keys.add(string())) _fail(GrafanaImportProblem.duplicateKey);
          space();
          if (at >= text.length || text.codeUnitAt(at++) != 58) {
            _fail(GrafanaImportProblem.invalidJson);
          }
        }
        value(depth + 1);
        space();
        if (at >= text.length) _fail(GrafanaImportProblem.invalidJson);
        final next = text.codeUnitAt(at++);
        if (next == end) return;
        if (next != 44) _fail(GrafanaImportProblem.invalidJson);
      }
    }
    final start = at;
    while (at < text.length &&
        !const [32, 9, 10, 13, 44, 93, 125].contains(text.codeUnitAt(at))) {
      at++;
    }
    if (at == start) _fail(GrafanaImportProblem.invalidJson);
  }
}
