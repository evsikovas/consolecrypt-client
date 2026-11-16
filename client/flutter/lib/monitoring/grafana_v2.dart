part of 'grafana_import.dart';

// Grafana v2 is a different structure, not a higher classic schemaVersion.
// Review individual queries only. Layout, links, variable current values,
// transformations and Grafana datasource names never become execution authority.
GrafanaImportReport _inspectV2(String input, Map<String, dynamic> resource) {
  final spec = _map(resource['spec']);
  final title = spec?['title'];
  final elements = _map(spec?['elements']);
  if (spec == null || title is! String || !_display(title, 240) || elements == null) {
    _fail(GrafanaImportProblem.invalidDashboard);
  }
  if (elements.length > 512) _fail(GrafanaImportProblem.tooManyPanels);
  final variables = <String>[];
  final declared = spec['variables'];
  if (declared != null && declared is! List) {
    _fail(GrafanaImportProblem.invalidDashboard);
  }
  for (final variable in (declared as List? ?? const [])) {
    final name = _map(_map(variable)?['spec'])?['name'];
    if (name is String && _display(name, 128)) variables.add(name);
  }
  // Repetition can be inherited from a row/tab and affects query semantics.
  // Follow references within the bounded, already-decoded layout only. A
  // repeated row must not block unrelated panels elsewhere in the dashboard.
  final repeatedElements = <String>{};
  void findRepeated(Object? value, bool inherited) {
    if (value is Map) {
      final repeated = inherited || value['repeat'] != null;
      if (repeated && value['kind'] == 'ElementReference' && value['name'] is String) {
        repeatedElements.add(value['name'] as String);
      }
      for (final child in value.values) {
        findRepeated(child, repeated);
      }
    } else if (value is List) {
      for (final child in value) {
        findRepeated(child, inherited);
      }
    }
  }

  findRepeated(spec['layout'], false);
  final panels = <GrafanaPanelReview>[];
  for (final entry in elements.entries) {
    // This name becomes a visible panel path. Keep attacker-sized keys out of
    // the UI even though the original dashboard remains byte-for-byte intact.
    if (!_display(entry.key, 128)) _fail(GrafanaImportProblem.invalidPanel);
    // RFC6901 escaping keeps arbitrary element names distinct and unambiguous.
    final path = 'elements/${entry.key.replaceAll('~', '~0').replaceAll('/', '~1')}';
    final element = _map(entry.value);
    final panel = _map(element?['spec']);
    if (element?['kind'] != 'Panel' || panel == null) {
      panels.add(GrafanaPanelReview(path: path, title: '', problems: const [GrafanaImportProblem.unsupportedPanel]));
      continue;
    }
    final data = _map(panel['data']);
    final group = _map(data?['spec']);
    final viz = _map(panel['vizConfig']);
    final vizSpec = _map(viz?['spec']);
    final queries = group?['queries'];
    final targets = <Map<String, dynamic>>[];
    var malformed =
        data?['kind'] != 'QueryGroup' ||
        group == null ||
        viz?['kind'] != 'VizConfig' ||
        vizSpec == null ||
        queries is! List;
    if (queries is List) {
      for (final value in queries) {
        final wrapped = _map(value);
        final target = _map(wrapped?['spec']);
        final query = _map(target?['query']);
        final querySpec = _map(query?['spec']);
        final source = _map(query?['datasource']);
        final prometheus =
            query?['kind'] == 'DataQuery' &&
            query?['group'] == 'prometheus' &&
            (query?['version'] == null || query?['version'] == 'v0');
        if (wrapped?['kind'] != 'PanelQuery' ||
            target == null ||
            querySpec == null ||
            (target['hidden'] != null && target['hidden'] is! bool) ||
            (querySpec['instant'] != null && querySpec['instant'] is! bool) ||
            (querySpec['range'] != null && querySpec['range'] is! bool) ||
            (querySpec['instant'] == true && querySpec['range'] == true)) {
          malformed = true;
        }
        targets.add({
          'expr': querySpec?['expr'],
          'hide': target?['hidden'],
          'instant': querySpec?['instant'],
          'datasource': prometheus ? {'type': 'prometheus', 'uid': source?['name']} : const {'type': 'unsupported'},
        });
      }
    }
    final options = _map(group?['queryOptions']);
    if (group?['queryOptions'] != null && options == null) malformed = true;
    final rawViz = viz?['group'];
    final review = _panel({
      'title': panel['title'],
      // Gauge presentation is omitted; only the unchanged single query goes
      // into the ConsoleCrypt editor, just as for a Grafana stat panel.
      'type': rawViz == 'gauge' ? 'stat' : rawViz,
      'targets': targets,
      'fieldConfig': vizSpec?['fieldConfig'],
      'transformations': group?['transformations'],
      if (repeatedElements.contains(entry.key)) 'repeat': true,
      if (options?['timeFrom'] != null ||
          options?['timeTo'] != null ||
          options?['timeShift'] != null ||
          options?['timeCompare'] != null)
        'timeFrom': true,
    }, path);
    panels.add(
      malformed
          ? GrafanaPanelReview(
              path: path,
              title: review.title,
              problems: {...review.problems, GrafanaImportProblem.invalidPanel},
            )
          : review,
    );
  }
  return GrafanaImportReport(
    originalJson: input,
    title: title,
    schemaVersion: null,
    panels: panels,
    variables: List.unmodifiable(variables),
    problems: [GrafanaImportProblem.presentationNotImported, if (variables.isNotEmpty) GrafanaImportProblem.variables],
  );
}
