import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_workspace_input.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_fakes.dart';
import '../helpers/monitoring_saved_panel_fakes.dart';
import '../helpers/test_app.dart' show settle;

class _TwoGraphs extends SavedPanelTestService {
  final requests = <MonitoringQuery>[];
  @override
  Future<QueryOutcome> query(MonitoringSource source, MonitoringQuery query) async {
    requests.add(query);
    return QueryOutcome(
      status: 'success',
      httpStatus: 200,
      partial: false,
      warnings: const [],
      infos: const [],
      data: QueryData(
        query.start == null ? 'vector' : 'matrix',
        series: [
          MonitoringSeries(
            {'__name__': query.expression},
            [MonitoringSample(query.start ?? 1000, SampleKind.finite, query.expression == 'new_counter' ? 20 : 900)],
          ),
        ],
      ),
    );
  }
}

Future<void> tap(WidgetTester tester, String key) async {
  final target = find.byKey(ValueKey(key));
  await Scrollable.ensureVisible(tester.element(target), alignment: .5);
  await settle(tester);
  expect(target.hitTestable(), findsOneWidget);
  await tester.tap(target);
  await settle(tester);
}

void main() {
  testWidgets('review on unsaved graph selects its accepted data instead of previous explorer graph', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1400, 1100);
    addTearDown(() {
      tester.view.resetPhysicalSize();
      tester.view.resetDevicePixelRatio();
    });
    final service = _TwoGraphs(), store = SavedPanelTestStore()..panels = [];
    MetricsAiWorkspaceInput? selected;
    final gate = Completer<void>();
    Widget app(bool active) => MaterialApp(
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(
        body: MonitoringWorkspace(
          active: active,
          service: service,
          store: store,
          reviewMetricsAi: (_, input) async {
            selected = input;
            await gate.future;
          },
        ),
      ),
    );
    await tester.pumpWidget(app(true));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('monitoring-expression')), 'old_counter');
    await tap(tester, 'monitoring-query');
    expect(service.requests.single.expression, 'old_counter');
    await tap(tester, 'saved-panel-new');
    await tester.enterText(find.byKey(const ValueKey('saved-panel-query')), 'new_counter');
    await tap(tester, 'saved-panel-preview');
    expect(service.requests.map((q) => q.expression), ['old_counter', 'new_counter']);
    await tap(tester, 'saved-panel-review-ai');
    final input = selected!;
    final fence = MetricsAiFence(
      profileLease: Object(),
      metricsLease: input.metricsLease,
      providerLease: Object(),
      sourceFingerprint: monitoringFixture.fingerprint,
    );
    final metric = input.metric!(fence);
    expect(metric.outcome.data!.series.single.labels['__name__'], 'new_counter');
    expect(metric.outcome.data!.series.single.samples.single.value, 20);
    final projection = MetricsAiProjection.selected(
      fence,
      metrics: [
        metric.select(0, [0]),
      ],
      alerts: [],
      createdUtc: DateTime.now().toUtc(),
    );
    expect(projection.dataJson, contains('new_counter'));
    expect(projection.dataJson, isNot(contains('old_counter')));
    expect(input.alerts, isNull, reason: 'Unselected alerts from another view are not attached to this graph.');
    expect(input.isCurrent(), isTrue);
    expect(store.saves, isEmpty);
    expect(service.requests, hasLength(2), reason: 'Opening AI review never refreshes either graph.');
    await tester.pumpWidget(app(false));
    await settle(tester);
    expect(input.isCurrent(), isFalse);
    gate.complete();
    await settle(tester);
    expect(tester.takeException(), isNull);
  });
}
