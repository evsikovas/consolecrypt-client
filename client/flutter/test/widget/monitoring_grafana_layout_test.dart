import 'dart:convert';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_prefill.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_worker.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/grafana_import_fixture.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';

class _OfflineInspector implements GrafanaDashboardInspector {
  int calls = 0;
  @override
  Future<GrafanaInspection> inspect(String input) async {
    calls++;
    return GrafanaInspection(inspectGrafanaDashboard(input));
  }

  @override
  void cancel() {}
  @override
  void dispose() {}
}

String _dashboard() => jsonEncode({
  'schemaVersion': 42,
  'title': 'Owned long dashboard',
  'templating': {
    'list': [
      {'name': 'unused_owned_variable', 'type': 'custom'},
    ],
  },
  'panels': [
    for (var i = 0; i < 40; i++)
      {
        ...grafanaPanel(title: 'Owned panel $i', type: i == 1 ? 'heatmap' : 'timeseries'),
        if (i == 1)
          'transformations': [
            {'id': 'reduce'},
          ],
      },
  ],
});

Future<void> _open(
  WidgetTester tester,
  _OfflineInspector inspector, {
  double width = 1000,
  bool russian = false,
  double scale = 1,
  required List<GrafanaPanelPrefill> drafts,
}) async {
  tester.view.physicalSize = Size(width, 720);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    RepaintBoundary(
      key: const ValueKey('automation-proof-root'),
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: Locale(russian ? 'ru' : 'en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
          child: child!,
        ),
        home: Scaffold(
          body: Padding(
            padding: const EdgeInsets.all(24),
            child: Center(
              child: MonitoringGrafanaImport(
                sources: const [monitoringFixture],
                isCurrent: () => true,
                loadJson: () async => null,
                inspectorFactory: () => inspector,
                onContinue: drafts.add,
                onCancel: () {},
              ),
            ),
          ),
        ),
      ),
    ),
  );
  await settle(tester);
  await tester.enterText(find.byKey(const ValueKey('grafana-import-json')), _dashboard());
  await settle(tester);
  await Scrollable.ensureVisible(tester.element(find.byKey(const ValueKey('grafana-import-inspect'))), alignment: .5);
  await settle(tester);
  await tapKey(tester, 'grafana-import-inspect');
  await settle(tester);
}

Future<void> _reveal(WidgetTester tester, String key) async {
  final target = find.byKey(ValueKey(key));
  await tester.ensureVisible(target);
  await tester.tap(target);
  await settle(tester);
}

void main() {
  testWidgets('long review gives list the remaining height and keeps source and fixed footer visible', (tester) async {
    final drafts = <GrafanaPanelPrefill>[], inspector = _OfflineInspector();
    await _open(tester, inspector, drafts: drafts);
    final panels = tester.getRect(find.byKey(const ValueKey('grafana-import-panels')));
    final source = tester.getRect(find.byType(DropdownButtonFormField<String>));
    final footer = tester.getRect(find.byKey(const ValueKey('grafana-import-continue')));
    expect(panels.height, greaterThan(250));
    expect(source.top, greaterThanOrEqualTo(panels.bottom));
    expect(footer.top, greaterThan(source.bottom));
    expect(footer.bottom, lessThanOrEqualTo(720));
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    tester.view.physicalSize = const Size(1000, 1000);
    await settle(tester);
    final tallerPanels = tester.getRect(find.byKey(const ValueKey('grafana-import-panels')));
    expect(tallerPanels.height, greaterThan(panels.height + 100));
    expect(tallerPanels.height, greaterThan(280));
    expect(inspector.calls, 1);
    expect(drafts, isEmpty);
    expect(tester.takeException(), isNull);
  });

  testWidgets('notes and unsupported reasons are compact but remain available through explicit expansion', (
    tester,
  ) async {
    await _open(tester, _OfflineInspector(), drafts: []);
    final l = tester.element(find.byType(MonitoringGrafanaImport)).l10n;
    expect(find.text(l.grafanaImportUnsupportedPanel), findsNothing);
    expect(find.text(l.grafanaImportTransformations), findsNothing);
    expect(find.text(l.grafanaImportVariableNames('unused_owned_variable')), findsNothing);
    await _reveal(tester, 'grafana-import-row-panels/1');
    expect(find.text(l.grafanaImportUnsupportedPanel), findsOneWidget);
    expect(find.text(l.grafanaImportTransformations), findsOneWidget);
    expect(find.byKey(const ValueKey('grafana-import-choose-panels/1')), findsNothing);
    await _reveal(tester, 'grafana-import-notes-toggle');
    expect(find.text(l.grafanaImportVariableNames('unused_owned_variable')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('compact input is reachable and changing it clears review and explicit selection', (tester) async {
    final drafts = <GrafanaPanelPrefill>[];
    await _open(tester, _OfflineInspector(), drafts: drafts);
    await _reveal(tester, 'grafana-import-choose-panels/0');
    final input = find.byKey(const ValueKey('grafana-import-json'));
    await tester.ensureVisible(input);
    await tester.enterText(input, grafanaDashboard(query: 'owned_changed'));
    await settle(tester);
    expect(find.byKey(const ValueKey('grafana-import-panels')), findsNothing);
    await tapKey(tester, 'grafana-import-inspect');
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('grafana-import-continue'))).onPressed, isNull);
    expect(drafts, isEmpty);
    expect(tester.takeException(), isNull);
  });

  for (final width in [580.0, 1000.0]) {
    testWidgets('Russian long list and footer fit width=$width with enlarged text', (tester) async {
      await _open(tester, _OfflineInspector(), width: width, russian: true, scale: 1.6, drafts: []);
      final panels = tester.getRect(find.byKey(const ValueKey('grafana-import-panels')));
      final source = tester.getRect(find.byType(DropdownButtonFormField<String>));
      final footer = tester.getRect(find.byKey(const ValueKey('grafana-import-continue')));
      expect(panels.height, greaterThan(160));
      expect(source.bottom, lessThan(footer.top));
      expect(footer.bottom, lessThanOrEqualTo(720));
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(tester, 'grafana-import-compact-ru-${width.toInt()}');
    });
  }
}
