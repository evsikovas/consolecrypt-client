import 'dart:convert';

import 'package:consolecrypt/monitoring/grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_prefill.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/grafana_import_fixture.dart';
import '../helpers/monitoring_fakes.dart';

void main() {
  test('one selected panel preserves exact expression while imported units remain unknown', () {
    const query = 'sum(rate(owned_counter_total{job="explicit"}[5m]))\n';
    final report = inspectGrafanaDashboard(grafanaDashboard(query: query));
    final draft = GrafanaPanelPrefill.choose(report, 'panels/0', monitoringFixture)!;
    expect(draft.definition.query, query);
    expect(draft.definition.sourceId, monitoringFixture.id);
    expect(draft.definition.sourceRevision, 1);
    expect(draft.definition.unitHint, SavedPanelUnitHint.unknown);
    expect(draft.matches(monitoringFixture), isTrue);
  });
  test('unknown or unsupported panel path never becomes an editor draft', () {
    final report = inspectGrafanaDashboard(grafanaDashboard(unsupported: true));
    expect(GrafanaPanelPrefill.choose(report, 'panels/1', monitoringFixture), isNull);
    expect(GrafanaPanelPrefill.choose(report, 'panels/99', monitoringFixture), isNull);
  });
  test('duplicate panel titles remain distinct by original path', () {
    final report = inspectGrafanaDashboard(
      jsonEncode({
        'schemaVersion': 42,
        'title': 'Duplicate title fixture',
        'panels': [grafanaPanel(query: 'up'), grafanaPanel(query: 'node_load1')],
      }),
    );
    expect(GrafanaPanelPrefill.choose(report, 'panels/0', monitoringFixture)!.definition.query, 'up');
    expect(GrafanaPanelPrefill.choose(report, 'panels/1', monitoringFixture)!.definition.query, 'node_load1');
  });
  test('source revision, configuration and identity changes invalidate a draft', () {
    final draft = GrafanaPanelPrefill.choose(
      inspectGrafanaDashboard(grafanaDashboard()),
      'panels/0',
      monitoringFixture,
    )!;
    expect(draft.matches(monitoringFixture.withRecord(revision: 2)), isFalse);
    expect(
      draft.matches(monitoringFixture.withRecord(id: '11111111-1111-4111-8111-111111111111', revision: 1)),
      isFalse,
    );
    final changed = MonitoringSource(
      id: monitoringFixture.id,
      revision: 1,
      name: monitoringFixture.name,
      endpoint: 'http://127.0.0.1:9091',
    );
    expect(draft.matches(changed), isFalse);
  });
  test('read-only and unsaved source cannot be selected', () {
    final report = inspectGrafanaDashboard(grafanaDashboard());
    final readOnly = MonitoringSource(
      id: monitoringFixture.id,
      revision: 1,
      name: monitoringFixture.name,
      endpoint: monitoringFixture.endpoint,
      readOnly: true,
    );
    const unsaved = MonitoringSource(id: '', name: 'Draft only', endpoint: 'http://127.0.0.1:9090');
    expect(GrafanaPanelPrefill.choose(report, 'panels/0', readOnly), isNull);
    expect(GrafanaPanelPrefill.choose(report, 'panels/0', unsaved), isNull);
  });
  test('choosing supported content leaves unsupported original input byte text untouched', () {
    final original = ' ${grafanaDashboard(unsupported: true)}\n';
    final report = inspectGrafanaDashboard(original);
    final draft = GrafanaPanelPrefill.choose(report, 'panels/0', monitoringFixture);
    expect(draft, isNotNull);
    expect(report.originalJson, original);
    expect(report.unsupported.single.problems, contains(GrafanaImportProblem.unsupportedPanel));
  });
}
