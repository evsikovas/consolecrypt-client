import 'dart:convert';

import 'package:consolecrypt/monitoring/grafana_constant_review.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_prefill.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_worker.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:flutter_test/flutter_test.dart';

void check(bool value) => expect(value, isTrue);
String dashboard({String expression = r'up{site="$site"}', String type = 'constant'}) => jsonEncode({
  'schemaVersion': 42,
  'title': 'Owned constant review dashboard',
  'templating': {
    'list': [
      {
        'name': 'site',
        'type': type,
        'current': {'value': 'never-use-current'},
        'query': 'never-use-default',
      },
      {'name': 'dynamic', 'type': 'query', 'query': 'never-execute-this-query'},
    ],
  },
  'panels': [
    {
      'title': 'Owned selected panel',
      'type': 'timeseries',
      'targets': [
        {'expr': expression},
      ],
      'datasource': {'type': 'prometheus', 'uid': 'display-hint-only'},
    },
  ],
});

Future<GrafanaConstantProblem> rejected(Future<Object> work) async {
  try {
    await work;
  } on GrafanaConstantException catch (error) {
    return error.problem;
  }
  throw StateError('Expected typed manual constant rejection');
}

void main() {
  late IsolatedGrafanaDashboardInspector worker;
  setUp(() => worker = IsolatedGrafanaDashboardInspector());
  tearDown(() => worker.dispose());
  final raw = dashboard();
  const source = MonitoringSource(
    id: '11111111-1111-4111-8111-111111111111',
    name: 'Chosen source',
    endpoint: 'https://metrics.invalid',
    revision: 7,
  );
  test('inspection offers constant names only and never substitutes saved defaults', () async {
    final report = await worker.inspect(raw);
    check(report.constantNames.length == 1 && report.constantNames.single == 'site');
    check(report.report.supported.isEmpty && report.report.originalJson == raw);
  });
  test('missing value remains distinct from deliberate empty literal', () async {
    check(await rejected(worker.reviewConstants(raw, 'panels/0', {})) == GrafanaConstantProblem.missingValue);
    final review = await worker.reviewConstants(raw, 'panels/0', {'site': ''});
    check(review.draft.expression == 'up{site=""}');
    check(review.original.originalJson == raw && review.original.supported.isEmpty);
  });
  test('map mutation after dispatch cannot change the captured manual value', () async {
    final values = {'site': 'owned-first'};
    final pending = worker.reviewConstants(raw, 'panels/0', values);
    values['site'] = 'owned-late';
    check((await pending).draft.expression == 'up{site="owned-first"}');
  });
  test('literal quotes and backslashes survive explicit resolver escaping', () async {
    const value = 'owned"quoted\\value';
    final review = await worker.reviewConstants(raw, 'panels/0', {'site': value});
    check(review.draft.expression == 'up{site=${jsonEncode(value)}}');
  });
  test('query variable declarations remain unsupported even with a manually supplied value', () async {
    check(
      await rejected(worker.reviewConstants(dashboard(type: 'query'), 'panels/0', {'site': 'owned'})) ==
          GrafanaConstantProblem.unsupportedVariable,
    );
  });
  test('unsupported partial matcher context is rejected without a replacement draft', () async {
    check(
      await rejected(
            worker.reviewConstants(dashboard(expression: r'up{site="prefix-$site"}'), 'panels/0', {'site': 'owned'}),
          ) ==
          GrafanaConstantProblem.unsupportedContext,
    );
  });
  test('new inspection cancels pending constant review using the same worker owner', () async {
    final old = worker
        .reviewConstants(raw, 'panels/0', {'site': 'owned'})
        .then<Object>((v) => v, onError: (Object error) => error);
    final fresh = worker.inspect(raw);
    check(await old is GrafanaInspectionCancelled);
    check((await fresh).report.originalJson == raw);
  });
  test('typed factory binds chosen Native source revision and keeps unverified units Unknown', () async {
    final review = await worker.reviewConstants(raw, 'panels/0', {'site': 'owned'});
    final draft = GrafanaPanelPrefill.fromConstants(review, source)!;
    check(draft.definition.query == review.draft.expression && draft.definition.sourceId == source.id);
    check(draft.definition.sourceRevision == 7 && draft.definition.unitHint == SavedPanelUnitHint.unknown);
    check(draft.matches(source) && !draft.matches(source.withRecord(revision: 8)));
    check(GrafanaPanelPrefill.fromConstants(review, source.withRecord(revision: 7, readOnly: true)) == null);
  });
  test('dispose revokes pending review and denies subsequent dispatch', () async {
    final old = worker
        .reviewConstants(raw, 'panels/0', {'site': 'owned'})
        .then<Object>((v) => v, onError: (Object error) => error);
    worker.dispose();
    check(await old is GrafanaInspectionCancelled);
    final late = await worker
        .reviewConstants(raw, 'panels/0', {'site': 'owned'})
        .then<Object>((v) => v, onError: (Object error) => error);
    check(late is GrafanaInspectionCancelled);
  });
}
